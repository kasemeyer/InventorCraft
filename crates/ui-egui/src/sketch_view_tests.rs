#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::DocumentKind::Part;
use solidcraft_engine::render::Camera;

use crate::scenario::Harness;
use crate::test_support::inventor;

/// Inventor's Part template with view changes landing at once.
fn inv() -> Harness {
    let mut h = inventor(Part);
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;
    h
}

/// The upstream profile with view changes landing at once.
fn sc() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;
    h
}

/// Home (iso), settled.
fn iso(h: &mut Harness) -> Camera {
    h.app.animate_view("home");
    h.frames(2);
    h.app.cam
}

fn plane(name: &str) -> Sel {
    Sel::Plane { name: name.into() }
}

/// A finished 20 × 10 rectangle sketch on XY, extruded 10 mm: (sketch id, extrude id).
fn extruded_rectangle(h: &mut Harness) -> (u64, u64) {
    let sketch = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let extrude = h.app.run("solid.extrude", json!({"distance":10})).unwrap()["feature"].as_u64().unwrap();
    h.frames(2);
    (sketch, extrude)
}

#[test]
fn a_sketch_from_the_control_channel_is_looked_at_under_inventor() {
    let mut h = inv();
    let c0 = iso(&mut h);
    assert!(c0.back().y.abs() < 0.99, "iso is not a side view: {:?}", c0.back());
    assert_eq!(h.call("engine.execute", json!({"command":"sketch.create","params":{"plane":"XZ"}}))["ok"], json!(true));
    h.frames(2);
    assert!(h.app.session.active_sketch.is_some());
    assert!(h.app.cam.back().y.abs() > 0.999, "facing the XZ sketch: {:?}", h.app.cam.back());
}

#[test]
fn finish_sketch_returns_to_the_view_before_it() {
    let mut h = inv();
    let c0 = iso(&mut h);
    crate::dialogs::start_sketch(&mut h.app, &plane("XZ")).unwrap();
    h.frames(2);
    assert!(h.app.cam != c0, "the sketch was looked at");
    h.app.finish_sketch();
    h.frames(2);
    assert!(h.app.session.active_sketch.is_none());
    assert!(h.app.cam == c0, "back at iso: {:?} vs {:?}", h.app.cam, c0);
}

#[test]
fn editing_a_sketch_returns_on_finish_too() {
    let mut h = inv();
    let (sketch, _) = extruded_rectangle(&mut h);
    let c0 = iso(&mut h);
    h.app.edit_sketch(sketch);
    h.frames(2);
    assert_eq!(h.app.session.active_sketch, Some(sketch));
    assert!(h.app.cam != c0, "the edited sketch was looked at");
    assert!(h.app.cam.back().z.abs() > 0.999, "facing XY: {:?}", h.app.cam.back());
    h.app.finish_sketch();
    h.frames(2);
    assert!(h.app.cam == c0, "back at iso: {:?} vs {:?}", h.app.cam, c0);
}

#[test]
fn extrude_from_inside_a_sketch_still_restores_the_view() {
    let mut h = inv();
    let c0 = iso(&mut h);
    crate::dialogs::start_sketch(&mut h.app, &plane("XY")).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]})).unwrap();
    h.frames(2);
    assert!(h.app.cam != c0);
    // Extrude opens over the open sketch; its OK (asked for as the marking menu does) makes the
    // feature, which leaves the sketch without Finish Sketch.
    h.app.start("solid.extrude");
    h.frames(3);
    assert!(h.app.session.active_sketch.is_some(), "the panel opens over the sketch");
    h.app.dialog.as_mut().expect("the Extrude panel").requested = Some(crate::dialogs::PanelAction::Ok);
    h.frames(3);
    assert!(h.app.dialog.is_none(), "OK applied the panel");
    assert_eq!(h.app.session.world_state().bodies.len(), 1, "the extrusion was made");
    assert!(h.app.session.active_sketch.is_none(), "Extrude finished the sketch");
    assert!(h.app.cam == c0, "back at iso: {:?} vs {:?}", h.app.cam, c0);
}

