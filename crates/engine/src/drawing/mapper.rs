//! The seam (spec §4.5, CADCraft research §3 option b): a view's curves are plain paper-space
//! entities of its sheet's layout, at view scale, in inches. Each carries the xdata key
//! `(view id, model edge id, segment)`:
//!
//! `[(1001, "TCICAD_VIEW"), (1000, "<view id>"), (1000, "<model_edge_id>"), (1071, "<segment>")]`
//!
//! (a centre mark or centreline's fourth tag is `(1000, "cm")`, R21). Regenerating a view looks
//! each key up and changes its entity in place (`modify_entity`, the handle kept), so the ported
//! `assoc::update` sees the moved source and re-solves the dimensions on it; new keys are added,
//! and keys no longer produced are removed. One `Drawing.groups` entry per view lists exactly its
//! curves, so a view selects and moves as a unit.
//!
//! CADCraft's DXF writer registers every application named by a `1001` tag in the APPID table,
//! so `TCICAD_VIEW` needs no table entry here.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;

use cadcraft_doc::{Arc as ArcEntity, Circle, Common, Entity, EntityKind, Group, HAlign, Handle, Line, LwPolyline, RawTag, Space, Text, VAlign};
use cadcraft_geom::{Mat3, PolyVertex, Vec2 as PaperVec2, Vec3 as PaperVec3};
use solidcraft_doc::{DrawingModel, DrawingView, ViewId};
use solidcraft_geom::Vec2;

use super::project::{PaperCurve, ViewCurve, ViewGeometry};

pub const XDATA_APP: &str = "TCICAD_VIEW";

/// The layer of a view's visible curves, and of its hidden ones.
pub(crate) const VISIBLE_LAYER: &str = "VISIBLE";
pub(crate) const HIDDEN_LAYER: &str = "HIDDEN";

/// The fourth xdata tag of a centre mark or centreline.
const MARK_TAG: &str = "cm";

/// The layer of a view's label (spec §3.1: `<identifier>` over `SCALE <scale>`).
pub(crate) const NOTE_LAYER: &str = "NOTE";
/// A label line's fourth xdata tag is `(1000, "label<n>")` (its third is empty): not a curve
/// key, so `xkey_of` skips it and `place` never removes it.
const LABEL_TAG: &str = "label";
/// The label's text height, the gap from the view's lowest curve to its first line, and the
/// distance between its lines, inches.
const LABEL_HEIGHT_IN: f64 = 0.12;
const LABEL_GAP_IN: f64 = 0.25;
const LABEL_LINE_IN: f64 = 0.2;

/// A curve's place along its model edge.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Seg {
    Index(u32),
    /// Centre marks and centrelines, tag "cm" (R21).
    Mark,
}

/// The key of a view entity: which view, which model edge, which piece of it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct XKey {
    pub view: ViewId,
    pub edge: String,
    pub seg: Seg,
}

impl XKey {
    /// The key as xdata tags.
    pub(crate) fn tags(&self) -> Vec<RawTag> {
        let tag = |code: i32, value: String| RawTag { code, value };
        let last = match self.seg {
            Seg::Index(i) => tag(1071, i.to_string()),
            Seg::Mark => tag(1000, MARK_TAG.into()),
        };
        vec![tag(1001, XDATA_APP.into()), tag(1000, self.view.to_string()), tag(1000, self.edge.clone()), last]
    }
}

/// The view key an entity carries, if it is a view entity.
pub(crate) fn xkey_of(e: &cadcraft_doc::Entity) -> Option<XKey> {
    let xdata = &e.common.xdata;
    let at = xdata.iter().position(|t| t.code == 1001 && t.value == XDATA_APP)?;
    let mut rest = xdata.get(at + 1..)?.iter().take_while(|t| t.code != 1001);
    let view = rest.next().filter(|t| t.code == 1000)?.value.parse().ok()?;
    let edge = rest.next().filter(|t| t.code == 1000)?.value.clone();
    let seg = match rest.next()? {
        t if t.code == 1071 => Seg::Index(t.value.parse().ok()?),
        t if t.code == 1000 && t.value == MARK_TAG => Seg::Mark,
        _ => return None,
    };
    Some(XKey { view, edge, seg })
}

