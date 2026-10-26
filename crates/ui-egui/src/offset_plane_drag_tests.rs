#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Offset Plane pulled by dragging (Plan B-2 addendum, owner-reported): under a profile with
//! `workflow.plane_drag` the arrow and the preview plane itself drag the Distance along the
//! picked face's normal, written in the design unit; SolidCraft keeps its behaviour.

use serde_json::json;
use solidcraft_engine::doc::DocumentKind::Part;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::{FeatureKind, PlaneRef};
use solidcraft_engine::geom::Vec3;

use crate::dialogs::Kind;
use crate::scenario::{Harness, handle_at};
use crate::test_support::inventor;

/// The point picked on the box's top face, and that face's outward normal.
const BASE: [f64; 3] = [25.4, 25.4, 12.7];
const MM_PER_IN: f64 = 25.4;

/// An inch Part (Inventor's template, or SolidCraft with its units set to inches) holding a
/// 2 x 2 x 0.5 in box (bare numbers are inches here) seen from iso, close up on the pick, with Offset Plane open on the box's top face.
fn offset_plane_on_top_face(inventor_profile: bool) -> Harness {
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
        {"call": "ui.view", "params": {"view": "iso"}},
        {"call": "ui.camera", "params": {"target": BASE, "distance": 250}},
        {"start": "construct.plane.offset"},
        {"click": {"world": BASE}}
    ]))
    .unwrap();
    h.settle();
    let d = h.app.dialog.as_ref().expect("the Offset Plane dialog");
    assert!(matches!(d.kind, Kind::OffsetPlane { .. }), "{:?}", d.kind);
    assert!(
        matches!(d.inputs.first().and_then(|i| i.items.first()), Some(solidcraft_engine::Sel::Face { .. })),
        "the top face is picked: {:?}",
        d.inputs
    );
    h
}

fn offset(h: &Harness) -> String {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::OffsetPlane { offset }) => offset.clone(),
        other => panic!("no Offset Plane dialog: {other:?}"),
    }
}

/// The dialog's picked plane input, as text.
fn picked(h: &Harness) -> String {
    format!("{:?}", h.app.dialog.as_ref().expect("the Offset Plane dialog").inputs.first().map(|i| &i.items))
}

fn screen(h: &mut Harness, p: Vec3) -> egui::Pos2 {
    let r = h.call("ui.worldToScreen", json!({"points": [[p.x, p.y, p.z]]}))["result"][0].clone();
    egui::pos2(r[0].as_f64().unwrap() as f32, r[1].as_f64().unwrap() as f32)
}

fn base() -> Vec3 {
    Vec3::new(BASE[0], BASE[1], BASE[2])
}

/// `inches` snapped as a drag snaps it in an inch design at this zoom (a round step of inches).
fn snapped_in(h: &Harness, inches: f64) -> f64 {
    let step = crate::canvas::snap_step(h.app.cam.half_height() / MM_PER_IN);
    (inches / step).round() * step
}

/// "0.7 in": three decimals at most, trailing zeros dropped (as the arrow writes millimetres).
fn inch_text(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{} in", if s == "-0" { "0" } else { s })
}

fn drag(h: &mut Harness, from: egui::Pos2, to: egui::Pos2) {
    assert!(!crate::shell::covered(&h.app, from), "the press point {from:?} is in the open viewport");
    assert!(!crate::shell::covered(&h.app, to), "the release point {to:?} is in the open viewport");
    h.call("ui.drag", json!({"x0": from.x, "y0": from.y, "x1": to.x, "y1": to.y, "steps": 8}));
    h.settle();
}

/// Drag the arrow's tip to where the top face's normal reaches `inches` above the pick.
fn drag_arrow_to(h: &mut Harness, inches: f64) {
    let tip = handle_at("arrow").expect("Offset Plane draws its distance arrow");
    let to = screen(h, base() + Vec3::new(0.0, 0.0, inches * MM_PER_IN));
    drag(h, tip, to);
}

/// A point on the preview plane (offset `inches_now`) clear of the arrow: towards an edge of the
/// preview square (whose half side is a quarter of the view's half height) along X, which iso
/// shows off to the side of the vertical arrow.
fn on_the_plane(h: &Harness, inches_now: f64) -> Vec3 {
    let r = h.app.cam.half_height() * 0.25 * 0.6;
    base() + Vec3::new(r, 0.0, inches_now * MM_PER_IN)
}

#[test]
fn dragging_the_arrow_writes_the_signed_distance_in_inches_and_moves_the_preview() {
    let mut h = offset_plane_on_top_face(true);
    assert_eq!(offset(&h), "0 mm", "Offset Plane opens at zero");
    drag_arrow_to(&mut h, 0.737);
    let want = snapped_in(&h, 0.737);
    assert!(want > 0.0, "the drag was upward: {want}");
    let text = offset(&h);
    assert_eq!(text, inch_text(want), "the Distance shows the dragged value in the design unit");
    let got = h.app.session.doc.eval(&text, ValueKind::Length).unwrap() / MM_PER_IN;
    assert!((got - want).abs() < 1e-3, "{got} in vs {want} in");
    // The preview plane sits at the dragged distance along the face's normal.
    let plane = handle_at("offset_plane").expect("the draggable preview plane is published");
    let expected = screen(&mut h, base() + Vec3::new(0.0, 0.0, want * MM_PER_IN));
    assert!(plane.distance(expected) < 0.5, "preview plane {plane:?} vs {expected:?}");
}

