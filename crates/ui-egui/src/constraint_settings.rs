//! Constraint Settings (R107): the sketch's drawing-time settings, opened from the Constrain
//! panel's Constraint Settings button (`sketch.constraint_settings`). For now it holds Point
//! alignment: a point being placed lines up horizontally or vertically with existing sketch
//! points (`crate::inference::point_alignment`). Saved per profile in the preferences, on unless
//! turned off; a change takes effect at once, Cancel puts back what the panel opened with.
//! SolidCraft's Sketch Palette carries the same check.

use crate::SolveApp;

/// Point alignment for the active profile (`Prefs::point_alignment`; absent = on).
pub fn point_alignment(app: &SolveApp) -> bool {
    app.preferences.point_alignment.get(&crate::profile::active(app).id).copied().unwrap_or(true)
}

/// Turn Point alignment on or off for the active profile (at once).
pub fn set_point_alignment(app: &mut SolveApp, on: bool) {
    if point_alignment(app) != on {
        let profile = crate::profile::active(app).id.clone();
        app.preferences.point_alignment.insert(profile, on);
    }
}

/// Open the panel (the Constraint Settings button).
pub fn open(app: &mut SolveApp) {
    app.menu.constraint_settings = Some(point_alignment(app));
}

/// The panel, while open: OK keeps what it shows, Cancel (or closing it) restores it.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(opened_with) = app.menu.constraint_settings else { return };
    let mut on = point_alignment(app);
    let title = crate::i18n::tr_label("Constraint Settings");
    let (closed, action) =
        crate::ui_kit::Confirm::new(egui::Id::new("sc_constraint_settings"), "settings", title).actions("OK", "Cancel", None).show(ctx, true, |ui| {
            let r = crate::ui_kit::checkbox(ui, crate::i18n::tr_label("ui.point_alignment"), &mut on)
                .on_hover_text(crate::i18n::tr_label("ui.point_alignment_tip"));
            crate::scenario::publish_handle("constraint_settings:point_alignment", r.rect.center());
        });
    set_point_alignment(app, on);
    match action {
        crate::ui_kit::Action::Ok => app.menu.constraint_settings = None,
        crate::ui_kit::Action::Cancel => cancel(app, opened_with),
        _ if closed => cancel(app, opened_with),
        _ => {}
    }
}

fn cancel(app: &mut SolveApp, opened_with: bool) {
    set_point_alignment(app, opened_with);
    app.menu.constraint_settings = None;
}

#[cfg(test)]
#[path = "constraint_settings_tests.rs"]
mod tests;
