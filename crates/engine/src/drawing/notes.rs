//! Notes on a drawing (spec §2.8): text notes, leader notes and hole notes. A text note is an
//! `MText` on NOTE. A leader note is a `Leader` (its first vertex the arrow end, a closed filled
//! arrow from the ANSI_INCH style's DIMLDRBLK) and an `MText` at its last vertex. A hole note is
//! a leader note whose text is generated from the model's `solid.hole` feature.
//!
//! Each note entity carries xdata naming what it is and its partner:
//!
//! `[(1001, "TCICAD_NOTE"), (1000, kind), (1000, partner handle), (1000, view), (1000, attached
//! handle), (1040, where on it), (1000, model edge id)]`
//!
//! `kind` is `text`, `leader`, `hole` or `label` (the MText of a leader or hole note, whose
//! partner is its leader); a leader's partner is its MText. A leader attached to a view edge
//! (`attach`) stores the view, the edge entity's handle and where its arrow end sits on it (a
//! line's fraction from its start, a circle's or arc's angle, a polyline's fraction of its
//! length); [`follow`] puts the arrow end back there after every drawing edit, so it follows the
//! edge through regeneration (handles are kept, spec §4.5). A hole note also stores the model
//! edge id its text is read from (`drawing.update` regenerates the text).

use std::collections::BTreeSet;
use std::f64::consts::TAU;

use cadcraft_doc::{Common, DimStyle, Drawing, Entity, EntityKind, Handle, Leader, MText, RawTag, Space};
use cadcraft_geom::{Vec2, Vec3};
use solidcraft_doc::expr::Kind;
use solidcraft_doc::{DrawingModel, FeatureKind, ViewId};

use super::mapper::NOTE_LAYER;
use super::model_ref::LoadedModel;
use super::template::{DIM_STYLE, TEXT_STYLE};
use crate::{EngineError, Result};

/// A note's text height, inches (spec §3.1).
pub const NOTE_HEIGHT_IN: f64 = 0.12;
/// A leader's arrowhead: closed filled (owner default; §8.4). The leaders use ANSI_INCH, whose
/// DIMLDRBLK is "" (closed filled), so this names it and is not written anywhere.
pub const LEADER_ARROW: &str = "closed_filled";

/// The xdata application of a note's entities.
pub(crate) const NOTE_APP: &str = "TCICAD_NOTE";

/// The command that refuses a non-hole edge.
const HOLE_CMD: &str = "drawing.note.hole";

/// What a note entity is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NoteKind {
    /// A text note's MText.
    Text,
    /// A leader note's Leader.
    Leader,
    /// A hole note's Leader.
    Hole,
    /// The MText of a leader or hole note.
    Label,
}

impl NoteKind {
    fn name(self) -> &'static str {
        match self {
            NoteKind::Text => "text",
            NoteKind::Leader => "leader",
            NoteKind::Hole => "hole",
            NoteKind::Label => "label",
        }
    }

    fn parse(s: &str) -> Option<NoteKind> {
        Some(match s {
            "text" => NoteKind::Text,
            "leader" => NoteKind::Leader,
            "hole" => NoteKind::Hole,
            "label" => NoteKind::Label,
            _ => return None,
        })
    }
}

/// Where a leader's arrow end is attached: an entity of a view, and where on it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Attach {
    pub view: ViewId,
    pub handle: Handle,
    pub at: f64,
}

/// A note entity's xdata.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Note {
    pub kind: NoteKind,
    pub partner: Option<Handle>,
    pub attach: Option<Attach>,
    /// The model edge a hole note reads (empty for the others).
    pub edge: String,
}

impl Note {
    pub(crate) fn tags(&self) -> Vec<RawTag> {
        let tag = |code: i32, value: String| RawTag { code, value };
        let (view, on, at) = match &self.attach {
            Some(a) => (a.view.to_string(), a.handle.hex(), a.at),
            None => (String::new(), String::new(), 0.0),
        };
        vec![
            tag(1001, NOTE_APP.into()),
            tag(1000, self.kind.name().into()),
            tag(1000, self.partner.map(Handle::hex).unwrap_or_default()),
            tag(1000, view),
            tag(1000, on),
            tag(1040, format!("{at:?}")),
        ]
        .into_iter()
        // The model edge, chunked when long (R62).
        .chain(super::mapper::string_tags(&self.edge))
        .collect()
    }
}

