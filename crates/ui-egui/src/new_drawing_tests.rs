#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! New Drawing (Plan D-2 Task 4): Create Drawing View from the open model (spec §2.1 path 2c),
//! File › New Drawing, the Quick Access New flyout and Home's tile; New itself still makes a part.

use serde_json::json;
use solidcraft_engine::doc::DocumentKind::{self, Drawing, Part};

use super::*;
use crate::context_menu::{Item, Target};
use crate::scenario::Harness;
use crate::test_support::{block_part, click, inventor};

/// A title block field of the active drawing's first sheet.
fn title_field(app: &SolveApp, tag: &str) -> String {
    let m = app.session.doc.drawing.as_deref().expect("a drawing model");
    let e = m.paper.entity(m.sheets[0].title_block).expect("the title block insert");
    let cadcraft_doc::EntityKind::Insert(ins) = &e.kind else { panic!("the title block is an Insert") };
    ins.attribs.iter().find(|a| a.tag == tag).map(|a| a.text.value.clone()).unwrap_or_else(|| panic!("no {tag} field"))
}

/// The active document is a new drawing: one D landscape sheet holding the border and title
/// block inserts, no panel open and nothing to undo.
fn assert_empty_drawing(app: &SolveApp) {
    assert_eq!(app.session.doc.kind, Drawing);
    let m = app.session.doc.drawing.as_deref().expect("a drawing model");
    assert_eq!(m.sheets.len(), 1);
    let sheet = &m.sheets[0];
    assert_eq!((sheet.size.letter(), sheet.landscape), ("D", true));
    let layout = m.paper.layouts.iter().find(|l| l.name == sheet.layout).expect("the sheet's layout");
    for insert in [sheet.border, sheet.title_block] {
        assert!(layout.entities.get(insert).is_some(), "the layout holds insert {insert:?}");
    }
    assert!(m.views.is_empty());
    assert!(app.drawing.panel.is_none());
    assert_eq!(app.session.undo.len(), 0, "a new drawing has nothing to undo (no document.units step)");
}

fn ids(v: &[Item]) -> Vec<&str> {
    v.iter().map(|i| i.id.as_str()).collect()
}

/// The menu item `id` in `items`, enabled.
fn item(items: &[Item], id: &str) -> Item {
    let item = items.iter().find(|i| i.id == id).unwrap_or_else(|| panic!("{id} not in {:?}", ids(items))).clone();
    assert!(item.enabled, "{id} is enabled");
    item
}

#[test]
fn create_drawing_view_opens_a_drawing_of_the_model() {
    let mut h = inventor(Part);
    let block = block_part(&h.dir);
    h.app.open_path_async(&block);
    h.settle();
    assert_eq!(h.app.session.path.as_deref(), Some(block.as_str()));
    assert_eq!(h.app.docs.count(), 1);
    // The empty-space marking menu's list (the part's Empty context) ends with Create Drawing View.
    let list = crate::context_menu::items(&h.app, &Target::Viewport);
    assert_eq!(ids(&list)[ids(&list).len() - 2..], ["-", "ui.createDrawingView"]);
    let create = item(&list, "ui.createDrawingView");
    assert_eq!((create.label.as_str(), create.icon.as_str()), ("Create Drawing View", "camera"));
    crate::context_menu::run_item(&mut h.app, &create, egui::Pos2::ZERO);
    h.frames(3);
    assert_eq!(h.app.docs.count(), 2, "the drawing opens in its own tab beside the model");
    assert_eq!(h.app.docs.active, 1);
    assert_eq!(title_field(&h.app, "TITLE"), "block");
    assert_eq!(title_field(&h.app, "DWG_NO"), "block");
    assert_empty_drawing(&h.app);
    assert!(h.app.session.path.is_none(), "the drawing is new and unsaved");
    assert!(!h.app.session.is_dirty());
}

#[test]
fn an_unsaved_model_is_saved_first() {
    // The save picker cancelled: no drawing, and the status says why.
    let mut h = inventor(Part);
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    h.app.services.pick_save = Some(Box::new(|_, _| Box::pin(std::future::ready(None))));
    create_drawing_view(&mut h.app);
    h.frames(3);
    assert_eq!(h.app.docs.count(), 1);
    assert_eq!(h.app.session.doc.kind, Part);
    assert!(h.app.session.path.is_none());
    let (message, _, error) = h.app.status.clone().expect("a status");
    assert_eq!(message, crate::i18n::tr("ui.drawing.model_not_saved"));
    assert!(error);

    // Answered: the part is saved where the picker said, then the drawing of it opens.
    let mut h = inventor(Part);
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    create_drawing_view(&mut h.app);
    h.settle();
    let saved = h.dir.join("Untitled.tcipart");
    assert!(saved.is_file(), "the model was saved first, at {}", saved.display());
    assert_eq!(h.app.docs.count(), 2);
    assert_eq!(h.app.session.doc.kind, Drawing);
    assert_eq!(title_field(&h.app, "DWG_NO"), "Untitled");
    assert_empty_drawing(&h.app);
    switch(&mut h.app, 0);
    assert_eq!(h.app.session.path.as_deref(), Some(saved.to_string_lossy().as_ref()));
    assert!(!h.app.session.is_dirty());
}

