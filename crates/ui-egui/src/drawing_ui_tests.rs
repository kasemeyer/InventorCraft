#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The drawing UI state and the command → tool table (R2, R11).

use serde_json::json;
use solidcraft_engine::doc::DocumentKind;
use solidcraft_engine::drawing::PaperPick;

use super::*;
use crate::test_support::inventor;

#[test]
fn the_table_holds_only_tool_rows() {
    // Task 3 declares the table; Task 6 gives it placement (updated by design), Tasks 8, 9 and 11
    // their rows.
    let ids: Vec<&str> = DRAWING_STARTS.iter().map(|(id, _)| *id).collect();
    assert_eq!(
        ids,
        [
            "drawing.view.base",
            "drawing.view.projected",
            "drawing.dim.general",
            "drawing.note.text",
            "drawing.note.leader",
            "drawing.note.hole",
            "drawing.centermark",
            "drawing.centerline.bisector",
            "drawing.export.pdf",
            "drawing.export.dxf"
        ]
    );
    assert!(start_for("drawing.view.base").is_some() && start_for("drawing.view.projected").is_some());
    assert!(start_for("drawing.dim.general").is_some());
    for id in ["drawing.sheet.add", "drawing.update", "solid.extrude"] {
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
