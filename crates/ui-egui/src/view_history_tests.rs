#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use solidcraft_engine::doc::DocumentKind::Part;

use crate::scenario::Harness;
use crate::test_support::inventor;

/// A harness whose view changes land at once (no animation).
fn h() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;
    h
}

#[test]
fn previous_view_returns_to_the_view_before_the_last_change() {
    let mut h = h();
    settle(&mut h.app, false);
    let c0 = h.app.cam;
    h.app.animate_view("top");
    settle(&mut h.app, false);
    assert!(h.app.cam != c0, "Top changed the view");
    assert!(previous(&mut h.app), "there is a view to go back to");
    settle(&mut h.app, false);
    assert!(h.app.cam == c0, "back at the view before Top: {:?}", h.app.cam);
}

#[test]
fn previous_view_with_no_history_does_nothing() {
    let mut h = h();
    let c0 = h.app.cam;
    assert!(!can_go_back(&h.app));
    assert!(!previous(&mut h.app));
    assert!(h.app.cam == c0, "the camera did not move");
}

#[test]
fn a_restored_view_is_not_pushed_again() {
    let mut h = h();
    settle(&mut h.app, false);
    let c0 = h.app.cam;
    h.app.animate_view("top");
    settle(&mut h.app, false);
    let c1 = h.app.cam;
    h.app.animate_view("front");
    settle(&mut h.app, false);
    assert!(c0 != c1 && h.app.cam != c1, "three distinct views");
    assert!(previous(&mut h.app));
    assert!(h.app.cam == c1, "first back: Top");
    settle(&mut h.app, false);
    assert!(previous(&mut h.app));
    assert!(h.app.cam == c0, "second back: the first view");
    settle(&mut h.app, false);
    assert!(!previous(&mut h.app), "the restored views were not pushed again");
    assert!(h.app.cam == c0);
}

#[test]
fn busy_frames_do_not_settle() {
    let mut h = h();
    settle(&mut h.app, false);
    let c0 = h.app.cam;
    h.app.cam.distance *= 2.0;
    settle(&mut h.app, true);
    assert!(settled(&h.app) == Some(c0), "a gesture in progress is not a settled view");
    assert!(!can_go_back(&h.app));
    settle(&mut h.app, false);
    assert!(settled(&h.app) == Some(h.app.cam), "settles once the gesture ends");
    assert!(can_go_back(&h.app));
}

#[test]
fn history_keeps_at_most_32_views() {
    let mut h = h();
    settle(&mut h.app, false);
    for i in 1..=40 {
        h.app.cam.distance = 100.0 + f64::from(i);
        settle(&mut h.app, false);
    }
    assert_eq!(h.app.views.back.len(), MAX_VIEWS);
    // The oldest views were dropped: the deepest one left is view 40 − 32 = 8.
    assert!((h.app.views.back[0].distance - 108.0).abs() < 1e-9, "{}", h.app.views.back[0].distance);
}

#[test]
fn f5_is_previous_view_under_inventor() {
    let h = inventor(Part);
    assert_eq!(crate::alias_keys::function_key_action(&h.app, egui::Key::F5), Some("ui.view_previous"));
    let h = Harness::new();
    assert_eq!(crate::alias_keys::function_key_action(&h.app, egui::Key::F5), None);
}

/// One frame of 3D mouse motion queued for the viewport, as `space_mouse::navigate_frame` hands it.
fn cap_turns(h: &mut Harness) {
    use crate::space_mouse::{DeviceMotion, Motion};
    h.app.viewport.device_motion = Some(DeviceMotion { motion: Motion { translation: [0.0; 3], rotation: [0.5, 0.0, 0.0] }, seconds: 0.02 });
    h.frames(1);
}

#[test]
fn a_3d_mouse_move_settles_once_after_the_cap_goes_idle() {
    let mut h = h();
    h.frames(2);
    let c0 = h.app.cam;
    assert!(settled(&h.app) == Some(c0));
    let depth = h.app.views.back.len();
    for _ in 0..10 {
        cap_turns(&mut h);
    }
    assert!(h.app.cam != c0, "the cap turned the view");
    assert_eq!(h.app.views.back.len(), depth, "nothing settles while the cap moves");
    // 300 ms is 18 frames at 60 Hz: still busy at 10, settled by 25.
    h.frames(10);
    assert_eq!(h.app.views.back.len(), depth, "still within 300 ms of the last motion");
    h.frames(15);
    assert_eq!(h.app.views.back.len(), depth + 1, "exactly one view pushed for the whole move");
    assert!(previous(&mut h.app));
    h.frames(2);
    assert!(h.app.cam == c0, "Previous View returns to the view before the move: {:?}", h.app.cam);
}

#[test]
fn a_spinning_view_does_not_settle() {
    let mut h = h();
    settle(&mut h.app, false);
    let c0 = h.app.cam;
    h.app.viewport.spin = true;
    h.app.cam.yaw += 0.3;
    h.frames(1);
    assert!(settled(&h.app) == Some(c0), "a spin in progress is not a settled view");
    assert!(!can_go_back(&h.app));
}

#[test]
fn cancelling_a_view_animation_ends_a_restore() {
    let mut h = h();
    h.app.views.restoring = true;
    h.app.cancel_view_animation();
    assert!(!h.app.views.restoring, "a restore the user took over is over");
}
