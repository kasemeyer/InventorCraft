#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The drawing browser (Plan D-2 Task 7, spec §2.10): the drawing's tree (resources, sheets,
//! views nested under their base view, the model), its menus and Drawing Properties.

use serde_json::{Value, json};
use solidcraft_engine::doc::DocumentKind::Drawing;
use solidcraft_engine::drawing::{COMPUTED_FIELDS, PaperPick, TITLE_FIELDS, default_drawing_path};

use super::*;
use crate::context_menu::{Item, Target, run_item};
use crate::scenario::{Harness, rect_of};
use crate::test_support::{block_part, click, inventor};

/// An Inventor drawing of the block saved beside it (`<dir>/block.tcidrw`): VIEW1 (front, hidden
/// lines) at [10, 8], VIEW2 projected above it at [10, 16] and VIEW3 to its right at [20, 8].
/// The harness, the block's path, the drawing's path and the three view ids.
fn three_views() -> (Harness, String, String, [u64; 3]) {
    let mut h = inventor(Drawing);
    h.app.home.open = false;
    let block = block_part(&h.dir);
    h.app.run("drawing.new", json!({"model": block})).unwrap();
    let id = |r: Value| r["view"].as_u64().unwrap();
    let v1 = id(h
        .app
        .run("drawing.view.base", json!({"model": block, "orientation": "front", "lines": "hidden", "scale": 1, "position": [10, 8]}))
        .unwrap());
    let v2 = id(h.app.run("drawing.view.projected", json!({"parent": v1, "position": [10, 16]})).unwrap());
    let v3 = id(h.app.run("drawing.view.projected", json!({"parent": v1, "position": [20, 8]})).unwrap());
    let drawing = default_drawing_path(&block);
    h.app.run("file.save_as", json!({"path": drawing})).unwrap();
    h.frames(3);
    (h, block, drawing, [v1, v2, v3])
}

/// The browser's rows as drawn: (label, depth), the depth from the label's indent.
fn tree() -> Vec<(String, usize)> {
    let rows = drawn_rows();
    let left = |label: &str| rect_of(&format!("row_label:{label}")).unwrap_or_else(|| panic!("{label} has no label")).left();
    let root = left(&rows[0].0);
    rows.iter().map(|(label, ..)| (label.clone(), ((left(label) - root) / INDENT).round() as usize)).collect()
}

fn ids(v: &[Item]) -> Vec<&str> {
    v.iter().map(|i| i.id.as_str()).collect()
}

fn title_field(app: &SolveApp, tag: &str) -> String {
    let m = app.session.doc.drawing.as_deref().unwrap();
    drawing::title_values(m, 0).into_iter().find(|(t, _)| t == tag).unwrap().1
}

/// Root menu › Drawing Properties….
fn open_properties(h: &mut Harness) {
    let item = crate::context_menu::items(&h.app, &Target::DrawingRoot).remove(0);
    run_item(&mut h.app, &item, egui::Pos2::ZERO);
}

#[test]
fn projected_views_nest_under_their_base() {
    let (h, ..) = three_views();
    let want = [
        ("block", 0),
        ("Drawing Resources", 1),
        ("Sheet:1", 1),
        ("Border", 2),
        ("Title Block", 2),
        ("VIEW1:block.tcipart", 2),
        ("VIEW2:block.tcipart", 3),
        ("VIEW3:block.tcipart", 3),
        ("block.tcipart", 3),
    ];
    let got = tree();
    assert_eq!(got, want.map(|(l, d)| (l.to_string(), d)));
    assert_eq!(h.app.session.doc.kind, Drawing);

    // Drawing Resources opens to one border and one title block.
    let mut h = h;
    click(&mut h, "fold:Drawing Resources");
    click(&mut h, "fold:Borders");
    click(&mut h, "fold:Title Blocks");
    let labels: Vec<String> = tree().into_iter().take(7).map(|(l, _)| l).collect();
    assert_eq!(labels[..3], ["block", "Drawing Resources", "Borders"]);
    assert_eq!(labels[4], "Title Blocks");
    assert_eq!(labels[5], "TCI_TITLE_ANSI");
    assert_eq!(labels[6], "Sheet:1");
}

