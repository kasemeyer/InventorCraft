//! The constraint value box at the foot of the browser (R185, Inventor's edit box): a single
//! click on a constraint's row (any of its three) picks it and shows its offset here, in the
//! document's units (an Angle in degrees). Enter edits it (`constraint.edit {offset}`, one
//! step), Escape puts the record's value back. It follows the picked row, hides when no row or
//! several are picked or the constraint is Lost, and is disabled while it is suppressed.

use serde_json::json;

use crate::SolveApp;
use crate::theme::Tokens;
use crate::ui_kit;

/// The box's text and what it started from.
#[derive(Clone, Debug, Default)]
pub struct ValueBox {
    pub constraint: u64,
    pub text: String,
    /// The record's value as the box shows it (its authored text, else the number formatted).
    pub base: String,
    pub error: Option<String>,
}

/// The constraint the box is for, whether it may be edited, and whether it is an angle.
fn target(app: &SolveApp) -> Option<(u64, bool, bool)> {
    use solidcraft_engine::doc::constraints::ConstraintStatus as S;
    if app.session.doc.kind == solidcraft_engine::doc::DocumentKind::Drawing {
        return None;
    }
    let [id] = app.tree.picked_constraints.as_slice() else { return None };
    let row = app.session.constraint_report.rows.iter().find(|r| r.id == *id)?;
    if matches!(row.status, S::Lost { .. }) {
        return None;
    }
    Some((*id, row.status != S::Suppressed, row.kind.is_angle()))
}

/// The record's value as the box shows it: the text it was typed as, else the number in the
/// document's units with three decimals ("0.000 in"; "30.000 deg" for an Angle).
fn base(app: &SolveApp, id: u64, angle: bool) -> String {
    let Some(c) = app.session.doc.assembly.constraint(&id.to_string()) else { return String::new() };
    c.exprs.offset.clone().unwrap_or_else(|| crate::dialogs_constraint::value_text(angle, c.offset, &crate::dialogs_constraint::units(app)))
}

/// The box as it stands this frame (tests, automation): its text and whether it is enabled.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn shown(app: &SolveApp) -> Option<(String, bool)> {
    let (id, enabled, _) = target(app)?;
    let b = app.tree.value_box.as_ref().filter(|b| b.constraint == id)?;
    Some((b.text.clone(), enabled))
}

/// Brought in line with the pick: a new constraint, or a record that changed under an untouched
/// box (an edit, an undo), starts from the record's value.
fn sync(app: &mut SolveApp) -> Option<(u64, bool, bool)> {
    let Some((id, enabled, angle)) = target(app) else {
        app.tree.value_box = None;
        return None;
    };
    let now = base(app, id, angle);
    match app.tree.value_box.as_mut() {
        Some(b) if b.constraint == id => {
            if b.base != now {
                if b.text == b.base {
                    b.text = now.clone();
                }
                b.base = now;
            }
        }
        _ => app.tree.value_box = Some(ValueBox { constraint: id, text: now.clone(), base: now, error: None }),
    }
    Some((id, enabled, angle))
}

/// The height the box takes at the foot of the browser this frame (0: none), so the rows above
/// leave it room.
pub(crate) fn height(app: &mut SolveApp) -> f32 {
    if sync(app).is_none() {
        return 0.0;
    }
    let t = Tokens::get();
    let error = app.tree.value_box.as_ref().is_some_and(|b| b.error.is_some());
    t.control_height + t.space_8 + if error { t.control_height } else { 0.0 }
}

/// The box, under the rows.
pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some((id, enabled, angle)) = sync(app) else { return };
    let Some(mut b) = app.tree.value_box.take() else { return };
    let t = Tokens::get();
    t.secondary_hline(ui.painter(), ui.max_rect().x_range(), ui.cursor().top() + 1.0, t.border);
    ui.add_space(t.space_4);
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let label = crate::i18n::tr(if angle { "ui.constraint.angle" } else { "ui.constraint.offset" });
    let inner = ui.add_enabled_ui(enabled, |ui| ui_kit::row(ui, label, |ui| (ui_kit::field(ui, &mut b.text), ui.min_rect())));
    let (enter, rect) = inner.inner;
    crate::scenario::publish_rect("browser:value_box", rect);
    crate::scenario::publish_handle("browser:value_box", rect.right_center() - egui::vec2(rect.width() / 4.0, 0.0));
    if escape && b.text != b.base {
        b.text = b.base.clone();
        b.error = None;
    }
    if enter && enabled {
        match commit(app, id, angle, &b) {
            // The box shows the edited record's value from the next frame.
            Ok(_) => {
                b.text = b.base.clone();
                b.error = None;
            }
            Err(e) => b.error = Some(e),
        }
    }
    if let Some(error) = &b.error {
        ui_kit::text(ui, error, ui_kit::Text::Report("constraint_value_error", true));
    }
    app.tree.value_box = Some(b);
}

/// Enter: the typed value as the constraint's offset, in one step (the text sent; None when
/// nothing changed), or why it can't be. A bare number is in the document's units (degrees for
/// an Angle) and is kept with them ("0.5 in").
fn commit(app: &mut SolveApp, id: u64, angle: bool, b: &ValueBox) -> Result<Option<String>, String> {
    let typed = b.text.trim();
    if typed == b.base.trim() {
        return Ok(None);
    }
    if let Some(problem) = crate::context_menu::modify_problem(app, id, typed) {
        return Err(crate::i18n::tr(problem).to_string());
    }
    let offset = if typed.parse::<f64>().is_ok() {
        format!("{typed} {}", if angle { "deg".to_string() } else { crate::dialogs_constraint::units(app) })
    } else {
        typed.to_string()
    };
    app.run("constraint.edit", json!({ "constraint": id, "offset": offset })).map_err(|e| e.replace("constraint.edit: ", ""))?;
    Ok(Some(offset))
}
