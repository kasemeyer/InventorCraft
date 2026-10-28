#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Fix wave 2 item 2 (owner-reported): while a picking drawing tool runs (Dimension, Leader
//! attach, Center Mark, Bisector, Hole Note) the edge, circle or arc under the cursor is the hover
//! candidate, drawn over the sheet in the viewport's `hover_edge` on its `hover_edge_halo`, with no document change.
//! One code path under every profile.

use egui::Rect;
use serde_json::json;
use solidcraft_engine::doc::DocumentKind::Drawing;
use solidcraft_engine::drawing::{CurveKind, PaperPick, pick};
use solidcraft_gpu::sheet::SheetScene;

use super::*;
use crate::scenario::Harness;
use crate::test_support::{block_part, inventor};

/// The block's front view at [10, 8] (4 x 3 in: its top edge runs along y 9.5, its hole is
/// centred on [10, 8] with radius 0.375).
fn front_view(mut h: Harness) -> Harness {
    h.app.home.open = false;
    let block = block_part(&h.dir);
    h.app.run("drawing.new", json!({"model": block})).unwrap();
    h.app.run("drawing.view.base", json!({"model": block, "orientation": "front", "lines": "hidden", "scale": 1, "position": [10, 8]})).unwrap();
    h.frames(3);
    h
}

fn canvas() -> Rect {
    crate::scenario::rect_of("sheet_canvas").unwrap()
}

fn hover(h: &mut Harness, p: [f64; 2]) {
    let s = screen_of(&h.app, canvas(), p);
    h.step(&json!({"move": [s.x, s.y]})).unwrap();
    h.frames(2);
}

fn start(h: &mut Harness, id: &str) {
    h.app.start(id);
    h.frames(2);
}

/// What sits exactly at `at` (a tight pick: the curve itself).
fn curve_at(h: &Harness, at: [f64; 2]) -> PaperPick {
    pick(&h.app.session, at, 0.01).unwrap()
}

fn hover_strokes(s: &SheetScene) -> usize {
    let c = Tokens::get().hover_edge.to_srgba_unmultiplied();
    s.strokes.iter().filter(|x| x.color == c).count()
}

fn handle(p: &PaperPick) -> u64 {
    match p {
        PaperPick::ViewEdge { handle, .. } => *handle,
        other => panic!("not a view edge: {other:?}"),
    }
}

const TOP: [f64; 2] = [10.0, 9.5];
/// Just off the top edge, within the 4 pt pick tolerance.
const NEAR_TOP: [f64; 2] = [10.0, 9.52];
const EMPTY: [f64; 2] = [25.0, 15.0];
const HOLE: [f64; 2] = [10.375, 8.0];

fn dimension_hover_marks_the_edge_under_the_cursor(mut h: Harness) {
    let model = h.app.session.doc.drawing.clone().unwrap();
    let undo = h.app.session.undo.len();
    start(&mut h, "drawing.dim.general");
    // A theme's hover token may also be a layer colour on paper (SolidCraft's): count from here.
    let base = hover_strokes(&scene(&h.app));
    let top = curve_at(&h, TOP);
    assert!(matches!(top, PaperPick::ViewEdge { curve: CurveKind::Line, .. }), "{top:?}");

    hover(&mut h, NEAR_TOP);
    let got = h.app.drawing.hover.clone().expect("the top edge is the hover candidate");
    assert_eq!(handle(&got), handle(&top), "exactly the curve under the cursor");
    assert_eq!(got, top);
    // Drawn once more, last (over the sheet and the preview), in the hover token; nothing else is.
    let s = scene(&h.app);
    assert_eq!(hover_strokes(&s), base + 1);
    let t = Tokens::get();
    let [.., halo, last] = s.strokes.as_slice() else { panic!("no strokes") };
    assert_eq!((last.color, last.width_px), (t.hover_edge.to_srgba_unmultiplied(), 2.0));
    // On the viewport's halo, 3 px wider, along the same points.
    assert_eq!((halo.color, halo.width_px, &halo.points), (t.hover_edge_halo.to_srgba_unmultiplied(), 5.0, &last.points));
    // It is the top edge: a straight stroke along y 9.5 between the fillets.
    assert!(
        last.points.iter().all(|p| (p[1] - 9.5).abs() < 1e-3) && last.points.iter().any(|p| p[0] < 10.0) && last.points.iter().any(|p| p[0] > 10.0),
        "{:?}",
        last.points
    );
    assert_eq!(crate::status_bar::texts(&h.app).left, crate::i18n::tr("ui.drawing.prompt_dimension"), "the prompt as before");

    hover(&mut h, EMPTY);
    assert_eq!(h.app.drawing.hover, None, "empty paper marks none");
    assert_eq!(hover_strokes(&scene(&h.app)), base);

    // Hovering is no command: no undo step, the same model.
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(std::sync::Arc::ptr_eq(&model, h.app.session.doc.drawing.as_ref().unwrap()));

    // The tool ends: nothing is marked.
    hover(&mut h, NEAR_TOP);
    assert!(h.app.drawing.hover.is_some());
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.drawing.hover, None);
    assert_eq!(hover_strokes(&scene(&h.app)), base);
}

#[test]
fn dimension_hover_marks_the_edge_under_the_cursor_under_inventor() {
    dimension_hover_marks_the_edge_under_the_cursor(front_view(inventor(Drawing)));
}

#[test]
fn dimension_hover_marks_the_edge_under_the_cursor_under_solidcraft() {
    dimension_hover_marks_the_edge_under_the_cursor(front_view(Harness::new()));
}

#[test]
fn no_picking_tool_no_hover() {
    let mut h = front_view(inventor(Drawing));
    hover(&mut h, NEAR_TOP);
    assert_eq!(h.app.drawing.hover, None, "no tool: the plain selection has no pre-highlight here");
    start(&mut h, "drawing.note.text");
    hover(&mut h, NEAR_TOP);
    assert_eq!(h.app.drawing.hover, None, "Text picks no curve");
}

#[test]
fn each_picking_tool_offers_only_what_it_takes() {
    let mut h = front_view(inventor(Drawing));
    let hole = curve_at(&h, HOLE);
    assert!(matches!(hole, PaperPick::ViewEdge { curve: CurveKind::Circle, .. }), "{hole:?}");
    let top = curve_at(&h, TOP);

    // Center Mark: circles and arcs only.
    start(&mut h, "drawing.centermark");
    hover(&mut h, NEAR_TOP);
    assert_eq!(h.app.drawing.hover, None, "a straight edge takes no centre mark");
    hover(&mut h, HOLE);
    assert_eq!(h.app.drawing.hover, Some(hole.clone()));
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);

    // Bisector: straight edges only.
    start(&mut h, "drawing.centerline.bisector");
    hover(&mut h, HOLE);
    assert_eq!(h.app.drawing.hover, None, "a circle is no bisector edge");
    hover(&mut h, NEAR_TOP);
    assert_eq!(h.app.drawing.hover, Some(top.clone()));
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);

    // Leader: the arrow end's attachment, then nothing once the leader has begun.
    start(&mut h, "drawing.note.leader");
    hover(&mut h, NEAR_TOP);
    assert_eq!(h.app.drawing.hover, Some(top.clone()));
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);

    // Hole Note: the edge.
    start(&mut h, "drawing.note.hole");
    hover(&mut h, HOLE);
    assert_eq!(h.app.drawing.hover, Some(hole));
}