#[test]
fn a_view_row_selects_and_offers_its_menu() {
    let (mut h, block, _, [v1, v2, v3]) = three_views();
    click(&mut h, "row:VIEW1:block.tcipart");
    assert_eq!(h.app.drawing.selection, [PaperPick::View { view: v1 }]);
    let menu = crate::drawing_menu::view_items(&h.app, v1);
    // Task 5 puts Edit View first (R93; updated by design).
    assert_eq!(ids(&menu), ["ui.drawing.editView", "ui.delete", "ui.drawing.openModel"]);
    assert!(menu.iter().all(|i| i.enabled), "all enabled: {menu:?}");
    assert_eq!((menu[0].icon.as_str(), menu[1].icon.as_str(), menu[2].icon.as_str()), ("edit", "delete", "open"));
    // The row's right-click lists the same.
    assert_eq!(ids(&crate::context_menu::items(&h.app, &Target::DrawingView { view: v1 })), ids(&menu));

    // Open Model: the block opens in its own tab, which becomes the active one.
    run_item(&mut h.app, &menu[2], egui::Pos2::ZERO);
    h.settle();
    assert_eq!(h.app.docs.count(), 2);
    assert_eq!(h.app.session.path.as_deref(), Some(block.as_str()));
    // Open again from the drawing: the tab holding it is activated, nothing more opens.
    crate::documents::switch(&mut h.app, 0);
    h.frames(2);
    let open = crate::drawing_menu::view_items(&h.app, v1).remove(2);
    run_item(&mut h.app, &open, egui::Pos2::ZERO);
    h.settle();
    assert_eq!((h.app.docs.count(), h.app.docs.active), (2, 1));
    assert_eq!(h.app.session.path.as_deref(), Some(block.as_str()));

    // Delete: one drawing.view.delete step, the projected views going with their base.
    crate::documents::switch(&mut h.app, 0);
    h.frames(2);
    click(&mut h, "row:VIEW1:block.tcipart");
    let undo = h.app.session.undo.len();
    let delete = crate::drawing_menu::view_items(&h.app, v1).remove(1);
    run_item(&mut h.app, &delete, egui::Pos2::ZERO);
    h.frames(2);
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    let views = &h.app.session.doc.drawing.as_deref().unwrap().views;
    for v in [v1, v2, v3] {
        assert!(!views.contains_key(&v), "view {v} is gone");
    }
    assert!(h.app.drawing.selection.is_empty());
    assert_eq!(h.app.session.doc.kind, Drawing, "never the part's delete");

    // The Delete key acts on the sheet's selection too (the part's delete never runs).
    let (mut h, _, _, [v1, v2, _]) = three_views();
    click(&mut h, "row:VIEW2:block.tcipart");
    let undo = h.app.session.undo.len();
    h.step(&json!({"key": "Delete"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let views = &h.app.session.doc.drawing.as_deref().unwrap().views;
    assert!(views.contains_key(&v1) && !views.contains_key(&v2));
}

#[test]
fn double_clicking_a_sheet_activates_it() {
    let (mut h, ..) = three_views();
    h.app.run("drawing.sheet.add", json!({})).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.drawing_sheet, 1);
    let labels: Vec<String> = tree().into_iter().map(|(l, _)| l).collect();
    assert!(labels.contains(&"Sheet:2".to_string()), "{labels:?}");
    h.step(&json!({"click": {"handle": "row:Sheet:1"}, "double": true})).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.drawing_sheet, 0);
    // The sheet's menu activates it too.
    let menu = crate::context_menu::items(&h.app, &Target::Sheet { index: 1 });
    assert_eq!(ids(&menu), ["drawing.sheet.activate"]);
    run_item(&mut h.app, &menu[0], egui::Pos2::ZERO);
    assert_eq!(h.app.session.drawing_sheet, 1);
}

#[test]
fn a_missing_model_is_flagged() {
    // The first harness keeps the scratch folder the files are in.
    let (_first, block, drawing, [v1, ..]) = three_views();
    std::fs::rename(&block, format!("{block}.moved")).unwrap();
    let mut h = inventor(Drawing);
    h.app.open_path_async(&drawing);
    h.settle();
    h.frames(2);
    assert_eq!(h.app.session.doc.kind, Drawing);
    let suffix = crate::i18n::tr("ui.drawing.model_missing");
    let view = tree().into_iter().map(|(l, _)| l).find(|l| l.starts_with("VIEW1:")).expect("the VIEW1 row");
    assert_eq!(view, format!("VIEW1:block.tcipart {suffix}"));
    let menu = crate::drawing_menu::view_items(&h.app, v1);
    assert_eq!(ids(&menu), ["ui.drawing.editView", "ui.delete", "ui.drawing.openModel"]);
    assert!(!menu[2].enabled, "nothing to open");
}

#[test]
fn drawing_properties_writes_the_title_block() {
    let (mut h, ..) = three_views();
    let root = crate::context_menu::items(&h.app, &Target::DrawingRoot);
    assert_eq!(ids(&root)[0], "ui.drawing.properties");
    assert_eq!((root[0].label.as_str(), root[0].icon.as_str()), ("Drawing Properties…", "settings"));
    run_item(&mut h.app, &root[0], egui::Pos2::ZERO);
    h.frames(3);
    let panel = |h: &Harness| match &h.app.drawing.panel {
        Some(crate::drawing_ui::DrawingPanel::Properties(p)) => p.clone(),
        _ => panic!("Drawing Properties is open"),
    };
    let p = panel(&h);
    let tags: Vec<&str> = p.fields.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(tags, TITLE_FIELDS);
    let read_only: Vec<&str> = tags.iter().copied().filter(|t| !drawing::PropertiesPanel::editable(t)).collect();
    assert_eq!(read_only, COMPUTED_FIELDS);
    assert_eq!(p.fields[0].1, "block");
    assert_eq!(p.fields[4], ("SIZE".to_string(), "D".to_string()));
    assert_eq!(p.fields, p.original);
    // The panel is drawn: an editable field per non-computed tag, a plain value for the others.
    for tag in TITLE_FIELDS {
        let field = crate::scenario::handle_at(&format!("drawing_properties:{tag}")).is_some();
        assert_eq!(field, !COMPUTED_FIELDS.contains(&tag), "{tag}");
    }
    assert!(crate::drawing_ui::panel_can_ok(&h.app));

    // TITLE BRACKET, OK: one step sending only the title.
    if let Some(crate::drawing_ui::DrawingPanel::Properties(p)) = &mut h.app.drawing.panel {
        p.fields[0].1 = "BRACKET".into();
    }
    assert_eq!(Value::Object(panel(&h).changes()), json!({"title": "BRACKET"}));
    let undo = h.app.session.undo.len();
    let ok = crate::marking_menu::item(&h.app, "ui.ok", crate::profile_workflow::MenuContext::Panel);
    assert!(ok.enabled);
    run_item(&mut h.app, &ok, egui::Pos2::ZERO);
    h.frames(2);
    assert!(h.app.drawing.panel.is_none());
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(title_field(&h.app, "TITLE"), "BRACKET");

    // Reopen, OK with nothing changed: no step and no error.
    open_properties(&mut h);
    h.frames(2);
    assert_eq!(panel(&h).fields[0].1, "BRACKET");
    assert!(panel(&h).changes().is_empty());
    let ok = crate::marking_menu::item(&h.app, "ui.ok", crate::profile_workflow::MenuContext::Panel);
    run_item(&mut h.app, &ok, egui::Pos2::ZERO);
    h.frames(2);
    assert!(h.app.drawing.panel.is_none());
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{:?}", h.app.status);

    // The company goes to the preference too, and Cancel sends nothing.
    open_properties(&mut h);
    if let Some(crate::drawing_ui::DrawingPanel::Properties(p)) = &mut h.app.drawing.panel {
        p.fields[1].1 = "TCI".into();
    }
    crate::drawing_ui::panel_ok(&mut h.app);
    assert_eq!((title_field(&h.app, "COMPANY").as_str(), h.app.preferences.drawing_company.as_str()), ("TCI", "TCI"));
    open_properties(&mut h);
    if let Some(crate::drawing_ui::DrawingPanel::Properties(p)) = &mut h.app.drawing.panel {
        p.fields[0].1 = "GONE".into();
    }
    let undo = h.app.session.undo.len();
    let cancel = crate::marking_menu::item(&h.app, "ui.cancel", crate::profile_workflow::MenuContext::Panel);
    run_item(&mut h.app, &cancel, egui::Pos2::ZERO);
    assert!(h.app.drawing.panel.is_none());
    assert_eq!((h.app.session.undo.len(), title_field(&h.app, "TITLE").as_str()), (undo, "BRACKET"));
}

#[test]
fn the_header_counts_sheets_on_a_drawing() {
    // Final wave item 5: "n sheets" on a drawing, the body count on a part (both profiles).
    for mut h in [Harness::new(), inventor(Drawing)] {
        h.app.home.open = false;
        h.app.run("drawing.new", json!({})).unwrap();
        h.frames(2);
        assert_eq!(header_count(&h.app), "1 sheet");
        h.app.run("drawing.sheet.add", json!({})).unwrap();
        h.frames(2);
        assert_eq!(header_count(&h.app), "2 sheets");
    }
    let mut h = Harness::new();
    h.app.home.open = false;
    assert_eq!(header_count(&h.app), "0 bodies");
    h.app.run("solid.box", json!({"length": 10, "width": 12, "height": 14})).unwrap();
    h.frames(2);
    assert_eq!(header_count(&h.app), "1 body");
}
