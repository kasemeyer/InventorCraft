#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Placing drawing views (Plan D-2 Task 6, spec §2.3-§2.4, §7.1 items 4, 6, 7): the on-sheet cube
//! picks the orientation without touching the camera, the live preview follows the cursor, a click
//! places the base view, later clicks project by direction, a placed view drags along its axis,
//! Esc, Enter and Done end, and a double-click opens Edit View.

use egui::Rect;
use serde_json::{Value, json};
use solidcraft_engine::doc::DocumentKind::{Drawing, Part};
use solidcraft_engine::doc::drawing::{CubeTarget, DrawingView, Projection};

use super::*;
use crate::drawing_ui::{DrawingPanel, DrawingTool};
use crate::drawing_view_panel::{Origin, ViewPanel};
use crate::scenario::{Harness, handle_at};
use crate::sheet_canvas::{curve_points, paper_at, screen_of};
use crate::test_support::{block_part, inventor};

/// An Inventor drawing of the block with the Base View panel open on the ribbon path (its model
/// set to the block, as picking it in the Model row does). The harness and the block's path.
fn placing() -> (Harness, String) {
    let (mut h, block) = block_drawing(inventor(Drawing));
    h.app.start("drawing.view.base");
    h.frames(3);
    panel_mut(&mut h).model = Some(block.clone());
    h.frames(2);
    (h, block)
}

fn block_drawing(mut h: Harness) -> (Harness, String) {
    h.app.home.open = false;
    let block = block_part(&h.dir);
    h.app.run("drawing.new", json!({"model": block})).unwrap();
    h.frames(2);
    let block = std::path::absolute(&block).unwrap().to_string_lossy().into_owned();
    (h, block)
}

fn panel(h: &Harness) -> &ViewPanel {
    match &h.app.drawing.panel {
        Some(DrawingPanel::View(p)) => p,
        _ => panic!("the Base View panel is not open"),
    }
}

fn panel_mut(h: &mut Harness) -> &mut ViewPanel {
    match &mut h.app.drawing.panel {
        Some(DrawingPanel::View(p)) => p,
        _ => panic!("the Base View panel is not open"),
    }
}

fn place_tool(h: &Harness) -> &Place {
    match &h.app.drawing.tool {
        Some(DrawingTool::Place(p)) => p,
        _ => panic!("no view is being placed"),
    }
}

fn canvas() -> Rect {
    crate::scenario::rect_of("sheet_canvas").unwrap()
}

fn screen(h: &Harness, p: [f64; 2]) -> egui::Pos2 {
    screen_of(&h.app, canvas(), p)
}

fn hover(h: &mut Harness, p: [f64; 2]) {
    let s = screen(h, p);
    h.step(&json!({"move": [s.x, s.y]})).unwrap();
    h.frames(2);
}

fn click_paper(h: &mut Harness, p: [f64; 2]) {
    let s = screen(h, p);
    h.step(&json!({"click": [s.x, s.y]})).unwrap();
    h.frames(2);
}

fn views(h: &Harness) -> Vec<DrawingView> {
    h.app.session.doc.drawing.as_deref().map_or_else(Vec::new, |m| m.views.values().cloned().collect())
}

fn view(h: &Harness, id: u64) -> DrawingView {
    h.app.session.doc.drawing.as_deref().unwrap().views[&id].clone()
}

/// The paper box [x0, y0, x1, y1] of the preview's curves.
fn preview_box(h: &Harness) -> [f64; 4] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for (c, _) in &h.app.drawing.preview {
        for [x, y] in curve_points(c) {
            let (x, y) = (f64::from(x), f64::from(y));
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        }
    }
    b
}

fn box_centre(b: [f64; 4]) -> [f64; 2] {
    [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0]
}

