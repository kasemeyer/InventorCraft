//! A drawing's links to its models (Plan D-2 Task 11): the open tabs serve a view its model as
//! the tab holds it, saved or not (D-1's `ModelProvider`); the active drawing is brought up to
//! date when its tab becomes active and before it is saved (R68), never per frame; PDF and DXF
//! export go through the save picker and run on the frame after the status line says so (R89);
//! Save As of a new drawing starts beside its model (owner default; §8.6).

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde_json::json;
use solidcraft_engine::doc::{Document, DocumentKind};

use crate::SolveApp;

/// Each tab's saved path (absolute) and document, as the last frame left them.
pub(crate) type Snapshot = Arc<RwLock<Vec<(PathBuf, Arc<Document>)>>>;

/// D-1's ModelProvider over the open tabs: the document of the tab whose saved path is `path`.
pub(crate) struct OpenTabs(pub(crate) Snapshot);

impl solidcraft_engine::drawing::ModelProvider for OpenTabs {
    fn document(&self, path: &Path) -> Option<Arc<Document>> {
        let tabs = self.0.read().ok()?;
        tabs.iter().find(|(at, _)| same_file(at, path)).map(|(_, doc)| doc.clone())
    }
}

/// `p` absolute (the engine asks with absolute paths).
fn absolute(p: &str) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| PathBuf::from(p))
}

/// Whether two absolute paths name one file (Windows paths ignore case).
fn same_file(a: &Path, b: &Path) -> bool {
    if cfg!(windows) { a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase() } else { a == b }
}

/// Give `session` the open tabs as its model provider (every session a tab takes).
pub(crate) fn install(app: &SolveApp, session: &mut solidcraft_engine::Session) {
    session.model_provider = Some(Arc::new(OpenTabs(app.docs.open_tabs.clone())));
}

/// Refresh the shared snapshot from the tabs (each frame, and before a refresh): Arc clones only.
pub(crate) fn sync(app: &mut SolveApp) {
    if app.session.model_provider.is_none() {
        let provider: Arc<dyn solidcraft_engine::drawing::ModelProvider> = Arc::new(OpenTabs(app.docs.open_tabs.clone()));
        app.session.model_provider = Some(provider);
    }
    let tabs: Vec<(PathBuf, Arc<Document>)> =
        crate::documents::tab_docs(app).into_iter().filter_map(|(path, doc)| path.map(|p| (absolute(&p), doc))).collect();
    if let Ok(mut shared) = app.docs.open_tabs.write() {
        *shared = tabs;
    }
}

/// R68: `drawing.update {}` on the active drawing (no step when nothing changed); its `missing`
/// → the status line.
pub(crate) fn refresh_drawing(app: &mut SolveApp) {
    if app.session.doc.kind != DocumentKind::Drawing || app.session.doc.drawing.as_deref().is_none_or(|m| m.views.is_empty()) {
        return;
    }
    sync(app);
    match app.run("drawing.update", json!({})) {
        Ok(r) => {
            let missing: Vec<&str> = r["missing"].as_array().map(|a| a.iter().filter_map(|p| p.as_str()).collect()).unwrap_or_default();
            if !missing.is_empty() {
                let text = crate::i18n::format("ui.drawing.models_missing", &[("paths", &missing.join(", "))]);
                app.set_status(text, false);
            }
        }
        Err(e) => app.set_status(e, true),
    }
}

/// The model the active drawing is of: its first view's (where it is now, else as stored), else
/// the one the open Base View panel names.
fn model_of(app: &SolveApp) -> Option<String> {
    let m = app.session.doc.drawing.as_deref()?;
    if let Some(v) = m.views.values().next() {
        return Some(crate::browser::drawing::model_path_of(app, v.id).unwrap_or_else(|| v.model.path.clone()));
    }
    match &app.drawing.panel {
        Some(crate::drawing_ui::DrawingPanel::View(p)) => p.model.clone(),
        _ => None,
    }
}

/// Where Save As of the active drawing starts: where it is saved, else
/// `<model folder>/<model stem>.tcidrw` (owner default; §8.6), else its name.
pub(crate) fn save_as_name(app: &SolveApp) -> String {
    let ext = DocumentKind::Drawing.extension();
    if let Some(path) = &app.session.path {
        return path.clone();
    }
    match model_of(app) {
        Some(model) => solidcraft_engine::drawing::default_drawing_path(&absolute(&model).to_string_lossy()),
        None => format!("{}.{ext}", app.session.doc.name),
    }
}

/// An export the save picker answered: it runs on the frame after the status line says so.
pub(crate) struct PendingExport {
    command: &'static str,
    path: String,
    /// The status line has had a frame to paint.
    painted: bool,
}

/// Where an export starts: beside the saved drawing (its stem), else the drawing's name.
fn export_name(app: &SolveApp, ext: &str) -> String {
    match &app.session.path {
        Some(path) => Path::new(path).with_extension(ext).to_string_lossy().into_owned(),
        None => format!("{}.{ext}", app.session.doc.name),
    }
}

fn export(app: &mut SolveApp, command: &'static str, ext: &'static str) {
    if app.session.doc.kind != DocumentKind::Drawing {
        return;
    }
    let name = export_name(app, ext);
    app.pick_save(&name, &[ext], move |app, path| {
        let Some(path) = path else { return };
        app.drawing.export = Some(PendingExport { command, path, painted: false });
        app.set_status(crate::i18n::tr("ui.drawing.exporting").to_string(), false);
    });
}

/// Save picker (`.pdf`, starting beside the drawing), then the export command with that path on
/// the next frame, after `ui.drawing.exporting` has painted (R89).
pub(crate) fn export_pdf(app: &mut SolveApp) {
    export(app, "drawing.export.pdf", "pdf");
}

/// As [`export_pdf`], for the active sheet as `.dxf`.
pub(crate) fn export_dxf(app: &mut SolveApp) {
    export(app, "drawing.export.dxf", "dxf");
}

/// Each frame: the tabs' snapshot, and an answered export once its status has painted.
pub(crate) fn tick(app: &mut SolveApp) {
    sync(app);
    let Some(pending) = app.drawing.export.as_mut() else { return };
    if !pending.painted {
        pending.painted = true;
        return;
    }
    let Some(pending) = app.drawing.export.take() else { return };
    run_export(app, pending);
}

/// An answered export the active drawing is about to lose (its tab is leaving, R89's paint frame
/// not reached yet): it runs now, so the answer is never dropped.
pub(crate) fn flush_export(app: &mut SolveApp) {
    if let Some(pending) = app.drawing.export.take() {
        run_export(app, pending);
    }
}

fn run_export(app: &mut SolveApp, PendingExport { command, path, .. }: PendingExport) {
    match app.run(command, json!({ "path": path })) {
        Ok(r) => {
            let pages = r["pages"].as_u64().map_or_else(String::new, |n| n.to_string());
            let text = if pages.is_empty() {
                crate::i18n::format("ui.drawing.exported", &[("path", &path)])
            } else {
                crate::i18n::format("ui.drawing.exported_pages", &[("path", &path), ("pages", &pages)])
            };
            app.set_status(text, false);
        }
        // An engine failure has already said so (`finish_command`, as an error); a run refused
        // before the engine (another file operation under way) has not: never leave
        // "Exporting…" standing over a failed export.
        Err(e) => {
            if app.status.as_ref().is_none_or(|(_, _, error)| !*error) {
                app.set_status(e, true);
            }
        }
    }
}

#[cfg(test)]
#[path = "drawing_models_tests.rs"]
mod tests;
