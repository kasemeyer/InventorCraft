//! A profile's navigation scheme, as data (R210): how the wheel zooms, how an orbit turns and
//! about what, which mouse buttons pan, orbit and zoom, the keys held to navigate and the keys
//! that change the view. SolidCraft's values are its behaviour before the block existed
//! ([`Navigation::solidcraft`]); the Inventor profile carries Inventor's documented defaults
//! (`assets/profiles/inventor.json`, sources in the planning repository's navigation audit).
//!
//! The values are defaults: a navigation preference the user has set (Preferences › Navigation,
//! or `ui.prefs`) wins over its key here (`navigation::user_chose`). `navigation::handle` is the
//! only reader that moves the camera.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::prefs::{NavBinding, NavButton, NavKeys};
use crate::profile::AliasScope;
use crate::viewport::NavMode;

/// Wheel input in points that one notch of a mouse wheel reports (egui's line scroll speed).
pub const NOTCH_POINTS: f64 = 40.0;

/// Which way turning the wheel away from you zooms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WheelDirection {
    /// Away from you zooms in (SolidCraft).
    ForwardZoomsIn,
    /// Away from you zooms out (Inventor, "Reverse direction" cleared).
    ForwardZoomsOut,
}

/// Which way a zoom drag (Ctrl+middle, F3 held) zooms when the pointer moves up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DragZoom {
    UpZoomsIn,
    UpZoomsOut,
}

/// How an orbit turns the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrbitStyle {
    /// About the world's up axis and the view's horizontal, the pitch held short of the poles.
    Turntable,
    /// About the screen's axes, through the poles.
    Free,
}

/// What an orbit turns about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrbitPivot {
    /// The view's centre (what Pan moves and Fit frames).
    ViewCentre,
    /// The model point under the cursor when the drag begins; the view's centre when nothing is
    /// under it.
    ModelPointUnderCursor,
    /// The centre of what is selected; the view's centre when nothing is.
    Selection,
}

/// What a held key or a mouse binding does to the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavAction {
    Pan,
    Zoom,
    Orbit,
}

impl NavAction {
    pub fn mode(self) -> NavMode {
        match self {
            NavAction::Pan => NavMode::Pan,
            NavAction::Zoom => NavMode::Zoom,
            NavAction::Orbit => NavMode::Orbit,
        }
    }
}

/// The drag that pans, orbits and zooms: `"middle"`, `"shift+middle"`, `"ctrl+middle"`… or none.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Buttons {
    pub pan: Option<String>,
    pub orbit: Option<String>,
    pub zoom: Option<String>,
}

/// A key (with its modifiers: `"Home"`, `"Shift+F5"`) that runs a UI action where `scope` is live.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavKey {
    pub key: String,
    pub action: String,
    pub scope: AliasScope,
}

/// A profile's `navigation` block. Every key is written out when the block is there; a profile
/// without one navigates as SolidCraft.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Navigation {
    pub wheel_direction: WheelDirection,
    /// The fraction of the view's distance one wheel notch zooms in by (out by its inverse).
    pub wheel_step: f64,
    /// Spread each notch over a few frames (off: each notch zooms at once).
    pub wheel_smoothing: bool,
    /// The wheel zooms about the point under the cursor (off: the view's centre).
    pub zoom_to_cursor: bool,
    pub drag_zoom: DragZoom,
    pub orbit_style: OrbitStyle,
    pub orbit_pivot: OrbitPivot,
    /// Mark the pivot while an orbit drag turns about it.
    pub pivot_glyph: bool,
    pub buttons: Buttons,
    /// A right drag orbits (off: the right button is the marking menu's alone).
    pub right_drag_orbit: bool,
    /// A Ctrl+left drag from empty space orbits.
    pub ctrl_left_drag_orbit: bool,
    /// Function key (`F1`–`F12`) → what a left drag does while it is held.
    pub hold_keys: BTreeMap<String, NavAction>,
    pub keys: Vec<NavKey>,
    /// Show the pan, zoom and orbit cursors while navigating.
    pub cursors: bool,
}