fn near(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
    (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol
}

/// Click the on-sheet cube's target `name` (its published facet handle).
fn cube_click(h: &mut Harness, name: &str) {
    let handle = format!("drawing_place:cube:{name}");
    let at = handle_at(&handle).unwrap_or_else(|| panic!("{handle} is not drawn: {:?}", crate::scenario::handles("drawing_place:cube")));
    h.step(&json!({"move": [at.x, at.y]})).unwrap();
    h.frames(1);
    h.step(&json!({"click": [at.x, at.y]})).unwrap();
    h.frames(3);
}

#[test]
fn each_cube_face_sets_its_orientation() {
    let (mut h, _) = placing();
    let cam = h.app.cam;
    let undo = h.app.session.undo.len();
    assert_eq!(panel(&h).orientation.name(), "front");
    assert_eq!(crate::scenario::count_of("drawing_place:cube"), Some(1.0));
    // The cube sits beside the preview's top-right corner, 60 points out.
    let centre = cube_centre(&h.app, canvas());
    let b = preview_box(&h);
    let corner = screen(&h, [b[2], b[3]]);
    assert!((centre.distance(corner) - CUBE_OUT).abs() < 0.5, "{centre:?} {corner:?}");
    assert!(centre.x > corner.x && centre.y < corner.y);
    // From the front only the front face, its edges and corners face the viewer: walk around the
    // cube through edges and corners to every face.
    for (name, kind) in [
        ("iso_top_right", "corner"),
        ("right", "face"),
        ("top_right", "edge"),
        ("top", "face"),
        ("back_top", "edge"),
        ("back", "face"),
        ("back_bottom", "edge"),
        ("bottom", "face"),
        ("bottom_left", "edge"),
        ("left", "face"),
        ("front_left", "edge"),
        ("front", "face"),
        ("front_top", "edge"),
    ] {
        cube_click(&mut h, name);
        assert_eq!(panel(&h).orientation, CubeTarget::from_name(name).unwrap(), "{kind} {name}");
        assert_eq!(h.app.cam, cam, "the cube is a picker: the sheet camera never moves");
        assert!(views(&h).is_empty(), "a cube click places nothing");
        assert_eq!(h.app.session.undo.len(), undo);
        assert!(matches!(place_tool(&h).stage, Stage::Base));
    }
    // The preview shows the picked orientation (front_top: the block seen from the front edge).
    let key = place_tool(&h).preview_key.unwrap();
    assert_eq!(key.1, CubeTarget::from_name("front_top").unwrap());
    assert!(!h.app.drawing.preview.is_empty());
}

#[test]
fn the_cube_reuses_the_navigation_cube_picker() {
    // `target_at` is the navigation cube's own hit test, edges on: the centre of the cube is the
    // facing face, a point off the cube nothing.
    let cam = solidcraft_engine::drawing::camera_for(&CubeTarget::from_name("front").unwrap().orientation(), Default::default(), 1.0);
    let c = egui::pos2(100.0, 100.0);
    let hit = crate::viewport::navigation_cube::target_at(&cam, c, f64::from(CUBE_HALF), c).unwrap();
    assert_eq!(target_of(hit), CubeTarget::from_name("front"));
    assert!(crate::viewport::navigation_cube::target_at(&cam, c, f64::from(CUBE_HALF), c + egui::vec2(200.0, 0.0)).is_none());
    let corner = c + egui::vec2(CUBE_HALF * 0.9, -CUBE_HALF * 0.9);
    assert_eq!(
        target_of(crate::viewport::navigation_cube::target_at(&cam, c, f64::from(CUBE_HALF), corner).unwrap()),
        CubeTarget::from_name("iso_top_right")
    );
}

#[test]
fn the_model_path_snaps_the_camera_direction() {
    let mut h = inventor(Part);
    let block = block_part(&h.dir);
    h.app.open_path_async(&block);
    h.settle();
    h.frames(2);
    let cam = h.app.cam;
    let look = -cam.back();
    crate::documents::create_drawing_view(&mut h.app);
    h.frames(3);
    assert_eq!(panel(&h).origin, Origin::FromModel);
    assert_eq!(panel(&h).orientation, CubeTarget::nearest(look));
    assert_eq!(panel(&h).orientation.name(), "iso_top_right");
    // Create Drawing View places as the ribbon does: the preview follows the cursor.
    assert!(matches!(place_tool(&h).stage, Stage::Base));
    hover(&mut h, [12.0, 10.0]);
    assert!(near(box_centre(preview_box(&h)), [12.0, 10.0], 0.01), "{:?}", preview_box(&h));
}

#[test]
fn the_preview_follows_the_cursor() {
    let (mut h, _) = placing();
    let undo = h.app.session.undo.len();
    hover(&mut h, [12.0, 10.0]);
    // The front view of the block: 4 lines, 4 corner arcs and the hole.
    assert_eq!(h.app.drawing.preview.len(), 9);
    let b = preview_box(&h);
    assert!(near(box_centre(b), [12.0, 10.0], 0.01), "{b:?}");
    assert!(near([b[2] - b[0], b[3] - b[1]], [4.0, 3.0], 0.01), "{b:?}");
    assert_eq!(h.app.session.undo.len(), undo, "the preview is not a command");
    assert!(views(&h).is_empty());
    // The same 0.05 in cell: no new preview_view (the same allocation).
    let first = h.app.drawing.preview.as_ptr();
    hover(&mut h, [12.01, 10.01]);
    assert_eq!(h.app.drawing.preview.as_ptr(), first);
    // Another cell: the preview moves with the cursor.
    hover(&mut h, [20.0, 6.0]);
    assert!(near(box_centre(preview_box(&h)), [20.0, 6.0], 0.01), "{:?}", preview_box(&h));
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn click_places_view1_then_top_and_right() {
    let (mut h, block) = placing();
    let cam = h.app.cam;
    hover(&mut h, [10.0, 8.0]);
    click_paper(&mut h, [10.0, 8.0]);
    let v1 = views(&h);
    assert_eq!(v1.len(), 1);
    let v1 = v1[0].clone();
    assert!(near([v1.position.x, v1.position.y], [10.0, 8.0], 1e-3), "{:?}", v1.position);
    assert_eq!((v1.identifier.as_str(), v1.parent), ("VIEW1", None));
    assert!(v1.model.path.ends_with("block.tcipart"), "{} {block}", v1.model.path);
    assert!(h.app.drawing.panel.is_none(), "the panel closes with the base view");
    assert!(matches!(place_tool(&h).stage, Stage::Project { parent } if parent == v1.id));
    assert_eq!(h.app.session.undo.len(), 1);
    assert_eq!(h.app.session.undo.last().unwrap().label, "Base View");

    // Above the base view: a top view, aligned on its axis; the preview showed it there.
    hover(&mut h, [11.0, 16.0]);
    assert!(near(box_centre(preview_box(&h)), [10.0, 16.0], 0.01), "{:?}", preview_box(&h));
    click_paper(&mut h, [11.0, 16.0]);
    // To its right, a little high: a right view, aligned on the base view's row.
    hover(&mut h, [20.0, 8.4]);
    click_paper(&mut h, [20.0, 8.4]);
    let all = views(&h);
    assert_eq!(all.len(), 3);
    let (v2, v3) = (all[1].clone(), all[2].clone());
    assert_eq!((v2.parent, v2.projection, v2.identifier.as_str()), (Some(v1.id), Some(Projection::Top), "VIEW2"));
    assert!(near([v2.position.x, v2.position.y], [v1.position.x, 16.0], 1e-3), "{:?}", v2.position);
    assert_eq!((v3.parent, v3.projection), (Some(v1.id), Some(Projection::Right)));
    assert!(near([v3.position.x, v3.position.y], [20.0, v1.position.y], 1e-3), "{:?}", v3.position);
    assert_eq!(place_tool(&h).placed, vec![v1.id, v2.id, v3.id]);
    // Enter ends: three undo steps, one per view, and the camera never moved.
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(2);
    assert!(h.app.drawing.tool.is_none() && h.app.drawing.preview.is_empty());
    let labels: Vec<&str> = h.app.session.undo.iter().map(|u| u.label.as_str()).collect();
    assert_eq!(labels, ["Base View", "Projected View", "Projected View"]);
    assert_eq!(views(&h).len(), 3);
    assert_eq!(h.app.cam, cam);
}

#[test]
fn enter_places_one_view_at_the_preview() {
    let (mut h, _) = placing();
    let rect = canvas();
    let centre = paper_at(&h.app, rect, rect.center());
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(2);
    let all = views(&h);
    assert_eq!(all.len(), 1);
    assert!(near([all[0].position.x, all[0].position.y], centre, 1e-6), "{:?} {centre:?}", all[0].position);
    assert_eq!(h.app.session.undo.len(), 1);
    assert!(h.app.drawing.panel.is_none());
    // The command goes on into projected views; another Enter ends it with nothing more.
    assert!(matches!(place_tool(&h).stage, Stage::Project { .. }));
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(2);
    assert!(h.app.drawing.tool.is_none());
    assert_eq!(views(&h).len(), 1);
}

/// Press at `from`, move to `to` in steps and stop there with the button held.
fn press_and_move(h: &mut Harness, from: egui::Pos2, to: egui::Pos2) {
    h.app.synthetic.push(egui::Event::PointerMoved(from));
    h.app.synthetic.push(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    for k in 1..=8 {
        h.app.synthetic.push(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 8.0)));
    }
    h.settle();
}

fn release(h: &mut Harness, at: egui::Pos2) {
    h.app.synthetic.push(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.settle();
}

#[test]
fn dragging_a_projected_view_keeps_its_axis() {
    let (mut h, _) = placing();
    let cam = h.app.cam;
    click_paper(&mut h, [10.0, 8.0]);
    click_paper(&mut h, [10.0, 16.0]);
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(2);
    let v2 = views(&h)[1].clone();
    assert!(near([v2.position.x, v2.position.y], [10.0, 16.0], 1e-3));
    let v1 = views(&h)[0].clone();
    let undo = h.app.session.undo.len();
    let (from, to) = (screen(&h, [v2.position.x, v2.position.y]), screen(&h, [v2.position.x + 4.0, v2.position.y + 1.0]));
    press_and_move(&mut h, from, to);
    // Mid-drag: the preview stays on the base view's column (R84), no command yet.
    let mid = box_centre(preview_box(&h));
    assert!(near(mid, [v1.position.x, v2.position.y + 1.0], 0.01), "{mid:?}");
    assert_eq!(h.app.session.undo.len(), undo);
    release(&mut h, to);
    let v2 = view(&h, v2.id);
    assert!(near([v2.position.x, v2.position.y], [v1.position.x, 17.0], 1e-3), "{:?}", v2.position);
    assert_eq!(v2.projection, Some(Projection::Top));
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.undo.last().unwrap().label, "Move View");
    assert!(h.app.drawing.preview.is_empty());
    assert_eq!(h.app.cam, cam);
}

#[test]
fn esc_after_the_base_view_keeps_it() {
    let (mut h, _) = placing();
    click_paper(&mut h, [10.0, 8.0]);
    hover(&mut h, [10.0, 16.0]);
    assert!(!h.app.drawing.preview.is_empty());
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert_eq!(views(&h).len(), 1);
    assert!(h.app.drawing.tool.is_none() && h.app.drawing.panel.is_none());
    assert!(h.app.drawing.preview.is_empty());
    assert_eq!(h.app.session.undo.len(), 1);

    // Esc before the base view: nothing placed, nothing open.
    let (mut h, _) = placing();
    hover(&mut h, [10.0, 8.0]);
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert!(views(&h).is_empty() && h.app.drawing.tool.is_none() && h.app.drawing.panel.is_none() && h.app.drawing.preview.is_empty());
}

#[test]
fn projected_starts_from_a_picked_parent() {
    let (mut h, block) = block_drawing(inventor(Drawing));
    let v1 = h.app.run("drawing.view.base", json!({"model": block, "orientation": "front", "position": [10, 8]})).unwrap()["view"].as_u64().unwrap();
    h.frames(2);
    for c in "PV".chars() {
        h.step(&json!({"key": c.to_string()})).unwrap();
    }
    if h.app.drawing.tool.is_none() {
        h.frames(60);
    }
    assert!(matches!(place_tool(&h).stage, Stage::PickParent));
    assert_eq!(h.app.status.as_ref().map(|s| s.0.as_str()), Some(crate::i18n::tr("ui.drawing.pick_parent")));
    assert!(h.app.drawing.panel.is_none());
    // A click off every view picks nothing.
    click_paper(&mut h, [25.0, 18.0]);
    assert!(matches!(place_tool(&h).stage, Stage::PickParent));
    click_paper(&mut h, [10.5, 8.5]);
    assert!(matches!(place_tool(&h).stage, Stage::Project { parent } if parent == v1));
    hover(&mut h, [10.0, 16.0]);
    click_paper(&mut h, [10.0, 16.0]);
    let all = views(&h);
    assert_eq!(all.len(), 2);
    assert_eq!((all[1].parent, all[1].projection), (Some(v1), Some(Projection::Top)));
    assert_eq!(h.app.session.undo.len(), 2);
}

fn ids(menu: &Value, key: &str) -> Vec<String> {
    menu[key].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_string()).collect()
}

