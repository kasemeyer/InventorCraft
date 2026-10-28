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
    // `context_menu::items` sends a sheet's viewport target here (no profile list for a sheet).
    let routed = crate::context_menu::items(&h.app, &crate::context_menu::Target::Viewport);
    assert_eq!(ids(&routed), ids(&items(&h.app)));
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
