//! Drawing notes and marks (`drawing.note.*`, `drawing.centermark`, `drawing.centerline.bisector`,
//! `drawing.annotation.*`; Plan D, spec §2.8): text notes, leader notes (a closed filled arrow;
//! attached to a view edge, the arrow end follows it), hole notes read from the model's hole
//! feature, centre marks and centreline bisectors that regenerate with their edges
//! (`drawing::mapper`), and moving and deleting annotations. One command = one undo step.

use cadcraft_doc::{DimKind, Drawing, Entity, EntityKind, Handle};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};
use solidcraft_doc::{DrawingModel, DrawingView, ViewId};

use super::CommandSpec;
use super::drawing_dim::resolve;
use crate::drawing::mapper::{self, Seg, XKey, xkey_of};
use crate::drawing::notes::{self, Attach, NOTE_HEIGHT_IN, NoteKind};
use crate::drawing::{self, centermark, model_ref, template};
use crate::params::{bad, vec2};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("drawing.note.text", "Text", text).icon("text").params(
        "position: [x, y] in inches, where the text starts (its upper left corner); corner?: [x, y], the other corner of a box the text wraps in; \
         text: what it says (a new line starts a line); height?: inches (default 0.12); justify?: left, center or right (default left): the lines line up \
         on the text's left side, centre or right side, `position` being its upper left, upper centre or upper right (with `corner`, in the box) → handle",
    ),
    CommandSpec::new("drawing.note.leader", "Leader Text", leader).icon("text").params(
        "points: two or more [x, y] in inches, the first the arrow end (a closed filled arrow), the text at the last; text: what it says; \
         attach?: {view: a view id, edge: a model edge id the view shows (or {edge, segment?} or {at: [x, y]})}: the arrow end sits on that edge and follows it; \
         height?: inches (default 0.12); justify?: left, center or right: the text starts at, centres on or ends at the last point \
         (default: starts there when the leader comes in from the left, ends there otherwise) → handle: the leader, text_handle: its text",
    ),
    CommandSpec::new("drawing.note.hole", "Hole and Thread Note", hole).icon("hole").params(
        "view: a view id; edge: the edge of a hole the view shows (a model edge id, {edge, segment?} or {at: [x, y]}); placement: [x, y] in inches, where the text goes. \
         The text comes from the model's hole feature: ⌀ the diameter, then THRU or ↧ the depth, then the thread on a second line, and is read again by drawing.update \
         → handle: the leader, text",
    ),
    CommandSpec::new("drawing.centermark", "Center Mark", centermark_cmd).icon("point_center").params(
        "view: a view id; edge: a circle or arc the view shows (a model edge id, {edge, segment?} or {at: [x, y]}). Two lines cross at its centre, 0.125 in past it; \
         they follow the circle when the view is updated → handles: the two lines",
    ),
    CommandSpec::new("drawing.centerline.bisector", "Centerline Bisector", bisector).icon("centerline").params(
        "view: a view id; a, b: two straight edges the view shows (model edge ids, {edge, segment?} or {at: [x, y]}). A centreline midway between them, \
         over their common length and 0.125 in past it; it follows them when the view is updated → handle",
    ),
    CommandSpec::new("drawing.annotation.move", "Move Annotation", move_annotation).icon("move").params(
        "handle: a note, a leader, a dimension or a centre mark; placement: [x, y] in inches: where a note's text or a dimension's text goes \
         (an attached arrow end and a dimension's measured points stay), or a centre mark's centre → handle, placement",
    ),
    CommandSpec::new("drawing.annotation.delete", "Delete Annotation", delete_annotation).icon("delete").params(
        "handles: notes, leaders, dimensions or centre marks (a leader note's leader and text go together, as do a centre mark's lines) → deleted: every handle removed",
    ),
];

/// Most characters of a note.
const MAX_TEXT: usize = 4000;
/// The tallest note, inches.
const MAX_HEIGHT_IN: f64 = 10.0;
/// The most leader points.
const MAX_POINTS: usize = 100;

fn point(p: &Value, key: &str, cmd: &str) -> Result<Vec2> {
    let v = p.get(key).and_then(vec2).ok_or_else(|| bad(cmd, format!("`{key}` must be two finite numbers [x, y]")))?;
    Ok(Vec2::new(v.x, v.y))
}