/// The view an entity of a view belongs to (its curves, centre marks and label), if any.
pub(crate) fn view_of(e: &Entity) -> Option<ViewId> {
    let xdata = &e.common.xdata;
    let at = xdata.iter().position(|t| t.code == 1001 && t.value == XDATA_APP)?;
    xdata.get(at + 1).filter(|t| t.code == 1000)?.value.parse().ok()
}

/// The label line (0: the identifier, 1: the scale) an entity is, with its view.
fn label_of(e: &Entity) -> Option<(ViewId, u8)> {
    let xdata = &e.common.xdata;
    let at = xdata.iter().position(|t| t.code == 1001 && t.value == XDATA_APP)?;
    let mut rest = xdata.get(at + 1..)?.iter().take_while(|t| t.code != 1001);
    let view = rest.next().filter(|t| t.code == 1000)?.value.parse().ok()?;
    rest.next().filter(|t| t.code == 1000 && t.value.is_empty())?;
    let line = rest.next().filter(|t| t.code == 1000)?.value.strip_prefix(LABEL_TAG)?.parse().ok()?;
    Some((view, line))
}

fn label_tags(view: ViewId, line: u8) -> Vec<RawTag> {
    let tag = |code: i32, value: String| RawTag { code, value };
    vec![tag(1001, XDATA_APP.into()), tag(1000, view.to_string()), tag(1000, String::new()), tag(1000, format!("{LABEL_TAG}{line}"))]
}

/// The handles of every entity of view `view` on `layout` (curves, marks, label).
fn all_of(m: &DrawingModel, layout: &str, view: ViewId) -> Vec<Handle> {
    m.paper.layout(layout).map(|l| l.entities.iter().filter(|e| view_of(e) == Some(view)).map(|e| e.handle).collect()).unwrap_or_default()
}

/// The name of the `Drawing.groups` entry of the view whose group handle is `group`.
pub(crate) fn group_name(group: Handle) -> String {
    format!("TCI_VIEW_{}", group.hex())
}

/// A view curve (mm, centred on the view) on paper: `position + mm / 25.4 · scale` (the view
/// frame is x = dir × up, y = up, so no rotation is needed).
pub(crate) fn to_paper(c: &PaperCurve, position: Vec2, scale: f64) -> PaperCurve {
    let k = scale / 25.4;
    let p = |q: Vec2| position + q * k;
    match c {
        PaperCurve::Line { a, b } => PaperCurve::Line { a: p(*a), b: p(*b) },
        PaperCurve::Arc { c, r, start, end } => PaperCurve::Arc { c: p(*c), r: r * k, start: *start, end: *end },
        PaperCurve::Circle { c, r } => PaperCurve::Circle { c: p(*c), r: r * k },
        PaperCurve::Poly { points } => PaperCurve::Poly { points: points.iter().map(|q| p(*q)).collect() },
    }
}

/// A paper curve as a CADCraft entity kind.
fn kind_of(c: &PaperCurve) -> EntityKind {
    let v3 = |p: Vec2| PaperVec3::new(p.x, p.y, 0.0);
    match c {
        PaperCurve::Line { a, b } => EntityKind::Line(Line { a: v3(*a), b: v3(*b) }),
        PaperCurve::Arc { c, r, start, end } => {
            EntityKind::Arc(ArcEntity { center: v3(*c), radius: *r, start: start.rem_euclid(TAU), end: end.rem_euclid(TAU) })
        }
        PaperCurve::Circle { c, r } => EntityKind::Circle(Circle { center: v3(*c), radius: *r }),
        PaperCurve::Poly { points } => EntityKind::LwPolyline(LwPolyline {
            vertices: points.iter().map(|p| PolyVertex { p: PaperVec2::new(p.x, p.y), ..PolyVertex::default() }).collect(),
            closed: false,
            const_width: 0.0,
            elevation: 0.0,
            plinegen: false,
        }),
    }
}

/// The handles of view `view`'s entities on `layout`, by key.
fn placed(m: &DrawingModel, layout: &str, view: ViewId) -> BTreeMap<XKey, Handle> {
    m.paper
        .layout(layout)
        .map(|l| l.entities.iter().filter_map(|e| xkey_of(e).filter(|k| k.view == view).map(|k| (k, e.handle))).collect())
        .unwrap_or_default()
}