#[test]
fn solidcraft_keeps_the_camera_after_finish() {
    let mut h = sc();
    iso(&mut h);
    crate::dialogs::start_sketch(&mut h.app, &plane("XZ")).unwrap();
    h.frames(2);
    h.app.finish_sketch();
    h.frames(2);
    assert!(h.app.cam.back().y.abs() > 0.999, "still facing XZ: {:?}", h.app.cam.back());
    let before = h.app.cam;
    assert_eq!(h.call("engine.execute", json!({"command":"sketch.create","params":{"plane":"XY"}}))["ok"], json!(true));
    h.frames(2);
    assert!(h.app.session.active_sketch.is_some());
    assert!(h.app.cam == before, "SolidCraft does not look at a control-channel sketch");
}

#[test]
fn the_sketch_palette_is_hidden_under_inventor() {
    let mut h = inv();
    crate::dialogs::start_sketch(&mut h.app, &plane("XY")).unwrap();
    h.app.tool = None;
    h.frames(2);
    assert!(h.app.session.active_sketch.is_some());
    assert!(h.app.viewport.palette_rect.is_none(), "no Sketch Palette under Inventor");
    let mut h = sc();
    crate::dialogs::start_sketch(&mut h.app, &plane("XY")).unwrap();
    h.app.tool = None;
    h.frames(2);
    assert!(h.app.viewport.palette_rect.is_some(), "SolidCraft keeps its Sketch Palette");
}

#[test]
fn two_quick_escapes_keep_the_sketch_open_under_inventor() {
    let mut h = inv();
    crate::dialogs::start_sketch(&mut h.app, &plane("XY")).unwrap();
    h.app.tool = None;
    h.frames(2);
    let sketch = h.app.session.active_sketch;
    assert!(sketch.is_some());
    h.step(&json!({"key":"Escape"})).unwrap();
    h.step(&json!({"key":"Escape"})).unwrap();
    assert_eq!(h.app.session.active_sketch, sketch, "double Esc does not finish an Inventor sketch");
}

#[test]
fn editing_a_consumed_sketch_hides_its_extrusion() {
    let mut h = inv();
    let (sketch, _) = extruded_rectangle(&mut h);
    assert_eq!(h.app.session.world_state().bodies.len(), 1);
    h.app.edit_sketch(sketch);
    h.frames(2);
    assert!(h.app.session.world_state().bodies.is_empty(), "the extrusion is rolled back while editing");
    h.app.finish_sketch();
    h.frames(2);
    assert_eq!(h.app.session.world_state().bodies.len(), 1, "and comes back on finish");
}

#[test]
fn editing_a_feature_rolls_back_later_features() {
    let mut h = inv();
    let (_, first) = extruded_rectangle(&mut h);
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[40,0],"p1":[60,10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.extrude", json!({"distance":5})).unwrap();
    let index = h.app.session.doc.feature_index(first).unwrap();
    h.app.edit_feature(first);
    assert_eq!(h.app.session.doc.marker, Some(index), "rolled back to just before the first extrude");
}

#[test]
fn switching_tabs_out_of_a_sketch_does_not_restore_its_view() {
    let mut h = inv();
    // Top, not iso: a new tab starts at iso, which would hide a view carried over to it.
    h.app.animate_view("top");
    h.frames(2);
    let c0 = h.app.cam;
    crate::dialogs::start_sketch(&mut h.app, &plane("XZ")).unwrap();
    h.frames(2);
    crate::documents::new_design(&mut h.app);
    assert_eq!(h.app.docs.active, 1, "a second tab");
    let fresh = h.app.cam;
    h.frames(2);
    assert!(h.app.cam == fresh, "the new tab keeps its own camera: {:?} vs {:?}", h.app.cam, fresh);
    crate::documents::switch(&mut h.app, 0);
    h.frames(2);
    assert!(h.app.session.active_sketch.is_some(), "the first tab is still sketching");
    assert!(h.app.cam.back().y.abs() > 0.999, "and still faces its sketch: {:?}", h.app.cam.back());
    h.app.finish_sketch();
    h.frames(2);
    assert!(h.app.cam == c0, "its finish still returns to the view from before it: {:?} vs {:?}", h.app.cam, c0);
}
