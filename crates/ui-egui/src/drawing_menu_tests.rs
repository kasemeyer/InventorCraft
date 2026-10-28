#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The drawing's right-click lists (R15) and `marking_menu::item`'s drawing arm (R72).

use serde_json::json;
use solidcraft_engine::doc::DocumentKind;
use solidcraft_engine::drawing::PaperPick;

use super::*;
use crate::test_support::inventor;

fn drawing() -> crate::scenario::Harness {
    let mut h = inventor(DocumentKind::Drawing);
    h.app.home.open = false;
    h.app.run("drawing.new", json!({})).unwrap();
    h.frames(2);
    h
}

fn ids(v: &[Item]) -> Vec<&str> {
    v.iter().map(|i| i.id.as_str()).collect()
}

#[test]
fn an_empty_sheet_lists_view_sheet_update_and_undo() {
    let mut h = drawing();
    let v = items(&h.app);
    assert_eq!(ids(&v), ["drawing.view.base", "drawing.sheet.add", "drawing.update", "-", "edit.undo"]);
    assert_eq!(v[0].label, "Base View");
    assert!(v[0].enabled && v[1].enabled && v[2].enabled);
    assert_eq!(v[0].icon, "camera");
    // Undo follows the session's registry state.
    assert_eq!(v[4].enabled, crate::context_menu::cmd(&h.app, "edit.undo", "Undo").enabled);
    h.app.run("drawing.sheet.add", json!({})).unwrap();
    assert!(items(&h.app)[4].enabled, "the added sheet can be undone");
    // Under Inventor the empty sheet's viewport list is the `drawing_empty` layout's (Task 10,
    // changed by design); a profile without one gets this list (`solidcraft_drawing_menus_are_lists`).
    let routed = crate::context_menu::items(&h.app, &crate::context_menu::Target::Viewport);
    assert_eq!(ids(&routed), ["ui.repeat", "-", "drawing.sheet.add", "ui.previousSheet", "ui.nextSheet", "-", "ui.previousView"]);
}

#[test]
fn drawing_actions_answer_to_the_drawing() {
    let mut h = drawing();
    let item = |app: &crate::SolveApp, id: &str| crate::marking_menu::item(app, id, MenuContext::Empty);
    // Nothing running, nothing open, nothing selected.
    for id in ["ui.done", "ui.cancel", "ui.ok", "ui.delete"] {
        assert!(!item(&h.app, id).enabled, "{id}");
    }
    // Delete follows the sheet's selection, not the session's.
    h.app.drawing.selection = vec![PaperPick::View { view: 1 }];
    assert!(item(&h.app, "ui.delete").enabled);
    assert!(h.app.session.selection.is_empty());
    assert_eq!(item(&h.app, "ui.home").label, "Home View");
    // No ring on a sheet: the profile's tool ring never shows there (R15).
    assert!(crate::marking_menu::ring(&h.app).is_none());
    assert!(crate::marking_menu::overflow(&h.app).is_none());
}

// ---------------------------------------------------------------------------------------------
// Task 10: the drawing marking menu (`DrawingEmpty`), the selection lists, Edit Note from the
// list, and sheet switching.
// ---------------------------------------------------------------------------------------------

use crate::drawing_notes_tool::{Justify, TextPanel};
use crate::drawing_ui::DrawingPanel;
use crate::scenario::Harness;
use crate::test_support::{block_part, click};

/// The block's drawing: VIEW1 its front at [10, 8], and the note "NOTES:" at [2, 2].
fn block_sheet(mut h: Harness) -> (Harness, u64, u64) {
    h.app.home.open = false;
    let block = block_part(&h.dir);
    h.app.run("drawing.new", json!({"model": block})).unwrap();
    let v1 =
        h.app.run("drawing.view.base", json!({"model": block, "orientation": "front", "lines": "hidden", "scale": 1, "position": [10, 8]})).unwrap()
            ["view"]
            .as_u64()
            .unwrap();
    let note = h.app.run("drawing.note.text", json!({"position": [2, 2], "text": "NOTES:"})).unwrap()["handle"].as_u64().unwrap();
    h.frames(3);
    (h, v1, note)
}