/// The first bit of a bumped segment (R56): above every run index the projection makes.
const BUMPED: u32 = 1 << 31;

/// A curve kind's rank in a bumped segment (R56).
fn kind_rank(c: &PaperCurve) -> u32 {
    match c {
        PaperCurve::Line { .. } => 0,
        PaperCurve::Arc { .. } => 1,
        PaperCurve::Circle { .. } => 2,
        PaperCurve::Poly { .. } => 3,
    }
}

/// Where a curve sorts among curves of its kind (an arc by its start angle, R56).
fn order_key(c: &PaperCurve) -> [f64; 4] {
    match c {
        PaperCurve::Line { a, b } => [a.x, a.y, b.x, b.y],
        PaperCurve::Arc { c, r, start, end } => [*start, *end, c.x + c.y, *r],
        PaperCurve::Circle { c, r } => [c.x, c.y, *r, 0.0],
        PaperCurve::Poly { points } => {
            let p = points.first().copied().unwrap_or_default();
            [p.x, p.y, points.len() as f64, 0.0]
        }
    }
}

/// The segment each curve's key uses (R56). A curve alone on its (edge, segment) uses that
/// segment. Curves sharing one (a joined circle on a piece of its first edge, R51) all take
/// bumped segments derived from their own kind and their order among the curves of that kind
/// (arcs by start angle), never from the order of the curves: `BUMPED | kind << 28 | segment
/// << 8 | rank`. So a regenerated piece can take over a handle only when it is the same
/// geometry: when one of them goes, the other's key changes and the dimensions on both go
/// sick instead of following the wrong curve.
fn segments(curves: &[super::project::ViewCurve]) -> Vec<u32> {
    let mut by_key: BTreeMap<(&str, u32), Vec<usize>> = BTreeMap::new();
    for (i, c) in curves.iter().enumerate() {
        by_key.entry((c.edge.as_str(), c.segment)).or_default().push(i);
    }
    let mut out: Vec<u32> = curves.iter().map(|c| c.segment).collect();
    for ((_, segment), mut group) in by_key.into_iter().filter(|(_, g)| g.len() > 1) {
        group.sort_by(|&i, &j| {
            let (a, b) = (&curves[i], &curves[j]);
            kind_rank(&a.curve)
                .cmp(&kind_rank(&b.curve))
                .then_with(|| {
                    order_key(&a.curve).iter().zip(order_key(&b.curve)).fold(std::cmp::Ordering::Equal, |o, (x, y)| o.then(x.total_cmp(&y)))
                })
                .then(a.hidden.cmp(&b.hidden))
        });
        let mut rank: BTreeMap<u32, u32> = BTreeMap::new();
        for i in group {
            let kind = kind_rank(&curves[i].curve);
            let r = rank.entry(kind).or_insert(0);
            if let Some(slot) = out.get_mut(i) {
                *slot = BUMPED | kind << 28 | segment.min(0xF_FFFF) << 8 | (*r).min(0xFF);
            }
            *r += 1;
        }
    }
    out
}