fn text_param(p: &Value, cmd: &str) -> Result<String> {
    let t = p.get("text").and_then(Value::as_str).filter(|t| !t.trim().is_empty()).ok_or_else(|| bad(cmd, "`text` must be some text"))?;
    if t.chars().count() > MAX_TEXT {
        return Err(bad(cmd, format!("`text` is longer than {MAX_TEXT} characters")));
    }
    Ok(t.to_string())
}

/// The active sheet's layout.
fn active_layout(s: &Session, m: &DrawingModel, cmd: &str) -> Result<String> {
    let index = s.drawing_sheet.min(m.sheets.len().saturating_sub(1));
    m.sheets.get(index).map(|sh| sh.layout.clone()).ok_or_else(|| bad(cmd, "the drawing has no sheet"))
}

fn view_of<'a>(m: &'a DrawingModel, p: &Value, cmd: &str) -> Result<&'a DrawingView> {
    let id: ViewId = p.get("view").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`view` must be a view id"))?;
    m.views.get(&id).ok_or_else(|| bad(cmd, format!("no view {id}")))
}

fn layout_of(m: &DrawingModel, v: &DrawingView, cmd: &str) -> Result<String> {
    m.sheets.get(v.sheet).map(|sh| sh.layout.clone()).ok_or_else(|| bad(cmd, format!("view {} has no sheet", v.id)))
}

/// The view entity `p[key]` names: a model edge id, `{edge, segment?}` or `{at}`. Returns it
/// with its key.
fn edge_entity(s: &Session, m: &DrawingModel, v: &DrawingView, p: &Value, key: &str, cmd: &str) -> Result<(Entity, XKey)> {
    let pick = match p.get(key) {
        Some(Value::String(edge)) => json!({"edge": edge}),
        Some(o @ Value::Object(_)) => o.clone(),
        _ => return Err(bad(cmd, format!("`{key}` must be a model edge id, {{edge, segment?}} or {{at: [x, y]}}"))),
    };
    let h = resolve(s, m, v, &pick, cmd)?;
    let e = m.paper.entity(h).ok_or_else(|| bad(cmd, "the picked curve is gone"))?;
    let k = xkey_of(e).filter(|k| k.seg != Seg::Mark).ok_or_else(|| bad(cmd, "pick an edge of the view"))?;
    Ok(((**e).clone(), k))
}

/// The `height` param: inches, default [`NOTE_HEIGHT_IN`].
fn height_param(p: &Value, cmd: &str) -> Result<f64> {
    match p.get("height") {
        None | Some(Value::Null) => Ok(NOTE_HEIGHT_IN),
        Some(h) => h
            .as_f64()
            .filter(|h| h.is_finite() && *h > 0.0 && *h <= MAX_HEIGHT_IN)
            .ok_or_else(|| bad(cmd, "`height` must be a number of inches above 0, at most 10")),
    }
}

/// The `justify` param (R65): 0 left, 1 centre, 2 right; None when not given.
fn justify_param(p: &Value, cmd: &str) -> Result<Option<u8>> {
    match p.get("justify") {
        None | Some(Value::Null) => Ok(None),
        Some(j) => match j.as_str() {
            Some("left") => Ok(Some(0)),
            Some("center") => Ok(Some(1)),
            Some("right") => Ok(Some(2)),
            _ => Err(bad(cmd, "`justify` must be left, center or right")),
        },
    }
}

fn text(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.note.text";
    let m = drawing::require(s, cmd)?;
    let at = point(p, "position", cmd)?;
    let corner = match p.get("corner") {
        None | Some(Value::Null) => None,
        Some(_) => Some(point(p, "corner", cmd)?),
    };
    let value = text_param(p, cmd)?;
    let height = height_param(p, cmd)?;
    let column = justify_param(p, cmd)?.unwrap_or(0);
    // Two corners: the box's upper left, upper centre or upper right (by `justify`), its width.
    let (insert, width) = match corner {
        Some(c) => {
            let width = (c.x - at.x).abs();
            (Vec2::new(at.x.min(c.x) + width * f64::from(column) / 2.0, at.y.max(c.y)), width)
        }
        None => (at, 0.0),
    };
    let layout = active_layout(s, m, cmd)?;
    // MText attachment 1, 2, 3: top left, top centre, top right.
    let note = notes::mtext(insert, height, width, 1 + column, notes::contents(&value));
    let h = drawing::edit(s, cmd, |m, _| notes::add_text(&mut m.paper, &layout, note))?;
    Ok(json!({"handle": h.0}))
}

