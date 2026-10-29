//! Edit Sheet (wave 3 item 2, R99, R104): the browser sheet row's Edit Sheet opens it on that
//! sheet. The fields follow the capture's dialog (docs/evidence/drawing-workflow.md §8), in its
//! order, with our own strings:
//! - Format: **Name**, **Size** (A-E), Height and Width (read-only for a standard size), Revision
//!   (shown disabled: not stored yet).
//! - Orientation: **Portrait** / **Landscape**. The capture's four title-block corner radios are
//!   left out: the title block always sits bottom-right.
//! - Options: Exclude from count and Exclude from printing (shown disabled: not stored yet).
//!
//! OK (or Enter) sends one command, so it is always one undo step: `drawing.sheet.set_size` when
//! the size or orientation changed, carrying the new name too when that changed (the border and
//! title block are placed again, and the canvas fits the new sheet); `drawing.sheet.set_name` when
//! only the name changed; nothing when nothing changed. Esc or Cancel closes and sends nothing.
//! Every profile (SolidCraft's drawings too) runs this code.

use serde_json::json;
use solidcraft_engine::doc::drawing::AnsiSize;

use crate::SolveApp;
use crate::drawing_ui::DrawingPanel;
use crate::i18n::tr;
use crate::scenario;

/// The sizes offered, in order.
const SIZES: [AnsiSize; 5] = [AnsiSize::A, AnsiSize::B, AnsiSize::C, AnsiSize::D, AnsiSize::E];

/// The Edit Sheet panel (`DrawingPanel::Sheet`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SheetPanel {
    /// The sheet's index.
    pub sheet: usize,
    pub name: String,
    pub size: AnsiSize,
    pub landscape: bool,
    /// The fields as the panel opened, so OK sends only what changed.
    original: (String, AnsiSize, bool),
}

impl SheetPanel {
    fn new(app: &SolveApp, sheet: usize) -> Option<SheetPanel> {
        let s = app.session.doc.drawing.as_deref()?.sheets.get(sheet)?;
        Some(SheetPanel { sheet, name: s.name.clone(), size: s.size, landscape: s.landscape, original: (s.name.clone(), s.size, s.landscape) })
    }

    /// Whether OK would act now: the sheet has a name.
    pub(crate) fn can_ok(&self) -> bool {
        !self.name.trim().is_empty()
    }
}

/// The sheet row's Edit Sheet: open the panel on sheet `sheet` (not over a running tool).
pub(crate) fn open(app: &mut SolveApp, sheet: usize) {
    if app.drawing.tool.is_some() {
        return;
    }
    if let Some(p) = SheetPanel::new(app, sheet) {
        app.drawing.panel = Some(DrawingPanel::Sheet(p));
    }
}

/// OK: one command for whatever changed (one undo step). False when it refused (the panel stays
/// open). On success the panel's `original` becomes what was applied, so a later OK on the same
/// panel sends only what changed since.
pub(crate) fn apply(app: &mut SolveApp, p: &mut SheetPanel) -> bool {
    if !p.can_ok() {
        return false;
    }
    let name = p.name.trim().to_string();
    let renamed = name != p.original.0;
    if (p.size, p.landscape) != (p.original.1, p.original.2) {
        let mut params = json!({ "sheet": p.sheet, "size": p.size.letter(), "landscape": p.landscape });
        if renamed {
            params["name"] = json!(name);
        }
        if app.run("drawing.sheet.set_size", params).is_err() {
            return false;
        }
        app.drawing.fit_pending = true;
    } else if renamed && app.run("drawing.sheet.set_name", json!({ "sheet": p.sheet, "name": name })).is_err() {
        return false;
    }
    p.original = (name, p.size, p.landscape);
    true
}

/// A field row with a published handle at its control.
fn field<R>(ui: &mut egui::Ui, label: &str, handle: &str, control: impl FnOnce(&mut egui::Ui) -> R) -> R {
    crate::ui_kit::row(ui, label, |ui| {
        scenario::publish_handle(handle, ui.available_rect_before_wrap().left_center());
        control(ui)
    })
}

/// A number in inches as the capture shows it (`22.000`).
fn inches(v: f64) -> String {
    format!("{v:.3}")
}