impl Navigation {
    /// SolidCraft's navigation as it was before the block: its Fusion mouse mapping, the wheel
    /// away zooming in by `exp(-40 × 0.0015)` a notch with smoothing, the turntable orbit about
    /// the view's centre, a right drag and a Ctrl+left drag from empty space orbiting, no hold
    /// keys or navigation keys and no navigation cursors.
    pub fn solidcraft() -> Navigation {
        Navigation {
            wheel_direction: WheelDirection::ForwardZoomsIn,
            wheel_step: 1.0 - (-NOTCH_POINTS * 0.0015).exp(),
            wheel_smoothing: true,
            zoom_to_cursor: true,
            drag_zoom: DragZoom::UpZoomsIn,
            orbit_style: OrbitStyle::Turntable,
            orbit_pivot: OrbitPivot::ViewCentre,
            pivot_glyph: false,
            buttons: Buttons { pan: Some("middle".into()), orbit: Some("shift+middle".into()), zoom: None },
            right_drag_orbit: true,
            ctrl_left_drag_orbit: true,
            hold_keys: BTreeMap::new(),
            keys: Vec::new(),
            cursors: false,
        }
    }

    /// The wheel's zoom rate per point of wheel input: a notch zooms by `exp(rate × 40)`.
    pub fn wheel_rate(&self) -> f64 {
        let step = if self.wheel_step.is_finite() { self.wheel_step.clamp(0.001, 0.9) } else { 0.06 };
        -(1.0 - step).ln() / NOTCH_POINTS
    }

    /// The mouse bindings the block lays out, most specific first (the first whose button is
    /// dragged with all its keys held wins), as a navigation preset's.
    pub fn bindings(&self) -> Vec<NavBinding> {
        let mut out: Vec<(usize, NavBinding)> =
            [(&self.buttons.orbit, NavMode::Orbit), (&self.buttons.zoom, NavMode::Zoom), (&self.buttons.pan, NavMode::Pan)]
                .into_iter()
                .filter_map(|(spec, mode)| {
                    let (button, keys) = parse_button(spec.as_deref()?).ok()?;
                    Some((usize::from(keys.shift) + usize::from(keys.ctrl) + usize::from(keys.alt), NavBinding { button, keys, mode }))
                })
                .collect();
        if self.right_drag_orbit {
            out.push((0, NavBinding { button: NavButton::Right, keys: NavKeys::default(), mode: NavMode::Orbit }));
        }
        // Stable: equally specific bindings keep the order above.
        out.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
        out.into_iter().map(|(_, b)| b).collect()
    }

    /// What dragging `button` with `mods` held does under the block's bindings.
    pub fn mode(&self, button: NavButton, mods: egui::Modifiers) -> Option<NavMode> {
        self.bindings().into_iter().find(|b| b.button == button && b.keys.held(mods)).map(|b| b.mode)
    }

    /// The held function key's action, if any of `down` is a hold key.
    pub fn hold(&self, down: impl Fn(egui::Key) -> bool) -> Option<NavAction> {
        self.hold_keys.iter().find_map(|(key, action)| egui::Key::from_name(key).filter(|k| down(*k)).map(|_| *action))
    }
}

/// `"shift+middle"` → the middle button with Shift.
pub fn parse_button(spec: &str) -> Result<(NavButton, NavKeys), String> {
    let mut keys = NavKeys::default();
    let mut parts: Vec<&str> = spec.split('+').map(str::trim).collect();
    let button = match parts.pop().map(str::to_ascii_lowercase).as_deref() {
        Some("left") => NavButton::Left,
        Some("middle") => NavButton::Middle,
        Some("right") => NavButton::Right,
        _ => return Err(format!("\"{spec}\": the last part must be left, middle or right")),
    };
    for part in parts {
        let flag = match part.to_ascii_lowercase().as_str() {
            "shift" => &mut keys.shift,
            "ctrl" => &mut keys.ctrl,
            "alt" => &mut keys.alt,
            _ => return Err(format!("\"{spec}\": \"{part}\" is not shift, ctrl or alt")),
        };
        if std::mem::replace(flag, true) {
            return Err(format!("\"{spec}\": \"{part}\" twice"));
        }
    }
    if button == NavButton::Left && keys == NavKeys::default() {
        return Err(format!("\"{spec}\": a plain left drag selects"));
    }
    Ok((button, keys))
}

