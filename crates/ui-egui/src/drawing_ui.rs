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
    /// The views whose model is found nowhere, and the document they were computed for (its
    /// `Arc` address, R77): the browser's suffix and Open Model read it.
    pub(crate) missing: Option<(usize, Vec<solidcraft_engine::doc::drawing::ViewId>)>,
    /// A placed view being dragged on the sheet (Task 6).
    pub(crate) drag: Option<crate::drawing_place::Drag>,
}

/// The running drawing tool. Task 8 adds `Dimension(drawing_dim_tool::DimTool)`, Task 9
/// `Note(drawing_notes_tool::NoteTool)`.
pub(crate) enum DrawingTool {
    /// Placing base and projected views (Task 6).
    Place(crate::drawing_place::Place),
}

/// Task 8 adds `Dimension(drawing_dim_tool::DimPanel)`, Task 9 `Text(drawing_notes_tool::TextPanel)`.
pub(crate) enum DrawingPanel {
    /// Base View and Edit View (Task 5).
    View(crate::drawing_view_panel::ViewPanel),
    /// Drawing Properties: the title block's fields (Task 7).
    Properties(crate::browser::drawing::PropertiesPanel),
}

/// Whether the open panel would accept OK now (R72; each panel variant answers for itself).
pub(crate) fn panel_can_ok(app: &SolveApp) -> bool {
    match &app.drawing.panel {
        // OK with nothing changed closes the panel and sends nothing (R87).
        Some(DrawingPanel::Properties(_)) => true,
        Some(DrawingPanel::View(p)) => p.can_ok(),
        None => false,
    }
}

/// OK on the open panel (the footer, Enter, the marking menu's OK): it applies and closes, or
/// stays open when its command refused.
pub(crate) fn panel_ok(app: &mut SolveApp) {
    match app.drawing.panel.take() {
        Some(DrawingPanel::Properties(p)) => {
            if !crate::browser::drawing::apply_properties(app, &p) {
                app.drawing.panel = Some(DrawingPanel::Properties(p));
            }
        }
        Some(DrawingPanel::View(mut p)) => {
            if !crate::drawing_view_panel::apply(app, &mut p) {
                app.drawing.panel = Some(DrawingPanel::View(p));
            }
        }
        None => {}
    }
}

/// End the running drawing tool, whichever it is (Esc, Done, Cancel): each tool cleans up after
/// itself (what it placed stays); any other tool is simply dropped with the preview.
pub(crate) fn end_tool(app: &mut SolveApp) {
    match &app.drawing.tool {
        Some(DrawingTool::Place(_)) => crate::drawing_place::finish(app),
        None => {}
    }
}

/// Cancel on the open panel: it closes and sends nothing.
pub(crate) fn panel_cancel(app: &mut SolveApp) {
    app.drawing.panel = None;
}

/// The open panel's card, drawn over the sheet.
pub(crate) fn show_panel(app: &mut SolveApp, ctx: &egui::Context) {
    match app.drawing.panel.take() {
        Some(DrawingPanel::Properties(p)) => crate::browser::drawing::properties_panel(app, ctx, p),
        Some(DrawingPanel::View(p)) => crate::drawing_view_panel::show(app, ctx, p),
        None => {}
    }
}

/// R2/R11: drawing command id → the UI start that runs instead of `run(id, {})`, consulted only
/// for a Drawing document. Rows: Task 6 places views (`drawing.view.base` opens the Base View
/// panel with its live preview, `drawing.view.projected` picks a parent first), Task 8
/// (`drawing.dim.general`), Task 9 (`drawing.note.text`, `drawing.note.leader`,
/// `drawing.note.hole`, `drawing.centermark`, `drawing.centerline.bisector`), Task 11
/// (`drawing.export.pdf`, `drawing.export.dxf`). `drawing.sheet.add` and `drawing.update` take no
/// parameters and run directly, as any command without a tool does.
pub(crate) const DRAWING_STARTS: &[(&str, fn(&mut SolveApp))] =
    &[("drawing.view.base", crate::drawing_place::start_base), ("drawing.view.projected", crate::drawing_place::start_projected)];

/// The start `DRAWING_STARTS` gives command `id`, if any.
pub(crate) fn start_for(id: &str) -> Option<fn(&mut SolveApp)> {
    DRAWING_STARTS.iter().find(|(row, _)| *row == id).map(|(_, start)| *start)
}

#[cfg(test)]
#[path = "drawing_ui_tests.rs"]
mod tests;
