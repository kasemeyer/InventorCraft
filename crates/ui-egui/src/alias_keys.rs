//! Typed command aliases (ruling D5): under a profile whose `letter_keys` is `Aliases`, plain
//! letters spell a short command name (`E` Extrude, `SH` Shell, `REC3` Three Point Rectangle)
//! instead of running each letter's registry shortcut. The table is the profile's data
//! (`assets/profiles/inventor.json`), every entry naming a SolidCraft registry command or one of
//! `profile::UI_ACTIONS`; Inventor's internal command names never enter this tree.
//!
//! A typed alias that is no prefix of a longer live one runs at once. One that begins a longer
//! one waits the profile's `alias_wait_ms` (Ruling 4) for the next letter; Enter or Space run it
//! now, Escape drops it, Backspace takes back a letter.

use crate::SolveApp;
use crate::profile::{AliasScope, LetterKeys};
use crate::ui_kit::{Container, Surface};

#[derive(Default)]
pub struct AliasBuffer {
    typed: String,
    last_key_at: f64,
    /// The command the last alias ran.
    pub fired: Option<String>,
}

/// Where the typing happens, which decides the aliases that are live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    Part,
    Assembly,
    Drawing,
    Sketch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// The only live alias that begins with the letters typed, and it is complete.
    Run(String),
    /// Longer live aliases begin with the letters typed: wait, then run the exact match (if any)
    /// when the wait ends or Enter is pressed.
    Wait(Option<String>),
    NoMatch,
}

pub fn context(app: &SolveApp) -> Context {
    use solidcraft_engine::doc::DocumentKind;
    if app.session.active_sketch.is_some() {
        return Context::Sketch;
    }
    match app.session.doc.kind {
        DocumentKind::Part => Context::Part,
        DocumentKind::Assembly => Context::Assembly,
        DocumentKind::Drawing => Context::Drawing,
    }
}

/// Whether an alias of this scope is live here.
fn applies(scope: AliasScope, ctx: Context) -> bool {
    match scope {
        AliasScope::Everywhere => true,
        AliasScope::Model => matches!(ctx, Context::Part | Context::Assembly),
        AliasScope::Part => ctx == Context::Part,
        AliasScope::Assembly => ctx == Context::Assembly,
        AliasScope::Sketch => ctx == Context::Sketch,
        AliasScope::SketchWrapper => matches!(ctx, Context::Sketch | Context::Part | Context::Assembly),
    }
}

/// The profile's aliases, with the user's Keyboard Shortcuts changes applied: a command the user
/// rebound or cleared loses its alias, and a command bound to one plain letter gains that alias.
pub fn table(app: &SolveApp) -> Vec<(String, String, AliasScope)> {
    let custom = &app.keymap.custom;
    let mut out: Vec<(String, String, AliasScope)> = crate::profile::active(app)
        .aliases
        .iter()
        .filter(|a| !custom.contains_key(&a.command))
        .map(|a| (a.keys.clone(), a.command.clone(), a.scope))
        .collect();
    for (id, key) in custom {
        if key.len() == 1 && key.bytes().all(|b| b.is_ascii_uppercase()) {
            let scope = match crate::keymap::scope(id) {
                crate::keymap::Scope::Sketch => AliasScope::Sketch,
                crate::keymap::Scope::Model => AliasScope::Model,
                crate::keymap::Scope::Everywhere => AliasScope::Everywhere,
            };
            out.push((key.clone(), id.clone(), scope));
        }
    }
    out
}

pub fn resolve(table: &[(String, String, AliasScope)], ctx: Context, typed: &str) -> Resolution {
    let candidates: Vec<&(String, String, AliasScope)> = table.iter().filter(|(k, _, s)| applies(*s, ctx) && k.starts_with(typed)).collect();
    match candidates.as_slice() {
        [] => Resolution::NoMatch,
        [(k, c, _)] if k == typed => Resolution::Run(c.clone()),
        _ => Resolution::Wait(candidates.iter().find(|(k, _, _)| k == typed).map(|(_, c, _)| c.clone())),
    }
}

/// The live alias spelled exactly by `typed`: its command and scope.
fn exact(app: &SolveApp, typed: &str) -> Option<(String, AliasScope)> {
    let ctx = context(app);
    table(app).into_iter().find(|(k, _, s)| k == typed && applies(*s, ctx)).map(|(_, c, s)| (c, s))
}

/// The capital letter or digit a key types, if any.
fn alias_char(key: egui::Key) -> Option<char> {
    let mut chars = key.name().chars();
    let c = chars.next()?;
    (chars.next().is_none() && (c.is_ascii_uppercase() || c.is_ascii_digit())).then_some(c)
}

fn plain(mods: egui::Modifiers) -> bool {
    !(mods.ctrl || mods.alt || mods.command || mods.mac_cmd || mods.shift)
}

