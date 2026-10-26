#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Every drag arrow writes the design unit (Plan B-2 fix wave 4): in an inch Part under a profile
//! with `workflow.plane_drag` (Inventor), pulling Extrude's distance (one side, the second side,
//! symmetric), Hole's depth, a pattern's second spacing or a chamfer's second distance writes
//! inches; SolidCraft's same drag still writes millimetres.

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::DocumentKind::Part;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::geom::Vec3;

use crate::dialogs::Kind;
use crate::scenario::{Harness, handle_at};
use crate::test_support::inventor;

const MM_PER_IN: f64 = 25.4;
/// The centre of the box's top face (a 2 x 2 x 0.5 in box at the origin).
const TOP: [f64; 3] = [25.4, 25.4, 12.7];
/// A point on the box's top front edge (along X).
const FRONT_EDGE: [f64; 3] = [25.4, 0.0, 12.7];

/// An inch Part (Inventor's template, or SolidCraft with its units set to inches) holding a
/// 2 x 2 x 0.5 in box (bare numbers are inches here), seen from iso.
fn inch_box(inventor_profile: bool) -> Harness {
    let mut h = if inventor_profile {
        let mut h = inventor(Part);
        h.app.home.open = false;
        crate::documents::new_document(&mut h.app, Part, Some("in"));
        h
    } else {
        let mut h = Harness::new();
        h.app.run("document.units", json!({"units": "in"})).unwrap();
        h
    };
    h.frames(3);
    assert_eq!(h.app.session.doc.units, "in");
    h.run(&json!([
        {"call": "engine.execute", "params": {"command": "solid.box", "params": {"corner": [0, 0, 0], "length": 2, "width": 2, "height": 0.5}}},
        {"call": "ui.view", "params": {"view": "iso"}}
    ]))
    .unwrap();
    h.settle();
    h
}

/// The top face, picked at its centre.
fn top_face(h: &Harness) -> Sel {
    let st = h.app.session.world_state();
    let body = st.bodies.first().expect("the box");
    let f = body.body.faces(0.001).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99)).expect("a top face");
    Sel::Face { body: body.name.clone(), index: f.index, point: Vec3::new(TOP[0], TOP[1], TOP[2]) }
}

/// Bring the open dialog's arrows close up: the camera on its anchor.
fn close_up(h: &mut Harness) -> (Vec3, Vec3) {
    let (base, n) = super::anchor(&h.app, h.app.dialog.as_ref().expect("a dialog")).expect("an anchor");
    let n = n.expect("an arrow direction");
    h.call("ui.camera", json!({"target": [base.x, base.y, base.z], "distance": 250}));
    h.settle();
    (base, n)
}

/// Extrude open on the top face, its kind adjusted by `set`.
fn extrude(inventor_profile: bool, set: impl FnOnce(&mut Kind)) -> (Harness, Vec3, Vec3) {
    let mut h = inch_box(inventor_profile);
    h.app.session.selection = vec![top_face(&h)];
    h.app.start("solid.extrude");
    let d = h.app.dialog.as_mut().expect("the Extrude dialog");
    assert!(matches!(d.kind, Kind::Extrude { .. }), "{:?}", d.kind);
    set(&mut d.kind);
    h.settle();
    let (base, n) = close_up(&mut h);
    assert!(n.dist(Vec3::Z) < 1e-6, "the top face's arrow points up: {n:?}");
    (h, base, n)
}

fn screen(h: &mut Harness, p: Vec3) -> egui::Pos2 {
    let r = h.call("ui.worldToScreen", json!({"points": [[p.x, p.y, p.z]]}))["result"][0].clone();
    egui::pos2(r[0].as_f64().unwrap() as f32, r[1].as_f64().unwrap() as f32)
}

