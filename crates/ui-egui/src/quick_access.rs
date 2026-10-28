//! Inventor's Quick Access Toolbar: New (and its flyout of document kinds), Open, Save, Undo,
//! Redo, the Select filter, Return and Update in the title bar, under a profile with
//! `quick_access`. SolidCraft's bar is unchanged.

use solidcraft_engine::doc::DocumentKind;

use crate::SolveApp;
use crate::toolbar::{APP_BAR_HEIGHT, FileAction, design_shown};
use crate::ui_kit::{Container, Surface};

/// The Select drop-down is open (a temp flag, as the File menu's is).
pub(crate) const SELECT: &str = "sc_qat_select";
/// The left edge of the Select button, where its drop-down hangs.
pub(crate) const SELECT_X: &str = "sc_qat_select_x";
/// The pass the drop-down opened on: that click is not a click outside it.
const SELECT_OPENED: &str = "sc_qat_select_opened";
/// The New flyout is open (the arrow beside New).
pub(crate) const NEW_FLYOUT: &str = "sc_qat_new";
/// The left edge of the New button, where its flyout hangs.
pub(crate) const NEW_X: &str = "sc_qat_new_x";
/// The pass the flyout opened on.
const NEW_OPENED: &str = "sc_qat_new_opened";

/// The New flyout's rows: (menu id, kind, icon, label id), in the capture's order minus the kinds TCI-CAD
/// has no document for [E: DW §1 row 2a]. One drawing template, so no template sub-list
/// (owner default; §8.3).
pub(crate) const NEW_KINDS: [(&str, DocumentKind, &str, &str); 3] = [
    ("qat.new.assembly", DocumentKind::Assembly, "component", "ui.qat.new_assembly"),
    ("qat.new.drawing", DocumentKind::Drawing, "ws_drawing", "ui.qat.new_drawing"),
    ("qat.new.part", DocumentKind::Part, "body", "ui.qat.new_part"),
];

/// The Quick Access Toolbar's entries, in order, for the active state: (id, icon, enabled).
/// Ids: "new", "open", "save", "undo", "redo", "select", "return", "update".
pub(crate) fn entries(app: &SolveApp) -> Vec<(&'static str, &'static str, bool)> {
    // Return and Update act on the design, so they wait behind Home like Save and Select.
    let live = design_shown(app) && !app.file_load.busy();
    let update = live && solidcraft_engine::find_command("timeline.compute_all").is_some_and(|c| c.info(&app.session).enabled);
    vec![
        ("new", "new", FileAction::New.enabled(app)),
        ("open", "open", FileAction::Open.enabled(app)),
        ("save", "save", FileAction::Save.enabled(app)),
        ("undo", "undo", FileAction::Undo.enabled(app)),
        ("redo", "redo", FileAction::Redo.enabled(app)),
        ("select", "filter", design_shown(app)),
        ("return", "finish", live && app.session.active_sketch.is_some()),
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
    let needs_design = matches!(id, "save" | "undo" | "redo" | "select" | "return" | "update");
    if needs_design && !design_shown(app) {
        crate::i18n::format("ui.needs_design", &[("command", label)])
    } else if shortcut.is_empty() {
        label.to_string()
    } else {
        format!("{label} ({shortcut})")
    }
}

/// Run an entry (the Select entry toggles its drop-down). A disabled entry does nothing, and no
/// entry runs while a file loads.
pub(crate) fn run(app: &mut SolveApp, ctx: &egui::Context, id: &str) {
    let entry = if id == "new_arrow" { "new" } else { id };
    if app.file_load.busy() || !entries(app).iter().any(|(e, _, enabled)| *e == entry && *enabled) {
        return;
    }
    match id {
        "new" => FileAction::New.run(app),
        "new_arrow" => toggle(ctx, NEW_FLYOUT, NEW_OPENED),
        "open" => FileAction::Open.run(app),
        "save" => crate::toolbar::save(app),
        "undo" => FileAction::Undo.run(app),
        "redo" => FileAction::Redo.run(app),
        "select" => toggle(ctx, SELECT, SELECT_OPENED),
        "return" => app.start("sketch.finish"),
        "update" => app.start("timeline.compute_all"),
        _ => {}
    }
}

/// Open or close the drop-down `flag`, remembering the pass it opened on (`opened`).
fn toggle(ctx: &egui::Context, flag: &str, opened: &str) {
    let open = ctx.data(|d| d.get_temp::<bool>(egui::Id::new(flag)).unwrap_or(false));
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        d.insert_temp(egui::Id::new(flag), !open);
        d.insert_temp(egui::Id::new(opened), pass);
    });
}

/// A new document of `kind` from the New flyout: a part or an assembly as File › New Part and
/// New Assembly make them (Units of new designs), a drawing as File › New Drawing.
pub(crate) fn new_kind(app: &mut SolveApp, kind: DocumentKind) {
    match kind {
        DocumentKind::Part => FileAction::NewPart(None).run(app),
        DocumentKind::Assembly => FileAction::NewAssembly(None).run(app),
        DocumentKind::Drawing => FileAction::NewDrawing.run(app),
    }
}

/// Whether the New flyout is open.
pub(crate) fn new_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::new(NEW_FLYOUT)).unwrap_or(false))
}

/// Close the New flyout.
pub(crate) fn close_new(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(NEW_FLYOUT), false));
}

/// The New flyout under its button: Assembly, Drawing, Part. A click outside closes it, and
/// Escape does through the shortcuts, as for the Select drop-down.
pub(crate) fn show_new(app: &mut SolveApp, ctx: &egui::Context) {
    if !new_open(ctx) {
        return;
    }
    if !crate::profile::active(app).quick_access || !FileAction::New.enabled(app) {
        close_new(ctx);
        return;
    }
    let x = ctx.data(|d| d.get_temp::<f32>(egui::Id::new(NEW_X))).unwrap_or(0.0);
    let mut picked = None;
    let resp = Container::new(ctx, Surface::Popup)
        .area(egui::Id::new("sc_qat_new_area"))
        .fixed_pos(egui::pos2(x, APP_BAR_HEIGHT))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            crate::ui_kit::Menu::show_frame(ui, crate::ui_kit::Menu::frame(), |ui| {
                ui.set_min_width(150.0);
                for (id, kind, icon, label) in NEW_KINDS {
                    if crate::context_menu::list_row(ui, &crate::context_menu::Item::action(id, app.tr(label), icon)).is_some() {
                        picked = Some(kind);
                    }
                }
            });
        });
    if let Some(kind) = picked {
        close_new(ctx);
        new_kind(app, kind);
        return;
    }
    let just_opened = ctx.data(|d| d.get_temp::<u64>(egui::Id::new(NEW_OPENED))) == Some(ctx.cumulative_pass_nr());
    if !just_opened && resp.response.clicked_elsewhere() {
        close_new(ctx);
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
