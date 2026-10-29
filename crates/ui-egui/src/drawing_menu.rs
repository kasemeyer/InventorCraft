//! The list a right-click on a drawing sheet opens (R15): the empty-sheet list (Task 3), a
//! running tool's rows (Tasks 6, 9), the view list (Tasks 5, 7), the dimension list (Task 8) and
//! the note list (Task 10). Under a profile with a `drawing_empty` layout the empty sheet's
//! right-click is that ring and its list instead (Task 10); sheet switching lives here too. Every row is built by `marking_menu::item` (R72), so a drawing's
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

/// A dimension's rows (spec §2.7; a list, never a ring). Dimension Type ▸ (wave 3, R101) sits
/// where the capture's Options ▸ does, after Move Dimension.
const DIMENSION: [&str; 8] = [
    "ui.repeat",
    "ui.delete",
    "ui.drawing.moveDim",
    "ui.drawing.dimType",
    "ui.drawing.precision",
    "ui.drawing.editDim",
    "ui.drawing.hideExt",
    "ui.drawing.showExt",
];

/// The sheet's right-click list now.
pub(crate) fn items(app: &SolveApp) -> Vec<Item> {
    if app.drawing.tool.is_none() {
        match app.drawing.selection.as_slice() {
            [PaperPick::Dimension { handle }] => return dimension_items(app, *handle),
            [PaperPick::View { view } | PaperPick::ViewEdge { view, .. }] => return view_items(app, *view),
            [PaperPick::Annotation { handle }] => return note_items(app, *handle),
            _ => {}
        }
    }
    let (ctx, ids): (MenuContext, &[&str]) = if crate::drawing_notes_tool::leader_running(app) {
        (MenuContext::Tool, &LEADER)
    } else if app.drawing.tool.is_some() {
        (MenuContext::Tool, &TOOL)
    } else {
        (MenuContext::Empty, &EMPTY_SHEET)
    };
    let mut items: Vec<Item> = ids.iter().map(|id| crate::marking_menu::item(app, id, ctx)).collect();
    // General Dimension with its picks: Dimension Type ▸ (wave 3, R101).
    if let Some((rows, chosen)) = crate::drawing_dim_tool::type_choices(app) {
        items.push(crate::marking_menu::item(app, "-", ctx));
        items.push(dim_type_item(app, rows, chosen, None));
    }
    items
}

/// Dimension Type ▸ with `rows` (`drawing_dim_tool::LINEAR_TYPES` or `RADIAL_TYPES`), `chosen`
/// checked; on placed dimension `dim` when given, else on the running tool's live dimension.
fn dim_type_item(app: &SolveApp, rows: &[(&str, &str)], chosen: Option<&str>, dim: Option<u64>) -> Item {
    let mut item = crate::marking_menu::item(app, "ui.drawing.dimType", MenuContext::Empty);
    item.children = rows
        .iter()
        .map(|(ty, label)| {
            let params = match dim {
                Some(dim) => json!({ "dim": dim, "type": ty }),
                None => json!({ "type": ty }),
            };
            crate::context_menu::act("ui.drawing.dimType", label, "").with(params).key(if chosen == Some(*ty) { "current" } else { "" })
        })
        .collect();
    item.on(!rows.is_empty())
}

/// A placed dimension's Dimension Type rows and its type now.
fn placed_types(app: &SolveApp, dim: u64) -> (&'static [(&'static str, &'static str)], Option<&'static str>) {
    use crate::drawing_dim_tool::{LINEAR_TYPES, RADIAL_TYPES};
    match app
        .session
        .doc
        .drawing
        .as_deref()
        .and_then(|m| solidcraft_engine::drawing::dimension_kind(m, dim))
        .and_then(crate::drawing_dim_tool::type_of_kind)
    {
        Some(ty @ ("radius" | "diameter")) => (&RADIAL_TYPES, Some(ty)),
        Some(ty) => (&LINEAR_TYPES, Some(ty)),
        None => (&[], None),
    }
}

/// A selected dimension's list: Repeat, Delete, Move, Dimension Type ▸, Precision ▸ 0-6, Edit Dimension, Hide and
/// Show Extension Lines.
fn dimension_items(app: &SolveApp, dim: u64) -> Vec<Item> {
    let params = json!({ "dim": dim });
    DIMENSION
        .iter()
        .map(|id| {
            let item = crate::marking_menu::item(app, id, MenuContext::Empty);
            match *id {
                "ui.repeat" | "ui.delete" => item,
                "ui.drawing.dimType" => {
                    let (rows, now) = placed_types(app, dim);
                    dim_type_item(app, rows, now, Some(dim))
                }
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

/// A note's, leader's or mark's list: Edit Note (only where the pick has text, R75), Move Note
/// and Delete.
fn note_items(app: &SolveApp, handle: u64) -> Vec<Item> {
    let params = json!({ "handle": handle });
    let has_text = app.session.doc.drawing.as_deref().and_then(|m| solidcraft_engine::drawing::note_at(m, handle)).is_some();
    let edit = crate::marking_menu::item(app, "ui.drawing.editNote", MenuContext::Empty).with(params.clone());
    let edit = if has_text { edit } else { edit.on(false) };
    let delete = crate::marking_menu::item(app, "ui.delete", MenuContext::Empty);
    vec![edit, crate::marking_menu::item(app, "ui.drawing.moveNote", MenuContext::Empty).with(params), delete]
}

/// The sheet `delta` sheets from the active one, if there is one (no wrap).
pub(crate) fn sheet_step(app: &SolveApp, delta: isize) -> Option<usize> {
    let m = app.session.doc.drawing.as_deref()?;
    let at = app.session.drawing_sheet.min(m.sheets.len().saturating_sub(1));
    at.checked_add_signed(delta).filter(|i| *i < m.sheets.len())
}

/// Previous or Next Sheet (Ctrl+PageUp / Ctrl+PageDown, the ring's list): the sheet before or
/// after the active one becomes active; nothing at either end.
pub(crate) fn step_sheet(app: &mut SolveApp, delta: isize) {
    if let Some(index) = sheet_step(app, delta) {
        crate::browser::drawing::activate_sheet(app, index);
    }
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
