//! The list a right-click on a drawing sheet opens (R15): the empty-sheet list (Task 3), a
//! running tool's rows (Tasks 6, 9), the view list (Tasks 5, 7), the dimension list (Task 8) and
//! the note list (Task 10). Every row is built by `marking_menu::item` (R72), so a drawing's
//! Done, Cancel, OK and Delete answer to the drawing's own tool, panel and selection.

use crate::SolveApp;
use crate::context_menu::Item;
use crate::profile_workflow::MenuContext;

/// The empty sheet's rows: place a view, add a sheet, refresh the views, undo.
const EMPTY_SHEET: [&str; 5] = ["drawing.view.base", "drawing.sheet.add", "drawing.update", "-", "edit.undo"];

/// A running drawing tool's rows (spec §2.4: Done is the action that ends it).
const TOOL: [&str; 2] = ["ui.done", "ui.cancel"];

/// The sheet's right-click list now.
pub(crate) fn items(app: &SolveApp) -> Vec<Item> {
    let (ctx, ids): (MenuContext, &[&str]) = if app.drawing.tool.is_some() { (MenuContext::Tool, &TOOL) } else { (MenuContext::Empty, &EMPTY_SHEET) };
    ids.iter().map(|id| crate::marking_menu::item(app, id, ctx)).collect()
}

#[cfg(test)]
#[path = "drawing_menu_tests.rs"]
mod tests;
