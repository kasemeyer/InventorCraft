#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use egui::{Key, Modifiers};
use serde_json::json;
use solidcraft_engine::doc::DocumentKind::Part;

use crate::prefs::NavigatorStyle;
use crate::profile::{INVENTOR, get};
use crate::scenario::Harness;
use crate::test_support::{inventor, key};

/// The Inventor profile's alias table, as `resolve` takes it.
fn t() -> Vec<(String, String, AliasScope)> {
    get(INVENTOR).unwrap().aliases.iter().map(|a| (a.keys.clone(), a.command.clone(), a.scope)).collect()
}

#[test]
fn resolve_runs_a_unique_match_and_waits_on_a_prefix() {
    let t = t();
    // SH, SW, ST and SP begin with S.
    assert_eq!(resolve(&t, Context::Part, "S"), Resolution::Wait(Some("sketch.create".into())));
    assert_eq!(resolve(&t, Context::Part, "SH"), Resolution::Run("solid.shell".into()));
    assert_eq!(resolve(&t, Context::Assembly, "S"), Resolution::Wait(Some("sketch.create".into())));
    assert_eq!(resolve(&t, Context::Sketch, "L"), Resolution::Run("sketch.line".into()));
    // In a sketch only SP begins with S, and S itself is no sketch alias.
    assert_eq!(resolve(&t, Context::Sketch, "S"), Resolution::Wait(None));
    assert_eq!(resolve(&t, Context::Part, "J"), Resolution::NoMatch);
    assert_eq!(resolve(&t, Context::Assembly, "J"), Resolution::Run("joint.create".into()));
    assert_eq!(resolve(&t, Context::Drawing, "E"), Resolution::NoMatch);
    assert_eq!(resolve(&t, Context::Drawing, "M"), Resolution::Run("inspect.measure".into()));
    assert_eq!(resolve(&t, Context::Sketch, "REC"), Resolution::Wait(Some("sketch.rectangle.two_point".into())));
    assert_eq!(resolve(&t, Context::Sketch, "REC3"), Resolution::Run("sketch.rectangle.three_point".into()));
}

#[test]
fn typing_sh_starts_shell_at_once() {
    let mut h = inventor(Part);
    h.frame_with(key(Key::S));
    h.frame_with(key(Key::H));
    assert_eq!(h.app.alias.fired.as_deref(), Some("solid.shell"));
}

/// Ruling 4: the one-key wait is the profile's `alias_wait_ms`, not a constant.
#[test]
fn the_alias_wait_comes_from_the_profile() {
    let mut h = inventor(Part);
    assert!(feed(&mut h.app, Key::E, Modifiers::NONE, 10.0));
    tick(&mut h.app, 10.749);
    assert!(h.app.alias.fired.is_none(), "E waits for EX until the profile's 750 ms have passed");
    tick(&mut h.app, 10.75);
    assert_eq!(h.app.alias.fired.as_deref(), Some("solid.extrude"));
}

#[test]
fn enter_runs_a_waiting_alias_at_once() {
    let mut h = inventor(Part);
    h.frame_with(key(Key::S));
    h.frame_with(key(Key::Enter));
    assert_eq!(h.app.alias.fired.as_deref(), Some("sketch.create"));
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Sketch)));
}

#[test]
fn escape_clears_a_waiting_alias() {
    let mut h = inventor(Part);
    h.frame_with(key(Key::S));
    h.frame_with(key(Key::Escape));
    h.frames(60);
    assert!(h.app.alias.fired.is_none());
    assert!(h.app.dialog.is_none());
}

#[test]
fn letters_inside_a_sketch_use_sketch_aliases() {
    let mut h = inventor(Part);
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.frame_with(key(Key::L));
    assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.line"));
}

#[test]
fn a_wrapper_alias_outside_a_sketch_opens_a_sketch_first() {
    let mut h = inventor(Part);
    // Outside a sketch L waits, because LO is live.
    h.frame_with(key(Key::L));
    assert!(h.app.dialog.is_none(), "L waits for LO");
    h.frame_with(key(Key::Enter));
    let d = h.app.dialog.as_ref().expect("Create Sketch opens");
    assert!(matches!(d.kind, crate::dialogs::Kind::Sketch));
    assert_eq!(d.then_start, Some("sketch.line"));
}

#[test]
fn the_solidcraft_profile_keeps_single_key_shortcuts() {
    let mut h = Harness::new();
    assert!(!feed(&mut h.app, Key::E, Modifiers::NONE, 0.0));
    assert_eq!(crate::keymap::effective(&h.app, "solid.extrude").as_deref(), Some("E"));
}

#[test]
fn inventor_shortcut_display_shows_the_alias() {
    let h = inventor(Part);
    assert_eq!(crate::keymap::effective(&h.app, "solid.shell").as_deref(), Some("SH"));
    // Its alias, though the registry key is M.
    assert_eq!(crate::keymap::effective(&h.app, "solid.move").as_deref(), Some("V"));
    // Chords survive.
    assert_eq!(crate::keymap::effective(&h.app, "view.front").as_deref(), Some("Ctrl+1"));
    // Registry key Q: a plain letter with no Inventor alias.
    assert_eq!(crate::keymap::effective(&h.app, "solid.press_pull"), None);
}

#[test]
fn a_cleared_shortcut_removes_its_alias() {
    let mut h = inventor(Part);
    assert!(table(&h.app).iter().any(|(k, c, _)| k == "E" && c == "solid.extrude"));
    h.app.keymap.custom.insert("solid.extrude".into(), String::new());
    assert!(!table(&h.app).iter().any(|(k, c, _)| k == "E" && c == "solid.extrude"));
}

#[test]
fn ui_actions_run_the_shell() {
    let mut h = inventor(Part);
    assert!(!h.app.prefs_window.open);
    h.frame_with(key(Key::O));
    h.frame_with(key(Key::P));
    assert!(h.app.prefs_window.open);

    let mut h = inventor(Part);
    assert_eq!(h.app.preferences.navigator_style, NavigatorStyle::Cube);
    h.frame_with(key(Key::V));
    h.frame_with(key(Key::C));
    assert_eq!(h.app.preferences.navigator_style, NavigatorStyle::Axes);
}

#[test]
fn shift_and_ctrl_letters_are_not_aliases() {
    let mut h = inventor(Part);
    // Shift+R is Repeat.
    assert!(!feed(&mut h.app, Key::R, Modifiers::SHIFT, 0.0));
    assert!(!feed(&mut h.app, Key::R, Modifiers::COMMAND, 0.0));
    assert!(h.app.alias.typed.is_empty());
}

/// A held letter's repeats are the alias buffer's too: they never reach the `S` palette or
/// `keymap::press`.
#[test]
fn a_held_letter_repeats_nothing() {
    let mut h = inventor(Part);
    let held = |key| vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: true, modifiers: Modifiers::NONE }];
    h.frame_with(key(Key::S));
    h.frame_with(held(Key::S));
    h.frame_with(held(Key::S));
    assert!(!h.app.ui.palette_open);
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.alias.typed, "S");
}
