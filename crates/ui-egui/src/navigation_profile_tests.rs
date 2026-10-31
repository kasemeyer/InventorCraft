#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The profile's `navigation` block (F-2, R210): what the wheel, the drags, the hold keys and the
//! navigation keys do under each profile. SolidCraft's navigation is pinned to the numbers the
//! code produced before the block existed (`solidcraft_navigation_is_unchanged`).

use serde_json::json;
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::render::Camera;

use crate::scenario::Harness;

/// The camera as numbers: target, yaw, pitch, roll, distance.
fn numbers(c: &Camera) -> [f64; 7] {
    [c.target.x, c.target.y, c.target.z, c.yaw, c.pitch, c.roll, c.distance]
}

fn start_camera() -> Camera {
    Camera { target: Vec3::new(5.0, 4.0, 3.0), yaw: 0.6, pitch: 0.5, roll: 0.0, distance: 200.0, fov: 0.0, ..Default::default() }
}

/// A box in the model view of `h`, the camera at `start_camera`.
fn model(h: &mut Harness) {
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.frames(3);
    h.app.cam_anim = None;
    h.app.cam = start_camera();
    h.frames(2);
}

fn drag(h: &mut Harness, from: egui::Pos2, by: egui::Vec2, button: &str, mods: serde_json::Value) {
    let mut p = json!({"x0": from.x, "y0": from.y, "x1": from.x + by.x, "y1": from.y + by.y, "button": button, "steps": 8});
    if let (Some(p), Some(m)) = (p.as_object_mut(), mods.as_object()) {
        p.extend(m.clone());
    }
    h.call("ui.drag", p);
    h.frames(2);
}

/// The model-view inputs the pin replays, the camera after each.
fn replay_model(h: &mut Harness) -> Vec<[f64; 7]> {
    let rect = h.app.viewport.rect.unwrap();
    let c = rect.center();
    let empty = egui::pos2(rect.left() + 320.0, rect.bottom() - 160.0);
    let mut seen = Vec::new();
    h.call("ui.scroll", json!({"x": c.x, "y": c.y, "delta": 120}));
    h.frames(40);
    seen.push(numbers(&h.app.cam));
    h.call("ui.scroll", json!({"x": c.x + 150.0, "y": c.y + 80.0, "delta": -240}));
    h.frames(40);
    seen.push(numbers(&h.app.cam));
    drag(h, c, egui::vec2(60.0, 30.0), "middle", json!({}));
    seen.push(numbers(&h.app.cam));
    drag(h, c, egui::vec2(50.0, 60.0), "middle", json!({"shift": true}));
    seen.push(numbers(&h.app.cam));
    drag(h, c, egui::vec2(-40.0, -30.0), "right", json!({}));
    seen.push(numbers(&h.app.cam));
    drag(h, c, egui::vec2(0.0, 80.0), "middle", json!({"ctrl": true}));
    seen.push(numbers(&h.app.cam));
    drag(h, empty, egui::vec2(70.0, 0.0), "left", json!({"ctrl": true}));
    seen.push(numbers(&h.app.cam));
    for key in ["F2", "F3", "F4"] {
        h.call("ui.key", json!({"key": key, "pressed": true}));
        drag(h, empty, egui::vec2(40.0, 40.0), "left", json!({}));
        h.call("ui.key", json!({"key": key, "pressed": false}));
        h.frames(2);
        seen.push(numbers(&h.app.cam));
    }
    for (key, shift) in [("Home", false), ("End", false), ("PageUp", false), ("F5", true)] {
        h.call("ui.key", json!({"key": key, "shift": shift}));
        h.frames(60);
        seen.push(numbers(&h.app.cam));
    }
    seen
}

/// A wheel and a middle drag on a drawing's sheet, the camera after each.
fn replay_sheet(h: &mut Harness) -> Vec<[f64; 7]> {
    h.app.home.open = false;
    h.app.run("drawing.new", json!({"properties": {"drawn_by": "TCI", "drawn_date": "2026-10-28"}})).unwrap();
    h.frames(3);
    let rect = crate::scenario::rect_of("sheet_canvas").expect("the sheet canvas is drawn");
    let c = rect.center();
    let mut seen = Vec::new();
    h.call("ui.scroll", json!({"x": c.x, "y": c.y, "delta": 120}));
    h.frames(40);
    seen.push(numbers(&h.app.cam));
    drag(h, c, egui::vec2(60.0, -40.0), "middle", json!({}));
    seen.push(numbers(&h.app.cam));
    seen
}