fn screen(h: &Harness, p: [f64; 2]) -> egui::Pos2 {
    crate::sheet_canvas::screen_of(&h.app, crate::scenario::rect_of("sheet_canvas").unwrap(), p)
}

fn right_click_paper(h: &mut Harness, p: [f64; 2]) {
    let s = screen(h, p);
    h.step(&json!({"click": [s.x, s.y], "button": "right"})).unwrap();
    h.frames(2);
}

fn menu_ids(menu: &serde_json::Value, key: &str) -> Vec<String> {
    menu[key].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_string()).collect()
}

fn enabled(menu: &serde_json::Value, id: &str) -> bool {
    menu["items"].as_array().unwrap().iter().find(|i| i["id"] == json!(id)).unwrap()["enabled"] == json!(true)
}

fn ctrl_key(h: &mut Harness, k: &str) {
    h.step(&json!({"key": k, "ctrl": true, "cmd": true})).unwrap();
    h.frames(2);
}

fn undo(h: &Harness) -> usize {
    h.app.session.undo.len()
}

#[test]
fn the_drawing_ring_matches_the_capture() {
    let mut h = drawing();
    let layout = crate::marking_menu::layout(&h.app).unwrap().get(&MenuContext::DrawingEmpty).unwrap().clone();
    assert_eq!(layout.evidence.as_deref(), Some("docs/evidence/screenshots/drawing/marking-menu.png"));
    assert_eq!(
        layout.ring,
        ["drawing.view.base", "drawing.note.leader", "drawing.note.hole", "drawing.centermark", "", "drawing.dim.general", "edit.undo", ""]
    );
    assert_eq!(layout.overflow, ["ui.repeat", "-", "drawing.sheet.add", "ui.previousSheet", "ui.nextSheet", "-", "ui.previousView"]);
    right_click_paper(&mut h, [17.0, 11.0]);
    let menu = crate::context_menu::describe(&h.app);
    assert_eq!(menu["open"], json!(true), "{menu}");
    let ring: Vec<String> = menu_ids(&menu, "radial").into_iter().map(|id| if id == "-" { String::new() } else { id }).collect();
    assert_eq!(ring, layout.ring, "slot for slot, clockwise from north");
    assert_eq!(menu_ids(&menu, "items"), layout.overflow);
    // One sheet: neither Previous nor Next Sheet goes anywhere; New Sheet does.
    assert!(!enabled(&menu, "ui.previousSheet") && !enabled(&menu, "ui.nextSheet") && enabled(&menu, "drawing.sheet.add"));
    let radial = menu["radial"].as_array().unwrap();
    assert_eq!(radial[0]["label"], json!("Base View"));
    assert_eq!(radial[0]["enabled"], json!(true));
    // The new context adds no unverified layout.
    assert_eq!(crate::profile_workflow::workflow_warnings(&crate::profile::active(&h.app).workflow), Vec::<String>::new());
}

