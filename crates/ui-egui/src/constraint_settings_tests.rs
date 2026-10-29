#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! R107: point alignment while placing sketch points, and Constraint Settings' Point alignment.

use serde_json::{Value, json};
use solidcraft_engine::geom::Vec2;

use crate::inference::{self, Kind};
use crate::scenario::Harness;

fn step(h: &mut Harness, value: Value) {
    h.step(&value).unwrap_or_else(|e| panic!("{value}: {e}"));
}

/// A sketch on XY holding one point at (10, 0), seen from the top.
fn fixture(inventor: bool) -> Harness {
    let mut h = if inventor { crate::test_support::inventor(solidcraft_engine::doc::DocumentKind::Part) } else { Harness::new() };
    h.app.preferences.look_at_anim = false;
    inference::clear_scrubbed();
    for (command, params) in [("sketch.create", json!({"plane":"XY"})), ("sketch.point", json!({"point":[10, 0]}))] {
        step(&mut h, json!({"call":"engine.execute","params":{"command":command,"params":params}}));
    }
    step(&mut h, json!({"call":"ui.view","params":{"view":"top"}}));
    step(&mut h, json!({"call":"ui.camera","params":{"target":[20,0,0],"distance":150}}));
    h
}

/// The hovered point of the running tool.
fn hover(h: &mut Harness, at: [f64; 2]) -> Vec2 {
    step(h, json!({"move":{"sketch":at}}));
    h.frames(1);
    h.app.tool.as_ref().and_then(|t| t.hover.as_ref()).map(|p| p.0).expect("a hover point")
}

#[test]
fn point_alignment_is_on_by_default_and_snaps_with_a_dotted_guide_under_both_profiles() {
    for inv in [false, true] {
        let mut h = fixture(inv);
        assert!(crate::constraint_settings::point_alignment(&h.app), "inventor={inv}: on by default");
        step(&mut h, json!({"start":"sketch.point"}));
        let p = hover(&mut h, [30.3, 0.4]);
        assert!(p.y.abs() < 1e-9, "inventor={inv}: snapped onto y = 0, got {p:?}");
        assert!((p.x - 30.3).abs() < 0.2, "inventor={inv}: x follows the cursor, got {p:?}");
        let i = inference::current().expect("an inference");
        assert_eq!(i.kind, Kind::Align);
        assert_eq!(i.guide.0, Vec2::new(10.0, 0.0), "inventor={inv}: the guide starts at the point");
        assert!(crate::scenario::handle_at("inference:align_guide").is_some(), "inventor={inv}: the dotted guide is drawn");
        // The click places the aligned point and adds no constraint.
        let before = h.app.session.world_state().sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch.constraints.len();
        step(&mut h, json!({"click":{"sketch":[30.3, 0.4]}}));
        let st = h.app.session.world_state();
        let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        assert!(sk.points.iter().any(|q| q.pos.y.abs() < 1e-9 && (q.pos.x - 30.3).abs() < 0.2), "{:?}", sk.points);
        assert_eq!(sk.constraints.len(), before, "point alignment adds no constraint");
    }
}

#[test]
fn constraint_settings_turns_point_alignment_off_at_once_per_profile_and_cancel_restores() {
    for inv in [false, true] {
        let mut h = fixture(inv);
        step(&mut h, json!({"start":"sketch.constraint_settings"}));
        h.frames(2);
        assert_eq!(h.app.menu.constraint_settings, Some(true));
        step(&mut h, json!({"click":{"handle":"constraint_settings:point_alignment"}}));
        h.frames(1);
        assert!(!crate::constraint_settings::point_alignment(&h.app), "inventor={inv}: takes effect at once");
        step(&mut h, json!({"click":{"handle":"dialog:Cancel"}}));
        h.frames(1);
        assert!(h.app.menu.constraint_settings.is_none());
        assert!(crate::constraint_settings::point_alignment(&h.app), "inventor={inv}: Cancel restores");
        // Off, through OK: no snap.
        step(&mut h, json!({"start":"sketch.constraint_settings"}));
        h.frames(2);
        step(&mut h, json!({"click":{"handle":"constraint_settings:point_alignment"}}));
        h.frames(1);
        step(&mut h, json!({"click":{"handle":"dialog:OK"}}));
        h.frames(1);
        assert!(!crate::constraint_settings::point_alignment(&h.app));
        let profile = crate::profile::active(&h.app).id.clone();
        assert_eq!(h.app.preferences.point_alignment.get(&profile), Some(&false));
        let saved: Value = serde_json::from_str(&h.app.prefs()).unwrap();
        assert_eq!(saved["preferences"]["point_alignment"][profile.as_str()], json!(false));
        step(&mut h, json!({"start":"sketch.line"}));
        let p = hover(&mut h, [30.3, 0.4]);
        assert!(p.y.abs() > 1e-6, "inventor={inv}: no snap when off, got {p:?}");
        assert!(inference::current().is_none_or(|i| i.kind != Kind::Align));
    }
}

