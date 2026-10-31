#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The `navigation` block's data: both profiles' values, the loader and the validator, and the
//! precedence of the user's navigation preferences over a profile's block.

use std::collections::BTreeMap;

use egui::Modifiers;
use serde_json::json;

use super::*;
use crate::prefs::{NAV_PRESETS, nav_preset};
use crate::profile::{INVENTOR, SOLIDCRAFT, get};

fn inventor() -> &'static Navigation {
    &get(INVENTOR).expect("the Inventor profile parses").navigation
}

fn solidcraft() -> &'static Navigation {
    &get(SOLIDCRAFT).unwrap().navigation
}

/// (button, shift, ctrl, alt, mode) of each binding, for comparing lists.
fn shape(bindings: &[NavBinding]) -> Vec<(NavButton, bool, bool, bool, NavMode)> {
    bindings.iter().map(|b| (b.button, b.keys.shift, b.keys.ctrl, b.keys.alt, b.mode)).collect()
}

#[test]
fn solidcraft_navigation_is_its_behaviour_before_the_block() {
    let n = solidcraft();
    assert_eq!(*n, Navigation::solidcraft());
    assert_eq!(n.wheel_direction, WheelDirection::ForwardZoomsIn);
    // The wheel's old rate, 0.0015 per point.
    assert!((n.wheel_rate() - 0.0015).abs() < 1e-15, "{}", n.wheel_rate());
    assert!(n.wheel_smoothing && n.zoom_to_cursor);
    assert_eq!((n.drag_zoom, n.orbit_style, n.orbit_pivot), (DragZoom::UpZoomsIn, OrbitStyle::Turntable, OrbitPivot::ViewCentre));
    assert!(n.right_drag_orbit && n.ctrl_left_drag_orbit && !n.pivot_glyph && !n.cursors);
    assert!(n.hold_keys.is_empty() && n.keys.is_empty());
    // Its buttons are SolidCraft's default mapping (Fusion's), binding for binding, in order.
    assert_eq!(crate::prefs::Prefs::default().nav, "fusion");
    assert_eq!(shape(&n.bindings()), shape(nav_preset("fusion").bindings));
}

#[test]
fn inventor_navigation_carries_inventors_documented_defaults() {
    let n = inventor();
    // Wheel away zooms out ([A1][A2][F1]); 15 % a notch, no smoothing tail ([A3]; owner Q3).
    assert_eq!(n.wheel_direction, WheelDirection::ForwardZoomsOut);
    assert_eq!(n.wheel_step, 0.15);
    assert!(!n.wheel_smoothing && n.zoom_to_cursor);
    // Drag-zoom: the cursor moving up zooms out ([A3][A4][A5]).
    assert_eq!(n.drag_zoom, DragZoom::UpZoomsOut);
    // Free Orbit is the default orbit type ([A7][A3]); the pivot is the model point ([A6][F4]).
    assert_eq!((n.orbit_style, n.orbit_pivot), (OrbitStyle::Free, OrbitPivot::ModelPointUnderCursor));
    assert!(n.pivot_glyph && n.cursors);
    // MMB pans, Shift+MMB orbits, Ctrl+MMB zooms ([A5][A6][F3]); the right button is the
    // marking menu's ([A10]) and Ctrl+left selects.
    assert!(!n.right_drag_orbit && !n.ctrl_left_drag_orbit);
    let want = vec![
        (NavButton::Middle, true, false, false, NavMode::Orbit),
        (NavButton::Middle, false, true, false, NavMode::Zoom),
        (NavButton::Middle, false, false, false, NavMode::Pan),
    ];
    assert_eq!(shape(&n.bindings()), want);
    assert_eq!(n.mode(NavButton::Right, Modifiers::NONE), None);
}

#[test]
fn both_profiles_validate() {
    for p in crate::profile::all() {
        let problems = validate(&p.navigation, &p.function_keys);
        assert!(problems.is_empty(), "{}: {problems:#?}", p.id);
    }
}

#[test]
fn a_profile_without_the_block_navigates_as_solidcraft() {
    let base = crate::profile::solidcraft();
    let text = include_str!("../../../assets/profiles/inventor.json");
    let mut json: serde_json::Value = serde_json::from_str(text).unwrap();
    json.as_object_mut().unwrap().remove("navigation");
    let p = crate::profile::parse(&json.to_string(), &base).unwrap();
    assert_eq!(p.navigation, Navigation::solidcraft());
}

