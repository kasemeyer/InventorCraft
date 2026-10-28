//! Drawing dimensions (`drawing.dim.*`, Plan D, spec §2.7): General Dimension chooses linear,
//! aligned, diameter or radius by what is picked, as Inventor's does; the direct commands make
//! one kind and check their geometry; Edit Dimension sets the text, precision, justification and
//! extension lines. Every dimension is attached to the view entities it measures
//! (`drawing::assoc`), so it follows the model through regeneration. One command = one undo step.

use cadcraft_doc::{AssocSnap, Dimension, EntityKind, Handle};
use cadcraft_geom::Vec2;
use serde_json::{Map, Value, json};
use solidcraft_doc::{DrawingModel, DrawingView, ViewId};

use super::CommandSpec;
use crate::drawing::dims::{self, Anchor, DXF_TOKEN, Direction, VALUE_TOKEN};
use crate::drawing::mapper::{self, Seg, XKey, xkey_of};
use crate::drawing::{self, AT_TOLERANCE_IN, PaperPick, pick};
use crate::params::{bad, vec2};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("drawing.dim.general", "General Dimension", general).icon("dimension").params(
        "As Inventor's General Dimension, the kind is chosen by the picks: one straight edge gives its length (horizontal or vertical by the edge, else aligned); \
         a circle gives a diameter, an arc a radius; two picks give the distance between their snap points (default: a line's mid point, a circle's or arc's centre), \
         horizontal when the placement is above or below them, vertical when it is left or right, unless orientation: horizontal|vertical|aligned is given. \
         view: a view id; picks: one or two of {edge: a model edge id shown by the view (segment?: which piece), snap?: start|end|mid|center|vertex (a polyline's vertex: vertex?: its index, else the one nearest `at`)} or {at: [x, y], snap?}; \
         placement: [x, y] in inches → handle, kind: linear|aligned|diameter|radius, text: what it draws",
    ),
    CommandSpec::new("drawing.dim.linear", "Linear Dimension", linear).icon("dimension").params(
        "A horizontal or vertical dimension of one straight edge or between two picks, chosen by the placement unless orientation: horizontal|vertical is given. \
         view: a view id; picks: one or two of {edge, segment?, snap?} or {at: [x, y], snap?}; placement: [x, y] in inches → handle, kind, text",
    ),
    CommandSpec::new("drawing.dim.aligned", "Aligned Dimension", aligned).icon("dimension").params(
        "A dimension along one straight edge or between two picks. view: a view id; picks: one or two of {edge, segment?, snap?} or {at: [x, y], snap?}; \
         placement: [x, y] in inches → handle, kind, text",
    ),
    CommandSpec::new("drawing.dim.diameter", "Diameter Dimension", diameter).icon("dimension").params(
        "The diameter of a circle or an arc (an arc is measured as its circle). view: a view id; picks: [{edge, segment?} or {at: [x, y]}]; \
         placement: [x, y], where the text goes, in inches → handle, kind, text",
    ),
    CommandSpec::new("drawing.dim.radius", "Radius Dimension", radius).icon("dimension").params(
        "The radius of a circle or an arc. view: a view id; picks: [{edge, segment?} or {at: [x, y]}]; placement: [x, y], where the text goes, in inches → handle, kind, text",
    ),
    CommandSpec::new("drawing.dim.edit", "Edit Dimension", edit).icon("edit").params(
        "dim: a dimension's handle; text?: what it says, where <<>> stands for the measured value (\"\" = the value alone); hide_value?: bool (true drops <<>>, false puts it back); \
         precision?: 0 to 6 decimals, or null for the style's; justify?: {horizontal?: center|first|second, vertical?: centered|above|below}; \
         ext_lines?: {first?: bool, second?: bool} (false hides that extension line) → handle, kind, text",
    ),
];

/// Most characters of a dimension's text.
const MAX_TEXT: usize = 1000;

/// What `drawing.dim.*` is asked to make.
#[derive(Clone, Copy, PartialEq)]
enum Want {
    General,
    Linear,
    Aligned,
    Diameter,
    Radius,
}

/// The snap asked for on a pick.
#[derive(Clone, Copy, PartialEq)]
enum Snap {
    Start,
    End,
    Mid,
    Center,
    /// A polyline's vertex: by index, or (None) the one nearest the pick's `at`.
    Vertex(Option<usize>),
}

