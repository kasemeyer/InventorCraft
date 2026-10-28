//! The list a right-click on a drawing sheet opens (R15): the empty-sheet list (Task 3), a
//! running tool's rows (Tasks 6, 9), the view list (Tasks 5, 7), the dimension list (Task 8) and
//! the note list (Task 10). Every row is built by `marking_menu::item` (R72), so a drawing's
//! Done, Cancel, OK and Delete answer to the drawing's own tool, panel and selection.

use serde_json::json;
use solidcraft_engine::drawing::PaperPick;

use crate::SolveApp;
use crate::context_menu::Item;
use crate::profile_workflow::MenuContext;

/// The empty sheet's rows: place a view, add a sheet, refresh the views, undo.
const EMPTY_SHEET: [&str; 5] = ["drawing.view.base", "drawing.sheet.add", "drawing.update", "-", "edit.undo"];

/// A running drawing tool's rows (spec §2.4: Done is the action that ends it).
const TOOL: [&str; 2] = ["ui.done", "ui.cancel"];

/// Leader Text's rows (spec §2.8): Continue, Back, Single-Segment Leader, Cancel.
const LEADER: [&str; 4] = ["ui.done", "ui.drawing.back", "ui.drawing.singleSegment", "ui.cancel"];

/// A dimension's rows (spec §2.7; a list, never a ring).
const DIMENSION: [&str; 7] =
    ["ui.repeat", "ui.delete", "ui.drawing.moveDim", "ui.drawing.precision", "ui.drawing.editDim", "ui.drawing.hideExt", "ui.drawing.showExt"];

/// The sheet's right-click list now.
pub(crate) fn items(app: &SolveApp) -> Vec<Item> {
    if app.drawing.tool.is_none()
        && let [PaperPick::Dimension { handle }] = app.drawing.selection.as_slice()
    {
        return dimension_items(app, *handle);
    }
    let (ctx, ids): (MenuContext, &[&str]) = if crate::drawing_notes_tool::leader_running(app) {
        (MenuContext::Tool, &LEADER)
    } else if app.drawing.tool.is_some() {
        (MenuContext::Tool, &TOOL)
    } else {
        (MenuContext::Empty, &EMPTY_SHEET)
    };
    ids.iter().map(|id| crate::marking_menu::item(app, id, ctx)).collect()
}

/// A selected dimension's list: Repeat, Delete, Move, Precision ▸ 0-6, Edit Dimension, Hide and
/// Show Extension Lines.
fn dimension_items(app: &SolveApp, dim: u64) -> Vec<Item> {
    let params = json!({ "dim": dim });
    DIMENSION
        .iter()
        .map(|id| {
            let item = crate::marking_menu::item(app, id, MenuContext::Empty);
            match *id {
                "ui.repeat" | "ui.delete" => item,
                "ui.drawing.precision" => {
                    let mut item = item.with(params.clone());
                    item.children =
                        (0..=6u8).map(|n| crate::context_menu::act(id, &n.to_string(), "").with(json!({ "dim": dim, "precision": n }))).collect();
                    item
                }
                _ => item.with(params.clone()),
            }
        })
        .collect()
}

/// A view's list (R40), in the browser (Task 7) and for a view selected on the sheet (Task 10):
/// Edit View (the Base View panel on it, R93), Delete (the sheet's selection, R72) and Open Model
/// (where the view's model is now, R77; disabled when it is found nowhere).
pub(crate) fn view_items(app: &SolveApp, view: u64) -> Vec<Item> {
    let edit = crate::marking_menu::item(app, "ui.drawing.editView", MenuContext::Empty).with(json!({ "view": view }));
    let open = crate::marking_menu::item(app, "ui.drawing.openModel", MenuContext::Empty)
        .with(json!({ "view": view }))
        .on(crate::browser::drawing::model_path_of(app, view).is_some());
    vec![edit, crate::marking_menu::item(app, "ui.delete", MenuContext::Empty), open]
}

/// Delete on a drawing (the Delete key, a menu's Delete): what the sheet's selection holds goes,
/// never the part's delete. Annotations go in one `drawing.annotation.delete`, then each selected
/// view not under another selected view in one `drawing.view.delete` (its projected views and
/// attached annotations with it); a single pick is one undo step. Picked view edges stay.
pub(crate) fn delete_selection(app: &mut SolveApp) {
    let picks = std::mem::take(&mut app.drawing.selection);
    let Some(m) = app.session.doc.drawing.clone() else { return };
    let mut handles: Vec<u64> = Vec::new();
    let mut views: Vec<u64> = Vec::new();
    for p in &picks {
        match p {
            PaperPick::Dimension { handle } | PaperPick::Annotation { handle } if !handles.contains(handle) => handles.push(*handle),
            PaperPick::View { view } if !views.contains(view) => views.push(*view),
            _ => {}
        }
    }
    // A view whose parent (or its parent's parent) is selected goes with it.
    let under_selected = |mut v: u64| {
        while let Some(parent) = m.views.get(&v).and_then(|x| x.parent) {
            if views.contains(&parent) {
                return true;
            }
            v = parent;
        }
        false
    };
    let views: Vec<u64> = views.iter().copied().filter(|v| m.views.contains_key(v) && !under_selected(*v)).collect();
    if !handles.is_empty() {
        let _ = app.run("drawing.annotation.delete", json!({ "handles": handles }));
    }
    for view in views {
        if app.session.doc.drawing.as_deref().is_some_and(|m| m.views.contains_key(&view)) {
            let _ = app.run("drawing.view.delete", json!({ "view": view }));
        }
    }
}

#[cfg(test)]
#[path = "drawing_menu_tests.rs"]
mod tests;
