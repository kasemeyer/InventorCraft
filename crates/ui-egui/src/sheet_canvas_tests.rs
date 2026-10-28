#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The sheet canvas (Plan D-2 Task 3): what the GPU pass is handed for the active sheet, the paper
//! mapping at any zoom, the 2D navigation guard, and drawing commands from the ribbon and menus.

use std::sync::Arc;

use egui::{Event, Rect, pos2, vec2};
use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::doc::DocumentKind;
use solidcraft_engine::drawing::{CurveKind, PaperPick};
use solidcraft_engine::render::{Camera, Scene};
use solidcraft_gpu::sheet::SheetScene;

use super::*;
use crate::scenario::Harness;
use crate::space_mouse::{DeviceMotion, Motion};
use crate::test_support::{block_part, inventor};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// `h` with Home closed and a new D drawing (`drawing.new {}`) on its session.
fn new_drawing(mut h: Harness) -> Harness {
    h.app.home.open = false;
    h.app.run("drawing.new", json!({})).unwrap();
    h.frames(3);
    h
}

/// An Inventor drawing with the block's front view VIEW1 at [10, 8] (hidden lines, scale 1,
/// shaded as asked); the block's path and the view id.
fn front_view(shaded: bool) -> (Harness, String, u64) {
    let mut h = new_drawing(inventor(DocumentKind::Drawing));
    let block = block_part(&h.dir);
    let r = h
        .app
        .run(
            "drawing.view.base",
            json!({"model": block, "orientation": "front", "lines": "hidden", "shaded": shaded, "scale": 1, "position": [10, 8]}),
        )
        .unwrap();
    h.frames(2);
    (h, block, r["view"].as_u64().unwrap())
}

fn canvas() -> Rect {
    crate::scenario::rect_of("sheet_canvas").expect("the sheet canvas is drawn")
}

fn rgba(c: egui::Color32) -> [u8; 4] {
    c.to_srgba_unmultiplied()
}

fn strokes_in(s: &SheetScene, color: [u8; 4]) -> usize {
    s.strokes.iter().filter(|x| x.color == color).count()
}

#[test]
fn a_drawing_shows_the_sheet() {
    let h = new_drawing(inventor(DocumentKind::Drawing));
    // The profile's theme (TCI Dark under Inventor) is on once the harness is.
    let t = Tokens::get();
    let s = scene(&h.app);
    assert_eq!(s.paper, [0.0, 0.0, 34.0, 22.0]);
    assert_eq!(s.paper_color, rgba(t.sheet_paper));
    assert!(canvas().width() > 100.0);
    // A blank sheet is its border and title block only, all in the border colour.
    let border = strokes_in(&s, rgba(t.sheet_border));
    // The frame, its zone ticks and letters, and the title block's lines and fields.
    assert_eq!(border, 220);
    assert_eq!(border, s.strokes.len());
    assert!(s.fills.iter().all(|f| f.color == rgba(t.sheet_border)));
    assert!(s.images.is_empty());
    // R88: a Drawing document with no model (the bare harness) shows the surround only.
    let mut bare = inventor(DocumentKind::Drawing);
    bare.app.home.open = false;
    bare.frames(2);
    let s = scene(&bare.app);
    assert!(s.strokes.is_empty() && s.fills.is_empty() && s.images.is_empty());
    assert_eq!(s.paper, [0.0; 4]);
    assert!(crate::scenario::rect_of("sheet_canvas").is_some());
}

