//! What is under a point of the active sheet (for D-2's selection and the dimension tools), and
//! the curves a view would place (its live preview, not a command).

use std::f64::consts::TAU;

use cadcraft_doc::{Entity, EntityKind};
use serde_json::Value;
use solidcraft_doc::ViewId;
use solidcraft_geom::Vec2;

use super::mapper::{self, Seg, xkey_of};
use super::project::PaperCurve;
use crate::Session;

/// The kind of a view curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveKind {
    Line,
    Arc,
    Circle,
    Poly,
}

/// A thing on a sheet.
#[derive(Clone, Debug, PartialEq)]
pub enum PaperPick {
    /// A view's curve: the model edge it shows, and its paper entity.
    ViewEdge {
        view: ViewId,
        edge: String,
        curve: CurveKind,
        handle: u64,
    },
    Dimension {
        handle: u64,
    },
    /// A note, leader, centre mark or centreline.
    Annotation {
        handle: u64,
    },
    /// A view, picked inside its box away from its curves.
    View {
        view: ViewId,
    },
}

/// The distance on paper from `at` to the segment `a`-`b`.
fn to_segment(at: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let t = if d.len2() > 0.0 { ((at - a).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
    (a + d * t).dist(at)
}

/// The distance on paper from `at` to entity `e` (to its box for kinds other than curves).
pub(crate) fn distance(paper: &cadcraft_doc::Drawing, e: &Entity, at: [f64; 2]) -> f64 {
    let p = Vec2::new(at[0], at[1]);
    let v = |q: cadcraft_doc::geom::Vec3| Vec2::new(q.x, q.y);
    match &e.kind {
        EntityKind::Line(l) => to_segment(p, v(l.a), v(l.b)),
        EntityKind::Circle(c) => (p.dist(v(c.center)) - c.radius).abs(),
        EntityKind::Arc(a) => {
            let c = v(a.center);
            let angle = (p.y - c.y).atan2(p.x - c.x);
            let sweep = (a.end - a.start).rem_euclid(TAU);
            let sweep = if sweep == 0.0 { TAU } else { sweep };
            if (angle - a.start).rem_euclid(TAU) <= sweep {
                (p.dist(c) - a.radius).abs()
            } else {
                let end = |t: f64| c + Vec2::new(t.cos(), t.sin()) * a.radius;
                p.dist(end(a.start)).min(p.dist(end(a.end)))
            }
        }
        EntityKind::LwPolyline(pl) => pl
            .vertices
            .windows(2)
            .filter_map(|w| Some(to_segment(p, Vec2::new(w.first()?.p.x, w.first()?.p.y), Vec2::new(w.get(1)?.p.x, w.get(1)?.p.y))))
            .fold(f64::INFINITY, f64::min),
        _ => {
            let b = cadcraft_doc::entity_bounds(paper, e, 0);
            if b.is_empty() {
                return f64::INFINITY;
            }
            let dx = (b.min.x - p.x).max(p.x - b.max.x).max(0.0);
            let dy = (b.min.y - p.y).max(p.y - b.max.y).max(0.0);
            dx.hypot(dy)
        }
    }
}

fn curve_kind(e: &Entity) -> Option<CurveKind> {
    match e.kind {
        EntityKind::Line(_) => Some(CurveKind::Line),
        EntityKind::Arc(_) => Some(CurveKind::Arc),
        EntityKind::Circle(_) => Some(CurveKind::Circle),
        EntityKind::LwPolyline(_) => Some(CurveKind::Poly),
        _ => None,
    }
}

/// The nearest pickable thing on sheet `s.drawing_sheet` within `tol` paper inches of `at`.
pub fn pick(s: &crate::Session, at: [f64; 2], tol: f64) -> Option<PaperPick> {
    let m = s.doc.drawing.as_deref()?;
    let index = s.drawing_sheet.min(m.sheets.len().checked_sub(1)?);
    let sheet = m.sheets.get(index)?;
    let layout = m.paper.layout(&sheet.layout)?;
    if !(at[0].is_finite() && at[1].is_finite() && tol.is_finite() && tol >= 0.0) {
        return None;
    }
    // A view edge within `tol` wins over anything else: a dimension or a note is measured by
    // its box (text and arrow room), which can cover the edges under it.
    let (mut edge, mut other): (Option<(f64, PaperPick)>, Option<(f64, PaperPick)>) = (None, None);
    for e in layout.entities.iter() {
        if e.handle == sheet.border || e.handle == sheet.title_block || !m.paper.is_visible(e) {
            continue;
        }
        let d = distance(&m.paper, e, at);
        if d > tol {
            continue;
        }
        let handle = e.handle.0;
        let (best, found) = match (xkey_of(e), curve_kind(e)) {
            (Some(k), Some(curve)) if k.seg != Seg::Mark => (&mut edge, PaperPick::ViewEdge { view: k.view, edge: k.edge, curve, handle }),
            (Some(_), _) => (&mut other, PaperPick::Annotation { handle }),
            (None, _) if matches!(e.kind, EntityKind::Dimension(_)) => (&mut other, PaperPick::Dimension { handle }),
            (None, _) => (&mut other, PaperPick::Annotation { handle }),
        };
        if best.as_ref().is_none_or(|(b, _)| d < *b) {
            *best = Some((d, found));
        }
    }
    if let Some((_, found)) = edge.or(other) {
        return Some(found);
    }
    // Inside a view's box (its entities' bounds, grown by `tol`): the view whose box is the
    // smallest (a view drawn over a bigger one stays pickable); equal boxes: the nearest centre.
    let inside = m.views.values().filter(|v| v.sheet == index).filter_map(|v| {
        let group = m.paper.groups.iter().find(|g| g.name == mapper::group_name(v.group))?;
        let mut b = cadcraft_doc::geom::Bounds2::EMPTY;
        for h in &group.members {
            if let Some(e) = m.paper.entity(*h) {
                b = b.union(&cadcraft_doc::entity_bounds(&m.paper, e, 0));
            }
        }
        let inside = !b.is_empty() && at[0] >= b.min.x - tol && at[0] <= b.max.x + tol && at[1] >= b.min.y - tol && at[1] <= b.max.y + tol;
        let area = (b.max.x - b.min.x) * (b.max.y - b.min.y);
        let centre = (v.position.x - at[0]).hypot(v.position.y - at[1]);
        inside.then_some((area, centre, v.id))
    });
    inside.min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1))).map(|(.., view)| PaperPick::View { view })
}

/// What `cmd` (drawing.view.base or drawing.view.projected) with `params` would place, placing nothing.
pub fn preview_view(s: &crate::Session, cmd: &str, params: &Value) -> crate::Result<Vec<(PaperCurve, bool /* hidden */)>> {
    let planned = crate::cmd::drawing_view::plan(s, cmd, params)?;
    Ok(planned.paper_curves())
}

/// The model edge id of view `view`'s curve nearest `at` (any distance), on its sheet.
// The tests' `edge_at` (the drawing fixture) is its only caller.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn nearest_edge(s: &Session, view: ViewId, at: [f64; 2]) -> Option<String> {
    let m = s.doc.drawing.as_deref()?;
    let sheet = m.sheets.get(m.views.get(&view)?.sheet)?;
    let layout = m.paper.layout(&sheet.layout)?;
    layout
        .entities
        .iter()
        .filter_map(|e| xkey_of(e).filter(|k| k.view == view && k.seg != Seg::Mark).map(|k| (distance(&m.paper, e, at), k.edge)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, edge)| edge)
}
