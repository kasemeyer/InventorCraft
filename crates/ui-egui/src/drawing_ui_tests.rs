#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The drawing UI state and the command → tool table (R2, R11).

use serde_json::json;
use solidcraft_engine::doc::DocumentKind;
use solidcraft_engine::drawing::PaperPick;

use super::*;
use crate::test_support::inventor;

#[test]
fn the_table_starts_empty_and_holds_no_rowless_command() {
    // Task 3 declares the table; Tasks 6, 8, 9 and 11 add their rows.
    assert!(DRAWING_STARTS.is_empty());
    for id in ["drawing.sheet.add", "drawing.update", "drawing.view.base", "solid.extrude"] {
        assert!(start_for(id).is_none(), "{id}");
    }
}

#[test]
fn a_new_tab_starts_with_a_clear_drawing_state() {
    let mut h = inventor(DocumentKind::Drawing);
    h.app.home.open = false;
    h.app.run("drawing.new", json!({})).unwrap();
    h.frames(2);
    h.app.drawing.selection = vec![PaperPick::View { view: 1 }];
    assert!(!panel_can_ok(&h.app));
    crate::documents::new_tab(&mut h.app);
    h.frames(2);
    assert!(h.app.drawing.selection.is_empty() && h.app.drawing.preview.is_empty());
    assert!(h.app.drawing.tool.is_none() && h.app.drawing.panel.is_none());
}