#[test]
fn the_canvas_starts_fitted() {
    let mut h = new_drawing(inventor(DocumentKind::Drawing));
    let rect = canvas();
    // The first frame fitted it; fitting again after a pan puts it back.
    h.app.cam.target.x += 5.0;
    fit(&mut h.app, rect);
    // It fits the part of the canvas the browser and the cards leave free.
    let area = crate::shell::content_rect(&h.app, rect);
    let (a, b) = (screen_of(&h.app, rect, [0.0, 0.0]), screen_of(&h.app, rect, [34.0, 22.0]));
    assert!(area.expand(0.01).contains(a) && area.expand(0.01).contains(b), "{a:?} {b:?} in {area:?} of {rect:?}");
    let (w, ht) = (b.x - a.x, a.y - b.y);
    let fills = (w / area.width() - 1.0).abs() < 0.02 || (ht / area.height() - 1.0).abs() < 0.02;
    assert!(fills, "{w} x {ht} in {area:?}");
    // Centred in it.
    let mid = pos2((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    assert!((mid - area.center()).length() < 0.5, "{mid:?} {area:?}");
    // Paper x right, y up.
    assert!(w > 0.0 && ht > 0.0);
    h.frames(2);
    assert!(on_sheet(&h.app.cam));
}

#[test]
fn picks_map_at_any_zoom() {
    let (mut h, _, v) = front_view(false);
    let rect = canvas();
    let d0 = h.app.cam.distance;
    let at = screen_of(&h.app, rect, [10.0, 6.5]);
    h.frame_with(vec![Event::PointerMoved(at)]);
    // Wheel in at the point until the sheet is 8 times bigger.
    let wheel = |dy: f32| Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: vec2(0.0, dy),
        modifiers: egui::Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    };
    h.frame_with(vec![Event::PointerMoved(at), wheel(3.0)]);
    h.frames(20);
    let sign = if h.app.cam.distance < d0 { 3.0 } else { -3.0 };
    for _ in 0..600 {
        if h.app.cam.distance <= d0 / 8.0 {
            break;
        }
        h.frame_with(vec![Event::PointerMoved(at), wheel(sign)]);
    }
    h.frames(30);
    assert!(h.app.cam.distance <= d0 / 8.0, "zoomed {} → {}", d0, h.app.cam.distance);
    assert!(on_sheet(&h.app.cam), "zooming keeps the sheet flat: {:?}", h.app.cam);
    // Then pan with the middle button.
    let before = h.app.cam;
    let c = rect.center();
    h.call("ui.drag", json!({"x0": c.x, "y0": c.y, "x1": c.x + 60.0, "y1": c.y - 40.0, "button": "middle", "steps": 6}));
    h.frames(3);
    assert!(h.app.cam.target.dist(before.target) > 1e-6, "the middle drag pans");
    assert_eq!((h.app.cam.yaw, h.app.cam.pitch, h.app.cam.roll, h.app.cam.distance), (before.yaw, before.pitch, before.roll, before.distance));
    let rect = canvas();
    for p in [[0.0, 0.0], [10.0, 6.5], [34.0, 22.0]] {
        let q = paper_at_f64(&h.app, rect, screen_of_f64(&h.app, rect, p));
        assert!((q[0] - p[0]).abs() < 1e-9 && (q[1] - p[1]).abs() < 1e-9, "{p:?} → {q:?}");
        // Through egui's f32 points, within a hundredth of a point.
        let r = paper_at(&h.app, rect, screen_of(&h.app, rect, p));
        let tol = 0.01 / points_per_inch(&h.app, rect);
        assert!((r[0] - p[0]).abs() < tol && (r[1] - p[1]).abs() < tol, "{p:?} → {r:?}");
    }
    // A click on the view's bottom edge selects it.
    let edge = screen_of(&h.app, rect, [10.0, 6.5]);
    assert!(rect.contains(edge), "{edge:?} in {rect:?}");
    h.step(&json!({"click": [edge.x, edge.y]})).unwrap();
    h.frames(2);
    match h.app.drawing.selection.as_slice() {
        [PaperPick::ViewEdge { view, curve, .. }] => assert_eq!((*view, *curve), (v, CurveKind::Line)),
        other => panic!("selected {other:?}"),
    }
    // Shift adds; a click on empty paper clears.
    let hole = screen_of(&h.app, rect, [10.375, 8.0]);
    if rect.contains(hole) {
        h.step(&json!({"click": [hole.x, hole.y], "shift": true})).unwrap();
        h.frames(2);
        assert_eq!(h.app.drawing.selection.len(), 2);
    }
}

#[test]
fn a_sick_dimension_draws_in_the_warning_colour() {
    let (mut h, block, v) = front_view(false);
    let t = Tokens::get();
    let r = h.app.run("drawing.dim.general", json!({"view": v, "picks": [{"at": [10.375, 8.0]}], "placement": [11.0, 8.6]})).unwrap();
    let dim = r["handle"].as_u64().unwrap();
    h.frames(2);
    assert_eq!(strokes_in(&scene(&h.app), rgba(t.warning)), 0, "a healthy dimension is not in the warning colour");
    // D-1's delete-hole path: the hole goes from the model, then the drawing updates.
    let mut m = Session::default();
    run(&mut m, "doc.open", json!({"path": block}));
    run(&mut m, "timeline.delete", json!({"feature": "Drill"}));
    run(&mut m, "file.save", json!({"path": block}));
    h.app.run("drawing.update", json!({})).unwrap();
    h.frames(2);
    let model = h.app.session.doc.drawing.as_deref().unwrap();
    assert_eq!(solidcraft_engine::drawing::sick_dimensions(model).into_iter().collect::<Vec<_>>(), [dim]);
    let s = scene(&h.app);
    let warned = strokes_in(&s, rgba(t.warning));
    assert!(warned >= 3, "the dimension's text and leader: {warned}");
    // Only the sick dimension: the border keeps its colour.
    assert!(strokes_in(&s, rgba(t.sheet_border)) > 20);
}

/// A rasterizer drawing a solid grey picture of the asked size.
struct Solid;

impl solidcraft_engine::drawing::ViewRasterizer for Solid {
    fn rasterize(&self, _: &Scene, _: &Camera, w: u32, h: u32) -> Result<solidcraft_engine::capture::Picture, String> {
        Ok(solidcraft_engine::capture::Picture { rgba: [128, 128, 128, 255].repeat((w * h) as usize), renderer: "test" })
    }
}

#[test]
fn only_a_fresh_picture_is_drawn() {
    let mut h = new_drawing(inventor(DocumentKind::Drawing));
    let blank = scene(&h.app).strokes.len();
    h.app.session.view_rasterizer = Some(Arc::new(Solid));
    let block = block_part(&h.dir);
    let r = h
        .app
        .run("drawing.view.base", json!({"model": block, "orientation": "front", "lines": "hidden", "shaded": true, "scale": 1, "position": [10, 8]}))
        .unwrap();
    let v = r["view"].as_u64().unwrap();
    h.frames(2);
    let s = scene(&h.app);
    assert_eq!(s.images.len(), 1, "one picture, drawn once");
    assert_eq!(s.images[0].rect, [8.0, 6.5, 12.0, 9.5]);
    assert_eq!(s.images[0].size, [800, 600]);
    assert_eq!(&s.images[0].rgba[..4], &[128, 128, 128, 255]);
    // Under the overlay: the view's lines are drawn too, and the pass draws pictures first.
    let lines = s.strokes.len();
    // The block's 9 visible curves and its label's strokes over the blank sheet's 220.
    assert_eq!((blank, lines), (220, 235));
    assert_eq!(solidcraft_gpu::sheet::SheetPass::draw_list(&s)[0], solidcraft_gpu::sheet::SheetDraw::Image);
    // No renderer: rescaling leaves the picture stale, so it is left out and the lines stay (R76).
    h.app.session.view_rasterizer = None;
    h.app.run("drawing.view.set_scale", json!({"view": v, "scale": 2})).unwrap();
    h.frames(2);
    let s = scene(&h.app);
    assert!(s.images.is_empty());
    assert!(s.strokes.len() > blank, "the view's lines are still drawn");
}

#[test]
fn rotation_turns_a_part_but_not_a_sheet() {
    let turn = DeviceMotion { motion: Motion { translation: [0.0; 3], rotation: [0.5, 0.5, 0.5] }, seconds: 0.05 };
    let slide = DeviceMotion { motion: Motion { translation: [0.5, 0.0, 0.5], rotation: [0.0; 3] }, seconds: 0.05 };
    let mut sheet = new_drawing(inventor(DocumentKind::Drawing));
    let cam = sheet.app.cam;
    sheet.app.viewport.device_motion = Some(turn);
    sheet.frame();
    assert!(sheet.app.viewport.device_motion.is_none(), "the sheet takes the motion");
    assert_eq!(sheet.app.cam, cam, "rotation axes leave the sheet alone");
    sheet.app.viewport.device_motion = Some(slide);
    sheet.frame();
    assert!(sheet.app.cam.target.dist(cam.target) > 1e-6, "translation pans the sheet");
    assert_eq!((sheet.app.cam.yaw, sheet.app.cam.pitch, sheet.app.cam.roll), (cam.yaw, cam.pitch, cam.roll));
    let mut part = inventor(DocumentKind::Part);
    part.app.home.open = false;
    part.frames(2);
    let cam = part.app.cam;
    part.app.viewport.device_motion = Some(turn);
    part.frame();
    assert!(part.app.cam.yaw != cam.yaw || part.app.cam.pitch != cam.pitch, "rotation turns a part");
}

#[test]
fn a_rowless_drawing_command_runs_directly() {
    let mut h = new_drawing(inventor(DocumentKind::Drawing));
    let undo = h.app.session.undo.len();
    assert!(crate::drawing_ui::start_for("drawing.sheet.add").is_none());
    h.app.start("drawing.sheet.add");
    h.frames(2);
    let m = h.app.session.doc.drawing.as_deref().unwrap();
    assert_eq!(m.sheets.iter().map(|s| s.layout.as_str()).collect::<Vec<_>>(), ["Sheet:1", "Sheet:2"]);
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    // The new sheet is the active one, and the canvas shows it.
    assert_eq!(h.app.session.drawing_sheet, 1);
    assert_eq!(scene(&h.app).paper, [0.0, 0.0, 34.0, 22.0]);
    // A part has no sheet to add to: the design is unchanged.
    let mut p = inventor(DocumentKind::Part);
    p.app.home.open = false;
    let (undo, revision) = (p.app.session.undo.len(), p.app.session.revision);
    p.app.start("drawing.sheet.add");
    p.frames(2);
    assert_eq!((p.app.session.undo.len(), p.app.session.revision), (undo, revision));
    assert!(p.app.session.doc.drawing.is_none());
    assert_eq!(p.app.session.doc.kind, DocumentKind::Part);
}

#[test]
fn a_sheet_never_shows_a_part_menu() {
    for h in [inventor(DocumentKind::Drawing), Harness::new()] {
        let mut h = new_drawing(h);
        let rect = canvas();
        let empty = screen_of(&h.app, rect, [25.0, 15.0]);
        h.step(&json!({"click": [empty.x, empty.y], "button": "right"})).unwrap();
        h.frames(2);
        let menu = crate::context_menu::describe(&h.app);
        assert_eq!(menu["open"], json!(true), "{menu}");
        let ids = |k: &str| menu[k].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        let (ring, list) = (ids("radial"), ids("items"));
        assert!(ring.iter().chain(&list).all(|id| !id.starts_with("solid.") && !id.starts_with("sketch.")), "{ring:?} {list:?}");
        assert_eq!(list, ["drawing.view.base", "drawing.sheet.add", "drawing.update", "-", "edit.undo"]);
        assert!(ring.is_empty(), "no ring on an empty sheet until Task 10's: {ring:?}");
    }
}

#[test]
fn the_raster_limit_matches_the_gpu() {
    assert_eq!(solidcraft_engine::drawing::MAX_RASTER_SIDE, solidcraft_gpu::capture::MAX_SIDE);
    assert_eq!(solidcraft_gpu::capture::MAX_SIDE, 8192);
}

#[test]
fn home_and_fit_frame_the_sheet() {
    let mut h = new_drawing(inventor(DocumentKind::Drawing));
    let fitted = h.app.cam;
    h.app.cam.target.x += 7.0;
    h.app.cam.distance *= 0.25;
    h.app.animate_view("home");
    assert!(h.app.cam_anim.is_none(), "a sheet does not animate a 3D view");
    h.frames(2);
    assert_eq!(h.app.cam, fitted);
    h.app.cam.target.y -= 3.0;
    h.app.fit_view();
    h.frames(2);
    assert_eq!(h.app.cam, fitted);
    // A standard 3D view turns nothing on a sheet.
    h.app.start("view.front");
    h.frames(2);
    assert_eq!(h.app.cam, fitted);
    // `{"paper": [x, y]}` is a scenario point.
    let p = h.at(&json!({"paper": [17.0, 11.0]})).unwrap();
    let c = screen_of_f64(&h.app, canvas(), [17.0, 11.0]);
    assert!((p[0] - c[0]).abs() < 1e-3 && (p[1] - c[1]).abs() < 1e-3, "{p:?} {c:?}");
}

#[test]
fn suppressed_curves_are_not_drawn() {
    let (mut h, _, v) = front_view(false);
    let r = h.app.run("drawing.view.projected", json!({"parent": v, "position": [10, 16]})).unwrap();
    let top = r["view"].as_u64().unwrap();
    h.frames(2);
    let shown = scene(&h.app).strokes.len();
    // Hidden Line Removed keeps the top view's two hidden lines on paper, invisible (R61): the
    // display list leaves them out, dashes and all.
    h.app.run("drawing.view.set_style", json!({"view": top, "lines": "removed"})).unwrap();
    h.frames(2);
    let removed = scene(&h.app).strokes.len();
    assert!(removed + 2 <= shown, "{shown} → {removed}");
    h.app.run("drawing.view.set_style", json!({"view": top, "lines": "hidden"})).unwrap();
    h.frames(2);
    assert_eq!(scene(&h.app).strokes.len(), shown);
}

#[test]
fn drawing_starts_are_exactly_the_tool_rows() {
    // R91 (D1): the table holds only drawing tools; Task 6 gives it its first two rows, Task 8
    // General Dimension.
    let ids: Vec<&str> = crate::drawing_ui::DRAWING_STARTS.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, ["drawing.view.base", "drawing.view.projected", "drawing.dim.general"]);
    for (i, id) in ids.iter().enumerate() {
        assert!(id.starts_with("drawing."), "{id}");
        assert_eq!(solidcraft_engine::find_command(id).map(|c| c.id), Some(*id), "{id} is a registry command");
        assert!(!ids[..i].contains(id), "{id} twice");
    }
    // BV starts placing (the panel and Stage::Base) instead of running the command.
    let mut h = new_drawing(inventor(DocumentKind::Drawing));
    for c in "BV".chars() {
        h.step(&json!({"key": c.to_string()})).unwrap();
    }
    h.frames(60);
    assert!(
        matches!(&h.app.drawing.tool, Some(crate::drawing_ui::DrawingTool::Place(p)) if matches!(p.stage, crate::drawing_place::Stage::Base)),
        "BV starts Stage::Base"
    );
    assert!(matches!(h.app.drawing.panel, Some(crate::drawing_ui::DrawingPanel::View(_))));
    assert_eq!(h.app.session.undo.len(), 0, "the command did not run");
    assert!(h.app.session.doc.drawing.as_deref().unwrap().views.is_empty());
}