/// The note an entity is, if it is one.
pub(crate) fn note_of(e: &Entity) -> Option<Note> {
    let xdata = &e.common.xdata;
    let at = xdata.iter().position(|t| t.code == 1001 && t.value == NOTE_APP)?;
    let rest: Vec<&RawTag> = xdata.get(at + 1..)?.iter().take_while(|t| t.code != 1001).collect();
    let text = |i: usize, code: i32| rest.get(i).filter(|t| t.code == code).map(|t| t.value.as_str());
    let kind = NoteKind::parse(text(0, 1000)?)?;
    let partner = text(1, 1000).filter(|s| !s.is_empty()).and_then(Handle::parse_hex);
    let attach = match (text(2, 1000).and_then(|s| s.parse().ok()), text(3, 1000).and_then(Handle::parse_hex), text(4, 1040)) {
        (Some(view), Some(handle), Some(at)) => at.parse().ok().filter(|a: &f64| a.is_finite()).map(|at| Attach { view, handle, at }),
        _ => None,
    };
    let edge = rest.get(5..).and_then(super::mapper::joined).unwrap_or_default();
    Some(Note { kind, partner, attach, edge })
}

/// The MText of the leader note `leader`.
// The tests read notes through it; D-2's note editing will too.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn text_of(m: &DrawingModel, leader: Handle) -> Option<Handle> {
    let note = note_of(m.paper.entity(leader)?)?;
    matches!(note.kind, NoteKind::Leader | NoteKind::Hole).then_some(note.partner?)
}

/// The leader of the note `h` (a leader, or a leader note's text), with its note data.
pub(crate) fn leader_of(paper: &Drawing, h: Handle) -> Option<(Handle, Note)> {
    let note = note_of(paper.entity(h)?)?;
    match note.kind {
        NoteKind::Leader | NoteKind::Hole => Some((h, note)),
        NoteKind::Label => {
            let leader = note.partner?;
            Some((leader, note_of(paper.entity(leader)?)?))
        }
        NoteKind::Text => None,
    }
}

/// `text` as MTEXT contents: backslashes and braces escaped, a new line as `\P`.
pub(crate) fn contents(text: &str) -> String {
    text.replace('\\', "\\\\").replace('{', "\\{").replace('}', "\\}").replace("\r\n", "\\P").replace('\n', "\\P")
}

fn v3(p: Vec2) -> Vec3 {
    Vec3::new(p.x, p.y, 0.0)
}

/// An MText on the template's text style; `attach` 1-9 (TL ... BR).
pub(crate) fn mtext(insert: Vec2, height: f64, width: f64, attach: u8, contents: String) -> MText {
    MText {
        insert: v3(insert),
        height,
        width,
        attach,
        rotation: 0.0,
        style: TEXT_STYLE.into(),
        contents,
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    }
}

fn on_note(note: &Note) -> Common {
    Common { layer: NOTE_LAYER.into(), xdata: note.tags(), ..Common::default() }
}

fn added(r: std::result::Result<Handle, impl std::fmt::Display>) -> Result<Handle> {
    r.map_err(|e| EngineError::Other(e.to_string()))
}

/// Adds a text note to `layout`.
pub(crate) fn add_text(paper: &mut Drawing, layout: &str, text: MText) -> Result<Handle> {
    let note = Note { kind: NoteKind::Text, partner: None, attach: None, edge: String::new() };
    added(paper.add(&Space::Paper(layout.into()), on_note(&note), EntityKind::MText(text)))
}

/// Adds a leader note to `layout`: a Leader through `points` (the first the arrow end, at least
/// two) and its MText at the last point, left or right of it by the last segment's direction.
/// Returns (leader, text).
pub(crate) fn add_leader(
    paper: &mut Drawing,
    layout: &str,
    points: &[Vec2],
    text: String,
    kind: NoteKind,
    attach: Option<Attach>,
    edge: &str,
) -> Result<(Handle, Handle)> {
    let (Some(last), Some(before)) = (points.last().copied(), points.len().checked_sub(2).and_then(|i| points.get(i)).copied()) else {
        return Err(EngineError::Other("a leader needs at least 2 points".into()));
    };
    let space = Space::Paper(layout.into());
    let leader = Leader { vertices: points.iter().map(|p| v3(*p)).collect(), arrow: true, spline: false, style: DIM_STYLE.into() };
    let note = Note { kind, partner: None, attach, edge: edge.to_string() };
    let lh = added(paper.add(&space, on_note(&note), EntityKind::Leader(leader)))?;
    // Middle left of the text when the leader comes in from the left, middle right otherwise.
    let side = if last.x >= before.x { 4 } else { 6 };
    let label = Note { kind: NoteKind::Label, partner: Some(lh), attach: None, edge: String::new() };
    let th = added(paper.add(&space, on_note(&label), EntityKind::MText(mtext(last, NOTE_HEIGHT_IN, 0.0, side, text))))?;
    let tags = Note { partner: Some(th), ..note }.tags();
    paper.modify_entity(lh, |e| e.common.xdata = tags).map_err(|e| EngineError::Other(e.to_string()))?;
    Ok((lh, th))
}