#[test]
fn a_selection_gets_a_list_not_a_ring() {
    let (mut h, v1, note) = block_sheet(inventor(DocumentKind::Drawing));
    h.app.drawing.selection = vec![PaperPick::View { view: v1 }];
    right_click_paper(&mut h, [30.0, 18.0]);
    let menu = crate::context_menu::describe(&h.app);
    assert!(menu_ids(&menu, "radial").is_empty(), "a list, never a ring: {menu}");
    assert_eq!(menu_ids(&menu, "items"), ["ui.drawing.editView", "ui.delete", "ui.drawing.openModel"]);
    assert_eq!(menu_ids(&menu, "items"), ids(&view_items(&h.app, v1)), "the browser's view list");
    crate::context_menu::close(&mut h.app);
    // A right-click on the note selects it and lists its rows.
    right_click_paper(&mut h, [2.2, 1.95]);
    assert_eq!(h.app.drawing.selection, vec![PaperPick::Annotation { handle: note }]);
    let menu = crate::context_menu::describe(&h.app);
    assert!(menu_ids(&menu, "radial").is_empty(), "{menu}");
    assert_eq!(menu_ids(&menu, "items"), ["ui.drawing.editNote", "ui.drawing.moveNote", "ui.delete"]);
    assert!(menu["items"].as_array().unwrap().iter().all(|i| i["enabled"] == json!(true)), "{menu}");
    let icons: Vec<&str> = menu["items"].as_array().unwrap().iter().map(|i| i["icon"].as_str().unwrap()).collect();
    assert_eq!(icons, ["text", "move", "delete"]);
    crate::context_menu::close(&mut h.app);
    // A centre mark's line: the same rows, but it has no text to edit.
    let lines = h.app.run("drawing.centermark", json!({"view": v1, "edge": {"at": [10.375, 8.0]}})).unwrap();
    let line = lines["handles"][0].as_u64().unwrap();
    h.app.drawing.selection = vec![PaperPick::Annotation { handle: line }];
    let list = items(&h.app);
    assert_eq!(ids(&list), ["ui.drawing.editNote", "ui.drawing.moveNote", "ui.delete"]);
    assert_eq!(list.iter().map(|i| i.enabled).collect::<Vec<_>>(), [false, true, true]);
    // Move Note: the next click puts the note's text there, one undo step, and the tool ends.
    h.frames(2);
    right_click_paper(&mut h, [2.2, 1.95]);
    let start = undo(&h);
    click(&mut h, "menu:ui.drawing.moveNote");
    assert!(h.app.drawing.tool.is_some());
    let s = screen(&h, [4.0, 3.0]);
    h.step(&json!({"move": [s.x, s.y]})).unwrap();
    h.frames(2);
    assert_eq!(undo(&h), start, "following the cursor is not an edit");
    h.step(&json!({"click": [s.x, s.y]})).unwrap();
    h.frames(2);
    assert_eq!(undo(&h), start + 1);
    assert!(h.app.drawing.tool.is_none());
    let m = h.app.session.doc.drawing.as_deref().unwrap();
    let info = solidcraft_engine::drawing::note_at(m, note).unwrap();
    assert_eq!(info.text, "NOTES:");
    // Where it went (final wave item 35): its text sits at [4, 3].
    let Some(cadcraft_doc::EntityKind::MText(t)) = m.paper.entity(cadcraft_doc::Handle(info.text_handle)).map(|e| &e.kind) else {
        panic!("the note's text")
    };
    assert!((t.insert.x - 4.0).abs() < 1e-3 && (t.insert.y - 3.0).abs() < 1e-3, "{:?}", t.insert);
}

fn text_panel(h: &Harness) -> &TextPanel {
    match &h.app.drawing.panel {
        Some(DrawingPanel::Text(p)) => p,
        _ => panic!("the text panel is not open"),
    }
}

#[test]
fn edit_note_changes_the_text() {
    let (mut h, _, note) = block_sheet(inventor(DocumentKind::Drawing));
    let start = undo(&h);
    right_click_paper(&mut h, [2.2, 1.95]);
    click(&mut h, "menu:ui.drawing.editNote");
    let p = text_panel(&h);
    assert_eq!((p.edit, p.text.as_str(), p.height_text.as_str(), p.justify), (Some(note), "NOTES:", "0.12 in", Some(Justify::Left)));
    click(&mut h, "drawing_text:text");
    h.step(&json!({"text": " 1"})).unwrap();
    h.frames(2);
    click(&mut h, "dialog:ok");
    assert!(h.app.drawing.panel.is_none());
    assert_eq!(undo(&h), start + 1);
    assert_eq!(h.app.session.undo.last().unwrap().label, "Edit Note");
    let m = h.app.session.doc.drawing.as_deref().unwrap();
    assert_eq!(solidcraft_engine::drawing::note_at(m, note).unwrap().text, "NOTES: 1");
    // Reopened and OK with nothing changed: no step.
    right_click_paper(&mut h, [2.2, 1.95]);
    click(&mut h, "menu:ui.drawing.editNote");
    assert_eq!(text_panel(&h).text, "NOTES: 1");
    click(&mut h, "dialog:ok");
    assert!(h.app.drawing.panel.is_none());
    assert_eq!(undo(&h), start + 1);
}

