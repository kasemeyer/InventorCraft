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
