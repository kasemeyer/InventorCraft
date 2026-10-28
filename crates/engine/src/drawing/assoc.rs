//! Associative dimensions on a drawing's paper: definition points attached to view entities
//! (`Dimension::assoc`) follow them. [`update`] runs after every drawing edit
//! (`drawing::edit`), so a regenerated view (its entities changed in place, handles kept, spec
//! §4.5) carries its dimensions along.
//!
//! Ported from CADCraft `crates/engine/src/assoc.rs` at d864a08b013353143086bd8bb568c26cb6256796
//! (MIT OR Apache-2.0; ATTRIBUTION.md): `snap_point`, `radial_assoc`, `recompute` and `update`
//! with the helpers they use. CADCraft's snap search (`find_snap`, `SnapIndex`, `auto_assoc`,
//! `reassociate`) and its post-command hook (`after_command`, hatch refill) are not ported: our
//! dimensions name their entities when they are made (`drawing::dims`). Adaptations are marked
//! `TCI-CAD:`.

use std::sync::Arc;

use cadcraft_doc::{AssocSnap, DimAssoc, DimKind, Dimension, Drawing, EntityKind, Handle};
use cadcraft_geom::{Vec2, Vec3};

/// Definition point names in the order they are tried.
const POINTS: [&str; 5] = ["defpt", "p13", "p14", "p15", "p16"];

/// A definition point of a dimension by name.
pub(crate) fn def_point(d: &Dimension, name: &str) -> Option<Vec3> {
    Some(match name {
        "defpt" => d.defpt,
        "p13" => d.p13,
        "p14" => d.p14,
        "p15" => d.p15,
        "p16" => d.p16,
        "text" => d.text_mid,
        _ => return None,
    })
}

fn def_point_mut<'a>(d: &'a mut Dimension, name: &str) -> Option<&'a mut Vec3> {
    Some(match name {
        "defpt" => &mut d.defpt,
        "p13" => &mut d.p13,
        "p14" => &mut d.p14,
        "p15" => &mut d.p15,
        "p16" => &mut d.p16,
        _ => return None,
    })
}

fn arc_geom(a: &cadcraft_doc::Arc) -> cadcraft_geom::Arc {
    cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end)
}

/// Where a snap sits on an object now.
pub(crate) fn snap_point(d: &Drawing, a: &DimAssoc) -> Option<Vec2> {
    let e = d.entity(a.handle)?;
    match (&e.kind, &a.snap) {
        (EntityKind::Line(l), AssocSnap::Start) => Some(l.a.xy()),
        (EntityKind::Line(l), AssocSnap::End) => Some(l.b.xy()),
        (EntityKind::Line(l), AssocSnap::Mid) => Some(l.a.xy().mid(l.b.xy())),
        (EntityKind::Line(l), AssocSnap::Intersection { other }) => match &d.entity(*other)?.kind {
            EntityKind::Line(m) => cadcraft_geom::line_line_infinite(l.a.xy(), l.b.xy(), m.a.xy(), m.b.xy()).map(|x| x.0),
            _ => None,
        },
        (EntityKind::Arc(ar), AssocSnap::Start) => Some(arc_geom(ar).start_point()),
        (EntityKind::Arc(ar), AssocSnap::End) => Some(arc_geom(ar).end_point()),
        (EntityKind::Arc(ar), AssocSnap::Mid) => Some(arc_geom(ar).mid_point()),
        (EntityKind::Arc(ar), AssocSnap::Center) => Some(ar.center.xy()),
        (EntityKind::Arc(ar), AssocSnap::OnCircle { angle }) => Some(ar.center.xy() + Vec2::from_angle(*angle) * ar.radius),
        (EntityKind::Circle(c), AssocSnap::Center) => Some(c.center.xy()),
        (EntityKind::Circle(c), AssocSnap::OnCircle { angle }) => Some(c.center.xy() + Vec2::from_angle(*angle) * c.radius),
        (EntityKind::LwPolyline(p), AssocSnap::Vertex { index }) => p.vertices.get(*index).map(|v| v.p),
        (EntityKind::LwPolyline(p), AssocSnap::Start) => p.vertices.first().map(|v| v.p),
        (EntityKind::LwPolyline(p), AssocSnap::End) => p.vertices.last().map(|v| v.p),
        (EntityKind::Point(p), AssocSnap::Start | AssocSnap::Center) => Some(p.p.xy()),
        _ => None,
    }
}

fn close(a: Vec2, b: Vec2) -> bool {
    a.dist(b) <= 1e-6 * (1.0 + a.x.abs().max(a.y.abs()))
}

/// A circle or arc (centre, radius) by handle.
pub(crate) fn circle_of(d: &Drawing, h: Handle) -> Option<(Vec2, f64)> {
    match &d.entity(h)?.kind {
        EntityKind::Circle(c) => Some((c.center.xy(), c.radius)),
        EntityKind::Arc(a) => Some((a.center.xy(), a.radius)),
        _ => None,
    }
}