/// The nearest point to `p` on a view curve, and where it is on it (see the module comment).
pub(crate) fn nearest_on(e: &Entity, p: Vec2) -> Option<(Vec2, f64)> {
    match &e.kind {
        EntityKind::Line(l) => {
            let (a, b) = (l.a.xy(), l.b.xy());
            let d = b - a;
            let len2 = d.dot(d);
            let t = if len2 > 0.0 { ((p - a).dot(d) / len2).clamp(0.0, 1.0) } else { 0.0 };
            Some((a + d * t, t))
        }
        EntityKind::Circle(c) => {
            let angle = c.center.xy().angle_to(p);
            Some((point_on(e, angle)?, angle))
        }
        EntityKind::Arc(a) => {
            let c = a.center.xy();
            let sweep = (a.end - a.start).rem_euclid(TAU);
            let sweep = if sweep == 0.0 { TAU } else { sweep };
            let off = (c.angle_to(p) - a.start).rem_euclid(TAU);
            // Off the arc: its nearer end.
            let off = if off <= sweep {
                off
            } else if off - sweep < TAU - off {
                sweep
            } else {
                0.0
            };
            let angle = a.start + off;
            Some((point_on(e, angle)?, angle))
        }
        EntityKind::LwPolyline(pl) => {
            let pts: Vec<Vec2> = pl.vertices.iter().map(|v| v.p).collect();
            let total: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
            let (mut best, mut walked) = (None::<(f64, Vec2, f64)>, 0.0);
            for w in pts.windows(2) {
                let (a, b) = (w[0], w[1]);
                let d = b - a;
                let len = d.len();
                let t = if len > 0.0 { ((p - a).dot(d) / (len * len)).clamp(0.0, 1.0) } else { 0.0 };
                let q = a + d * t;
                if best.is_none_or(|(bd, ..)| q.dist(p) < bd) {
                    best = Some((q.dist(p), q, if total > 0.0 { (walked + t * len) / total } else { 0.0 }));
                }
                walked += len;
            }
            best.map(|(_, q, f)| (q, f))
        }
        _ => None,
    }
}

/// The point `at` on a view curve (see the module comment).
pub(crate) fn point_on(e: &Entity, at: f64) -> Option<Vec2> {
    match &e.kind {
        EntityKind::Line(l) => Some(l.a.xy() + (l.b.xy() - l.a.xy()) * at),
        EntityKind::Circle(c) => Some(c.center.xy() + Vec2::from_angle(at) * c.radius),
        EntityKind::Arc(a) => Some(a.center.xy() + Vec2::from_angle(at) * a.radius),
        EntityKind::LwPolyline(pl) => {
            let pts: Vec<Vec2> = pl.vertices.iter().map(|v| v.p).collect();
            let total: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
            let mut left = at.clamp(0.0, 1.0) * total;
            for w in pts.windows(2) {
                let len = w[0].dist(w[1]);
                if left <= len && len > 0.0 {
                    return Some(w[0] + (w[1] - w[0]) * (left / len));
                }
                left -= len;
            }
            pts.last().copied()
        }
        _ => None,
    }
}

/// Puts the arrow end of every attached leader back on its edge (after a drawing edit). A
/// leader whose edge is gone keeps where it was. Returns how many moved.
pub(crate) fn follow(paper: &mut Drawing) -> usize {
    let mut moves: Vec<(Handle, Vec2)> = Vec::new();
    for store in std::iter::once(&paper.model).chain(paper.layouts.iter().map(|l| &l.entities)) {
        for e in store.iter() {
            let EntityKind::Leader(l) = &e.kind else { continue };
            let Some(a) = note_of(e).and_then(|n| n.attach) else { continue };
            let (Some(target), Some(first)) = (paper.entity(a.handle).and_then(|t| point_on(t, a.at)), l.vertices.first()) else { continue };
            if target.dist(first.xy()) > 1e-12 {
                moves.push((e.handle, target));
            }
        }
    }
    for (h, p) in &moves {
        let _ = paper.modify_entity(*h, |e| {
            if let EntityKind::Leader(l) = &mut e.kind
                && let Some(first) = l.vertices.first_mut()
            {
                *first = v3(*p);
            }
        });
    }
    moves.len()
}

