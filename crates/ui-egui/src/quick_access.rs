//! Inventor's Quick Access Toolbar: New, Open, Save, Undo, Redo, the Select filter, Return and
//! Update in the title bar, under a profile with `quick_access`. SolidCraft's bar is unchanged.

use crate::SolveApp;
use crate::toolbar::{APP_BAR_HEIGHT, FileAction, design_shown};
use crate::ui_kit::{Container, Surface};

/// The Select drop-down is open (a temp flag, as the File menu's is).
pub(crate) const SELECT: &str = "sc_qat_select";
/// The left edge of the Select button, where its drop-down hangs.
pub(crate) const SELECT_X: &str = "sc_qat_select_x";
/// The pass the drop-down opened on: that click is not a click outside it.
const SELECT_OPENED: &str = "sc_qat_select_opened";

/// The Quick Access Toolbar's entries, in order, for the active state: (id, icon, enabled).
/// Ids: "new", "open", "save", "undo", "redo", "select", "return", "update".
pub(crate) fn entries(app: &SolveApp) -> Vec<(&'static str, &'static str, bool)> {
    let update = solidcraft_engine::find_command("timeline.compute_all").is_some_and(|c| c.info(&app.session).enabled);
    vec![
        ("new", "new", FileAction::New.enabled(app)),
        ("open", "open", FileAction::Open.enabled(app)),
        ("save", "save", FileAction::Save.enabled(app)),
        ("undo", "undo", FileAction::Undo.enabled(app)),
        ("redo", "redo", FileAction::Redo.enabled(app)),
        ("select", "filter", design_shown(app)),
        ("return", "finish", app.session.active_sketch.is_some()),
        ("update", "compute", update),
    ]
}

/// An entry's tooltip: its name and shortcut, or why it waits for a design.
pub(crate) fn tip(app: &SolveApp, id: &str) -> String {
    let (key, shortcut) = match id {
        "new" => ("ui.qat.new", "Ctrl+N"),
        "open" => ("ui.open", "Ctrl+O"),
        "save" => ("ui.save", "Ctrl+S"),
        "undo" => ("ui.undo", "Ctrl+Z"),
        "redo" => ("ui.redo", "Ctrl+Y"),
        "select" => ("ui.qat.select", ""),
        "return" => ("ui.qat.return", ""),
        _ => ("ui.qat.update", ""),
    };
    let label = app.tr(key);
    let needs_design = matches!(id, "save" | "undo" | "redo" | "select");
    if needs_design && !design_shown(app) {
        crate::i18n::format("ui.needs_design", &[("command", label)])
    } else if shortcut.is_empty() {
        label.to_string()
    } else {
        format!("{label} ({shortcut})")
    }
}

/// Run an entry (the Select entry toggles its drop-down). A disabled entry does nothing.
pub(crate) fn run(app: &mut SolveApp, ctx: &egui::Context, id: &str) {
    if !entries(app).iter().any(|(entry, _, enabled)| *entry == id && *enabled) {
        return;
    }
    match id {
        "new" => FileAction::New.run(app),
        "open" => FileAction::Open.run(app),
        "save" => crate::toolbar::save(app),
        "undo" => FileAction::Undo.run(app),
        "redo" => FileAction::Redo.run(app),
        "select" => {
            let open = ctx.data(|d| d.get_temp::<bool>(egui::Id::new(SELECT)).unwrap_or(false));
            let pass = ctx.cumulative_pass_nr();
            ctx.data_mut(|d| {
                d.insert_temp(egui::Id::new(SELECT), !open);
                d.insert_temp(egui::Id::new(SELECT_OPENED), pass);
            });
        }
        "return" => app.start("sketch.finish"),
        "update" => app.start("timeline.compute_all"),
        _ => {}
    }
}

/// Whether the Select drop-down is open.
pub(crate) fn select_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::new(SELECT)).unwrap_or(false))
}

/// Close the Select drop-down.
pub(crate) fn close_select(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(SELECT), false));
}

/// The Select drop-down under its button: the selection filter. A click outside closes it, and
/// Escape does through the shortcuts, as for the File menu.
pub(crate) fn show_select(app: &mut SolveApp, ctx: &egui::Context) {
    if !select_open(ctx) {
        return;
    }
    if !crate::profile::active(app).quick_access || !design_shown(app) {
        close_select(ctx);
        return;
    }
    let x = ctx.data(|d| d.get_temp::<f32>(egui::Id::new(SELECT_X))).unwrap_or(0.0);
    let resp = Container::new(ctx, Surface::Popup)
        .area(egui::Id::new("sc_qat_select_area"))
        .fixed_pos(egui::pos2(x, APP_BAR_HEIGHT))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            crate::ui_kit::Menu::show_frame(ui, crate::ui_kit::Menu::frame(), |ui| crate::toolbar::selection_filter(app, ui));
        });
    let just_opened = ctx.data(|d| d.get_temp::<u64>(egui::Id::new(SELECT_OPENED))) == Some(ctx.cumulative_pass_nr());
    if !just_opened && resp.response.clicked_elsewhere() {
        close_select(ctx);
    }
}

#[cfg(test)]
#[path = "quick_access_tests.rs"]
mod tests;
