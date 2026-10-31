#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Finish Sketch returns to the feature that was waiting for a profile (R197; the clean-room
//! theme document §Sketch ribbon): Extrude started with no sketch → Start 2D Sketch → Finish →
//! Extrude's panel again, the new sketch's profile picked. A profile switch; SolidCraft keeps
//! its own behaviour (the sketch replaces the panel, and Finish ends there).

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::DocumentKind::Part;

use crate::dialogs::Kind;
use crate::scenario::Harness;
use crate::test_support::inventor;

fn ready(mut h: Harness) -> Harness {
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;
    h
}

/// Extrude with nothing to extrude, then Start 2D Sketch on XY and a 20 x 10 rectangle in it:
/// the new sketch's id.
fn sketch_from_extrude(h: &mut Harness) -> u64 {
    h.app.start("solid.extrude");
    h.frames(2);
    let d = h.app.dialog.as_ref().expect("Extrude opened");
    assert!(matches!(d.kind, Kind::Extrude { .. }) && d.inputs[0].items.is_empty(), "no profile yet");
    h.app.start("sketch.create");
    h.frames(2);
    let d = h.app.dialog.as_mut().expect("Start 2D Sketch asks for a plane");
    assert!(matches!(d.kind, Kind::Sketch));
    d.inputs[0].items.push(Sel::Plane { name: "XY".into() });
    h.frames(2);
    let sketch = h.app.session.active_sketch.expect("the sketch started on XY");
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]})).unwrap();
    h.frames(1);
    sketch
}

#[test]
fn finish_sketch_resumes_the_extrude_that_was_waiting_with_the_new_profile_picked() {
    let mut h = ready(inventor(Part));
    let sketch = sketch_from_extrude(&mut h);
    h.app.finish_sketch();
    h.frames(2);
    assert!(h.app.session.active_sketch.is_none());
    let d = h.app.dialog.as_ref().expect("Extrude's panel is back");
    assert!(matches!(d.kind, Kind::Extrude { .. }), "{:?}", std::mem::discriminant(&d.kind));
    assert!(d.editing.is_none(), "a new extrude");
    let picked = &d.inputs[0].items;
    assert_eq!(picked, &vec![Sel::Profile { sketch, index: 0 }], "the new profile is picked");
    // OK makes the extrude from it.
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(4);
    assert!(h.app.dialog.is_none(), "{:?}", h.app.status);
    assert_eq!(h.app.session.world_state().bodies.len(), 1, "the extrude was made");
}

#[test]
fn cancel_sketch_drops_the_waiting_feature() {
    let mut h = ready(inventor(Part));
    sketch_from_extrude(&mut h);
    h.app.cancel_sketch();
    h.frames(2);
    assert!(h.app.dialog.is_none(), "nothing resumes after Cancel Sketch");
    // A later sketch finishes on its own.
    h.app.run("sketch.create", json!({"plane": "XZ"})).unwrap();
    h.app.finish_sketch();
    h.frames(2);
    assert!(h.app.dialog.is_none());
}

#[test]
fn solidcraft_finish_sketch_does_not_resume_a_feature() {
    let mut h = ready(Harness::new());
    sketch_from_extrude(&mut h);
    h.app.finish_sketch();
    h.frames(2);
    assert!(h.app.session.active_sketch.is_none());
    assert!(h.app.dialog.is_none(), "SolidCraft: Finish ends at the model");
}

#[test]
fn the_switch_is_profile_data() {
    assert!(crate::profile::get("inventor").unwrap().workflow.finish_sketch_resumes_feature);
    assert!(!crate::profile::get("solidcraft").unwrap().workflow.finish_sketch_resumes_feature);
}

/// Hole started with nothing to place it by → Start 2D Sketch → two points → Finish: Hole's
/// panel comes back placed at the sketch's points (Multiple, the existing Points placement), as
/// Inventor's Hole takes a finished sketch's centre points.
#[test]
fn finish_sketch_resumes_hole_with_the_sketch_points_picked() {
    let mut h = ready(inventor(Part));
    h.app.run("solid.box", json!({"length": 40, "width": 40, "height": 20, "corner": [0, 0, -20]})).unwrap();
    h.app.start("solid.hole");
    h.frames(2);
    assert!(h.app.dialog.as_ref().is_some_and(|d| matches!(d.kind, Kind::Hole { .. }) && d.inputs[0].items.is_empty()));
    h.app.start("sketch.create");
    h.frames(2);
    h.app.dialog.as_mut().expect("plane pick").inputs[0].items.push(Sel::Plane { name: "XY".into() });
    h.frames(2);
    let sketch = h.app.session.active_sketch.expect("the sketch started");
    // A circle (its centre is not a hole point when standalone points exist) and two points.
    h.app.run("sketch.circle.center", json!({"center": [30, 30], "radius": 3})).unwrap();
    h.app.run("sketch.point", json!({"point": [10, 10], "id": "a"})).unwrap();
    h.app.run("sketch.point", json!({"point": [20, 10], "id": "b"})).unwrap();
    h.app.finish_sketch();
    h.frames(2);
    let d = h.app.dialog.as_ref().expect("Hole's panel is back");
    let Kind::Hole { opts, .. } = &d.kind else { panic!("not Hole") };
    assert!(opts.multiple, "placed by points");
    assert_eq!(d.inputs[0].label, "Points");
    let want = vec![Sel::SketchPoint { id: format!("{sketch}:a") }, Sel::SketchPoint { id: format!("{sketch}:b") }];
    assert_eq!(d.inputs[0].items, want, "the sketch's points are the placements");
}