#[test]
fn the_loader_wants_every_key_and_no_unknown_one() {
    let base = crate::profile::solidcraft();
    let text = include_str!("../../../assets/profiles/inventor.json");
    let json: serde_json::Value = serde_json::from_str(text).unwrap();
    let mut missing = json.clone();
    missing["navigation"].as_object_mut().unwrap().remove("wheel_step");
    assert!(crate::profile::parse(&missing.to_string(), &base).unwrap_err().contains("wheel_step"));
    let mut extra = json.clone();
    extra["navigation"]["wheel_speed"] = json!(2);
    assert!(crate::profile::parse(&extra.to_string(), &base).unwrap_err().contains("wheel_speed"));
    let mut bad = json;
    bad["navigation"]["orbit_style"] = json!("tumble");
    assert!(crate::profile::parse(&bad.to_string(), &base).is_err());
}

#[test]
fn the_validator_names_each_problem() {
    let mut n = Navigation::solidcraft();
    n.wheel_step = 0.0;
    n.buttons.zoom = Some("shift+middle".into());
    n.buttons.pan = Some("middle+".into());
    n.hold_keys = BTreeMap::from([("F6".into(), NavAction::Zoom), ("Home".into(), NavAction::Pan)]);
    n.keys = vec![
        NavKey { key: "Home".into(), action: "ui.view_fit".into(), scope: crate::profile::AliasScope::Everywhere },
        NavKey { key: "Home".into(), action: "ui.view_home".into(), scope: crate::profile::AliasScope::Part },
        NavKey { key: "Hyper+End".into(), action: "ui.nothing".into(), scope: crate::profile::AliasScope::Model },
        NavKey { key: "F6".into(), action: "ui.view_fit".into(), scope: crate::profile::AliasScope::Model },
    ];
    let function_keys = BTreeMap::from([("F6".to_string(), "ui.view_fit".to_string())]);
    let problems = validate(&n, &function_keys).join("\n");
    for want in [
        "wheel_step 0",
        "buttons.zoom \"shift+middle\": also orbit",
        "buttons.pan \"middle+\"",
        "hold_keys \"F6\": also a function key",
        "hold_keys \"Home\": must be F1 to F12",
        "keys \"Home\": conflict between ui.view_fit and ui.view_home",
        "\"Hyper+End\": \"Hyper\" is not Shift, Ctrl or Alt",
        "keys \"Hyper+End\": ui.nothing is not a UI action",
        "keys \"F6\": also a function key",
    ] {
        assert!(problems.contains(want), "{want} in:\n{problems}");
    }
    let mut right = Navigation::solidcraft();
    right.buttons.zoom = Some("right".into());
    assert!(validate(&right, &BTreeMap::new()).join("\n").contains("a right drag orbits"));
    assert!(parse_button("left").is_err() && parse_button("ctrl+left").is_ok());
}

// ---- precedence: the user's navigation preferences over the profile's block ----

fn app(profile: &str) -> crate::scenario::Harness {
    let mut h = crate::scenario::Harness::new();
    assert_eq!(h.call("ui.prefs", json!({"ui_profile": profile}))["ok"], json!(true));
    h
}

#[test]
fn unset_preferences_follow_the_profile() {
    use crate::navigation::{Pivot, free_orbit, pivot, smooth_wheel, wheel_zooms_out, zoom_to_cursor};
    let s = app(SOLIDCRAFT);
    assert!(!wheel_zooms_out(&s.app) && !free_orbit(&s.app) && smooth_wheel(&s.app) && zoom_to_cursor(&s.app));
    assert_eq!(pivot(&s.app), Pivot::ViewCentre);
    assert!(!crate::navigation::preset_chosen(&s.app));
    let i = app(INVENTOR);
    assert!(wheel_zooms_out(&i.app) && free_orbit(&i.app) && !smooth_wheel(&i.app) && zoom_to_cursor(&i.app));
    assert_eq!(pivot(&i.app), Pivot::CursorElseCentre);
    // Choosing the profile set the preference `nav` to its own mapping: the profile's buttons rule.
    assert_eq!(i.app.preferences.nav, "inventor");
    assert!(!crate::navigation::preset_chosen(&i.app));
    assert_eq!(crate::prefs::nav_mode(&i.app, NavButton::Right, Modifiers::NONE), None, "no right-drag orbit under Inventor");
    assert_eq!(crate::prefs::nav_mode(&s.app, NavButton::Right, Modifiers::NONE), Some(NavMode::Orbit));
}