/// `"Shift+F5"` → (Shift, Ctrl, Alt, the key).
pub fn parse_key(spec: &str) -> Result<(bool, bool, bool, egui::Key), String> {
    let mut parts: Vec<&str> = spec.split('+').collect();
    let key = parts.pop().and_then(egui::Key::from_name).ok_or_else(|| format!("\"{spec}\": no such key"))?;
    let (mut shift, mut ctrl, mut alt) = (false, false, false);
    for part in parts {
        let flag = match part {
            "Shift" => &mut shift,
            "Ctrl" => &mut ctrl,
            "Alt" => &mut alt,
            _ => return Err(format!("\"{spec}\": \"{part}\" is not Shift, Ctrl or Alt")),
        };
        if std::mem::replace(flag, true) {
            return Err(format!("\"{spec}\": \"{part}\" twice"));
        }
    }
    Ok((shift, ctrl, alt, key))
}

/// Whether `spec` names this press exactly (no other modifier held).
pub fn key_matches(spec: &str, key: egui::Key, mods: egui::Modifiers) -> bool {
    parse_key(spec).is_ok_and(|(shift, ctrl, alt, k)| k == key && shift == mods.shift && ctrl == (mods.ctrl || mods.command) && alt == mods.alt)
}

/// The block's problems, one message per problem. `function_keys` are the profile's own (a held
/// key must not also run an action).
pub fn validate(nav: &Navigation, function_keys: &BTreeMap<String, String>) -> Vec<String> {
    let mut out = Vec::new();
    if !(nav.wheel_step.is_finite() && nav.wheel_step > 0.0 && nav.wheel_step <= 0.5) {
        out.push(format!("navigation.wheel_step {}: must be above 0 and at most 0.5", nav.wheel_step));
    }
    let mut seen: Vec<(NavButton, NavKeys, &str)> = Vec::new();
    for (what, spec) in [("pan", &nav.buttons.pan), ("orbit", &nav.buttons.orbit), ("zoom", &nav.buttons.zoom)] {
        let Some(spec) = spec else { continue };
        match parse_button(spec) {
            Ok((button, keys)) => {
                if let Some((_, _, other)) = seen.iter().find(|(b, k, _)| *b == button && *k == keys) {
                    out.push(format!("navigation.buttons.{what} \"{spec}\": also {other}"));
                }
                if nav.right_drag_orbit && button == NavButton::Right && keys == NavKeys::default() {
                    out.push(format!("navigation.buttons.{what} \"{spec}\": a right drag orbits (right_drag_orbit)"));
                }
                seen.push((button, keys, what));
            }
            Err(e) => out.push(format!("navigation.buttons.{what} {e}")),
        }
    }
    for key in nav.hold_keys.keys() {
        let n = key.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()).filter(|n| key.get(1..) == Some(n.to_string().as_str()));
        if !n.is_some_and(|n| (1..=12).contains(&n)) {
            out.push(format!("navigation.hold_keys \"{key}\": must be F1 to F12"));
        } else if function_keys.contains_key(key) {
            out.push(format!("navigation.hold_keys \"{key}\": also a function key"));
        }
    }
    for (i, k) in nav.keys.iter().enumerate() {
        match parse_key(&k.key) {
            Ok((false, false, false, key)) if function_keys.contains_key(key.name()) => {
                out.push(format!("navigation.keys \"{}\": also a function key", k.key));
            }
            Ok(_) => {}
            Err(e) => out.push(format!("navigation.keys {e}")),
        }
        if nav.hold_keys.contains_key(&k.key) {
            out.push(format!("navigation.keys \"{}\": also a hold key", k.key));
        }
        if !crate::profile::UI_ACTIONS.contains(&k.action.as_str()) {
            out.push(format!("navigation.keys \"{}\": {} is not a UI action", k.key, k.action));
        }
        for other in nav.keys[..i].iter().filter(|o| o.key == k.key && o.scope.overlaps(k.scope)) {
            out.push(format!("navigation.keys \"{}\": conflict between {} and {}", k.key, other.action, k.action));
        }
    }
    out
}

#[cfg(test)]
#[path = "profile_navigation_tests.rs"]
mod tests;