fn leader(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.note.leader";
    let m = drawing::require(s, cmd)?;
    let list = p.get("points").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`points` must be a list of [x, y] points"))?;
    if list.len() < 2 {
        return Err(bad(cmd, "a leader needs at least 2 points"));
    }
    if list.len() > MAX_POINTS {
        return Err(bad(cmd, format!("a leader has at most {MAX_POINTS} points")));
    }
    let mut points: Vec<Vec2> = list
        .iter()
        .map(|v| vec2(v).map(|q| Vec2::new(q.x, q.y)).ok_or_else(|| bad(cmd, "`points` must be a list of [x, y] points")))
        .collect::<Result<_>>()?;
    let value = text_param(p, cmd)?;
    let text = notes::LeaderText { height: height_param(p, cmd)?, column: justify_param(p, cmd)? };
    let (layout, attach) = match p.get("attach") {
        None | Some(Value::Null) => (active_layout(s, m, cmd)?, None),
        Some(a @ Value::Object(_)) => {
            let v = view_of(m, a, cmd)?;
            let (e, _) = edge_entity(s, m, v, a, "edge", cmd)?;
            let first = points.first().copied().unwrap_or_default();
            let (on, at) = notes::nearest_on(&e, first).ok_or_else(|| bad(cmd, "a leader attaches to a line, circle, arc or polyline"))?;
            if let Some(p0) = points.first_mut() {
                *p0 = on;
            }
            (layout_of(m, v, cmd)?, Some(Attach { view: v.id, handle: e.handle, at }))
        }
        Some(_) => return Err(bad(cmd, "`attach` must be {view, edge}")),
    };
    let (lh, th) =
        drawing::edit(s, cmd, |m, _| notes::add_leader(&mut m.paper, &layout, &points, notes::contents(&value), text, NoteKind::Leader, attach, ""))?;
    Ok(json!({"handle": lh.0, "text_handle": th.0}))
}

fn hole(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.note.hole";
    let m = drawing::require(s, cmd)?;
    let v = view_of(m, p, cmd)?;
    let (e, key) = edge_entity(s, m, v, p, "edge", cmd)?;
    let placement = point(p, "placement", cmd)?;
    let loaded = model_ref::load(s, &v.model).map_err(|e| EngineError::Other(format!("{cmd}: {e}")))?;
    let style = m.paper.dim_style(template::DIM_STYLE).cloned().unwrap_or_else(template::ansi_inch_dim_style);
    let value = notes::hole_note_text(&loaded, &key.edge, &style)?;
    let (on, at) = notes::nearest_on(&e, placement).ok_or_else(|| bad(cmd, "pick the edge of a hole"))?;
    if on.dist(placement) <= 1e-9 {
        return Err(bad(cmd, "place the note off the edge"));
    }
    let layout = layout_of(m, v, cmd)?;
    let attach = Attach { view: v.id, handle: e.handle, at };
    let (lh, _) = drawing::edit(s, cmd, |m, _| {
        notes::add_leader(
            &mut m.paper,
            &layout,
            &[on, placement],
            notes::contents(&value),
            notes::LeaderText::default(),
            NoteKind::Hole,
            Some(attach),
            &key.edge,
        )
    })?;
    Ok(json!({"handle": lh.0, "text": value}))
}

fn centermark_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.centermark";
    let m = drawing::require(s, cmd)?;
    let v = view_of(m, p, cmd)?.clone();
    let (e, key) = edge_entity(s, m, &v, p, "edge", cmd)?;
    // An arc is marked as its circle (R51).
    let (c, r) = match &e.kind {
        EntityKind::Circle(c) => (c.center.xy(), c.radius),
        EntityKind::Arc(a) => (a.center.xy(), a.radius),
        _ => return Err(bad(cmd, "a center mark needs a circle or an arc")),
    };
    let lines = centermark::center_mark(c, r);
    let handles = drawing::edit(s, cmd, |m, _| mapper::add_marks(m, &v, &key.edge, &lines))?;
    Ok(json!({"handles": handles.iter().map(|h| h.0).collect::<Vec<_>>()}))
}