/// Drag the `handle` arrow's tip to `inches` from `base` along `dir`.
fn drag_tip(h: &mut Harness, handle: &str, base: Vec3, dir: Vec3, inches: f64) {
    let from = handle_at(handle).unwrap_or_else(|| panic!("no {handle} handle"));
    let to = screen(h, base + dir * (inches * MM_PER_IN));
    assert!(!crate::shell::covered(&h.app, from), "the press point {from:?} is in the open viewport");
    assert!(!crate::shell::covered(&h.app, to), "the release point {to:?} is in the open viewport");
    h.call("ui.drag", json!({"x0": from.x, "y0": from.y, "x1": to.x, "y1": to.y, "steps": 8}));
    h.settle();
}

/// "0.7 in": three decimals at most, trailing zeros dropped.
fn inch_text(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{} in", if s == "-0" { "0" } else { s })
}

/// What a drag to `inches` writes, times `scale` (a symmetric extrude's total is twice the half
/// dragged): inches snapped to a round inch step under Inventor, millimetres snapped to a round
/// millimetre step under SolidCraft.
fn expected(h: &Harness, inventor_profile: bool, inches: f64, scale: f64) -> String {
    let hh = h.app.cam.half_height();
    if inventor_profile {
        let step = super::snap_step(hh / MM_PER_IN);
        inch_text((inches / step).round() * step * scale)
    } else {
        let step = super::snap_step(hh);
        super::format_len((inches * MM_PER_IN / step).round() * step * scale)
    }
}

/// The written text reads `want` exactly, and (under Inventor) evaluates to it within 1e-3 in.
fn assert_written(h: &Harness, inventor_profile: bool, field: &str, got: &str, want: &str) {
    assert_eq!(got, want, "{field}");
    let unit = if inventor_profile { " in" } else { " mm" };
    assert!(got.ends_with(unit), "{field} is written in{unit}: {got}");
    if inventor_profile {
        let v = h.app.session.doc.eval(got, ValueKind::Length).unwrap() / MM_PER_IN;
        let w: f64 = want.trim_end_matches(" in").parse().unwrap();
        assert!((v - w).abs() < 1e-3, "{field}: {v} in vs {w} in");
    }
}

fn extrude_fields(h: &Harness) -> (String, String) {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Extrude { distance, distance2, .. }) => (distance.clone(), distance2.clone()),
        other => panic!("no Extrude dialog: {other:?}"),
    }
}

#[test]
fn the_extrude_distance_arrow_writes_the_design_unit() {
    for inv in [true, false] {
        let (mut h, base, n) = extrude(inv, |k| {
            if let Kind::Extrude { direction, distance, .. } = k {
                *direction = 0;
                *distance = "0.25 in".into();
            }
        });
        drag_tip(&mut h, "arrow", base, n, 0.737);
        let want = expected(&h, inv, 0.737, 1.0);
        assert_written(&h, inv, "Distance", &extrude_fields(&h).0, &want);
    }
}

#[test]
fn the_extrude_second_side_arrow_writes_the_design_unit() {
    for inv in [true, false] {
        let (mut h, base, n) = extrude(inv, |k| {
            if let Kind::Extrude { direction, distance, distance2, .. } = k {
                *direction = crate::dialogs::TWO_SIDES;
                *distance = "0.25 in".into();
                *distance2 = "0.25 in".into();
            }
        });
        let first = extrude_fields(&h).0;
        drag_tip(&mut h, "arrow2", base, -n, 0.413);
        let want = expected(&h, inv, 0.413, 1.0);
        let (distance, distance2) = extrude_fields(&h);
        assert_written(&h, inv, "Distance 2", &distance2, &want);
        assert_eq!(distance, first, "the second side's arrow leaves the first distance alone");
    }
}

#[test]
fn the_symmetric_extrude_arrow_writes_the_whole_distance_in_the_design_unit() {
    for inv in [true, false] {
        let (mut h, base, n) = extrude(inv, |k| {
            if let Kind::Extrude { direction, distance, .. } = k {
                *direction = 2;
                *distance = "0.5 in".into();
            }
        });
        // The arrow shows half the total distance; the value written is the whole of it.
        drag_tip(&mut h, "arrow", base, n, 0.413);
        let want = expected(&h, inv, 0.413, 2.0);
        assert_written(&h, inv, "Distance", &extrude_fields(&h).0, &want);
    }
}