#[test]
fn done_in_the_list_ends_projection() {
    for h in [inventor(Drawing), Harness::new()] {
        let (mut h, block) = block_drawing(h);
        h.app.start("drawing.view.base");
        h.frames(3);
        panel_mut(&mut h).model = Some(block);
        click_paper(&mut h, [10.0, 8.0]);
        assert_eq!(views(&h).len(), 1);
        let at = screen(&h, [25.0, 15.0]);
        h.step(&json!({"click": [at.x, at.y], "button": "right"})).unwrap();
        h.frames(2);
        let menu = crate::context_menu::describe(&h.app);
        assert_eq!(menu["open"], json!(true), "{menu}");
        assert!(ids(&menu, "radial").is_empty(), "no ring while placing: {menu}");
        assert_eq!(ids(&menu, "items"), ["ui.done", "ui.cancel"]);
        assert!(menu["items"].as_array().unwrap().iter().all(|i| i["enabled"] == json!(true)), "{menu}");
        crate::test_support::click(&mut h, "menu:ui.done");
        assert!(h.app.drawing.tool.is_none() && h.app.drawing.preview.is_empty());
        assert_eq!(views(&h).len(), 1);
    }
}

#[test]
fn double_click_on_a_view_opens_it_for_edit() {
    let (mut h, block) = block_drawing(inventor(Drawing));
    let v1 = h.app.run("drawing.view.base", json!({"model": block, "orientation": "front", "position": [10, 8]})).unwrap()["view"].as_u64().unwrap();
    h.frames(2);
    let at = screen(&h, [10.5, 8.5]);
    h.step(&json!({"click": [at.x, at.y], "double": true})).unwrap();
    h.frames(3);
    let p = panel(&h);
    assert_eq!(p.origin, Origin::Edit(v1));
    assert_eq!((p.identifier.as_str(), p.orientation.name()), ("VIEW1", "front".to_string()));
    assert!(h.app.drawing.tool.is_none(), "editing a view places nothing");
    assert_eq!(h.app.session.undo.len(), 1);
}