/// A key press the alias buffer owns: under a profile whose `letter_keys` is `Aliases`, a plain
/// letter, or a digit after a letter. The shortcut loop also swallows a held key's repeats with
/// this, so in alias mode no letter reaches `keymap::press`, the `S` palette or the `V` toggle.
pub fn owns(app: &SolveApp, key: egui::Key, mods: egui::Modifiers) -> bool {
    crate::profile::active(app).letter_keys == LetterKeys::Aliases
        && plain(mods)
        && alias_char(key).is_some_and(|c| c.is_ascii_uppercase() || !app.alias.typed.is_empty())
}

/// Feed one key press; true when it was consumed. False when the profile's `letter_keys` is
/// `Registry`.
pub fn feed(app: &mut SolveApp, key: egui::Key, mods: egui::Modifiers, now: f64) -> bool {
    if crate::profile::active(app).letter_keys != LetterKeys::Aliases || !plain(mods) {
        return false;
    }
    if owns(app, key, mods)
        && let Some(c) = alias_char(key)
    {
        app.alias.typed.push(c);
        app.alias.last_key_at = now;
        match resolve(&table(app), context(app), &app.alias.typed) {
            Resolution::Run(_) => fire_exact(app),
            Resolution::Wait(_) => {}
            Resolution::NoMatch => app.alias.typed.clear(),
        }
        return true;
    }
    if app.alias.typed.is_empty() {
        return false;
    }
    match key {
        egui::Key::Enter | egui::Key::Space => fire_exact(app),
        egui::Key::Escape => app.alias.typed.clear(),
        egui::Key::Backspace => {
            app.alias.typed.pop();
            // The shorter alias waits afresh rather than running on the next frame.
            app.alias.last_key_at = now;
        }
        _ => return false,
    }
    true
}

/// Run a waiting exact match once the active profile's `alias_wait_ms` has passed since the
/// last key (Ruling 4).
pub fn tick(app: &mut SolveApp, now: f64) {
    if !app.alias.typed.is_empty() && now - app.alias.last_key_at >= wait_s(app) {
        fire_exact(app);
    }
}

fn wait_s(app: &SolveApp) -> f64 {
    f64::from(crate::profile::active(app).alias_wait_ms) / 1000.0
}

/// Run the live alias the buffer spells exactly, if any, and empty the buffer.
fn fire_exact(app: &mut SolveApp) {
    let typed = std::mem::take(&mut app.alias.typed);
    if let Some((id, scope)) = exact(app, &typed) {
        fire(app, &id, scope);
    }
}

fn fire(app: &mut SolveApp, id: &str, scope: AliasScope) {
    app.alias.fired = Some(id.to_string());
    match id {
        "ui.preferences" => crate::prefs::open_at(app, 0),
        "ui.view_cube" => {
            use crate::prefs::NavigatorStyle;
            let style = &mut app.preferences.navigator_style;
            *style = if *style == NavigatorStyle::Cube { NavigatorStyle::Axes } else { NavigatorStyle::Cube };
        }
        _ => {
            // Ruling 9: the registry's own `&'static str`.
            let Some(id) = solidcraft_engine::find_command(id).map(|c| c.id) else { return };
            // A shortcut starts the family's remembered variant, as `keymap::press` does.
            let id = app.preferences.sketch_variants.resolve(id);
            if scope == AliasScope::SketchWrapper && app.session.active_sketch.is_none() {
                crate::keymap::start_sketch_tool(app, id);
            } else {
                app.start(id);
            }
        }
    }
}

/// The label the hint shows for an alias target.
fn target_label(id: &str) -> String {
    match (id, solidcraft_engine::find_command(id)) {
        (_, Some(c)) => crate::i18n::tr_label(c.label).to_string(),
        ("ui.view_cube", None) => crate::i18n::tr_label("Cube").to_string(),
        (_, None) => crate::i18n::tr(id).to_string(),
    }
}

/// The typed letters and their match, beside the pointer, while waiting.
pub fn show(app: &SolveApp, ctx: &egui::Context) {
    let typed = app.alias.typed.as_str();
    if typed.is_empty() {
        return;
    }
    // No other input may come to wake a frame: ask for one when the wait ends.
    let left = wait_s(app) - (ctx.input(|i| i.time) - app.alias.last_key_at);
    ctx.request_repaint_after(std::time::Duration::try_from_secs_f64(left.max(0.0)).unwrap_or_default());
    let label = exact(app, typed).map_or_else(|| "…".to_string(), |(id, _)| target_label(&id));
    let at = ctx.input(|i| i.pointer.latest_pos()).unwrap_or(ctx.content_rect().center()) + egui::vec2(16.0, 16.0);
    let text = crate::i18n::format("ui.alias_hint", &[("keys", typed), ("command", &label)]);
    Container::new(ctx, Surface::Popup).area(egui::Id::new("sc_alias_hint")).fixed_pos(at).order(egui::Order::Foreground).interactable(false).show(
        ctx,
        |ui| {
            ui.label(text);
        },
    );
}

#[cfg(test)]
#[path = "alias_keys_tests.rs"]
mod tests;