/// Writes `g` for view `v` into its sheet's layout: modify_entity for keys that exist, Drawing::add for new
/// keys, remove_entity for keys no longer produced; `v.group` then lists exactly the view's entities.
///
/// Centre marks and centrelines (`Seg::Mark`) are not curves of `g`: they are regenerated from
/// the curves they mark (in place, handles kept) and stay in the group; a mark whose circle or
/// edges are gone is removed (`place_marks`).
pub(crate) fn place(m: &mut DrawingModel, v: &DrawingView, g: &ViewGeometry) {
    let Some(layout) = m.sheets.get(v.sheet).map(|s| s.layout.clone()) else { return };
    let space = Space::Paper(layout.clone());
    let before = placed(m, &layout, v.id);
    let position = v.position;
    let mut members: Vec<Handle> = Vec::new();
    let mut produced: BTreeSet<XKey> = BTreeSet::new();
    for (curve, segment) in g.curves.iter().zip(segments(&g.curves)) {
        let mut key = XKey { view: v.id, edge: curve.edge.clone(), seg: Seg::Index(segment) };
        // Only past the bumped keys' range (module `segments`): still unique, deterministic.
        let mut segment = segment;
        while produced.contains(&key) {
            segment = segment.wrapping_add(1) | BUMPED;
            key.seg = Seg::Index(segment);
        }
        let kind = kind_of(&to_paper(&curve.curve, position, v.scale));
        let layer = if curve.hidden { HIDDEN_LAYER } else { VISIBLE_LAYER };
        let handle = match before.get(&key) {
            Some(&h) => {
                let same = m.paper.entity(h).is_some_and(|e| e.kind == kind && e.common.layer == layer);
                if !same {
                    let _ = m.paper.modify_entity(h, |e| {
                        e.kind = kind;
                        e.common.layer = layer.to_string();
                    });
                }
                Some(h)
            }
            None => {
                let common = Common { layer: layer.to_string(), xdata: key.tags(), ..Common::default() };
                m.paper.add(&space, common, kind).ok()
            }
        };
        if let Some(h) = handle {
            members.push(h);
        }
        produced.insert(key);
    }
    if let Some(l) = m.paper.layout(&layout) {
        members.extend(l.entities.iter().filter(|e| label_of(e).is_some_and(|(view, _)| view == v.id)).map(|e| e.handle));
    }
    for (key, h) in &before {
        if matches!(key.seg, Seg::Index(_)) && !produced.contains(key) {
            m.paper.remove_entity(*h);
        }
    }
    let on_paper: Vec<(&ViewCurve, PaperCurve)> = g.curves.iter().map(|c| (c, to_paper(&c.curve, position, v.scale))).collect();
    members.extend(place_marks(m, &layout, v.id, &on_paper));
    set_group(m, v, members);
}

/// The layer of centre marks and centrelines.
pub(crate) const CENTER_LAYER: &str = "CENTER";

fn paper_point(p: Vec2) -> PaperVec2 {
    PaperVec2::new(p.x, p.y)
}

/// The circle (centre, radius) a centre mark on `edge` marks in `curves`: the circle or arc
/// showing `edge` (a joined circle names it among its sources; an arc is taken as its circle,
/// R51), a circle before an arc, the lowest segment first.
fn mark_circle(curves: &[(&ViewCurve, PaperCurve)], edge: &str) -> Option<(PaperVec2, f64)> {
    curves
        .iter()
        .filter(|(c, _)| c.edge == edge || c.sources.iter().any(|s| s == edge))
        .filter_map(|(c, p)| match p {
            PaperCurve::Circle { c: o, r } => Some((0, c.segment, paper_point(*o), *r)),
            PaperCurve::Arc { c: o, r, .. } => Some((1, c.segment, paper_point(*o), *r)),
            _ => None,
        })
        .min_by_key(|(rank, segment, ..)| (*rank, *segment))
        .map(|(_, _, c, r)| (c, r))
}

/// The straight piece (lowest segment) showing `edge` in `curves`.
fn mark_line(curves: &[(&ViewCurve, PaperCurve)], edge: &str) -> Option<(PaperVec2, PaperVec2)> {
    curves
        .iter()
        .filter(|(c, _)| c.edge == edge)
        .filter_map(|(c, p)| match p {
            PaperCurve::Line { a, b } => Some((c.segment, paper_point(*a), paper_point(*b))),
            _ => None,
        })
        .min_by_key(|(segment, ..)| *segment)
        .map(|(_, a, b)| (a, b))
}

/// The lines a mark keyed `edge` draws now: a centre mark's two (across, up) when `edge` shows a
/// circle or arc, else a centreline's one when `edge` is two straight edges `<a>|<b>` (R21; an
/// edge id may itself hold `|`, so every split is tried). None: what it marked is gone.
fn mark_lines(curves: &[(&ViewCurve, PaperCurve)], edge: &str) -> Option<Vec<(PaperVec2, PaperVec2)>> {
    if let Some((c, r)) = mark_circle(curves, edge) {
        return Some(super::centermark::center_mark(c, r).to_vec());
    }
    edge.match_indices('|').find_map(|(i, _)| {
        let (a, b) = (edge.get(..i)?, edge.get(i + 1..)?);
        let line = super::centermark::center_line(mark_line(curves, a)?, mark_line(curves, b)?)?;
        Some(vec![line])
    })
}