#[test]
fn qat_and_home_make_an_empty_drawing() {
    // QAT New ▸ Drawing, from a part with a feature (a new tab).
    let mut h = inventor(Part);
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    h.frames(2);
    click(&mut h, "appbar:new_arrow");
    let rows: Vec<String> = crate::quick_access::NEW_KINDS.iter().map(|(id, ..)| format!("menu:{id}")).collect();
    let ys: Vec<f32> = rows.iter().map(|r| crate::scenario::rect_of(r).unwrap_or_else(|| panic!("{r} is not drawn")).top()).collect();
    assert!(ys.windows(2).all(|w| w[0] < w[1]), "Assembly, Drawing, Part, top to bottom: {ys:?}");
    assert_eq!(crate::quick_access::NEW_KINDS.map(|(_, kind, ..)| kind), [DocumentKind::Assembly, Drawing, Part]);
    click(&mut h, "menu:qat.new.drawing");
    assert_eq!(h.app.docs.count(), 2);
    assert_empty_drawing(&h.app);
    assert!(crate::scenario::rect_of("menu:qat.new.drawing").is_none(), "the flyout closes");

    // Home's New Drawing tile replaces the blank launch design.
    let mut h = inventor(Part);
    h.app.home.show(0);
    h.frames(3);
    click(&mut h, "home:new_drawing");
    assert_eq!(h.app.docs.count(), 1, "the blank design is replaced");
    assert!(!h.app.home.open);
    assert_empty_drawing(&h.app);

    // File › New Drawing.
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    crate::toolbar::FileAction::NewDrawing.run(&mut h.app);
    h.frames(2);
    assert_eq!(h.app.docs.count(), 2);
    assert_empty_drawing(&h.app);
}

#[test]
fn new_still_makes_a_part() {
    let mut h = inventor(Part);
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    h.frames(2);
    assert_eq!(crate::profile::active(&h.app).defaults.new_document, Some(Part));
    click(&mut h, "appbar:new");
    assert_eq!(h.app.docs.count(), 2);
    assert_eq!(h.app.session.doc.kind, Part, "the New face makes a part");
    assert!(crate::scenario::rect_of("menu:qat.new.drawing").is_none(), "the face opens no flyout");
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    h.step(&json!({"key": "N", "cmd": true})).unwrap();
    h.frames(2);
    assert_eq!(h.app.docs.count(), 3);
    assert_eq!(h.app.session.doc.kind, Part, "Ctrl+N makes a part");
}

#[test]
fn only_inventor_offers_create_drawing_view_in_the_root_menu() {
    let mut h = inventor(Part);
    h.app.home.open = false;
    h.frames(2);
    assert!(crate::profile::active(&h.app).workflow.create_drawing_in_root_menu);
    let root = crate::context_menu::items(&h.app, &Target::Component { id: 0 });
    assert_eq!(ids(&root)[ids(&root).len() - 2..], ["-", "ui.createDrawingView"]);
    assert_eq!(item(&root, "ui.createDrawingView").icon, "camera");

    let h = Harness::new();
    assert!(!crate::profile::active(&h.app).workflow.create_drawing_in_root_menu);
    let root = crate::context_menu::items(&h.app, &Target::Component { id: 0 });
    assert_eq!(
        ids(&root),
        [
            "ui.activate",
            "component.create",
            "ui.copyDesign",
            "ui.paste",
            "component.from_bodies",
            "-",
            "ui.material",
            "ui.appearance",
            "ui.properties",
            "ui.designProperties",
            "-",
            "ui.export",
            "ui.saveMesh",
            "-",
            "ui.rename",
            "-",
            "ui.hide",
            "ui.showAll",
            "ui.isolate",
            "ui.newGroup",
            "-",
            "browser.expand_all",
            "browser.collapse_all",
        ],
        "SolidCraft's root menu is today's"
    );
}

#[test]
fn the_company_preference_reaches_the_title_block() {
    let mut h = inventor(Part);
    h.app.home.open = false;
    h.app.preferences.drawing_company = "TCI".into();
    crate::toolbar::FileAction::NewDrawing.run(&mut h.app);
    h.frames(2);
    assert_eq!(title_field(&h.app, "COMPANY"), "TCI");
    // Create Drawing View sends it too.
    let mut h = inventor(Part);
    let block = block_part(&h.dir);
    h.app.open_path_async(&block);
    h.settle();
    h.app.preferences.drawing_company = "TCI".into();
    create_drawing_view(&mut h.app);
    h.frames(2);
    assert_eq!((title_field(&h.app, "COMPANY").as_str(), title_field(&h.app, "TITLE").as_str()), ("TCI", "block"));
    // Empty: the field stays empty.
    let mut h = inventor(Part);
    h.app.home.open = false;
    crate::toolbar::FileAction::NewDrawing.run(&mut h.app);
    assert_eq!(title_field(&h.app, "COMPANY"), "");
}