fn bisector(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.centerline.bisector";
    let m = drawing::require(s, cmd)?;
    let v = view_of(m, p, cmd)?.clone();
    let (a, ka) = edge_entity(s, m, &v, p, "a", cmd)?;
    let (b, kb) = edge_entity(s, m, &v, p, "b", cmd)?;
    let (EntityKind::Line(la), EntityKind::Line(lb)) = (&a.kind, &b.kind) else {
        return Err(bad(cmd, "a centerline bisector needs two straight edges"));
    };
    if ka.edge == kb.edge {
        return Err(bad(cmd, "pick two different edges"));
    }
    let line =
        centermark::center_line((la.a.xy(), la.b.xy()), (lb.a.xy(), lb.b.xy())).ok_or_else(|| bad(cmd, "the two edges do not run side by side"))?;
    // R21: the key names both edges, sorted.
    let mut edges = [ka.edge, kb.edge];
    edges.sort();
    let edge = edges.join("|");
    let handles = drawing::edit(s, cmd, |m, _| mapper::add_marks(m, &v, &edge, &[line]))?;
    Ok(json!({"handle": handles.first().map(|h| h.0)}))
}

/// What an annotation command acts on.
enum Target {
    /// A text note's MText.
    Text(Handle),
    /// A leader note: its leader, its text, whether its arrow end is attached.
    Leader {
        leader: Handle,
        text: Option<Handle>,
        attached: bool,
    },
    Dimension(Handle),
    /// A centre mark's or centreline's lines.
    Mark(Vec<Handle>),
}

impl Target {
    fn handles(&self) -> Vec<Handle> {
        match self {
            Target::Text(h) | Target::Dimension(h) => vec![*h],
            Target::Leader { leader, text, .. } => std::iter::once(*leader).chain(*text).collect(),
            Target::Mark(hs) => hs.clone(),
        }
    }
}

/// The annotation entity `h` belongs to, or why it is none.
fn target(m: &DrawingModel, h: u64, cmd: &str) -> Result<Target> {
    let paper: &Drawing = &m.paper;
    let e = paper.entity(Handle(h)).ok_or_else(|| bad(cmd, format!("no entity {h}")))?;
    if let Some(key) = xkey_of(e).filter(|k| k.seg == Seg::Mark) {
        let layout = m.views.get(&key.view).and_then(|v| m.sheets.get(v.sheet)).and_then(|sh| paper.layout(&sh.layout));
        let mut lines: Vec<Handle> =
            layout.map(|l| l.entities.iter().filter(|x| xkey_of(x).as_ref() == Some(&key)).map(|x| x.handle).collect()).unwrap_or_default();
        if lines.is_empty() {
            lines.push(e.handle);
        }
        return Ok(Target::Mark(lines));
    }
    if let Some(view) = mapper::view_of(e) {
        let name = m.views.get(&view).map_or_else(|| format!("view {view}"), |v| v.identifier.clone());
        return Err(bad(cmd, format!("{name} entities move with their view (drawing.view.set_position)")));
    }
    if let Some(note) = notes::note_of(e) {
        return Ok(match note.kind {
            NoteKind::Text => Target::Text(e.handle),
            _ => {
                let (leader, data) = notes::leader_of(paper, e.handle).ok_or_else(|| bad(cmd, format!("{h}'s leader is gone")))?;
                Target::Leader { leader, text: data.partner.filter(|t| paper.entity(*t).is_some()), attached: data.attach.is_some() }
            }
        });
    }
    if matches!(e.kind, EntityKind::Dimension(_)) {
        return Ok(Target::Dimension(e.handle));
    }
    Err(bad(cmd, format!("{h} is not an annotation (a note, a leader, a dimension or a centre mark)")))
}

fn handle_param(p: &Value, cmd: &str) -> Result<u64> {
    p.get("handle").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`handle` must be an entity's handle"))
}

fn moved(p: Vec3, d: Vec2) -> Vec3 {
    Vec3::new(p.x + d.x, p.y + d.y, p.z)
}

