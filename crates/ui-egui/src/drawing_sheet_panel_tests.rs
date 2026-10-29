#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Edit Sheet (wave 3 item 2, R99/R104): opened from the browser's sheet row, pre-filled from
//! the sheet; OK applies the name and the size/orientation; Esc closes and sends nothing.

use cadcraft_doc::{EntityKind, Handle};
use serde_json::json;
use solidcraft_engine::doc::DocumentKind::Drawing;
use solidcraft_engine::doc::drawing::{AnsiSize, DrawingModel};

use super::*;
use crate::context_menu::{Target, run_item};
use crate::drawing_ui::DrawingPanel;
use crate::scenario::Harness;
use crate::test_support::inventor;

fn drawing(mut h: Harness) -> Harness {
    h.app.home.open = false;
    h.app.run("drawing.new", json!({})).unwrap();
    h.frames(2);
    h
}

fn model(h: &Harness) -> &DrawingModel {
    h.app.session.doc.drawing.as_deref().unwrap()
}

fn panel(h: &Harness) -> &SheetPanel {
    match &h.app.drawing.panel {
        Some(DrawingPanel::Sheet(p)) => p,
        _ => panic!("Edit Sheet is not open"),
    }
}

fn panel_mut(h: &mut Harness) -> &mut SheetPanel {
    match &mut h.app.drawing.panel {
        Some(DrawingPanel::Sheet(p)) => p,
        _ => panic!("Edit Sheet is not open"),
    }
}

/// The block and insertion point of an Insert on the paper.
fn insert_of(m: &DrawingModel, h: Handle) -> (String, [f64; 2]) {
    let EntityKind::Insert(ins) = &m.paper.entity(h).expect("an insert").kind else { panic!("not an Insert") };
    (ins.block.clone(), [ins.insert.x, ins.insert.y])
}

/// The browser's sheet row menu's Edit Sheet, run on sheet 0.
fn edit_sheet_from_the_browser(h: &mut Harness) {
    let menu = crate::context_menu::items(&h.app, &Target::Sheet { index: 0 });
    let row = menu.iter().find(|i| i.id == "ui.drawing.editSheet").expect("Edit Sheet on the sheet's menu");
    assert!(row.enabled);
    run_item(&mut h.app, row, egui::Pos2::ZERO);
    h.frames(2);
}

/// D landscape → B portrait from the browser row: one `drawing.sheet.set_size`, one undo step;
/// the border becomes B's portrait border and the title block is re-placed at the new
/// bottom-right corner inside the border (B portrait is 11 × 17 in: 11 − 0.5, 0.5).
fn size_and_orientation(h: Harness) {
    let mut h = drawing(h);
    edit_sheet_from_the_browser(&mut h);
    let p = panel(&h);
    assert_eq!((p.sheet, p.name.as_str(), p.size, p.landscape), (0, "Sheet", AnsiSize::D, true));
    let undo = h.app.session.undo.len();
    panel_mut(&mut h).size = AnsiSize::B;
    panel_mut(&mut h).landscape = false;
    crate::drawing_ui::panel_ok(&mut h.app);
    assert!(h.app.drawing.panel.is_none());
    assert!(h.app.drawing.fit_pending, "the canvas fits the new sheet");
    h.frames(2);
    let sheet = &model(&h).sheets[0];
    assert_eq!((sheet.size, sheet.landscape, sheet.name.as_str()), (AnsiSize::B, false, "Sheet"));
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    let (border, _) = insert_of(model(&h), sheet.border);
    assert_eq!(border, "TCI_BORDER_B_PORTRAIT");
    let (_, at) = insert_of(model(&h), sheet.title_block);
    assert!((at[0] - 10.5).abs() < 1e-9 && (at[1] - 0.5).abs() < 1e-9, "{at:?}");
}

#[test]
fn edit_sheet_sets_size_and_orientation_under_inventor() {
    size_and_orientation(inventor(Drawing));
}

#[test]
fn edit_sheet_sets_size_and_orientation_under_solidcraft() {
    size_and_orientation(Harness::new());
}