/// A picked view entity.
struct Picked {
    handle: Handle,
    kind: EntityKind,
    snap: Option<Snap>,
    /// Where an `{at}` pick was made.
    at: Option<Vec2>,
}

fn snap_param(p: &Value, cmd: &str) -> Result<Option<Snap>> {
    match p.get("snap") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_str().map(str::trim) {
            Some("start") => Ok(Some(Snap::Start)),
            Some("end") => Ok(Some(Snap::End)),
            Some("mid") => Ok(Some(Snap::Mid)),
            Some("center") => Ok(Some(Snap::Center)),
            Some("vertex") => match p.get("vertex") {
                None | Some(Value::Null) => Ok(Some(Snap::Vertex(None))),
                Some(n) => {
                    let n = n.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad(cmd, "`vertex` must be a whole number"))?;
                    Ok(Some(Snap::Vertex(Some(n))))
                }
            },
            _ => Err(bad(cmd, "`snap` must be start, end, mid, center or vertex")),
        },
    }
}

/// The view curve `{edge, segment?}` or `{at}` names.
pub(crate) fn resolve(s: &Session, m: &DrawingModel, v: &DrawingView, p: &Value, cmd: &str) -> Result<Handle> {
    if let Some(at) = p.get("at") {
        let at = vec2(at).ok_or_else(|| bad(cmd, "`at` must be two finite numbers"))?;
        return match pick(s, [at.x, at.y], AT_TOLERANCE_IN) {
            Some(PaperPick::ViewEdge { view, handle, .. }) if view == v.id => Ok(Handle(handle)),
            _ => Err(bad(cmd, format!("no edge of view {} at [{}, {}]", v.id, at.x, at.y))),
        };
    }
    let edge = p.get("edge").and_then(Value::as_str).ok_or_else(|| bad(cmd, "each pick needs `edge` (a model edge id) or `at` ([x, y])"))?;
    let segment = match p.get("segment") {
        None | Some(Value::Null) => None,
        Some(n) => Some(n.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad(cmd, "`segment` must be a whole number"))?),
    };
    // The first piece of the edge the view shows (or the one asked for).
    let first = || {
        let layout = m.paper.layout(&m.sheets.get(v.sheet)?.layout)?;
        layout
            .entities
            .iter()
            // A curve the style suppresses (invisible, R61) is not shown.
            .filter(|e| e.common.visible)
            .filter_map(|e| xkey_of(e).filter(|k| k.view == v.id && k.edge == edge))
            .filter_map(|k| match k.seg {
                Seg::Index(i) => Some(i),
                Seg::Mark => None,
            })
            .min()
    };
    segment
        .or_else(first)
        .and_then(|seg| mapper::handle_of(m, &XKey { view: v.id, edge: edge.to_string(), seg: Seg::Index(seg) }))
        .filter(|h| m.paper.entity(*h).is_some_and(|e| e.common.visible))
        .ok_or_else(|| bad(cmd, format!("view {} shows no edge {edge}", v.id)))
}

fn picks(s: &Session, m: &DrawingModel, v: &DrawingView, p: &Value, cmd: &str) -> Result<Vec<Picked>> {
    let list = p
        .get("picks")
        .and_then(Value::as_array)
        .filter(|l| (1..=2).contains(&l.len()))
        .ok_or_else(|| bad(cmd, "`picks` must hold one or two picks"))?;
    list.iter()
        .map(|pk| {
            if !pk.is_object() {
                return Err(bad(cmd, "each pick must be {edge, snap?} or {at, snap?}"));
            }
            let handle = resolve(s, m, v, pk, cmd)?;
            let kind = m.paper.entity(handle).map(|e| e.kind.clone()).ok_or_else(|| bad(cmd, "the picked curve is gone"))?;
            let at = pk.get("at").and_then(vec2).map(|q| Vec2::new(q.x, q.y));
            Ok(Picked { handle, kind, snap: snap_param(pk, cmd)?, at })
        })
        .collect()
}

