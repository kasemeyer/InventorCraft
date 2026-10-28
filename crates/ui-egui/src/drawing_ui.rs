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
    /// The views whose model is found nowhere, and the document they were computed for (its tab
    /// and `Arc` address, R77): the browser's suffix and Open Model read it.
    pub(crate) missing: Option<((usize, usize), Vec<solidcraft_engine::doc::drawing::ViewId>)>,
    /// A placed view being dragged on the sheet (Task 6).
    pub(crate) drag: Option<crate::drawing_place::Drag>,
    /// The last annotation command the note tools sent, and its parameters (Task 9).
    pub(crate) notes_last_sent: Option<(&'static str, serde_json::Value)>,
    /// An export the save picker answered, waiting a frame for its status to paint (Task 11).
    pub(crate) export: Option<crate::drawing_models::PendingExport>,
}

/// The running drawing tool.
pub(crate) enum DrawingTool {
    /// Placing base and projected views (Task 6).
    Place(crate::drawing_place::Place),
    /// General Dimension, and the list's Move Dimension (Task 8).
    Dimension(crate::drawing_dim_tool::DimTool),
    /// Text, Leader Text, Center Mark, Centerline Bisector, Hole and Thread Note (Task 9).
    Note(crate::drawing_notes_tool::NoteTool),
}

/// The ribbon command a running drawing tool stands for, shown pressed while it runs (fix wave
/// 2): the button that started it. Move Dimension and Move Note come from a list, not a button.
pub(crate) fn command_of(tool: &DrawingTool) -> Option<&'static str> {
    use crate::drawing_notes_tool::NoteTool;
    match tool {
        DrawingTool::Place(p) if p.from_base => Some("drawing.view.base"),
        DrawingTool::Place(_) => Some("drawing.view.projected"),
        DrawingTool::Dimension(t) if t.moving.is_some() => None,
        DrawingTool::Dimension(_) => Some("drawing.dim.general"),
        DrawingTool::Note(t) => match t {
            NoteTool::Text { .. } => Some("drawing.note.text"),
            NoteTool::Leader { .. } => Some("drawing.note.leader"),
            NoteTool::CenterMark { .. } => Some("drawing.centermark"),
            NoteTool::Bisector { .. } => Some("drawing.centerline.bisector"),
            NoteTool::HoleNote { .. } => Some("drawing.note.hole"),
            NoteTool::Move { .. } => None,
        },
    }
}

/// The open drawing panel.
pub(crate) enum DrawingPanel {
    /// Base View and Edit View (Task 5).
    View(crate::drawing_view_panel::ViewPanel),
    /// Drawing Properties: the title block's fields (Task 7).
    Properties(crate::browser::drawing::PropertiesPanel),
    /// Edit Dimension (Task 8).
    Dimension(crate::drawing_dim_tool::DimPanel),
    /// A note's text, new or edited (Task 9).
    Text(crate::drawing_notes_tool::TextPanel),
}

/// Whether the open panel would accept OK now (R72; each panel variant answers for itself).
pub(crate) fn panel_can_ok(app: &SolveApp) -> bool {
    match &app.drawing.panel {
        // OK with nothing changed closes the panel and sends nothing (R87).
        Some(DrawingPanel::Properties(_)) => true,
        Some(DrawingPanel::View(p)) => p.can_ok(),
        // OK with nothing changed closes the panel and sends nothing.
        Some(DrawingPanel::Dimension(_)) => true,
        Some(DrawingPanel::Text(p)) => p.can_ok(),
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
        Some(DrawingPanel::Dimension(mut p)) => {
            if !crate::drawing_dim_tool::apply(app, &mut p) {
                app.drawing.panel = Some(DrawingPanel::Dimension(p));
            }
        }
        Some(DrawingPanel::Text(mut p)) => {
            if !crate::drawing_notes_tool::apply(app, &mut p) {
                app.drawing.panel = Some(DrawingPanel::Text(p));
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
        Some(DrawingTool::Dimension(_)) => crate::drawing_dim_tool::finish(app),
        Some(DrawingTool::Note(_)) => crate::drawing_notes_tool::finish(app),
        None => {}
    }
}

/// What the status line asks for while a drawing tool runs (a catalog key).
pub(crate) fn prompt(app: &SolveApp) -> Option<&'static str> {
    crate::drawing_dim_tool::prompt(app).or_else(|| crate::drawing_notes_tool::prompt(app))
}

/// Cancel on the open panel: it closes and sends nothing (a note's text panel also drops the
/// note its tool was placing; the tool goes on).
pub(crate) fn panel_cancel(app: &mut SolveApp) {
    if let Some(DrawingPanel::Text(p)) = app.drawing.panel.take()
        && p.edit.is_none()
    {
        crate::drawing_notes_tool::reset(app);
    }
}

/// The open panel's card, drawn over the sheet.
pub(crate) fn show_panel(app: &mut SolveApp, ctx: &egui::Context) {
    match app.drawing.panel.take() {
        Some(DrawingPanel::Properties(p)) => crate::browser::drawing::properties_panel(app, ctx, p),
        Some(DrawingPanel::View(p)) => crate::drawing_view_panel::show(app, ctx, p),
        Some(DrawingPanel::Dimension(p)) => crate::drawing_dim_tool::show(app, ctx, p),
        Some(DrawingPanel::Text(p)) => crate::drawing_notes_tool::show(app, ctx, p),
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
pub(crate) const DRAWING_STARTS: &[(&str, fn(&mut SolveApp))] = &[
    ("drawing.view.base", crate::drawing_place::start_base),
    ("drawing.view.projected", crate::drawing_place::start_projected),
    ("drawing.dim.general", crate::drawing_dim_tool::start),
    ("drawing.note.text", crate::drawing_notes_tool::start_text),
    ("drawing.note.leader", crate::drawing_notes_tool::start_leader),
    ("drawing.note.hole", crate::drawing_notes_tool::start_hole_note),
    ("drawing.centermark", crate::drawing_notes_tool::start_center_mark),
    ("drawing.centerline.bisector", crate::drawing_notes_tool::start_bisector),
    ("drawing.export.pdf", crate::drawing_models::export_pdf),
    ("drawing.export.dxf", crate::drawing_models::export_dxf),
];

/// The start `DRAWING_STARTS` gives command `id`, if any.
pub(crate) fn start_for(id: &str) -> Option<fn(&mut SolveApp)> {
    DRAWING_STARTS.iter().find(|(row, _)| *row == id).map(|(_, start)| *start)
}

#[cfg(test)]
#[path = "drawing_ui_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "drawing_ui_ribbon_tests.rs"]
mod ribbon_tests;
