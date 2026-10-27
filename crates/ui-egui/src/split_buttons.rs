//! Ribbon split buttons (a panel's `dropdowns` in the profile) and, under the profile's
//! `workflow.sticky_split_buttons`, the command last picked from each one's list (Plan B-2 fix
//! wave 5): the face then shows and runs that command. The picks are user preferences, kept per
//! profile (`Prefs::split_faces`), so they survive a restart and never touch the design.

use solidcraft_engine::{CommandSpec, find_command};

use crate::SolveApp;
use crate::profile::Dropdown;

/// The split button whose default face is `face` on that panel of the active ribbon.
pub(crate) fn dropdown(app: &SolveApp, tab: &str, panel: &str, face: &str) -> Option<&'static Dropdown> {
    crate::profile::ribbon(app).tab(tab)?.panel(panel)?.dropdown(face)
}

/// Whether this drop-down's face follows the last pick here.
fn sticky(app: &SolveApp, d: &Dropdown) -> bool {
    crate::profile::active(app).workflow.sticky_split_buttons && !d.fixed_face
}

/// The command last picked from `d`'s list, when its face follows picks and the pick is still
/// one of its commands (a stale or foreign id reads as none).
pub(crate) fn remembered(app: &SolveApp, d: &Dropdown) -> Option<&'static CommandSpec> {
    if !sticky(app, d) {
        return None;
    }
    let id = app.preferences.split_faces.get(&crate::profile::active(app).id)?.get(&d.id)?;
    d.commands.iter().find(|c| *c == id).and_then(|c| find_command(c))
}

/// What the face shows and runs: the remembered command, else the drop-down's default face.
pub(crate) fn face(app: &SolveApp, d: &Dropdown) -> Option<&'static CommandSpec> {
    remembered(app, d).or_else(|| find_command(&d.face))
}

/// The face's label (English; callers translate it): a remembered command's own name ("Offset
/// Plane"), else the default face's ribbon text ("Plane").
pub(crate) fn face_label(app: &SolveApp, d: &Dropdown) -> &'static str {
    match remembered(app, d) {
        Some(c) => c.label,
        None => find_command(&d.face).map_or("", |c| crate::profile::button_label(app, c)),
    }
}

/// A row of `d`'s list was picked: start that exact command and, when the face follows picks,
/// remember it for this profile.
pub(crate) fn pick(app: &mut SolveApp, d: &Dropdown, id: &'static str) {
    if sticky(app, d) && d.commands.iter().any(|c| c == id) {
        let profile = crate::profile::active(app).id.clone();
        app.preferences.split_faces.entry(profile).or_default().insert(d.id.clone(), id.to_string());
    }
    app.start(id);
}

#[cfg(test)]
#[path = "split_buttons_tests.rs"]
mod tests;