/// Regenerates view `view`'s centre marks and centrelines from its curves on paper: each line is
/// changed in place (a mark's line across stays across), and the marks whose edges are gone
/// are removed. Returns the handles kept.
fn place_marks(m: &mut DrawingModel, layout: &str, view: ViewId, curves: &[(&ViewCurve, PaperCurve)]) -> Vec<Handle> {
    let mut marks: BTreeMap<String, Vec<(Handle, bool)>> = BTreeMap::new();
    if let Some(l) = m.paper.layout(layout) {
        for e in l.entities.iter() {
            let Some(key) = xkey_of(e).filter(|k| k.view == view && k.seg == Seg::Mark) else { continue };
            let across = match &e.kind {
                EntityKind::Line(l) => (l.b.x - l.a.x).abs() >= (l.b.y - l.a.y).abs(),
                _ => true,
            };
            marks.entry(key.edge).or_default().push((e.handle, across));
        }
    }
    let mut kept = Vec::new();
    for (edge, handles) in marks {
        let Some(lines) = mark_lines(curves, &edge) else {
            for (h, _) in handles {
                m.paper.remove_entity(h);
            }
            continue;
        };
        for (h, across) in handles {
            let pick = if across { lines.first() } else { lines.get(1).or_else(|| lines.first()) };
            let Some(&(a, b)) = pick else { continue };
            let kind = EntityKind::Line(Line { a: PaperVec3::new(a.x, a.y, 0.0), b: PaperVec3::new(b.x, b.y, 0.0) });
            if m.paper.entity(h).is_some_and(|e| e.kind != kind) {
                let _ = m.paper.modify_entity(h, |e| e.kind = kind);
            }
            kept.push(h);
        }
    }
    kept
}

/// Adds a mark of view `v` keyed `edge`: `lines` on CENTER with the key `(v, edge, "cm")`, in
/// the view's group. Returns their handles.
pub(crate) fn add_marks(m: &mut DrawingModel, v: &DrawingView, edge: &str, lines: &[(PaperVec2, PaperVec2)]) -> crate::Result<Vec<Handle>> {
    let layout = m.sheets.get(v.sheet).map(|s| s.layout.clone()).ok_or_else(|| crate::EngineError::Other(format!("view {} has no sheet", v.id)))?;
    let key = XKey { view: v.id, edge: edge.to_string(), seg: Seg::Mark };
    let mut out = Vec::new();
    for (a, b) in lines {
        let common = Common { layer: CENTER_LAYER.to_string(), xdata: key.tags(), ..Common::default() };
        let kind = EntityKind::Line(Line { a: PaperVec3::new(a.x, a.y, 0.0), b: PaperVec3::new(b.x, b.y, 0.0) });
        out.push(m.paper.add(&Space::Paper(layout.clone()), common, kind).map_err(|e| crate::EngineError::Other(e.to_string()))?);
    }
    let name = group_name(v.group);
    if let Some(group) = m.paper.groups.iter_mut().find(|g| g.name == name) {
        group.members.extend(out.iter().copied());
    }
    Ok(out)
}

/// `v`'s group lists `members` and is described by its identifier.
fn set_group(m: &mut DrawingModel, v: &DrawingView, members: Vec<Handle>) {
    let name = group_name(v.group);
    match m.paper.groups.iter_mut().find(|gr| gr.name == name) {
        Some(group) => {
            group.members = members;
            group.description.clone_from(&v.identifier);
        }
        None => m.paper.groups.push(Group { name, description: v.identifier.clone(), selectable: true, members }),
    }
}

/// Applies `t` to every entity of view `v` (curves, marks and label) in place, handles kept.
pub(crate) fn transform_view(m: &mut DrawingModel, v: &DrawingView, t: &Mat3) {
    let Some(layout) = m.sheets.get(v.sheet).map(|s| s.layout.clone()) else { return };
    for h in all_of(m, &layout, v.id) {
        let _ = m.paper.modify_entity(h, |e| e.kind.transform(t));
    }
}

