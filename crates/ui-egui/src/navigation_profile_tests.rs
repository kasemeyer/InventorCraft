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

// ---- F-2 item 2: the wheel ----

/// A harness on `profile` (SolidCraft or Inventor) with an empty part in view, the camera at
/// `start_camera`.
fn on(profile: &str) -> Harness {
    let mut h = Harness::new();
    assert_eq!(h.call("ui.prefs", json!({"ui_profile": profile}))["ok"], json!(true));
    h.app.home.open = false;
    h.frames(3);
    h.app.cam_anim = None;
    h.app.cam = start_camera();
    h.app.cam.fov = 0.0;
    h.frames(2);
    h
}

fn wheel(lines: f32) -> egui::Event {
    egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, lines),
        modifiers: egui::Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    }
}

/// The distance ratio one notch (`lines` = ±1) leaves after the view settles.
fn notch(h: &mut Harness, lines: f32) -> f64 {
    let rect = h.app.viewport.rect.unwrap();
    let at = rect.center() + egui::vec2(120.0, -60.0);
    h.frame_with(vec![egui::Event::PointerMoved(at)]);
    let before = h.app.cam.distance;
    h.frame_with(vec![egui::Event::PointerMoved(at), wheel(lines)]);
    h.frames(60);
    h.app.cam.distance / before
}

#[test]
fn the_wheel_zooms_each_profiles_way_by_its_step() {
    // SolidCraft: away zooms in by exp(-40 × 0.0015) a notch, as always.
    let mut s = on("solidcraft");
    let step = (-40.0_f64 * 0.0015).exp();
    assert!((notch(&mut s, 1.0) - step).abs() < 1e-9, "SolidCraft forward");
    assert!((notch(&mut s, -1.0) - 1.0 / step).abs() < 1e-9, "SolidCraft back");
    // Inventor: away zooms out, 15 % a notch; toward you zooms in by 15 %.
    let mut i = on("inventor");
    assert!((notch(&mut i, 1.0) - 1.0 / 0.85).abs() < 1e-9, "Inventor forward: {}", notch(&mut on("inventor"), 1.0));
    assert!((notch(&mut i, -1.0) - 0.85).abs() < 1e-9, "Inventor back");
    // Three notches are three steps.
    assert!((notch(&mut i, -3.0) - 0.85_f64.powi(3)).abs() < 1e-9);
}

#[test]
fn inventors_wheel_has_no_smoothing_tail_and_solidcrafts_keeps_it() {
    for (profile, smooth) in [("inventor", false), ("solidcraft", true)] {
        let mut h = on(profile);
        let rect = h.app.viewport.rect.unwrap();
        let at = rect.center();
        h.frame_with(vec![egui::Event::PointerMoved(at)]);
        let before = h.app.cam.distance;
        h.frame_with(vec![egui::Event::PointerMoved(at), wheel(1.0)]);
        let first = h.app.cam.distance;
        h.frames(60);
        let last = h.app.cam.distance;
        assert_ne!(first, before, "{profile}: the notch moves the view in its frame");
        if smooth {
            assert!((first - last).abs() > 1e-6, "{profile}: the notch is spread over frames");
        } else {
            assert_eq!(first, last, "{profile}: the whole notch at once, nothing after it");
        }
    }
}

#[test]
fn the_wheel_zooms_about_the_cursor_under_both_profiles() {
    for profile in ["solidcraft", "inventor"] {
        let mut h = on(profile);
        let rect = h.app.viewport.rect.unwrap();
        let at = rect.center() + egui::vec2(200.0, 120.0);
        // The point on the target's depth plane under the cursor stays under it.
        let proj = crate::viewport::projection(&h.app, rect);
        let (o, d) = proj.ray(at);
        let n = h.app.cam.back();
        let world = o + d * ((h.app.cam.target - o).dot(n) / d.dot(n));
        h.frame_with(vec![egui::Event::PointerMoved(at)]);
        for lines in [1.0, -2.0] {
            h.frame_with(vec![egui::Event::PointerMoved(at), wheel(lines)]);
            h.frames(60);
            let back = crate::viewport::projection(&h.app, rect).to_screen(world).unwrap();
            assert!(back.distance(at) < 0.5, "{profile} {lines}: {back:?} vs {at:?}");
        }
    }
}

#[test]
fn a_zoom_drag_runs_each_profiles_way() {
    // Inventor: Ctrl+middle drag up zooms out (the view gets farther), down zooms in.
    let mut i = on("inventor");
    let c = i.app.viewport.rect.unwrap().center();
    let d0 = i.app.cam.distance;
    drag(&mut i, c, egui::vec2(0.0, -100.0), "middle", json!({"ctrl": true}));
    assert!((i.app.cam.distance / d0 - 1.0_f64.exp()).abs() < 1e-6, "Inventor up: {}", i.app.cam.distance / d0);
    let d1 = i.app.cam.distance;
    drag(&mut i, c, egui::vec2(0.0, 50.0), "middle", json!({"ctrl": true}));
    assert!((i.app.cam.distance / d1 - (-0.5_f64).exp()).abs() < 1e-6, "Inventor down");
    // SolidCraft (its SolidWorks mapping's Shift+middle zoom): up zooms in, as before.
    let mut s = on("solidcraft");
    s.call("ui.prefs", json!({"nav": "solidworks"}));
    let d0 = s.app.cam.distance;
    drag(&mut s, c, egui::vec2(0.0, -100.0), "middle", json!({"shift": true}));
    assert!((s.app.cam.distance / d0 - (-1.0_f64).exp()).abs() < 1e-6, "SolidCraft up: {}", s.app.cam.distance / d0);
}

#[test]
fn a_drawing_sheet_zooms_each_profiles_way() {
    for (profile, out) in [("solidcraft", false), ("inventor", true)] {
        let mut h = Harness::new();
        assert_eq!(h.call("ui.prefs", json!({"ui_profile": profile}))["ok"], json!(true));
        h.app.home.open = false;
        h.app.run("drawing.new", json!({"properties": {"drawn_by": "TCI", "drawn_date": "2026-10-28"}})).unwrap();
        h.frames(3);
        let rect = crate::scenario::rect_of("sheet_canvas").expect("the sheet canvas is drawn");
        h.frame_with(vec![egui::Event::PointerMoved(rect.center())]);
        let before = h.app.cam.distance;
        h.frame_with(vec![egui::Event::PointerMoved(rect.center()), wheel(1.0)]);
        h.frames(60);
        let ratio = h.app.cam.distance / before;
        let want = if out { 1.0 / 0.85 } else { (-40.0_f64 * 0.0015).exp() };
        assert!((ratio - want).abs() < 1e-9, "{profile}: a notch away on the sheet {ratio}");
    }
}
