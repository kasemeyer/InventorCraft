#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use serde_json::json;
use solidcraft_engine::doc::DocumentKind::Part;

use crate::scenario::{Harness, handle_at, rect_of};
use crate::shell::LayoutMode;
use crate::test_support::{click, inventor};

const ORDER: [&str; 8] = ["new", "open", "save", "undo", "redo", "select", "return", "update"];

/// The design on screen, drawn twice.
fn on_design(mut h: Harness) -> Harness {
    h.app.home.open = false;
    h.frames(2);
    h
}

fn select_open(h: &Harness) -> bool {
    h.ctx.data(|d| d.get_temp::<bool>(egui::Id::new("sc_qat_select")).unwrap_or(false))
}

#[test]
fn inventor_shows_the_quick_access_toolbar_in_inventor_order() {
    let mut h = inventor(Part);
    h.frames(2);
    let xs: Vec<f32> = ORDER.iter().map(|id| handle_at(&format!("appbar:{id}")).unwrap_or_else(|| panic!("appbar:{id} is not drawn")).x).collect();
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "the entries run left to right in Inventor's order: {xs:?}");
    let ids: Vec<&str> = entries(&h.app).iter().map(|(id, ..)| *id).collect();
    assert_eq!(ids, ORDER);
    // The group's separator follows its last entry.
    assert!(rect_of("appbar:update").unwrap().right() < rect_of("titlebar:separator:documents").unwrap().left());

    let mut h = Harness::new();
    h.frames(2);
    for gone in ["appbar:new", "appbar:select", "appbar:return", "appbar:update"] {
        assert!(handle_at(gone).is_none(), "{gone} is Inventor's, not SolidCraft's");
    }
    assert!(handle_at("appbar:save").is_some(), "SolidCraft keeps its Save button");
}

#[test]
fn the_select_drop_down_holds_the_selection_filter() {
    let mut h = on_design(inventor(Part));
    let heading = crate::i18n::tr_label("Selection filter");
    assert!(h.widget(heading).is_none(), "the filter is shut until Select opens it");
    click(&mut h, "appbar:select");
    h.frames(2);
    assert!(select_open(&h));
    let [x, y] = h.widget(heading).expect("the Select drop-down shows the selection filter");
    let button = rect_of("appbar:select").unwrap();
    assert!(y as f32 > button.bottom(), "the drop-down hangs below its button");
    assert!((x as f32 - button.left()).abs() < 200.0);
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert!(!select_open(&h), "Escape closes the drop-down");
    assert!(h.widget(heading).is_none());
}

#[test]
fn return_finishes_a_sketch_and_update_recomputes() {
    let mut h = on_design(inventor(Part));
    assert!(entries(&h.app).contains(&("return", "finish", false)));
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.line", json!({"points": [[0, 0], [10, 0]]})).unwrap();
    assert!(entries(&h.app).contains(&("return", "finish", true)));
    let ctx = h.ctx.clone();
    run(&mut h.app, &ctx, "return");
    assert!(h.app.session.active_sketch.is_none(), "Return finishes the sketch");
    assert!(entries(&h.app).contains(&("update", "compute", true)));
    run(&mut h.app, &ctx, "update");
    h.frames(2);
    assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "Update leaves no error: {:?}", h.app.status);
}

#[test]
fn new_and_open_use_the_file_actions() {
    let mut h = on_design(inventor(Part));
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    let slots = h.app.docs.count();
    let ctx = h.ctx.clone();
    run(&mut h.app, &ctx, "new");
    assert_eq!(h.app.docs.count(), slots + 1, "New adds a document");
    assert!(entries(&h.app).contains(&("open", "open", true)));
    run(&mut h.app, &ctx, "open");
    assert!(h.app.file_picker_busy(), "Open asks for a file as File > Open does");
    assert!(entries(&h.app).contains(&("new", "new", false)), "New waits for the file pick");
}

#[test]
fn the_workspace_switcher_is_drawn_only_under_solidcraft() {
    for layout in [LayoutMode::Floating, LayoutMode::Docked] {
        let mut h = Harness::new();
        h.app.ui.layout = layout;
        let mut h = on_design(h);
        let ws = rect_of("workspace_switcher").unwrap_or_else(|| panic!("{layout:?}: SolidCraft draws its workspace switcher"));
        assert!(h.app.shell.cards.contains(&ws));
        h.call("ui.prefs", json!({"ui_profile": "inventor"}));
        h.frames(2);
        assert!(rect_of("workspace_switcher").is_none(), "{layout:?}: Inventor has no workspace switcher");
        assert!(handle_at("ribbon:workspace").is_none());
        // The ribbon takes the switcher's place.
        let toolbar = rect_of("toolbar").unwrap();
        let first_tab = crate::scenario::rects("toolbar_tab:").into_iter().map(|(_, r)| r.left()).fold(f32::INFINITY, f32::min);
        assert!(first_tab < toolbar.left() + crate::toolbar::WORKSPACE_WIDTH, "{layout:?}: tabs start at {first_tab}");
        assert!(h.app.shell.cards.iter().all(|c| c.width() > crate::toolbar::WORKSPACE_WIDTH), "{layout:?}: {:?}", h.app.shell.cards);
    }
}

#[test]
fn return_and_update_wait_for_a_design_behind_home() {
    let mut h = inventor(Part);
    h.frames(2);
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.line", json!({"points": [[0, 0], [10, 0]]})).unwrap();
    h.app.home.open = true;
    h.frames(2);
    assert!(entries(&h.app).contains(&("return", "finish", false)), "Return waits for the design behind Home");
    assert!(entries(&h.app).contains(&("update", "compute", false)), "Update waits for the design behind Home");
    let needs = |label: &str| crate::i18n::format("ui.needs_design", &[("command", label)]);
    assert_eq!(tip(&h.app, "return"), needs(h.app.tr("ui.qat.return")));
    assert_eq!(tip(&h.app, "update"), needs(h.app.tr("ui.qat.update")));
    let ctx = h.ctx.clone();
    run(&mut h.app, &ctx, "return");
    assert!(h.app.session.active_sketch.is_some(), "a click on Return behind Home does nothing");

    h.app.home.open = false;
    h.frames(2);
    assert!(entries(&h.app).contains(&("return", "finish", true)));
    assert!(entries(&h.app).contains(&("update", "compute", true)));
    assert_eq!(tip(&h.app, "return"), h.app.tr("ui.qat.return"));
    run(&mut h.app, &ctx, "return");
    assert!(h.app.session.active_sketch.is_none(), "Return works with the design shown");
}