/// The Edit Sheet card (a `ui_kit::Window`): Enter or OK applies, Esc or Cancel closes.
pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context, mut p: SheetPanel) {
    let menu_enter = crate::marking_menu::holds_keys(app) && ctx.input(|i| i.key_pressed(egui::Key::Enter));
    let mut enter = false;
    let can_ok = p.can_ok();
    let (closed, action) = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_sheet"), "sheet", tr("ui.drawing.edit_sheet_title"))
        .placed(egui::Align2::LEFT_TOP, crate::browser::panel_pos(app))
        .actions(crate::i18n::tr_label("OK"), tr("ui.cancel"), None)
        // Esc is the application's (`toolbar::shortcuts`): the marking menu takes it first.
        .keep_open_on_escape()
        .show(ctx, can_ok, |ui| {
            crate::ui_kit::text(ui, tr("ui.drawing.sheet_format"), crate::ui_kit::Text::Heading("drawing_sheet:format"));
            field(ui, tr("ui.drawing.sheet_name"), "drawing_sheet:name", |ui| enter |= crate::ui_kit::text_field(ui, &mut p.name, "").0);
            field(ui, tr("ui.drawing.sheet_size"), "drawing_sheet:size", |ui| {
                crate::ui_kit::dropdown_menu(ui, "drawing_sheet_size", p.size.letter(), |ui| {
                    for size in SIZES {
                        let row = crate::ui_kit::list_picker(ui, &format!("drawing_sheet_size:{}", size.letter()), size.letter(), p.size == size);
                        scenario::publish_handle(&format!("drawing_sheet:size:{}", size.letter()), row.rect.center());
                        if row.clicked() {
                            p.size = size;
                        }
                    }
                });
            });
            let (w, h) = p.size.inches(p.landscape);
            field(ui, tr("ui.drawing.sheet_height"), "drawing_sheet:height", |ui| {
                crate::ui_kit::text(ui, &inches(h), crate::ui_kit::Text::Value("", false))
            });
            field(ui, tr("ui.drawing.sheet_width"), "drawing_sheet:width", |ui| {
                crate::ui_kit::text(ui, &inches(w), crate::ui_kit::Text::Value("", false))
            });
            // Not stored yet: shown as the capture has it, disabled.
            let mut revision = String::new();
            field(ui, tr("ui.drawing.sheet_revision"), "drawing_sheet:revision", |ui| {
                ui.add_enabled_ui(false, |ui| crate::ui_kit::text_field(ui, &mut revision, ""));
            });
            scenario::publish_count("drawing_sheet:revision:enabled", 0.0);
            crate::ui_kit::text(ui, tr("ui.drawing.sheet_orientation"), crate::ui_kit::Text::Heading("drawing_sheet:orientation"));
            let mut selected = usize::from(p.landscape);
            let options = [tr("ui.drawing.sheet_portrait"), tr("ui.drawing.sheet_landscape")];
            let shown = ui.scope(|ui| crate::ui_kit::segmented(ui, &mut selected, &options, crate::ui_kit::Segments::default()));
            let r = shown.response.rect;
            for (i, handle) in ["drawing_sheet:portrait", "drawing_sheet:landscape"].iter().enumerate() {
                scenario::publish_handle(handle, egui::pos2(r.left() + r.width() * (i as f32 * 2.0 + 1.0) / 4.0, r.center().y));
            }
            p.landscape = selected == 1;
            crate::ui_kit::text(ui, tr("ui.drawing.sheet_options"), crate::ui_kit::Text::Heading("drawing_sheet:options"));
            ui.add_enabled_ui(false, |ui| {
                let (mut count, mut print) = (false, false);
                let r = crate::ui_kit::checkbox(ui, tr("ui.drawing.sheet_exclude_count"), &mut count);
                scenario::publish_handle("drawing_sheet:exclude_count", r.rect.center());
                let r = crate::ui_kit::checkbox(ui, tr("ui.drawing.sheet_exclude_print"), &mut print);
                scenario::publish_handle("drawing_sheet:exclude_print", r.rect.center());
            });
            scenario::publish_count("drawing_sheet:exclude:enabled", 0.0);
        });
    if closed || action == crate::ui_kit::Action::Cancel {
        app.drawing.panel = None;
        return;
    }
    if ((enter && can_ok) || action == crate::ui_kit::Action::Ok) && !menu_enter {
        app.drawing.panel = if apply(app, &mut p) { None } else { Some(DrawingPanel::Sheet(p)) };
        return;
    }
    app.drawing.panel = Some(DrawingPanel::Sheet(p));
}

#[cfg(test)]
#[path = "drawing_sheet_panel_tests.rs"]
mod tests;