#[test]
fn the_dimension_tool_text_never_aligns() {
    let mut h = fixture(false);
    step(&mut h, json!({"call":"engine.execute","params":{"command":"sketch.line","params":{"points":[[0,20],[40,20]]}}}));
    step(&mut h, json!({"start":"sketch.dimension"}));
    step(&mut h, json!({"move":{"sketch":[30.3, 0.4]}}));
    h.frames(1);
    assert!(inference::current().is_none_or(|i| i.kind != Kind::Align), "{:?}", inference::current());
}

/// The point of the active sketch a paste would go to at sketch position `at`.
fn paste_point(h: &mut Harness, at: [f64; 2]) -> Vec2 {
    let [x, y] = h.at(&json!({"sketch": at})).unwrap();
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    crate::viewport::sketch_point_at(&h.app, &proj, egui::pos2(x as f32, y as f32)).unwrap().0
}

#[test]
fn a_tool_change_forgets_scrubbed_points_and_a_paste_after_a_draw_tool_is_not_aligned() {
    let mut h = fixture(false);
    step(&mut h, json!({"start":"sketch.line"}));
    // Passing over the point scrubs it; aligned while the Line tool runs.
    step(&mut h, json!({"move":{"sketch":[10, 0]}}));
    h.frames(1);
    assert!(!inference::scrubbed().is_empty(), "the point was scrubbed");
    let p = hover(&mut h, [30.3, 0.4]);
    assert!(p.y.abs() < 1e-9, "{p:?}");
    // The tool ends: the scrubbed points go, and a paste location is not aligned.
    step(&mut h, json!({"key":"Escape"}));
    h.frames(2);
    assert!(h.app.tool.is_none(), "the tool ended");
    assert!(h.app.session.active_sketch.is_some(), "still in the sketch");
    assert!(inference::scrubbed().is_empty(), "a tool change forgets the scrubbed points");
    let q = paste_point(&mut h, [30.3, 0.4]);
    assert!(q.y > 0.2, "a paste location is not aligned after the tool ends: {q:?}");
}

#[test]
fn dragging_existing_dimension_text_never_snaps() {
    for inv in [false, true] {
        let mut h = fixture(inv);
        step(&mut h, json!({"call":"engine.execute","params":{"command":"sketch.line","params":{"points":[[0,20],[40,20]],"infer":false}}}));
        let d =
            h.app.run("sketch.dimension", json!({"entities":["l1"],"value":40,"text_at":[20,28]})).unwrap()["param"].as_str().unwrap().to_string();
        h.settle();
        let from = crate::dim_view::drawn_at(&d).expect("the dimension is drawn");
        // Dropped where a drawn point would align with (10, 0) (y = 0) and off the grid.
        step(&mut h, json!({"drag":[{"dimension": d}, {"sketch":[30.33, 0.47]}]}));
        h.settle();
        let to = crate::dim_view::drawn_at(&d).unwrap();
        assert!(to.dist(Vec2::new(30.33, 0.47)) < 0.05, "inventor={inv}: from {from:?} to {to:?}");
        assert!(inference::current().is_none_or(|i| i.kind != Kind::Align), "inventor={inv}");
    }
}