#[test]
fn a_preference_the_user_set_wins_over_either_profile() {
    use crate::navigation::{Pivot, free_orbit, pivot, smooth_wheel, wheel_zooms_out, zoom_to_cursor};
    for profile in [SOLIDCRAFT, INVENTOR] {
        let mut h = app(profile);
        let before = (wheel_zooms_out(&h.app), free_orbit(&h.app), smooth_wheel(&h.app), zoom_to_cursor(&h.app));
        // Each set to the other way by name, even to its default value: the user's from now on.
        let r = h.call(
            "ui.prefs",
            json!({"zoom_reverse": !before.0, "free_orbit": !before.1, "smooth_wheel_zoom": !before.2, "zoom_to_cursor": !before.3, "orbit_around": "origin"}),
        );
        assert_eq!(r["ok"], json!(true), "{r}");
        assert_eq!(
            (wheel_zooms_out(&h.app), free_orbit(&h.app), smooth_wheel(&h.app), zoom_to_cursor(&h.app)),
            (!before.0, !before.1, !before.2, !before.3),
            "{profile}"
        );
        assert_eq!(pivot(&h.app), Pivot::Origin);
        // Switching profile keeps the user's choices.
        let other = if profile == SOLIDCRAFT { INVENTOR } else { SOLIDCRAFT };
        h.call("ui.prefs", json!({"ui_profile": other}));
        assert_eq!((wheel_zooms_out(&h.app), free_orbit(&h.app)), (!before.0, !before.1), "{profile} → {other}");
        // Restore Defaults on the Navigation page hands them back to the profile.
        h.call("ui.prefs", json!({"open": "Navigation"}));
        h.call("ui.prefs", json!({"button": "defaults"}));
        assert!(h.app.preferences.navigation_set.is_empty());
        let i = other == INVENTOR;
        assert_eq!((wheel_zooms_out(&h.app), free_orbit(&h.app), smooth_wheel(&h.app)), (i, i, !i), "{other} after Restore Defaults");
        assert_eq!(h.app.preferences.nav, if i { "inventor" } else { "fusion" });
        assert!(!crate::navigation::preset_chosen(&h.app));
    }
    // A user who picked the SolidCraft direction under Inventor keeps it: the default value,
    // explicitly set, still wins.
    let mut h = app(INVENTOR);
    h.call("ui.prefs", json!({"zoom_reverse": false, "free_orbit": false}));
    assert!(!wheel_zooms_out(&h.app) && !free_orbit(&h.app));
}

#[test]
fn a_picked_mouse_mapping_wins_over_the_profiles_buttons() {
    let mut h = app(INVENTOR);
    assert_eq!(crate::prefs::nav_mode(&h.app, NavButton::Right, Modifiers::NONE), None);
    // Picking a mapping (even Inventor's, which right-drag orbits) makes its bindings the user's.
    for preset in NAV_PRESETS.iter().filter(|p| p.id != crate::prefs::TOUCHPAD_NAV) {
        crate::prefs::choose_preset(&mut h.app, preset.id);
        assert!(crate::navigation::preset_chosen(&h.app));
        assert_eq!(shape(&crate::navigation::bindings(&h.app)), shape(preset.bindings), "{}", preset.id);
        assert_eq!(crate::navigation::wheel_zooms_out(&h.app), preset.reverse_zoom, "{}", preset.id);
    }
    // Choosing the profile again gives the mouse back to the profile's buttons.
    h.call("ui.prefs", json!({"ui_profile": INVENTOR}));
    assert!(!crate::navigation::preset_chosen(&h.app));
}

#[test]
fn saved_preferences_from_before_the_block_keep_their_changes() {
    // A preference saved away from its default before `navigation_set` existed was the user's.
    let mut h = app(INVENTOR);
    h.app.preferences.navigation_set.clear();
    h.app.preferences.smooth_wheel_zoom = false;
    h.app.preferences.free_orbit = true;
    h.app.preferences.zoom_reverse = true;
    assert!(crate::navigation::wheel_zooms_out(&h.app) && crate::navigation::free_orbit(&h.app));
    let mut s = app(SOLIDCRAFT);
    s.app.preferences.navigation_set.clear();
    s.app.preferences.free_orbit = true;
    assert!(crate::navigation::free_orbit(&s.app));
}