/// A pick's point: its snap (default: a line's middle, a polyline's start, a circle's or arc's
/// centre).
fn anchor(paper: &cadcraft_doc::Drawing, pk: &Picked, cmd: &str) -> Result<Anchor> {
    let snap = match (&pk.kind, pk.snap) {
        (EntityKind::Line(_), None | Some(Snap::Mid)) => AssocSnap::Mid,
        (EntityKind::Line(_) | EntityKind::Arc(_) | EntityKind::LwPolyline(_), Some(Snap::Start)) => AssocSnap::Start,
        (EntityKind::Line(_) | EntityKind::Arc(_) | EntityKind::LwPolyline(_), Some(Snap::End)) => AssocSnap::End,
        (EntityKind::Arc(_), Some(Snap::Mid)) => AssocSnap::Mid,
        (EntityKind::Arc(_) | EntityKind::Circle(_), None | Some(Snap::Center)) => AssocSnap::Center,
        (EntityKind::LwPolyline(_), None) => AssocSnap::Start,
        // GAP 6: a polyline's vertex (an iso view's rims are polylines).
        (EntityKind::LwPolyline(_), Some(Snap::Vertex(Some(index)))) => AssocSnap::Vertex { index },
        (EntityKind::LwPolyline(p), Some(Snap::Vertex(None))) => {
            let at = pk.at.ok_or_else(|| bad(cmd, "a vertex snap on an `edge` pick names its `vertex` (an index)"))?;
            let index = p
                .vertices
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| a.p.dist(at).total_cmp(&b.p.dist(at)))
                .map(|(i, _)| i)
                .ok_or_else(|| bad(cmd, "the picked polyline has no vertex"))?;
            AssocSnap::Vertex { index }
        }
        _ => {
            return Err(bad(
                cmd,
                "that snap is not on the picked curve (a line has start, end and mid; an arc also center; a circle center; a polyline start, end and vertex)",
            ));
        }
    };
    let assoc = cadcraft_doc::DimAssoc { point: String::new(), handle: pk.handle, snap };
    let at = drawing::assoc::snap_point(paper, &assoc).ok_or_else(|| bad(cmd, "the picked curve has no such point"))?;
    Ok(Anchor { at, handle: pk.handle, snap: assoc.snap })
}

/// `orientation`, if given.
fn orientation(p: &Value, cmd: &str, aligned_ok: bool) -> Result<Option<Direction>> {
    match p.get("orientation") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_str().map(str::trim) {
            Some("horizontal") => Ok(Some(Direction::Horizontal)),
            Some("vertical") => Ok(Some(Direction::Vertical)),
            Some("aligned") if aligned_ok => Ok(Some(Direction::Aligned)),
            _ if aligned_ok => Err(bad(cmd, "`orientation` must be horizontal, vertical or aligned")),
            _ => Err(bad(cmd, "`orientation` must be horizontal or vertical")),
        },
    }
}

/// `d`, a given orientation, when it measures something between `a` and `b`: a horizontal
/// dimension of two points one above the other (or a vertical one of two side by side) would
/// read 0 and is refused.
fn measurable(a: Vec2, b: Vec2, d: Direction, cmd: &str) -> Result<Direction> {
    let (extent, name) = match d {
        Direction::Horizontal => ((b.x - a.x).abs(), "horizontal"),
        Direction::Vertical => ((b.y - a.y).abs(), "vertical"),
        Direction::Aligned => (a.dist(b), "aligned"),
    };
    if extent <= 1e-9 {
        return Err(bad(cmd, format!("a {name} dimension here would measure nothing: give another orientation")));
    }
    Ok(d)
}

/// Horizontal when `placement` is above or below the box of `a` and `b`, vertical when it is
/// left or right of it (the larger way out wins); inside, along the box's longer side.
fn by_placement(a: Vec2, b: Vec2, placement: Vec2) -> Direction {
    let (lo, hi) = (Vec2::new(a.x.min(b.x), a.y.min(b.y)), Vec2::new(a.x.max(b.x), a.y.max(b.y)));
    let ex = (lo.x - placement.x).max(placement.x - hi.x).max(0.0);
    let ey = (lo.y - placement.y).max(placement.y - hi.y).max(0.0);
    if ey > 0.0 && ey >= ex {
        Direction::Horizontal
    } else if ex > 0.0 {
        Direction::Vertical
    } else if hi.x - lo.x >= hi.y - lo.y {
        Direction::Horizontal
    } else {
        Direction::Vertical
    }
}

/// A straight edge's direction: horizontal, vertical or neither.
fn edge_direction(a: Vec2, b: Vec2) -> Direction {
    let d = b - a;
    let tol = 1e-9 * (1.0 + d.len());
    if d.y.abs() <= tol {
        Direction::Horizontal
    } else if d.x.abs() <= tol {
        Direction::Vertical
    } else {
        Direction::Aligned
    }
}

