//! The drawing environment's UI state (Plan D-2): what is selected on the sheet, the running
//! drawing tool and open drawing panel, the live preview, and the command → tool table (R2, R11).
//! It lives on `SolveApp::drawing` and is reset when the active tab changes.

use solidcraft_engine::drawing::{PaperCurve, PaperPick};

use crate::SolveApp;

#[derive(Default)]
pub(crate) struct DrawingUi {
    /// Paper picks selected on the sheet (D-1 `PaperPick`), in click order.
    pub selection: Vec<PaperPick>,
    /// The running drawing tool (Tasks 6, 8, 9), if any.
    pub tool: Option<DrawingTool>,
    /// The open drawing panel (Tasks 5, 7, 8, 9), if any.
    pub panel: Option<DrawingPanel>,
    /// Preview curves on paper (inches), drawn over the sheet, never saved (Task 6). `true`: hidden.
    pub preview: Vec<(PaperCurve, bool)>,
    /// Fit the sheet in the canvas on its next frame (`ui.view_fit`, `ui.home`, Home).
    pub fit_pending: bool,
    /// The scene the canvas drew last, and what it was built from.
    pub(crate) scene: Option<crate::sheet_canvas::SceneCache>,
}

/// Declared with no variants in Task 3 (R12). Task 6 adds `Place(drawing_place::Place)`, Task 8
/// `Dimension(drawing_dim_tool::DimTool)`, Task 9 `Note(drawing_notes_tool::NoteTool)`.
pub(crate) enum DrawingTool {}

/// Task 5 adds `View(drawing_view_panel::ViewPanel)`, Task 7 `Properties(browser::drawing::PropertiesPanel)`,
/// Task 8 `Dimension(drawing_dim_tool::DimPanel)`, Task 9 `Text(drawing_notes_tool::TextPanel)`.
pub(crate) enum DrawingPanel {}

/// Whether the open panel would accept OK now (R72; each panel variant answers for itself).
pub(crate) fn panel_can_ok(app: &SolveApp) -> bool {
    match &app.drawing.panel {
        Some(panel) => match *panel {},
        None => false,
    }
}

/// R2/R11: drawing command id → the UI start that runs instead of `run(id, {})`, consulted only
/// for a Drawing document. Rows: Task 6 (`drawing.view.base`, `drawing.view.projected`), Task 8
/// (`drawing.dim.general`), Task 9 (`drawing.note.text`, `drawing.note.leader`,
/// `drawing.note.hole`, `drawing.centermark`, `drawing.centerline.bisector`), Task 11
/// (`drawing.export.pdf`, `drawing.export.dxf`). `drawing.sheet.add` and `drawing.update` take no
/// parameters and run directly, as any command without a tool does.
pub(crate) const DRAWING_STARTS: &[(&str, fn(&mut SolveApp))] = &[];

/// The start `DRAWING_STARTS` gives command `id`, if any.
pub(crate) fn start_for(id: &str) -> Option<fn(&mut SolveApp)> {
    DRAWING_STARTS.iter().find(|(row, _)| *row == id).map(|(_, start)| *start)
}

#[cfg(test)]
#[path = "drawing_ui_tests.rs"]
mod tests;