/// Moves the annotation `t` so its text (or a mark's centre) is at `to`.
fn move_target(paper: &mut Drawing, t: Target, to: Vec2) -> cadcraft_doc::Result<()> {
    match t {
        Target::Text(th) => paper.modify_entity(th, |e| {
            if let EntityKind::MText(t) = &mut e.kind {
                t.insert = Vec3::new(to.x, to.y, t.insert.z);
            }
        }),
        Target::Leader { leader, text, attached } => {
            // The text goes to the placement; the leader's other points move with it, the arrow
            // end too unless it is attached.
            let from = match text.and_then(|t| paper.entity(t)).map(|e| &e.kind) {
                Some(EntityKind::MText(t)) => t.insert.xy(),
                _ => match paper.entity(leader).map(|e| &e.kind) {
                    Some(EntityKind::Leader(l)) => l.vertices.last().map_or(to, |v| v.xy()),
                    _ => to,
                },
            };
            let d = to - from;
            if let Some(th) = text {
                paper.modify_entity(th, |e| {
                    if let EntityKind::MText(t) = &mut e.kind {
                        t.insert = moved(t.insert, d);
                    }
                })?;
            }
            paper.modify_entity(leader, |e| {
                if let EntityKind::Leader(l) = &mut e.kind {
                    for (i, v) in l.vertices.iter_mut().enumerate() {
                        if i > 0 || !attached {
                            *v = moved(*v, d);
                        }
                    }
                }
            })
        }
        Target::Dimension(dh) => paper.modify_entity(dh, |e| {
            if let EntityKind::Dimension(d) = &mut e.kind {
                // Task 10 review: the text and the dimension line move; the attached definition
                // points (what it measures) never do, or the next regeneration would leave the
                // dimension behind (`assoc::recompute` drops a point moved off its object).
                let delta = to - d.text_mid.xy();
                let line_points: &[&str] = match d.kind {
                    DimKind::Linear { .. } | DimKind::Aligned => &["defpt"],
                    _ => &["defpt", "p15", "p16"],
                };
                let free: Vec<&str> = line_points.iter().copied().filter(|n| !d.assoc.iter().any(|a| a.point == *n)).collect();
                for name in free {
                    match name {
                        "defpt" => d.defpt = moved(d.defpt, delta),
                        "p15" => d.p15 = moved(d.p15, delta),
                        _ => d.p16 = moved(d.p16, delta),
                    }
                }
                d.text_mid = Vec3::new(to.x, to.y, d.text_mid.z);
                d.user_text_pos = true;
                d.block = None;
            }
        }),
        Target::Mark(lines) => {
            // The mark's centre (its first line's middle) to the placement; the next
            // regeneration of its view puts it back on what it marks.
            let centre = lines.first().and_then(|h| paper.entity(*h)).and_then(|e| match &e.kind {
                EntityKind::Line(l) => Some(l.a.xy().mid(l.b.xy())),
                _ => None,
            });
            let d = centre.map_or(Vec2::ZERO, |c| to - c);
            for lh in lines {
                paper.modify_entity(lh, |e| {
                    if let EntityKind::Line(l) = &mut e.kind {
                        (l.a, l.b) = (moved(l.a, d), moved(l.b, d));
                    }
                })?;
            }
            Ok(())
        }
    }
}

fn move_annotation(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.annotation.move";
    let m = drawing::require(s, cmd)?;
    let h = handle_param(p, cmd)?;
    let to = point(p, "placement", cmd)?;
    let t = target(m, h, cmd)?;
    drawing::edit(s, cmd, |m, _| move_target(&mut m.paper, t, to).map_err(|e| EngineError::Other(format!("{cmd}: {e}"))))?;
    Ok(json!({"handle": h, "placement": [to.x, to.y]}))
}

fn delete_annotation(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.annotation.delete";
    let m = drawing::require(s, cmd)?;
    let given: Vec<u64> = match p.get("handles") {
        Some(Value::Array(a)) if !a.is_empty() => {
            a.iter().map(|h| h.as_u64().ok_or_else(|| bad(cmd, "`handles` must be a list of entity handles"))).collect::<Result<_>>()?
        }
        _ => return Err(bad(cmd, "`handles` must be a list of entity handles")),
    };
    let mut gone: Vec<Handle> = Vec::new();
    for h in given {
        for x in target(m, h, cmd)?.handles() {
            if !gone.contains(&x) {
                gone.push(x);
            }
        }
    }
    drawing::edit(s, cmd, |m, _| {
        for h in &gone {
            m.paper.remove_entity(*h);
        }
        for group in &mut m.paper.groups {
            group.members.retain(|x| !gone.contains(x));
        }
        Ok(())
    })?;
    Ok(json!({"deleted": gone.iter().map(|h| h.0).collect::<Vec<_>>()}))
}

#[cfg(test)]
#[path = "drawing_note_tests.rs"]
mod drawing_note_tests;