#[test]
fn the_hole_depth_arrow_writes_the_design_unit() {
    for inv in [true, false] {
        let mut h = inch_box(inv);
        h.run(&json!([{"start": "solid.hole"}, {"click": {"world": TOP}}])).unwrap();
        h.settle();
        let d = h.app.dialog.as_mut().expect("the Hole dialog");
        let Kind::Hole { depth, opts, .. } = &mut d.kind else { panic!("Hole opened {:?}", d.kind) };
        *depth = "0.25 in".into();
        opts.all = false;
        h.settle();
        let (base, n) = close_up(&mut h);
        assert!(n.dist(Vec3::Z) < 1e-6, "the hole stands on the top face: {n:?}");
        // The depth arrow points into the material.
        drag_tip(&mut h, "arrow2", base, -n, 0.413);
        let want = expected(&h, inv, 0.413, 1.0);
        let Some(Kind::Hole { depth, .. }) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("the Hole dialog closed") };
        assert_written(&h, inv, "Depth", depth, &want);
    }
}

#[test]
fn the_pattern_second_spacing_arrow_writes_the_design_unit() {
    for inv in [true, false] {
        let mut h = inch_box(inv);
        h.run(&json!([
            {"start": "solid.pattern.rectangular"},
            {"click": {"world": TOP}},
            {"call": "ui.dialogInput", "params": {"index": 1}},
            {"click": {"axis": "X"}},
            {"call": "ui.dialogInput", "params": {"index": 2}},
            {"click": {"axis": "Y"}}
        ]))
        .unwrap();
        h.settle();
        let d = h.app.dialog.as_mut().expect("the pattern dialog");
        let Kind::PatternRect { spacing2, .. } = &mut d.kind else { panic!("pattern opened {:?}", d.kind) };
        *spacing2 = "0.5 in".into();
        h.settle();
        let (base, _) = close_up(&mut h);
        let dir2 = super::pattern_dir2(&h.app, h.app.dialog.as_ref().unwrap()).expect("a second direction");
        assert!(dir2.dist(Vec3::Y).min(dir2.dist(-Vec3::Y)) < 1e-6, "the second direction is Y: {dir2:?}");
        drag_tip(&mut h, "arrow2", base, dir2, 1.137);
        let want = expected(&h, inv, 1.137, 1.0);
        let Some(Kind::PatternRect { spacing2, .. }) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("the pattern dialog closed") };
        assert_written(&h, inv, "Spacing 2", spacing2, &want);
    }
}

#[test]
fn the_chamfer_second_distance_arrow_writes_the_design_unit() {
    for inv in [true, false] {
        let mut h = inch_box(inv);
        let p = Vec3::new(FRONT_EDGE[0], FRONT_EDGE[1], FRONT_EDGE[2]);
        let st = h.app.session.world_state();
        let body = st.bodies.first().unwrap();
        let m = body.mesh();
        let index = m.edges.iter().position(|e| e.windows(2).any(|w| p.dist_to_segment(w[0], w[1]) < 1e-9)).expect("the front top edge");
        let edge = Sel::Edge { body: body.name.clone(), index, point: p };
        drop(st);
        h.app.session.selection = vec![edge];
        h.app.start("solid.chamfer");
        let d = h.app.dialog.as_mut().expect("the Chamfer dialog");
        let Kind::Fillet { chamfer: true, ctype, radius, distance2, .. } = &mut d.kind else { panic!("Chamfer opened {:?}", d.kind) };
        *ctype = 1;
        *radius = "0.1 in".into();
        *distance2 = "0.1 in".into();
        h.settle();
        let (base, _) = close_up(&mut h);
        let (_, t2) = super::chamfer_face_dirs(&h.app, h.app.dialog.as_ref().unwrap(), base).expect("two face directions");
        drag_tip(&mut h, "arrow2", base, t2, 0.413);
        let want = expected(&h, inv, 0.413, 1.0);
        let Some(Kind::Fillet { distance2, .. }) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("the Chamfer dialog closed") };
        assert_written(&h, inv, "Distance 2", distance2, &want);
    }
}