/// `p -> to + (p - from) * k`: a view centred at `from` moved to `to` and scaled by `k`.
pub(crate) fn move_and_scale(from: Vec2, to: Vec2, k: f64) -> Mat3 {
    Mat3::translate(PaperVec2::new(to.x, to.y)).then_before(Mat3::scale(k, k)).then_before(Mat3::translate(PaperVec2::new(-from.x, -from.y)))
}

/// The handles of every entity of view `v` (curves, marks, label).
pub(crate) fn view_handles(m: &DrawingModel, v: &DrawingView) -> Vec<Handle> {
    m.sheets.get(v.sheet).map(|s| all_of(m, &s.layout, v.id)).unwrap_or_default()
}

/// Removes every entity of view `v` and its group.
pub(crate) fn remove_view(m: &mut DrawingModel, v: &DrawingView) {
    if let Some(layout) = m.sheets.get(v.sheet).map(|s| s.layout.clone()) {
        for h in all_of(m, &layout, v.id) {
            m.paper.remove_entity(h);
        }
    }
    let name = group_name(v.group);
    m.paper.groups.retain(|g| g.name != name);
}

/// View `v`'s label as it should stand (spec §3.1): with `label_visible`, two texts on NOTE
/// centred under the view's curves, `<identifier>` over `SCALE <scale>`; else none. Existing
/// label texts are changed in place. The group is refreshed (members, description).
pub(crate) fn place_label(m: &mut DrawingModel, v: &DrawingView) {
    let Some(layout) = m.sheets.get(v.sheet).map(|s| s.layout.clone()) else { return };
    let mut lines: BTreeMap<u8, Handle> = BTreeMap::new();
    let mut members: Vec<Handle> = Vec::new();
    let mut bottom = f64::INFINITY;
    if let Some(l) = m.paper.layout(&layout) {
        for e in l.entities.iter() {
            if let Some((view, line)) = label_of(e) {
                if view == v.id {
                    lines.insert(line, e.handle);
                }
            } else if xkey_of(e).is_some_and(|k| k.view == v.id) {
                members.push(e.handle);
                let b = cadcraft_doc::entity_bounds(&m.paper, e, 0);
                if !b.is_empty() {
                    bottom = bottom.min(b.min.y);
                }
            }
        }
    }
    if !bottom.is_finite() {
        bottom = v.position.y;
    }
    let texts = [v.identifier.clone(), format!("SCALE {}", solidcraft_doc::format_scale(v.scale))];
    for (line, value) in (0u8..).zip(texts) {
        let existing = lines.remove(&line);
        if !v.label_visible {
            if let Some(h) = existing {
                m.paper.remove_entity(h);
            }
            continue;
        }
        let at = PaperVec3::new(v.position.x, bottom - LABEL_GAP_IN - LABEL_LINE_IN * f64::from(line), 0.0);
        let text = Text {
            insert: at,
            align_pt: Some(at),
            height: LABEL_HEIGHT_IN,
            value,
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: super::template::TEXT_STYLE.into(),
            halign: HAlign::Center,
            valign: VAlign::Top,
        };
        let kind = EntityKind::Text(text);
        let handle = match existing {
            Some(h) => {
                let _ = m.paper.modify_entity(h, |e| {
                    e.kind = kind;
                    e.common.layer = NOTE_LAYER.to_string();
                });
                Some(h)
            }
            None => {
                let common = Common { layer: NOTE_LAYER.to_string(), xdata: label_tags(v.id, line), ..Common::default() };
                m.paper.add(&Space::Paper(layout.clone()), common, kind).ok()
            }
        };
        members.extend(handle);
    }
    // Lines no label has any more.
    for h in lines.into_values() {
        m.paper.remove_entity(h);
    }
    set_group(m, v, members);
}

/// The handle of the entity keyed `key`, on its view's sheet.
pub(crate) fn handle_of(m: &DrawingModel, key: &XKey) -> Option<cadcraft_doc::Handle> {
    let layout = m.sheets.get(m.views.get(&key.view)?.sheet)?.layout.as_str();
    m.paper.layout(layout)?.entities.iter().find(|e| xkey_of(e).as_ref() == Some(key)).map(|e: &std::sync::Arc<Entity>| e.handle)
}