/// Associations for a radius/diameter dimension made by picking a circle or arc.
// TCI-CAD: an arc is measured as its circle (R51: a back view's hole rim may be arcs).
pub(crate) fn radial_assoc(d: &Drawing, h: Handle, diameter: bool, defpt: Vec2, p15: Vec2) -> Vec<DimAssoc> {
    let Some((c, _)) = circle_of(d, h) else { return Vec::new() };
    let on = |name: &str, p: Vec2| DimAssoc { point: name.into(), handle: h, snap: AssocSnap::OnCircle { angle: c.angle_to(p) } };
    if diameter {
        vec![on("defpt", defpt), on("p15", p15)]
    } else {
        vec![DimAssoc { point: "defpt".into(), handle: h, snap: AssocSnap::Center }, on("p15", p15)]
    }
}

fn src_changed(before: &Drawing, after: &Drawing, h: Handle) -> bool {
    match (before.entity(h), after.entity(h)) {
        (Some(x), Some(y)) => !Arc::ptr_eq(x, y) && x.kind != y.kind,
        (Some(_), None) => true,
        _ => false,
    }
}

/// Whether a dimension is sick: one of its attachments names an object that is gone, or one
/// that no longer gives its snap (a regenerated view whose curve changed kind).
// TCI-CAD: ours (spec §4.5 "Orphans"); CADCraft drops such attachments instead.
pub(crate) fn is_sick(d: &Drawing, dm: &Dimension) -> bool {
    dm.assoc.iter().any(|a| snap_point(d, a).is_none())
}

/// Recompute an associative dimension's definition points from its objects. Attachments whose
/// point had been moved off the object before this change are dropped.
/// `prev` is the dimension as it was before the change (when it existed).
pub(crate) fn recompute(dm: &Dimension, prev: Option<&Dimension>, before: &Drawing, after: &Drawing) -> Dimension {
    // TCI-CAD: a dimension whose object is gone (its model edge left the view) or no longer
    // gives its snap keeps its last geometry and text and its dangling attachments, so it is
    // reported sick (spec §4.5 "Orphans") instead of silently losing or changing what it
    // measures. CADCraft drops the attachment and moves the rest.
    if is_sick(after, dm) {
        return dm.clone();
    }
    let mut nd = dm.clone();
    let mut deltas: Vec<Vec2> = Vec::new();
    let mut moved: Vec<String> = Vec::new();
    let was = prev.unwrap_or(dm);
    nd.assoc.retain(|a| {
        let Some(old) = def_point(was, &a.point) else { return false };
        // Only follow if the point sat on the object before the change.
        if let Some(prev) = snap_point(before, a)
            && !close(prev, old.xy())
        {
            return false;
        }
        snap_point(after, a).is_some()
    });
    for a in &nd.assoc {
        if let (Some(p), Some(old)) = (snap_point(after, a), def_point(dm, &a.point)) {
            deltas.push(p - old.xy());
            moved.push(a.point.clone());
        }
    }
    for a in nd.assoc.clone() {
        if let (Some(p), Some(slot)) = (snap_point(after, &a), def_point_mut(&mut nd, &a.point)) {
            slot.x = p.x;
            slot.y = p.y;
        }
    }
    // A rigid move of every attached point carries the rest of the dimension along. The measured
    // origins of a linear dimension are the exception: a free one stays where it was placed.
    let linear = matches!(nd.kind, DimKind::Linear { .. } | DimKind::Aligned);
    if let Some(first) = deltas.first().copied()
        && deltas.iter().all(|d| d.dist(first) <= 1e-9 * (1.0 + first.len()))
        && first.len() > 0.0
    {
        // An ordinate's `defpt` is its fixed datum (the drawing origin); it must not follow the feature.
        let ordinate = matches!(nd.kind, DimKind::Ordinate { .. });
        for name in POINTS {
            if !(moved.iter().any(|m| m == name) || ordinate && name == "defpt")
                && !(linear && (name == "p13" || name == "p14"))
                && let Some(slot) = def_point_mut(&mut nd, name)
            {
                slot.x += first.x;
                slot.y += first.y;
            }
        }
        nd.text_mid.x += first.x;
        nd.text_mid.y += first.y;
    }
    if nd != *dm {
        nd.block = None;
    }
    nd
}

/// After a change: update every associative dimension whose objects changed.
/// Returns the number of dimensions updated.
// TCI-CAD: `after` is our paper, not an `Arc` (R29); the stored measurement (DXF 42) is
// brought up to date with the points.
pub(crate) fn update(after: &mut Drawing, before: &Drawing) -> usize {
    let mut changes: Vec<(Handle, Dimension)> = Vec::new();
    {
        let d: &Drawing = after;
        let stores = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities));
        for store in stores {
            for e in store.iter() {
                let EntityKind::Dimension(dm) = &e.kind else { continue };
                if dm.assoc.is_empty() {
                    continue;
                }
                let touched = dm.assoc.iter().any(|a| {
                    src_changed(before, d, a.handle) || matches!(a.snap, AssocSnap::Intersection { other } if src_changed(before, d, other))
                });
                if !touched {
                    continue;
                }
                let prev = match before.entity(e.handle).map(|x| &x.kind) {
                    Some(EntityKind::Dimension(p)) => Some(p),
                    _ => None,
                };
                let mut nd = recompute(dm, prev, before, d);
                if nd != *dm {
                    nd.measurement = cadcraft_render::dimension_in(d, &nd).measurement;
                    changes.push((e.handle, nd));
                }
            }
        }
    }
    let n = changes.len();
    for (h, nd) in changes {
        let _ = after.modify_entity(h, |e| e.kind = EntityKind::Dimension(nd));
    }
    n
}