fn assert_same(what: &str, seen: &[[f64; 7]], pinned: &[[f64; 7]]) {
    assert_eq!(seen.len(), pinned.len(), "{what}: {seen:?}");
    for (i, (a, b)) in seen.iter().zip(pinned).enumerate() {
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() <= 1e-6 * y.abs().max(1.0), "{what} step {i}: {a:?} is not the pinned {b:?}\nall: {seen:?}");
        }
    }
}

/// The pin (F-2): written and recorded against the code before the profile's `navigation`
/// block existed. Under SolidCraft (never chosen, as tests and headless use run, and chosen) the
/// same wheel, drag, hold-key and navigation-key inputs leave the camera exactly where they did.
#[test]
fn solidcraft_navigation_is_unchanged() {
    for chosen in [None, Some("solidcraft")] {
        let mut h = Harness::new();
        if let Some(id) = chosen {
            assert_eq!(h.call("ui.prefs", json!({"ui_profile": id}))["ok"], json!(true));
        }
        model(&mut h);
        assert_same(&format!("{chosen:?} model"), &replay_model(&mut h), &PINNED_MODEL);
        let mut h = Harness::new();
        if let Some(id) = chosen {
            assert_eq!(h.call("ui.prefs", json!({"ui_profile": id}))["ok"], json!(true));
        }
        assert_same(&format!("{chosen:?} sheet"), &replay_sheet(&mut h), &PINNED_SHEET);
    }
}

/// Recorded at 6a7a74e8, before the `navigation` block.
#[rustfmt::skip]
const PINNED_MODEL: [[f64; 7]; 14] = [
    // wheel forward 120 at the centre (over the box)
    [4.549209149718985, 3.3410808411643496, 3.4361488472990303, 0.6, 0.5, 0.0, 167.0540420582175],
    // wheel back 240 off the centre
    [-1.5743084570397983, 10.316324294284655, 7.645023527377685, 0.6, 0.5, 0.0, 239.44347293477682],
    // middle drag: pan
    [-9.783603880494177, 19.388239133862808, 12.86567983560202, 0.6, 0.5, 0.0, 239.44347293477682],
    // Shift+middle drag: orbit
    [-9.783603880494177, 19.388239133862808, 12.86567983560202, 1.0, 0.98, 0.0, 239.44347293477682],
    // right drag: orbit
    [-9.783603880494177, 19.388239133862808, 12.86567983560202, 0.6799999999999999, 0.74, 0.0, 239.44347293477682],
    // Ctrl+middle drag: pans (Fusion's middle binding)
    [-3.057571723855112, 27.7057282132111, 24.580560256660515, 0.6799999999999999, 0.74, 0.0, 239.44347293477682],
    // Ctrl+left drag on empty space: orbit
    [-3.057571723855112, 27.7057282132111, 24.580560256660515, 1.24, 0.74, 0.0, 239.44347293477682],
    // F2 held + left drag: nothing
    [-3.057571723855112, 27.7057282132111, 24.580560256660515, 1.24, 0.74, 0.0, 239.44347293477682],
    // F3 held + left drag: nothing
    [-3.057571723855112, 27.7057282132111, 24.580560256660515, 1.24, 0.74, 0.0, 239.44347293477682],
    // F4 held + left drag: nothing
    [-3.057571723855112, 27.7057282132111, 24.580560256660515, 1.24, 0.74, 0.0, 239.44347293477682],
    // Home: nothing
    [-3.057571723855112, 27.7057282132111, 24.580560256660515, 1.24, 0.74, 0.0, 239.44347293477682],
    // End: nothing
    [-3.057571723855112, 27.7057282132111, 24.580560256660515, 1.24, 0.74, 0.0, 239.44347293477682],
    // PageUp: nothing
    [-3.057571723855112, 27.7057282132111, 24.580560256660515, 1.24, 0.74, 0.0, 239.44347293477682],
    // Shift+F5: nothing
    [-3.057571723855112, 27.7057282132111, 24.580560256660515, 1.24, 0.74, 0.0, 239.44347293477682],
];
/// Recorded at 6a7a74e8, before the `navigation` block.
#[rustfmt::skip]
const PINNED_SHEET: [[f64; 7]; 2] = [
    // wheel forward 120 at the canvas centre
    [13.244507042253522, 12.095352112676057, 0.0, 0.0, std::f64::consts::FRAC_PI_2, 0.0, 28.695061034246557],
    // middle drag: pan
    [11.818665500179156, 11.14479108462648, -5.820507601975651e-17, 0.0, std::f64::consts::FRAC_PI_2, 0.0, 28.695061034246557],
];