/// The SHEET field of sheet `index`'s title block.
fn sheet_field(h: &Harness, index: usize) -> String {
    let m = h.app.session.doc.drawing.as_deref().unwrap();
    crate::browser::drawing::title_values(m, index).into_iter().find(|(tag, _)| tag == "SHEET").unwrap().1
}

fn layouts(h: &Harness) -> Vec<String> {
    h.app.session.doc.drawing.as_deref().unwrap().paper.layouts.iter().map(|l| l.name.clone()).collect()
}

#[test]
fn new_sheet_and_sheet_switching() {
    let mut h = drawing();
    right_click_paper(&mut h, [17.0, 11.0]);
    click(&mut h, "menu:drawing.sheet.add");
    assert_eq!(layouts(&h), ["Sheet:1", "Sheet:2"]);
    assert_eq!(h.app.session.drawing_sheet, 1, "the new sheet is active (R7)");
    let after_add = undo(&h);
    ctrl_key(&mut h, "PageUp");
    assert_eq!(h.app.session.drawing_sheet, 0);
    // The canvas draws the sheet it shows now.
    assert_eq!(h.app.drawing.scene.as_ref().map(crate::sheet_canvas::SceneCache::sheet), Some(0));
    ctrl_key(&mut h, "PageUp");
    assert_eq!(h.app.session.drawing_sheet, 0, "no wrap before the first sheet");
    ctrl_key(&mut h, "PageDown");
    assert_eq!(h.app.session.drawing_sheet, 1);
    assert_eq!(h.app.drawing.scene.as_ref().map(crate::sheet_canvas::SceneCache::sheet), Some(1));
    assert_eq!((sheet_field(&h, 0), sheet_field(&h, 1)), ("1 OF 2".to_string(), "2 OF 2".to_string()));
    ctrl_key(&mut h, "PageDown");
    assert_eq!(h.app.session.drawing_sheet, 1, "no wrap past the last sheet");
    assert_eq!(undo(&h), after_add, "switching sheets is not an edit");
    // The list's Previous Sheet does what Ctrl+PageUp does.
    right_click_paper(&mut h, [17.0, 11.0]);
    let menu = crate::context_menu::describe(&h.app);
    assert!(enabled(&menu, "ui.previousSheet") && !enabled(&menu, "ui.nextSheet"), "{menu}");
    let row = menu["items"].as_array().unwrap().iter().find(|i| i["id"] == json!("ui.previousSheet")).unwrap().clone();
    assert_eq!((row["icon"].as_str(), row["shortcut"].as_str()), (Some("activate"), Some("Ctrl+PageUp")));
    click(&mut h, "menu:ui.previousSheet");
    assert_eq!(h.app.session.drawing_sheet, 0);
}

#[test]
fn solidcraft_drawing_menus_are_lists() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("drawing.new", json!({})).unwrap();
    h.frames(2);
    right_click_paper(&mut h, [17.0, 11.0]);
    let menu = crate::context_menu::describe(&h.app);
    assert_eq!(menu["open"], json!(true), "{menu}");
    assert!(menu_ids(&menu, "radial").is_empty(), "no ring: {menu}");
    assert_eq!(menu_ids(&menu, "items"), ["drawing.view.base", "drawing.sheet.add", "drawing.update", "-", "edit.undo"]);
    // Its keys switch sheets too.
    crate::context_menu::close(&mut h.app);
    h.app.run("drawing.sheet.add", json!({})).unwrap();
    h.frames(2);
    ctrl_key(&mut h, "PageUp");
    assert_eq!(h.app.session.drawing_sheet, 0);
}
