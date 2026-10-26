#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The view cube under a profile that turns on its edge targets, compass ring and menu.

use super::*;
use crate::context_menu::{Target, items, run_item};
use crate::scenario::{Harness, handle_at};
use crate::test_support::inventor;
use solidcraft_engine::doc::DocumentKind;
use solidcraft_engine::render::{StandardView, UpAxis};

const VIEWS: [StandardView; 7] =
    [StandardView::Front, StandardView::Back, StandardView::Top, StandardView::Bottom, StandardView::Left, StandardView::Right, StandardView::Iso];

fn directions(up: UpAxis, edges: bool) -> Vec<Vec3> {
    let mut out: Vec<Vec3> = Vec::new();
    for view in VIEWS {
        let mut cam = Camera { up, ..Default::default() };
        cam.set_view(view);
        for facet in facets(&cam, Pos2::ZERO, HALF, edges) {
            if !out.contains(&facet.direction) {
                out.push(facet.direction);
            }
        }
    }
    out
}

fn click(h: &mut Harness, handle: &str) {
    let p = handle_at(handle).unwrap_or_else(|| panic!("no handle {handle}"));
    h.call("ui.click", json!({"x":p.x,"y":p.y}));
}

fn looks_along(h: &Harness, dir: Vec3) -> bool {
    h.app.cam.back().dist(dir.normalized().unwrap()) < 2e-6
}

#[test]
fn edge_facets_make_twenty_six_targets() {
    for up in [UpAxis::Z, UpAxis::Y] {
        let with_edges = directions(up, true);
        assert_eq!(with_edges.len(), 26, "{up:?}: six faces, twelve edges and eight corners");
        let edges = with_edges.iter().filter(|d| [d.x, d.y, d.z].into_iter().filter(|v| *v != 0.0).count() == 2).count();
        assert_eq!(edges, 12, "{up:?}");
        assert_eq!(directions(up, false).len(), 14, "{up:?}: upstream keeps edges on their faces");
    }
}

#[test]
fn inventor_cube_edges_turn_to_the_edge_view() {
    let mut h = inventor(DocumentKind::Part);
    h.app.preferences.look_at_anim = false;
    let design = h.app.session.doc.to_json();
    for (name, dir) in [("front_right", Vec3::X - Vec3::Y), ("top_front", Vec3::Z - Vec3::Y), ("top_right", Vec3::Z + Vec3::X)] {
        h.call("ui.view", json!({"view":"iso"}));
        click(&mut h, &format!("navigation_cube:{name}"));
        assert!(looks_along(&h, dir), "{name}: {:?}", h.app.cam.back());
    }
    assert_eq!(h.app.session.doc.to_json(), design);
    assert!(h.app.session.undo.is_empty());
}

#[test]
fn solidcraft_cube_keeps_edges_on_their_faces() {
    let mut h = Harness::new();
    h.call("ui.prefs", json!({"navigator_style":"cube"}));
    h.call("ui.view", json!({"view":"iso"}));
    assert!(handle_at("navigation_cube:front").is_some(), "the cube is shown");
    assert!(handle_at("navigation_cube:front_right").is_none());
    assert!(handle_at("navigation_cube:top_front").is_none());
}

#[test]
fn compass_letters_turn_the_view_to_that_side() {
    let mut h = inventor(DocumentKind::Part);
    h.app.preferences.look_at_anim = false;
    assert_eq!(h.app.cam.up, UpAxis::Z);
    h.call("ui.view", json!({"view":"iso"}));
    for (letter, dir) in [("s", -Vec3::Y), ("e", Vec3::X), ("n", Vec3::Y), ("w", -Vec3::X)] {
        click(&mut h, &format!("navigation_cube:compass:{letter}"));
        assert!(looks_along(&h, dir), "{letter}: {:?}", h.app.cam.back());
    }
    assert!(h.app.session.undo.is_empty());

    let mut h = Harness::new();
    h.call("ui.prefs", json!({"navigator_style":"cube"}));
    h.call("ui.view", json!({"view":"iso"}));
    assert!(handle_at("navigation_cube:front").is_some(), "the cube is shown");
    for name in ["n", "e", "s", "w", "ring"] {
        assert!(handle_at(&format!("navigation_cube:compass:{name}")).is_none(), "{name}");
    }
}

#[test]
fn dragging_the_compass_ring_turns_about_up_only() {
    let mut h = inventor(DocumentKind::Part);
    h.call("ui.view", json!({"view":"iso"}));
    h.frames(2);
    let (pitch, yaw) = (h.app.cam.pitch, h.app.cam.yaw);
    let p = handle_at("navigation_cube:compass:ring").unwrap();
    h.call("ui.drag", json!({"x0":p.x,"y0":p.y,"x1":p.x+40.0,"y1":p.y,"steps":20}));
    h.frames(2);
    assert!(h.app.viewport.axis_drag.is_none(), "release ends the ring gesture");
    assert!((h.app.cam.pitch - pitch).abs() < 1e-9, "pitch {pitch} -> {}", h.app.cam.pitch);
    assert!((h.app.cam.yaw - yaw).abs() > 0.1, "yaw {yaw} -> {}", h.app.cam.yaw);
}

#[test]
fn right_click_on_the_cube_offers_home_and_projection() {
    let mut h = inventor(DocumentKind::Part);
    h.app.preferences.look_at_anim = false;
    let p = handle_at("navigation_cube:front").unwrap();
    h.call("ui.click", json!({"x":p.x,"y":p.y,"button":"right"}));
    assert!(h.app.viewport.context_menu.is_some(), "the menu opens");
    assert_eq!(h.app.menu.target, Target::ViewCube);
    let menu = items(&h.app, &Target::ViewCube);
    let ids: Vec<_> = menu.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["ui.viewcube.home", "ui.viewcube.orthographic", "ui.viewcube.perspective"]);
    let item = |app: &SolveApp, id: &str| items(app, &Target::ViewCube).into_iter().find(|i| i.id == id).unwrap();

    let pick = item(&h.app, "ui.viewcube.perspective");
    run_item(&mut h.app, &pick, p);
    assert!(h.app.ui.perspective);
    assert_eq!(item(&h.app, "ui.viewcube.perspective").shortcut, "current", "the active projection is checked");
    assert_eq!(item(&h.app, "ui.viewcube.orthographic").shortcut, "");
    let pick = item(&h.app, "ui.viewcube.orthographic");
    run_item(&mut h.app, &pick, p);
    assert!(!h.app.ui.perspective);
    assert_eq!(item(&h.app, "ui.viewcube.orthographic").shortcut, "current");

    h.call("ui.view", json!({"view":"top"}));
    assert!(h.app.cam.back().dist(Camera::default().back()) > 0.1);
    let pick = item(&h.app, "ui.viewcube.home");
    run_item(&mut h.app, &pick, p);
    assert!(h.app.cam.back().dist(Camera::default().back()) < 2e-6);

    // SolidCraft's cube keeps its own behaviour: the viewport menu, not the cube's.
    let mut h = Harness::new();
    h.call("ui.prefs", json!({"navigator_style":"cube"}));
    let p = handle_at("navigation_cube:front").unwrap();
    h.call("ui.click", json!({"x":p.x,"y":p.y,"button":"right"}));
    assert_ne!(h.app.menu.target, Target::ViewCube);
}