#[test]
fn preview_view_is_fast_enough_for_hover() {
    let (h, block) = placing();
    let params = json!({"model": block, "orientation": "iso_top_right", "lines": "hidden", "position": [12, 10]});
    solidcraft_engine::drawing::preview_view(&h.app.session, "drawing.view.base", &params).unwrap();
    let t = std::time::Instant::now();
    for _ in 0..5 {
        solidcraft_engine::drawing::preview_view(&h.app.session, "drawing.view.base", &params).unwrap();
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / 5.0;
    println!("preview_view: {ms:.2} ms per call");
    // It runs once per 0.05 in cell (R89), not per frame: well under a quarter second even in a
    // debug build (measured about 27 ms).
    assert!(ms < 250.0, "preview_view took {ms:.1} ms");
}

#[test]
fn the_cube_steps_out_of_a_larger_preview() {
    // Final wave item 15: after a turn to a larger preview the cube still sits beyond its
    // top-right corner, never on it.
    let (mut h, _) = placing();
    hover(&mut h, [10.0, 8.0]);
    let front = preview_box(&h);
    cube_click(&mut h, "iso_top_right");
    let iso = preview_box(&h);
    assert!(iso[3] > front[3] + 0.5, "the iso preview is taller: {front:?} {iso:?}");
    let centre = cube_centre(&h.app, canvas());
    let corner = screen(&h, [iso[2], iso[3]]);
    let out = CUBE_OUT * std::f32::consts::FRAC_1_SQRT_2 - 0.5;
    assert!(centre.x - corner.x >= out && corner.y - centre.y >= out, "{centre:?} beyond {corner:?}");
}

#[test]
fn a_double_click_projects_one_view() {
    // Final wave item 16.
    let (mut h, _) = placing();
    click_paper(&mut h, [10.0, 8.0]);
    hover(&mut h, [10.0, 16.0]);
    let at = screen(&h, [10.0, 16.0]);
    h.step(&json!({"click": [at.x, at.y], "double": true})).unwrap();
    h.frames(3);
    assert_eq!(
        views(&h).len(),
        2,
        "VIEW1 and one projected view: {:?}",
        views(&h).iter().map(|v| (v.identifier.clone(), v.position)).collect::<Vec<_>>()
    );
    assert_eq!(h.app.session.undo.len(), 2);
    // A later single click still projects.
    crate::test_support::clicks_expire(&mut h);
    click_paper(&mut h, [20.0, 8.0]);
    assert_eq!(views(&h).len(), 3);
}

#[test]
fn two_edges_of_one_view_project_from_it() {
    // Final wave item 17.
    let (mut h, block) = block_drawing(inventor(Drawing));
    let v1 = h.app.run("drawing.view.base", json!({"model": block, "orientation": "front", "position": [10, 8]})).unwrap()["view"].as_u64().unwrap();
    h.frames(2);
    let edge = |n: u64| solidcraft_engine::drawing::PaperPick::ViewEdge {
        view: v1,
        edge: format!("e{n}"),
        curve: solidcraft_engine::drawing::CurveKind::Line,
        handle: n,
    };
    h.app.drawing.selection = vec![edge(1), edge(2)];
    start_projected(&mut h.app);
    assert!(matches!(place_tool(&h).stage, Stage::Project { parent } if parent == v1));
}
