#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Fix wave 2 item 1 (owner-reported): the ribbon keeps the running drawing tool's button pressed,
//! as Inventor keeps the running command's, through one mapping (`command_of`) feeding the
//! ribbon's active check (`toolbar::running_command`), under both profiles.

use serde_json::json;
use solidcraft_engine::doc::DocumentKind::{Drawing, Part};

use super::*;
use crate::drawing_dim_tool::DimTool;
use crate::drawing_notes_tool::NoteTool;
use crate::drawing_place::{Place, Stage};
use crate::scenario::Harness;
use crate::test_support::{block_part, inventor};
use crate::toolbar::running_command;

/// `h` with Home closed and a drawing of the block (no view yet).
fn block_drawing(mut h: Harness) -> Harness {
    h.app.home.open = false;
    let block = block_part(&h.dir);
    h.app.run("drawing.new", json!({"model": block})).unwrap();
    h.frames(2);
    h
}

fn escape(h: &mut Harness) {
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
}

fn start(h: &mut Harness, id: &str) {
    h.app.start(id);
    h.frames(2);
}

#[test]
fn every_tool_names_its_ribbon_command() {
    let note = |t: NoteTool| command_of(&DrawingTool::Note(t));
    assert_eq!(command_of(&DrawingTool::Place(Place::new(Stage::Base))), Some("drawing.view.base"));
    assert_eq!(command_of(&DrawingTool::Place(Place::new(Stage::PickParent))), Some("drawing.view.projected"));
    assert_eq!(command_of(&DrawingTool::Place(Place::new(Stage::Project { parent: 7 }))), Some("drawing.view.projected"));
    assert_eq!(command_of(&DrawingTool::Dimension(DimTool::default())), Some("drawing.dim.general"));
    let mut moving = DimTool::default();
    moving.moving = Some(3);
    assert_eq!(command_of(&DrawingTool::Dimension(moving)), None, "Move Dimension has no button");
    assert_eq!(note(NoteTool::Text { position: None, corner: None }), Some("drawing.note.text"));
    assert_eq!(note(NoteTool::Leader { points: Vec::new(), attach: None, single: false }), Some("drawing.note.leader"));
    assert_eq!(note(NoteTool::CenterMark { view: None }), Some("drawing.centermark"));
    assert_eq!(note(NoteTool::Bisector { first: None }), Some("drawing.centerline.bisector"));
    assert_eq!(note(NoteTool::HoleNote { edge: None }), Some("drawing.note.hole"));
    assert_eq!(note(NoteTool::Move { handle: 3 }), None, "Move Note has no button");
    // Every id named is a drawing start row: the button that started the tool.
    for id in ["drawing.view.base", "drawing.view.projected", "drawing.dim.general", "drawing.note.text", "drawing.note.leader"] {
        assert!(start_for(id).is_some(), "{id}");
    }
}

#[test]
fn a_base_view_placed_keeps_base_view_pressed_while_it_projects() {
    let mut p = Place::new(Stage::Base);
    p.stage = Stage::Project { parent: 7 };
    p.placed.push(7);
    assert_eq!(command_of(&DrawingTool::Place(p)), Some("drawing.view.base"));
}

fn the_running_drawing_tool_is_pressed(mut h: Harness) {
    assert_eq!(running_command(&h.app), None);
    start(&mut h, "drawing.dim.general");
    assert!(matches!(h.app.drawing.tool, Some(DrawingTool::Dimension(_))));
    assert_eq!(running_command(&h.app).as_deref(), Some("drawing.dim.general"));
    escape(&mut h);
    assert_eq!(running_command(&h.app), None);

    start(&mut h, "drawing.view.base");
    assert!(matches!(h.app.drawing.tool, Some(DrawingTool::Place(_))));
    assert_eq!(running_command(&h.app).as_deref(), Some("drawing.view.base"));
    escape(&mut h);
    assert!(h.app.drawing.tool.is_none());
    assert_eq!(running_command(&h.app), None);

    start(&mut h, "drawing.note.text");
    assert_eq!(running_command(&h.app).as_deref(), Some("drawing.note.text"));
    escape(&mut h);
    assert_eq!(running_command(&h.app), None);

    start(&mut h, "drawing.centermark");
    assert_eq!(running_command(&h.app).as_deref(), Some("drawing.centermark"));
    escape(&mut h);
    assert_eq!(running_command(&h.app), None);
}

#[test]
fn the_running_drawing_tool_is_pressed_under_inventor() {
    the_running_drawing_tool_is_pressed(block_drawing(inventor(Drawing)));
}

#[test]
fn the_running_drawing_tool_is_pressed_under_solidcraft() {
    the_running_drawing_tool_is_pressed(block_drawing(Harness::new()));
}

#[test]
fn a_part_tool_is_pressed_as_before() {
    for mut h in [inventor(Part), Harness::new()] {
        h.app.home.open = false;
        h.app.preferences.look_at_anim = false;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.start("sketch.line");
        h.frames(2);
        assert_eq!(running_command(&h.app).as_deref(), Some("sketch.line"));
        assert!(h.app.drawing.tool.is_none());
        escape(&mut h);
        assert_eq!(running_command(&h.app), None);
    }
}