/// The ids of the features whose faces name the model edge `edge`: the `F<id>:` that starts
/// each of its `|`-separated face names (after `/e/`, or `/s/` for a silhouette).
fn feature_ids(edge: &str) -> Vec<u64> {
    let faces = ["/e/", "/s/"].iter().filter_map(|m| edge.find(m).map(|i| &edge[i + m.len()..])).next().unwrap_or(edge);
    faces
        .split('|')
        .filter_map(|f| {
            let rest = f.strip_prefix('F')?;
            let (id, _) = rest.split_once(':')?;
            id.parse().ok()
        })
        .collect()
}

/// The hole note for the hole feature whose face bounds `edge` (spec §2.8): `⌀<diameter>`, then
/// ` THRU` (no depth) or ` ↧<depth>`, then a second line (`\n`) with the thread designation when
/// threaded; numbers by `style` (ANSI_INCH: 2 decimals, no leading zero), in inches. Any other
/// edge: "pick the edge of a hole".
pub(crate) fn hole_note_text(model: &LoadedModel, edge: &str, style: &DimStyle) -> Result<String> {
    let doc = &model.session.doc;
    let not_a_hole = || crate::params::bad(HOLE_CMD, "pick the edge of a hole");
    let hole = feature_ids(edge)
        .into_iter()
        .find_map(|id| doc.features.iter().find(|f| f.id == id && !f.suppressed && matches!(f.kind, FeatureKind::Hole { .. })))
        .ok_or_else(not_a_hole)?;
    let FeatureKind::Hole { diameter, depth, thread, to, .. } = &hole.kind else { return Err(not_a_hole()) };
    let inches = |e: &str| -> Result<String> {
        let mm = doc.eval(e, Kind::Length).map_err(|e| EngineError::Other(format!("{HOLE_CMD}: {}: {e}", hole.name)))?;
        Ok(cadcraft_render::format_linear_value(mm / 25.4, style))
    };
    let mut text = format!("⌀{}", inches(diameter)?);
    match (depth, to) {
        (Some(d), None) => text += &format!(" ↧{}", inches(d)?),
        (None, None) => text += " THRU",
        // To Object: its depth is where the face is, not a parameter.
        (_, Some(_)) => {}
    }
    if let Some(t) = thread.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let name = solidcraft_doc::parse_tapped_thread(t).map_or_else(|| t.to_string(), |size| size.designation);
        text += &format!("\n{name}");
    }
    Ok(text)
}

/// The hole notes of view `view`, their texts read again from `model`: (text handle, contents).
/// A note whose hole is gone keeps its text.
pub(crate) fn hole_texts(m: &DrawingModel, view: ViewId, model: &LoadedModel) -> Vec<(Handle, String)> {
    let Some(style) = m.paper.dim_style(DIM_STYLE).cloned() else { return Vec::new() };
    let mut out = Vec::new();
    for store in m.paper.layouts.iter().map(|l| &l.entities) {
        for e in store.iter() {
            let Some(note) = note_of(e).filter(|n| n.kind == NoteKind::Hole && n.attach.as_ref().is_some_and(|a| a.view == view)) else { continue };
            let (Some(text), Ok(value)) = (note.partner, hole_note_text(model, &note.edge, &style)) else { continue };
            out.push((text, contents(&value)));
        }
    }
    out
}

/// Sets the contents of the MTexts `texts` names (where they differ).
pub(crate) fn set_texts(paper: &mut Drawing, texts: &[(Handle, String)]) {
    for (h, value) in texts {
        let same = paper.entity(*h).is_some_and(|e| matches!(&e.kind, EntityKind::MText(t) if &t.contents == value));
        if !same {
            let _ = paper.modify_entity(*h, |e| {
                if let EntityKind::MText(t) = &mut e.kind {
                    t.contents.clone_from(value);
                }
            });
        }
    }
}

/// Removes every leader note attached to one of `views` (with its text). Returns how many.
pub(crate) fn remove_attached(paper: &mut Drawing, views: &BTreeSet<ViewId>) -> usize {
    let mut gone: Vec<Handle> = Vec::new();
    for store in std::iter::once(&paper.model).chain(paper.layouts.iter().map(|l| &l.entities)) {
        for e in store.iter() {
            let Some(note) = note_of(e).filter(|n| n.attach.as_ref().is_some_and(|a| views.contains(&a.view))) else { continue };
            gone.push(e.handle);
            gone.extend(note.partner);
        }
    }
    for h in &gone {
        paper.remove_entity(*h);
    }
    gone.len() / 2
}