#[test]
fn dragging_the_plane_itself_to_the_other_side_flips_the_sign() {
    let mut h = offset_plane_on_top_face(true);
    let press = on_the_plane(&h, 0.0);
    let from = screen(&mut h, press);
    let tip = handle_at("arrow").unwrap();
    assert!(from.distance(tip) > 20.0, "the press is on the plane, away from the arrow's tip");
    let to = screen(&mut h, press + Vec3::new(0.0, 0.0, -0.61 * MM_PER_IN));
    drag(&mut h, from, to);
    let want = snapped_in(&h, -0.61);
    assert!(want < 0.0, "below the face: {want}");
    assert_eq!(offset(&h), inch_text(want), "pressing on the preview plane drags it, signed");
    let plane = handle_at("offset_plane").unwrap();
    let expected = screen(&mut h, base() + Vec3::new(0.0, 0.0, want * MM_PER_IN));
    assert!(plane.distance(expected) < 0.5, "the preview plane followed the drag: {plane:?} vs {expected:?}");
    // And back up past the face with the arrow: positive again.
    drag_arrow_to(&mut h, 0.4);
    assert_eq!(offset(&h), inch_text(snapped_in(&h, 0.4)));
}

#[test]
fn typing_after_a_drag_replaces_the_dragged_value() {
    let mut h = offset_plane_on_top_face(true);
    drag_arrow_to(&mut h, 0.737);
    assert_eq!(offset(&h), inch_text(snapped_in(&h, 0.737)));
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": "0.25 in"})).unwrap();
    h.frames(5);
    assert_eq!(offset(&h), "0.25 in", "the typed value wins and the drag does not re-assert");
    let plane = handle_at("offset_plane").unwrap();
    let expected = screen(&mut h, base() + Vec3::new(0.0, 0.0, 0.25 * MM_PER_IN));
    assert!(plane.distance(expected) < 0.5, "the preview follows the typed value: {plane:?} vs {expected:?}");
}

#[test]
fn enter_creates_the_plane_at_the_dragged_offset() {
    let mut h = offset_plane_on_top_face(true);
    // Pulled by the preview plane itself (the grip), not the arrow.
    let press = on_the_plane(&h, 0.0);
    let from = screen(&mut h, press);
    let tip = handle_at("arrow").unwrap();
    assert!(from.distance(tip) > 20.0, "the press is on the plane, away from the arrow's tip");
    let to = screen(&mut h, press + Vec3::new(0.0, 0.0, 0.737 * MM_PER_IN));
    drag(&mut h, from, to);
    let want = inch_text(snapped_in(&h, 0.737));
    assert_eq!(offset(&h), want, "the plane drag wrote the distance");
    h.step(&json!({"key": "Enter"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "Enter is OK");
    let planes: Vec<String> = h
        .app
        .session
        .doc
        .features
        .iter()
        .filter_map(|f| match &f.kind {
            FeatureKind::ConstructionPlane { plane: PlaneRef::Offset { distance, .. } } => Some(distance.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(planes, [want], "one offset plane, at the dragged distance");
}

#[test]
fn solidcraft_keeps_its_arrow_in_millimetres_and_the_plane_does_not_drag() {
    let mut h = offset_plane_on_top_face(false);
    assert!(handle_at("offset_plane").is_none(), "no draggable preview plane without plane_drag");
    // Upstream's arrow: it snaps and writes millimetres, whatever the design unit.
    drag_arrow_to(&mut h, 0.737);
    let step = crate::canvas::snap_step(h.app.cam.half_height());
    let mm = (0.737 * MM_PER_IN / step).round() * step;
    let shown = crate::canvas::format_len(mm);
    assert!(shown.ends_with(" mm"), "{shown}");
    assert_eq!(offset(&h), shown);
    // A press on the preview plane is not a drag handle: the distance stays.
    let press = on_the_plane(&h, mm / MM_PER_IN);
    let from = screen(&mut h, press);
    let to = screen(&mut h, press + Vec3::new(0.0, 0.0, -0.61 * MM_PER_IN));
    h.call("ui.drag", json!({"x0": from.x, "y0": from.y, "x1": to.x, "y1": to.y, "steps": 8}));
    h.settle();
    assert_eq!(offset(&h), shown, "the plane does not pull under SolidCraft");
}

#[test]
fn pulling_the_plane_never_moves_the_camera() {
    let mut h = offset_plane_on_top_face(true);
    let cam = h.app.cam;
    let pick = picked(&h);
    let press = on_the_plane(&h, 0.0);
    let from = screen(&mut h, press);
    let to = screen(&mut h, press + Vec3::new(0.0, 0.0, 0.5 * MM_PER_IN));
    drag(&mut h, from, to);
    assert_ne!(offset(&h), "0 mm", "the plane was pulled");
    assert_eq!(h.app.cam, cam, "the plane drag leaves the view where it was");
    drag_arrow_to(&mut h, -0.3);
    assert_eq!(offset(&h), inch_text(snapped_in(&h, -0.3)));
    assert_eq!(h.app.cam, cam, "the arrow drag leaves the view where it was");
    assert_eq!(picked(&h), pick, "the drags did not re-pick the plane's face");
}
