//! Explicit dependency save choices for source-backed parts. Child failures leave the assembly
//! unsaved and the list open; skipped source drafts remain recoverable in the assembly.
use crate::{SolveApp, ui_kit};
use serde_json::json;

pub(crate) struct Row {
    pub component: u64,
    pub file: String,
    pub save: bool,
    pub saved: bool,
    pub tab: Option<usize>,
}
pub(crate) struct Panel {
    pub rows: Vec<Row>,
    pub assembly: bool,
    pub error: Option<String>,
    document: std::sync::Arc<solidcraft_engine::doc::Document>,
}

pub(crate) fn request(app: &mut SolveApp) -> bool {
    if app.docs.source_save.is_some() {
        return true;
    }
    if app.file_picker_busy() {
        return false;
    }
    let value = match app.session.execute("component.source_status", &json!({})) {
        Ok(value) => value,
        Err(error) => {
            app.set_status(error.to_string(), true);
            return !app.session.doc.source_edits.is_empty();
        }
    };
    let mut rows: Vec<_> = value["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["dirty"] == true)
        .filter_map(|r| Some(Row { component: r["component"].as_u64()?, file: r["file"].as_str()?.to_string(), save: true, saved: false, tab: None }))
        .collect();
    let key = |p: &str| std::path::Path::new(p).canonicalize().unwrap_or_else(|_| std::path::PathBuf::from(p));
    if let Ok(links) = app.session.execute("component.links", &json!({})) {
        for (index, path) in crate::documents::dirty_linked_tabs(app) {
            if rows.iter().any(|r| key(&r.file) == key(&path)) {
                continue;
            }
            if let Some(component) = links["links"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|l| l["file"].as_str().is_some_and(|p| key(p) == key(&path)))
                .and_then(|l| l["component"].as_u64())
            {
                rows.push(Row { component, file: path, save: true, saved: false, tab: Some(index) });
            }
        }
    }
    if rows.is_empty() {
        return false;
    }
    app.docs.source_save = Some(Panel { rows, assembly: true, error: None, document: app.session.doc.clone() });
    true
}
pub(crate) fn apply(app: &mut SolveApp, mut panel: Panel, all: bool) {
    if !std::sync::Arc::ptr_eq(&panel.document, &app.session.doc) {
        app.set_status("The document changed. Review its save list again.", true);
        return;
    }
    for row in &mut panel.rows {
        if !row.saved && (all || row.save) {
            if row.tab.is_none() && crate::documents::source_tab_dirty(app, &row.file) {
                panel.error = Some("The source tab has unsaved changes. Save or close that tab before saving this edit.".into());
                panel.document = app.session.doc.clone();
                app.docs.source_save = Some(panel);
                return;
            }
            let outcome = if let Some(index) = row.tab {
                crate::documents::save_source_tab(app, index, &row.file)
                    .and_then(|()| app.run("component.update_links", json!({"component":row.component})).map(|_| ()))
            } else {
                app.run("component.save_source", json!({"component":row.component})).map(|_| ())
            };
            if let Err(error) = outcome {
                panel.error = Some(error);
                panel.document = app.session.doc.clone();
                app.docs.source_save = Some(panel);
                return;
            }
            if row.tab.is_none()
                && let Err(error) = crate::documents::refresh_source_tab(app, &row.file)
            {
                app.set_status(error, true);
            }
            row.saved = true;
            row.save = false;
        }
    }
    if all || panel.assembly {
        crate::toolbar::save_document(app);
    }
}
pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut panel) = app.docs.source_save.take() else {
        return;
    };
    if !std::sync::Arc::ptr_eq(&panel.document, &app.session.doc) {
        app.set_status("The document changed. Review its save list again.", true);
        return;
    }
    let (closed, action) = ui_kit::Window::new(egui::Id::new("source_save"), "save", "Save Changes")
        .actions("Save Selected", "Cancel", Some("Save All"))
        .show(ctx, true, |ui| {
            ui_kit::text(ui, "Choose the files to save.", ui_kit::Text::Report("source_save_intro", false));
            egui::ScrollArea::vertical().max_height(360.).show(ui, |ui| {
                let root = app.session.path.as_deref().unwrap_or("Assembly (choose a file)");
                let name = std::path::Path::new(root).file_name().map_or_else(|| root.to_string(), |n| n.to_string_lossy().into_owned());
                ui_kit::checkbox(ui, &format!("{name} (Assembly)"), &mut panel.assembly).on_hover_text(root);
                for row in &mut panel.rows {
                    let name = std::path::Path::new(&row.file).file_name().map_or_else(|| row.file.clone(), |n| n.to_string_lossy().into_owned());
                    let state = if row.saved { "Saved" } else { "Modified" };
                    ui_kit::checkbox(ui, &format!("{name} ({state})"), &mut row.save).on_hover_text(&row.file);
                }
            });
            if let Some(error) = &panel.error {
                ui_kit::text(ui, error, ui_kit::Text::Report("source_save_error", true));
            }
        });
    match action {
        ui_kit::Action::Ok => apply(app, panel, false),
        ui_kit::Action::Apply => apply(app, panel, true),
        ui_kit::Action::Cancel => {}
        _ if closed => {}
        _ => app.docs.source_save = Some(panel),
    }
}
