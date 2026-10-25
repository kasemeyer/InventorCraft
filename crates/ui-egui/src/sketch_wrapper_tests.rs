#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::scenario::Harness;
use crate::test_support::inventor;
use serde_json::json;
use solidcraft_engine::doc::DocumentKind::Part;
use solidcraft_engine::find_command;

#[test]
fn sketch_tools_are_enabled_on_the_inventor_sketch_tab() {
    let line = find_command("sketch.line").unwrap();
    let h = inventor(Part);
    assert!(h.app.session.active_sketch.is_none());
    assert!(crate::render_workspace::interactive_command_info(&h.app, line).enabled, "Inventor: a sketch tool starts a sketch");
    let h = Harness::new();
    assert!(!crate::render_workspace::interactive_command_info(&h.app, line).enabled, "SolidCraft: unchanged, greyed outside a sketch");
}

#[test]
fn a_sketch_tool_outside_a_sketch_opens_create_sketch_then_the_tool() {
    let mut h = inventor(Part);
    h.app.start("sketch.line");
    let d = h.app.dialog.as_ref().expect("Create Sketch opens");
    assert!(matches!(d.kind, crate::dialogs::Kind::Sketch));
    assert_eq!(d.then_start, Some("sketch.line"));
}

#[test]
fn finish_sketch_is_never_a_wrapper() {
    let h = inventor(Part);
    assert!(!starts_sketch(&h.app, "sketch.finish"));
    assert!(!starts_sketch(&h.app, "sketch.construction"));
    assert!(starts_sketch(&h.app, "sketch.line"));
}

#[test]
fn the_sketch_tab_is_contextual_only_while_sketching() {
    let mut h = inventor(Part);
    assert!(!crate::toolbar::contextual_tab(&h.app, "SKETCH"));
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    assert!(crate::toolbar::contextual_tab(&h.app, "SKETCH"));
}

/// The registry's SKETCH tab also holds commands that run outside a sketch (Zebra, Insert DXF,
/// Canvas): they start themselves, not a sketch.
#[test]
fn commands_that_run_outside_a_sketch_are_never_wrapped() {
    let mut h = inventor(Part);
    for id in ["inspect.zebra", "sketch.insert_dxf", "canvas.insert"] {
        assert_eq!(find_command(id).unwrap().tab, "SKETCH", "{id}: still a registry SKETCH-tab command");
        assert!(!starts_sketch(&h.app, id), "{id}");
    }
    h.app.start("inspect.zebra");
    assert!(!h.app.dialog.as_ref().is_some_and(|d| matches!(d.kind, crate::dialogs::Kind::Sketch)), "Zebra opens no Create Sketch");
    assert!(h.app.session.active_sketch.is_none());
}

fn running(h: &Harness) -> Option<&str> {
    h.app.tool.as_ref().map(|t| t.cmd)
}

fn remembered(h: &Harness, family_default: &'static str) -> &'static str {
    h.app.preferences.sketch_variants.resolve(family_default)
}

/// Review fix: a variant picked by hand (a drop-down row, a button) outside a sketch is the tool
/// that starts once the plane is picked, and it becomes the family's remembered variant.
#[test]
fn an_explicit_variant_pick_outside_a_sketch_starts_that_variant() {
    let mut h = inventor(Part);
    h.app.preferences.sketch_variants.remember("sketch.rectangle.center");
    h.app.start("sketch.rectangle.two_point");
    assert_eq!(h.app.dialog.as_ref().and_then(|d| d.then_start), Some("sketch.rectangle.two_point"));
    h.step(&json!({"click": {"plane": "XY"}})).unwrap();
    h.settle();
    assert!(h.app.session.active_sketch.is_some());
    assert_eq!(running(&h), Some("sketch.rectangle.two_point"));
    assert_eq!(remembered(&h, "sketch.rectangle.two_point"), "sketch.rectangle.two_point", "the explicit pick is remembered");
}

/// A pinned button outside a sketch starts its exact tool and leaves the family's memory alone,
/// as it does inside a sketch.
#[test]
fn a_pinned_variant_outside_a_sketch_starts_exactly_and_is_not_remembered() {
    let mut h = inventor(Part);
    h.app.preferences.sketch_variants.remember("sketch.rectangle.center");
    h.app.start_pinned("sketch.rectangle.two_point");
    assert_eq!(h.app.dialog.as_ref().and_then(|d| d.then_start), Some("sketch.rectangle.two_point"));
    h.step(&json!({"click": {"plane": "XY"}})).unwrap();
    h.settle();
    assert_eq!(running(&h), Some("sketch.rectangle.two_point"));
    assert_eq!(remembered(&h, "sketch.rectangle.two_point"), "sketch.rectangle.center", "a pinned start remembers nothing");
}

/// The shortcut path still resolves the family's remembered variant.
#[test]
fn a_shortcut_outside_a_sketch_still_starts_the_remembered_variant() {
    let mut h = inventor(Part);
    h.app.preferences.sketch_variants.remember("sketch.rectangle.center");
    let key = effective(&h.app, "sketch.rectangle.two_point").expect("Rectangle has a shortcut");
    press(&mut h.app, &key, false);
    assert_eq!(h.app.dialog.as_ref().and_then(|d| d.then_start), Some("sketch.rectangle.center"));
}