fn make(s: &mut Session, p: &Value, want: Want, cmd: &str) -> Result<Value> {
    let m = drawing::require(s, cmd)?;
    let view_id: ViewId = p.get("view").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`view` must be a view id"))?;
    let v = m.views.get(&view_id).ok_or_else(|| bad(cmd, format!("no view {view_id}")))?;
    let placement = p.get("placement").and_then(vec2).ok_or_else(|| bad(cmd, "placement must be two finite numbers"))?;
    let placement = Vec2::new(placement.x, placement.y);
    let layout = m.sheets.get(v.sheet).map(|sh| sh.layout.clone()).ok_or_else(|| bad(cmd, format!("view {view_id} has no sheet")))?;
    let picked = picks(s, m, v, p, cmd)?;
    let given = orientation(p, cmd, want == Want::General)?;
    let scale = v.scale;
    let paper = &m.paper;
    let need_round = || bad(cmd, if want == Want::Radius { "a radius needs a circle or an arc" } else { "a diameter needs a circle or an arc" });
    let need_straight = || {
        bad(
            cmd,
            if want == Want::Aligned {
                "an aligned dimension needs a straight edge or two points"
            } else {
                "a linear dimension needs a straight edge or two points"
            },
        )
    };
    let dimension: Dimension = match (want, picked.as_slice()) {
        (Want::Diameter | Want::Radius, [one]) | (Want::General, [one @ Picked { kind: EntityKind::Circle(_) | EntityKind::Arc(_), .. }]) => {
            let (c, r) = match &one.kind {
                EntityKind::Circle(c) => (c.center.xy(), c.radius),
                EntityKind::Arc(a) => (a.center.xy(), a.radius),
                _ => return Err(need_round()),
            };
            let diameter = match want {
                Want::Radius => false,
                Want::Diameter => true,
                _ => matches!(one.kind, EntityKind::Circle(_)),
            };
            dims::radial(paper, one.handle, c, r, diameter, placement, scale).ok_or_else(need_round)?
        }
        (Want::Diameter | Want::Radius, _) => return Err(need_round()),
        (_, [one]) => {
            let EntityKind::Line(l) = &one.kind else {
                return Err(match want {
                    Want::General => bad(cmd, "pick a straight edge, a circle or an arc, or two points"),
                    _ => need_straight(),
                });
            };
            let start = Anchor { at: l.a.xy(), handle: one.handle, snap: AssocSnap::Start };
            let end = Anchor { at: l.b.xy(), handle: one.handle, snap: AssocSnap::End };
            let direction = match (want, given) {
                (Want::Aligned, _) => Direction::Aligned,
                (_, Some(d)) => measurable(start.at, end.at, d, cmd)?,
                (Want::General, None) => edge_direction(start.at, end.at),
                _ => by_placement(start.at, end.at, placement),
            };
            dims::linear(&start, &end, direction, placement, scale)
        }
        (_, [a, b]) => {
            let (a, b) = (anchor(paper, a, cmd)?, anchor(paper, b, cmd)?);
            if a.at.dist(b.at) <= 1e-9 {
                return Err(bad(cmd, "the two picks are at the same point"));
            }
            let direction = match (want, given) {
                (Want::Aligned, _) => Direction::Aligned,
                (_, Some(d)) => measurable(a.at, b.at, d, cmd)?,
                (_, None) => by_placement(a.at, b.at, placement),
            };
            dims::linear(&a, &b, direction, placement, scale)
        }
        _ => return Err(bad(cmd, "`picks` must hold one or two picks")),
    };
    let kind = dims::kind_name(&dimension);
    let handle = drawing::edit(s, cmd, |m, _| dims::add(&mut m.paper, &layout, dimension))?;
    let text = drawing::require(s, cmd).ok().and_then(|m| dims::dimension_text(m, handle.0)).unwrap_or_default();
    Ok(json!({"handle": handle.0, "kind": kind, "text": text}))
}

fn general(s: &mut Session, p: &Value) -> Result<Value> {
    make(s, p, Want::General, "drawing.dim.general")
}

fn linear(s: &mut Session, p: &Value) -> Result<Value> {
    make(s, p, Want::Linear, "drawing.dim.linear")
}

fn aligned(s: &mut Session, p: &Value) -> Result<Value> {
    make(s, p, Want::Aligned, "drawing.dim.aligned")
}

fn diameter(s: &mut Session, p: &Value) -> Result<Value> {
    make(s, p, Want::Diameter, "drawing.dim.diameter")
}

fn radius(s: &mut Session, p: &Value) -> Result<Value> {
    make(s, p, Want::Radius, "drawing.dim.radius")
}