/// The name: one `drawing.sheet.set_name` step; the browser row reads `<name>:1`. With the size
/// changed too, OK runs ONE `drawing.sheet.set_size` carrying the name: one undo step, and one
/// undo puts both back. A successful OK refreshes the panel's starting point, so OK again on the
/// same panel sends nothing.
/// An empty name keeps OK off; Esc sends nothing.
fn name(h: Harness) {
    let mut h = drawing(h);
    edit_sheet_from_the_browser(&mut h);
    let undo = h.app.session.undo.len();
    panel_mut(&mut h).name = "   ".into();
    assert!(!crate::drawing_ui::panel_can_ok(&h.app), "a sheet needs a name");
    panel_mut(&mut h).name = "Details".into();
    assert!(crate::drawing_ui::panel_can_ok(&h.app));
    crate::drawing_ui::panel_ok(&mut h.app);
    h.frames(2);
    assert_eq!(model(&h).sheets[0].name, "Details");
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(crate::scenario::handle_at("row:Details:1").is_some(), "the browser row reads Details:1");
    // Name and size: one step.
    edit_sheet_from_the_browser(&mut h);
    assert_eq!(panel(&h).name, "Details");
    panel_mut(&mut h).name = "Layout".into();
    panel_mut(&mut h).size = AnsiSize::C;
    crate::drawing_ui::panel_ok(&mut h.app);
    h.frames(2);
    assert_eq!((model(&h).sheets[0].name.as_str(), model(&h).sheets[0].size), ("Layout", AnsiSize::C));
    assert_eq!(h.app.session.undo.len(), undo + 2, "name and size together are one undo step");
    // Esc: nothing.
    edit_sheet_from_the_browser(&mut h);
    panel_mut(&mut h).name = "Other".into();
    panel_mut(&mut h).size = AnsiSize::A;
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert!(h.app.drawing.panel.is_none());
    assert_eq!((model(&h).sheets[0].name.as_str(), model(&h).sheets[0].size), ("Layout", AnsiSize::C));
    assert_eq!(h.app.session.undo.len(), undo + 2);
    // OK with nothing changed sends nothing.
    edit_sheet_from_the_browser(&mut h);
    crate::drawing_ui::panel_ok(&mut h.app);
    assert_eq!(h.app.session.undo.len(), undo + 2);
    // A successful apply refreshes the panel's starting point: the same panel applied again
    // sends nothing.
    edit_sheet_from_the_browser(&mut h);
    let mut p = panel(&h).clone();
    p.name = "Final".into();
    p.landscape = false;
    assert!(crate::drawing_sheet_panel::apply(&mut h.app, &mut p));
    assert_eq!(h.app.session.undo.len(), undo + 3);
    assert!(crate::drawing_sheet_panel::apply(&mut h.app, &mut p));
    assert_eq!(h.app.session.undo.len(), undo + 3, "nothing changed since the last apply");
    // One undo puts the name and the size back together.
    h.app.run("edit.undo", json!({})).unwrap();
    h.app.run("edit.undo", json!({})).unwrap();
    let sheet = &model(&h).sheets[0];
    assert_eq!((sheet.name.as_str(), sheet.size), ("Details", AnsiSize::D));
}

#[test]
fn edit_sheet_names_the_sheet_under_inventor() {
    name(inventor(Drawing));
}

#[test]
fn edit_sheet_names_the_sheet_under_solidcraft() {
    name(Harness::new());
}

/// The panel shows the capture's fields in its order; the ones we do not have yet (Revision,
/// Exclude from count, Exclude from printing) are shown disabled; Height and Width are read-only.
#[test]
fn edit_sheet_shows_the_captured_fields() {
    let mut h = drawing(inventor(Drawing));
    edit_sheet_from_the_browser(&mut h);
    h.frames(2);
    for handle in [
        "drawing_sheet:name",
        "drawing_sheet:size",
        "drawing_sheet:height",
        "drawing_sheet:width",
        "drawing_sheet:revision",
        "drawing_sheet:portrait",
        "drawing_sheet:landscape",
        "drawing_sheet:exclude_count",
        "drawing_sheet:exclude_print",
    ] {
        assert!(crate::scenario::handle_at(handle).is_some(), "{handle} is not shown");
    }
    assert_eq!(crate::scenario::count_of("drawing_sheet:revision:enabled"), Some(0.0));
    assert_eq!(crate::scenario::count_of("drawing_sheet:exclude:enabled"), Some(0.0));
}