fn bool_of(v: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(b) => b.as_bool().map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// `drawing.dim.edit`: the text (with `<<>>`), hide value, precision (DIMDEC), justification
/// (DIMJUST, DIMTAD) and extension lines (DIMSE1, DIMSE2) of one dimension.
fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.dim.edit";
    let m = drawing::require(s, cmd)?;
    let h = p.get("dim").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`dim` must be a dimension's handle"))?;
    let current = dims::dimension_of(m, h).ok_or_else(|| bad(cmd, format!("{h} is not a dimension")))?;
    let mut d = current.clone();
    let mut overrides = Map::new();
    let mut named = false;
    if let Some(t) = p.get("text").filter(|t| !t.is_null()) {
        let t = t.as_str().ok_or_else(|| bad(cmd, "`text` must be text"))?;
        if t.chars().count() > MAX_TEXT {
            return Err(bad(cmd, format!("text is longer than {MAX_TEXT} characters")));
        }
        d.text = t.replace(VALUE_TOKEN, DXF_TOKEN);
        named = true;
    }
    match bool_of(p, "hide_value", cmd)? {
        Some(true) => {
            let rest = d.text.replace(DXF_TOKEN, "").trim().to_string();
            // A blank text draws nothing (DXF).
            d.text = if rest.is_empty() { " ".into() } else { rest };
            named = true;
        }
        Some(false) => {
            if !d.text.contains(DXF_TOKEN) {
                let rest = d.text.trim();
                d.text = if rest.is_empty() { String::new() } else { format!("{DXF_TOKEN} {rest}") };
            }
            named = true;
        }
        None => {}
    }
    // `precision: null` clears the dimension's own precision: it takes the style's again.
    let clear_precision = matches!(p.get("precision"), Some(Value::Null));
    if let Some(n) = p.get("precision").filter(|n| !n.is_null()) {
        let n = n.as_u64().filter(|n| *n <= 6).ok_or_else(|| bad(cmd, "precision must be 0 to 6"))?;
        overrides.insert("decimals".into(), json!(n));
    }
    if let Some(j) = p.get("justify").filter(|j| !j.is_null()) {
        if !j.is_object() {
            return Err(bad(cmd, "`justify` must be {horizontal?, vertical?}"));
        }
        if let Some(hz) = j.get("horizontal").filter(|x| !x.is_null()) {
            let n = match hz.as_str().map(str::trim) {
                Some("center") => 0,
                Some("first") => 1,
                Some("second") => 2,
                _ => return Err(bad(cmd, "`justify.horizontal` must be center, first or second")),
            };
            overrides.insert("textJust".into(), json!(n));
        }
        if let Some(vt) = j.get("vertical").filter(|x| !x.is_null()) {
            let n = match vt.as_str().map(str::trim) {
                Some("centered") => 0,
                Some("above") => 1,
                Some("below") => 4,
                _ => return Err(bad(cmd, "`justify.vertical` must be centered, above or below")),
            };
            overrides.insert("textAbove".into(), json!(n));
        }
    }
    if let Some(x) = p.get("ext_lines").filter(|x| !x.is_null()) {
        if !x.is_object() {
            return Err(bad(cmd, "`ext_lines` must be {first?: bool, second?: bool}"));
        }
        if let Some(first) = bool_of(x, "first", cmd)? {
            overrides.insert("suppressExt1".into(), json!(!first));
        }
        if let Some(second) = bool_of(x, "second", cmd)? {
            overrides.insert("suppressExt2".into(), json!(!second));
        }
    }
    if !named && overrides.is_empty() && !clear_precision {
        return Err(bad(cmd, "name text, hide_value, precision, justify or ext_lines"));
    }
    d.overrides.extend(overrides);
    if clear_precision {
        d.overrides.remove("decimals");
    }
    d.block = None;
    let kind = dims::kind_name(&d);
    drawing::edit(s, cmd, |m, _| {
        m.paper.modify_entity(Handle(h), |e| e.kind = EntityKind::Dimension(d)).map_err(|e| crate::EngineError::Other(format!("{cmd}: {e}")))
    })?;
    let text = drawing::require(s, cmd).ok().and_then(|m| dims::dimension_text(m, h)).unwrap_or_default();
    Ok(json!({"handle": h, "kind": kind, "text": text}))
}

#[cfg(test)]
#[path = "drawing_dim_tests.rs"]
mod drawing_dim_tests;
