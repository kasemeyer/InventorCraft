#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! General Dimension and Edit Dimension (Plan D-2 Task 8, spec §2.7, §7.1 items 8, 9): D or the
//! ribbon starts the tool; an edge, a circle, an arc or two picks then a click place one
//! dimension through `drawing.dim.general` (one undo step each) while a live dimension follows the
//! cursor; the tool goes on until Esc or Done; a double-click opens Edit Dimension, whose OK sends
//! only what changed through `drawing.dim.edit`; a dimension's right-click is a list.

use egui::Rect;
use serde_json::{Value, json};
use solidcraft_engine::doc::DocumentKind::Drawing;
use solidcraft_engine::drawing::{CurveKind, dimension_text, sick_dimensions};

use super::*;
use crate::drawing_ui::{DrawingPanel, DrawingTool};
use crate::scenario::Harness;
use crate::sheet_canvas::screen_of;
use crate::test_support::{block_part, click, inventor};

/// A drawing of the block with VIEW1, its front, at [10, 8] (hidden lines, R91 D8). The harness
/// and VIEW1's id.
fn front_view(h: Harness) -> (Harness, u64) {
    view_of(h, "front")
}

fn view_of(mut h: Harness, orientation: &str) -> (Harness, u64) {
    h.app.home.open = false;
    let block = block_part(&h.dir);
    h.app.run("drawing.new", json!({"model": block})).unwrap();
    let v = h
        .app
        .run("drawing.view.base", json!({"model": block, "orientation": orientation, "lines": "hidden", "scale": 1, "position": [10, 8]}))
        .unwrap()["view"]
        .as_u64()
        .unwrap();
    h.frames(3);
    (h, v)
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
    hover(h, p);
    let s = screen(h, p);
    h.step(&json!({"click": [s.x, s.y]})).unwrap();
    h.frames(2);
}

fn double_click_paper(h: &mut Harness, p: [f64; 2]) {
    // Clear of the last click, so this pair is a double-click and not a triple.
    h.frames(40);
    let s = screen(h, p);
    h.step(&json!({"click": [s.x, s.y], "double": true})).unwrap();
    h.frames(3);
}

fn right_click_paper(h: &mut Harness, p: [f64; 2]) {
    let s = screen(h, p);
    h.step(&json!({"click": [s.x, s.y], "button": "right"})).unwrap();
    h.frames(2);
}

fn alias(h: &mut Harness, keys: &str) {
    for c in keys.chars() {
        h.step(&json!({"key": c.to_string()})).unwrap();
    }
    if h.app.drawing.tool.is_none() {
        h.frames(60);
    }
}

fn tool(h: &Harness) -> &DimTool {
    match &h.app.drawing.tool {
        Some(DrawingTool::Dimension(t)) => t,
        _ => panic!("the dimension tool is not running"),
    }
}

fn panel(h: &Harness) -> &DimPanel {
    match &h.app.drawing.panel {
        Some(DrawingPanel::Dimension(p)) => p,
        _ => panic!("Edit Dimension is not open"),
    }
}

fn text(h: &Harness, dim: u64) -> String {
    dimension_text(h.app.session.doc.drawing.as_deref().unwrap(), dim).unwrap()
}

fn near(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
    (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol
}

/// A dimension's text point (DXF 11) on paper.
fn text_point(h: &Harness, dim: u64) -> [f64; 2] {
    let m = h.app.session.doc.drawing.as_deref().unwrap();
    match &m.paper.entity(cadcraft_doc::Handle(dim)).unwrap().kind {
        cadcraft_doc::EntityKind::Dimension(d) => [d.text_mid.x, d.text_mid.y],
        k => panic!("{dim} is no dimension: {k:?}"),
    }
}

/// The last command's picks, each `at` rounded to 1e-3 in (a screen click is an f32 point).
fn sent_picks(h: &Harness) -> Vec<Value> {
    let sent = tool(h).last_sent.clone().expect("no dimension was sent");
    sent["picks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let mut p = p.clone();
            let at: Vec<f64> = p["at"].as_array().unwrap().iter().map(|x| (x.as_f64().unwrap() * 1000.0).round() / 1000.0).collect();
            p["at"] = json!(at);
            p
        })
        .collect()
}

/// Place one dimension with the tool: the picks, then the placement click. Its handle.
fn dimension(h: &mut Harness, picks: &[[f64; 2]], placement: [f64; 2]) -> u64 {
    let before = tool(h).placed.len();
    for p in picks {
        click_paper(h, *p);
    }
    hover(h, placement);
    click_paper(h, placement);
    let placed = &tool(h).placed;
    assert_eq!(placed.len(), before + 1, "no dimension placed at {placement:?}: {:?}", h.app.status);
    placed[before]
}

fn ids(menu: &Value, key: &str) -> Vec<String> {
    menu[key].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_string()).collect()
}

/// `drawing.dim.general` run directly (no tool): its handle.
fn run_dim(h: &mut Harness, view: u64, at: [f64; 2], placement: [f64; 2]) -> u64 {
    let r = h.app.run("drawing.dim.general", json!({"view": view, "picks": [{"at": at}], "placement": placement})).unwrap();
    h.frames(2);
    r["handle"].as_u64().unwrap()
}

#[test]
fn d_then_edges_make_the_capture_dimensions() {
    let (mut h, v1) = front_view(inventor(Drawing));
    let cam = h.app.cam;
    let undo = h.app.session.undo.len();
    alias(&mut h, "D");
    assert!(tool(&h).picks.is_empty() && tool(&h).view.is_none());
    assert_eq!(crate::status_bar::texts(&h.app).left, crate::i18n::tr("ui.drawing.prompt_dimension"));

    // The bottom edge, then the placement: the live dimension follows the cursor, no command yet.
    click_paper(&mut h, [10.0, 6.5]);
    assert_eq!(tool(&h).view, Some(v1));
    assert_eq!(tool(&h).picks.len(), 1);
    assert!(near(tool(&h).picks[0].0, [10.0, 6.5], 1e-3) && tool(&h).picks[0].1 == CurveKind::Line, "{:?}", tool(&h).picks);
    hover(&mut h, [10.0, 5.8]);
    assert!(!h.app.drawing.preview.is_empty(), "a live dimension at the cursor");
    assert_eq!(h.app.session.undo.len(), undo, "the live dimension is a preview, not a command");
    let first = h.app.drawing.preview.clone();
    hover(&mut h, [10.0, 5.2]);
    assert_ne!(h.app.drawing.preview, first, "the live dimension follows the cursor");
    click_paper(&mut h, [10.0, 5.8]);
    let linear = *tool(&h).placed.last().unwrap();
    assert_eq!(text(&h, linear), "3.00");
    assert_eq!(sent_picks(&h), [json!({"at": [10.0, 6.5]})]);
    assert!(tool(&h).last_sent.as_ref().unwrap().get("orientation").is_none());
    assert!(near(text_point(&h, linear), [10.0, 5.8], 1e-3), "{:?}", text_point(&h, linear));
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.undo.last().unwrap().label, "General Dimension");
    // The tool goes on, its picks cleared.
    assert!(tool(&h).picks.is_empty() && h.app.drawing.preview.is_empty());

    // The hole: a diameter (a circle needs no second pick: the next click places it).
    let hole = dimension(&mut h, &[[10.375, 8.0]], [11.5, 9.5]);
    assert_eq!(text(&h, hole), "⌀.75");
    // The bottom-left fillet: a radius (R44).
    let fillet = dimension(&mut h, &[[8.146, 6.646]], [7.2, 6.0]);
    assert_eq!(text(&h, fillet), "R.50");
    // Two edges: the distance between them, with no orientation sent (R81).
    let width = dimension(&mut h, &[[8.0, 8.0], [12.0, 8.0]], [10.0, 10.5]);
    assert_eq!(text(&h, width), "4.00");
    assert_eq!(sent_picks(&h), [json!({"at": [8.0, 8.0]}), json!({"at": [12.0, 8.0]})]);
    assert!(tool(&h).last_sent.as_ref().unwrap().get("orientation").is_none());
    assert_eq!(h.app.session.undo.len(), undo + 4);

    // Esc ends, with no partial state: a pick made, then Esc.
    click_paper(&mut h, [10.0, 6.5]);
    assert_eq!(tool(&h).picks.len(), 1);
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert!(h.app.drawing.tool.is_none() && h.app.drawing.preview.is_empty() && h.app.drawing.selection.is_empty());
    assert_eq!(h.app.session.undo.len(), undo + 4);
    assert_eq!(h.app.cam, cam, "the camera never moves");
}

#[test]
fn done_in_the_list_ends_the_tool_and_a_view_does_not_drag() {
    let (mut h, v1) = front_view(inventor(Drawing));
    alias(&mut h, "D");
    let at = h.app.session.doc.drawing.as_deref().unwrap().views[&v1].position;
    // A drag that starts on the view while dimensioning moves nothing.
    let (from, to) = (screen(&h, [10.5, 8.5]), screen(&h, [14.0, 12.0]));
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
    h.app.synthetic.push(egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.settle();
    assert!(h.app.drawing.drag.is_none());
    assert_eq!(h.app.session.doc.drawing.as_deref().unwrap().views[&v1].position, at);
    assert_eq!(h.app.session.undo.len(), 1);
    assert!(matches!(h.app.drawing.tool, Some(DrawingTool::Dimension(_))));
    // Done from the right-click list (R72: a list, no ring).
    click_paper(&mut h, [10.0, 6.5]);
    right_click_paper(&mut h, [25.0, 15.0]);
    let menu = crate::context_menu::describe(&h.app);
    assert!(ids(&menu, "radial").is_empty(), "{menu}");
    assert_eq!(ids(&menu, "items"), ["ui.done", "ui.cancel"]);
    click(&mut h, "menu:ui.done");
    assert!(h.app.drawing.tool.is_none() && h.app.drawing.preview.is_empty() && h.app.drawing.selection.is_empty());
    assert_eq!(h.app.session.undo.len(), 1);
}

#[test]
fn edit_sets_precision_and_text() {
    let (mut h, v1) = front_view(inventor(Drawing));
    let dim = run_dim(&mut h, v1, [10.0, 6.5], [10.0, 5.8]);
    let undo = h.app.session.undo.len();
    double_click_paper(&mut h, [10.0, 5.8]);
    let p = panel(&h);
    assert_eq!((p.dim, p.tab, p.text.as_str(), p.hide_value, p.precision, p.edit_when_created), (dim, 0, "<<>>", false, None, false));
    assert!(h.app.drawing.tool.is_none());
    click(&mut h, "drawing_dim:tab:precision");
    assert_eq!(panel(&h).tab, 1);
    click(&mut h, "drawing_dim:precision:3");
    assert_eq!(panel(&h).precision, Some(3));
    click(&mut h, "dialog:ok");
    assert!(h.app.drawing.panel.is_none());
    assert_eq!(text(&h, dim), "3.000");
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.undo.last().unwrap().label, "Edit Dimension");

    // Reopened: it shows what it has now; Hide Dimension Value, then TYP.
    double_click_paper(&mut h, [10.0, 5.8]);
    assert_eq!((panel(&h).tab, panel(&h).precision), (0, Some(3)));
    click(&mut h, "drawing_dim:hide_value");
    assert!(panel(&h).hide_value);
    assert_eq!(panel(&h).text, "", "the value's token leaves the text box");
    click(&mut h, "drawing_dim:text");
    h.step(&json!({"text": "TYP"})).unwrap();
    h.frames(2);
    assert_eq!(panel(&h).text, "TYP");
    click(&mut h, "dialog:ok");
    assert_eq!(text(&h, dim), "TYP");
    assert_eq!(h.app.session.undo.len(), undo + 2);

    // OK with nothing changed sends nothing.
    double_click_paper(&mut h, [10.0, 5.8]);
    assert_eq!((panel(&h).text.as_str(), panel(&h).hide_value), ("TYP", true));
    click(&mut h, "dialog:ok");
    assert!(h.app.drawing.panel.is_none());
    assert_eq!(h.app.session.undo.len(), undo + 2);
}

#[test]
fn edit_sends_only_the_changed_fields() {
    let (mut h, v1) = front_view(inventor(Drawing));
    let dim = run_dim(&mut h, v1, [10.0, 6.5], [10.0, 5.8]);
    open_panel(&mut h.app, dim);
    h.frames(2);
    let mut p = panel(&h).clone();
    assert_eq!(edits(&p), None);
    p.precision = Some(1);
    assert_eq!(edits(&p), Some(json!({"dim": dim, "precision": 1})));
    p.text = "<<>> TYP".into();
    assert_eq!(edits(&p), Some(json!({"dim": dim, "precision": 1, "text": "<<>> TYP"})));
    p.justify = [1, 2];
    assert_eq!(edits(&p), Some(json!({"dim": dim, "precision": 1, "text": "<<>> TYP", "justify": {"horizontal": "first", "vertical": "below"}})));
}

#[test]
fn the_dimension_menu_is_a_list() {
    let (mut h, v1) = front_view(inventor(Drawing));
    let linear = run_dim(&mut h, v1, [10.0, 6.5], [10.0, 5.8]);
    let radius = run_dim(&mut h, v1, [8.146, 6.646], [7.2, 6.0]);
    assert_eq!(text(&h, radius), "R.50");
    let undo = h.app.session.undo.len();
    right_click_paper(&mut h, [10.0, 5.8]);
    let menu = crate::context_menu::describe(&h.app);
    assert_eq!(menu["open"], json!(true), "{menu}");
    assert!(ids(&menu, "radial").is_empty(), "a list, never a ring: {menu}");
    assert_eq!(
        ids(&menu, "items"),
        ["ui.repeat", "ui.delete", "ui.drawing.moveDim", "ui.drawing.precision", "ui.drawing.editDim", "ui.drawing.hideExt", "ui.drawing.showExt"]
    );
    let precision = &menu["items"][3];
    let children: Vec<&str> = precision["children"].as_array().unwrap().iter().map(|c| c["label"].as_str().unwrap()).collect();
    assert_eq!(children, ["0", "1", "2", "3", "4", "5", "6"]);
    // The icons R91 pins.
    let icons: Vec<&str> = menu["items"].as_array().unwrap()[2..].iter().map(|i| i["icon"].as_str().unwrap()).collect();
    assert_eq!(icons, ["move", "dimension", "edit", "dimension", "dimension"]);
    crate::context_menu::pick(&mut h.app, "1").unwrap();
    h.frames(2);
    assert_eq!(text(&h, linear), "3.0");
    assert_eq!(h.app.session.undo.len(), undo + 1);

    // Hide and show the extension lines: one step each.
    right_click_paper(&mut h, [10.0, 5.8]);
    crate::context_menu::pick(&mut h.app, "ui.drawing.hideExt").unwrap();
    h.frames(2);
    let ext = |h: &Harness| {
        let m = h.app.session.doc.drawing.as_deref().unwrap();
        match &m.paper.entity(cadcraft_doc::Handle(linear)).unwrap().kind {
            cadcraft_doc::EntityKind::Dimension(d) => (d.overrides.get("suppressExt1").cloned(), d.overrides.get("suppressExt2").cloned()),
            _ => panic!(),
        }
    };
    assert_eq!(ext(&h), (Some(json!(true)), Some(json!(true))));
    right_click_paper(&mut h, [10.0, 5.8]);
    crate::context_menu::pick(&mut h.app, "ui.drawing.showExt").unwrap();
    h.frames(2);
    assert_eq!(ext(&h), (Some(json!(false)), Some(json!(false))));
    assert_eq!(h.app.session.undo.len(), undo + 3);

    // Move on the R.50: one click puts its text there, one step.
    right_click_paper(&mut h, [7.2, 6.0]);
    assert_eq!(h.app.drawing.selection, [solidcraft_engine::drawing::PaperPick::Dimension { handle: radius }]);
    crate::context_menu::pick(&mut h.app, "ui.drawing.moveDim").unwrap();
    h.frames(2);
    assert_eq!(tool(&h).moving, Some(radius));
    hover(&mut h, [7.5, 6.2]);
    assert!(!h.app.drawing.preview.is_empty());
    assert_eq!(h.app.session.undo.len(), undo + 3);
    click_paper(&mut h, [7.5, 6.2]);
    assert!(near(text_point(&h, radius), [7.5, 6.2], 1e-3), "{:?}", text_point(&h, radius));
    assert_eq!(h.app.session.undo.len(), undo + 4);
    assert!(h.app.drawing.tool.is_none() && h.app.drawing.preview.is_empty());
    assert_eq!(text(&h, radius), "R.50");

    // Edit Dimension from the list opens the panel on it.
    right_click_paper(&mut h, [10.0, 5.8]);
    crate::context_menu::pick(&mut h.app, "ui.drawing.editDim").unwrap();
    h.frames(2);
    assert_eq!(panel(&h).dim, linear);
}

#[test]
fn edit_when_created_opens_the_panel() {
    let (mut h, v1) = front_view(inventor(Drawing));
    run_dim(&mut h, v1, [10.0, 6.5], [10.0, 5.8]);
    double_click_paper(&mut h, [10.0, 5.8]);
    assert!(!panel(&h).edit_when_created);
    click(&mut h, "drawing_dim:edit_when_created");
    assert!(panel(&h).edit_when_created);
    click(&mut h, "dialog:cancel");
    assert!(h.app.drawing.panel.is_none());
    let undo = h.app.session.undo.len();
    alias(&mut h, "D");
    let hole = dimension(&mut h, &[[10.375, 8.0]], [11.5, 9.5]);
    let p = panel(&h);
    assert_eq!((p.dim, p.edit_when_created, p.text.as_str()), (hole, true, "<<>>"));
    assert!(matches!(h.app.drawing.tool, Some(DrawingTool::Dimension(_))), "the tool goes on under the panel");
    assert_eq!(h.app.session.undo.len(), undo + 1);
    // Esc closes the panel first; the tool is still there for the next dimension.
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert!(h.app.drawing.panel.is_none());
    assert!(matches!(h.app.drawing.tool, Some(DrawingTool::Dimension(_))));
}

#[test]
fn an_iso_rim_is_dimensioned_by_vertex() {
    let (mut h, v) = view_of(inventor(Drawing), "iso_top_right");
    // The hole's rim: a polyline of the view that a click on two of its vertices picks.
    let polys: Vec<(u64, Vec<[f64; 2]>)> = {
        let m = h.app.session.doc.drawing.as_deref().unwrap();
        let layout = m.paper.layout(&m.sheets[0].layout).unwrap();
        layout
            .entities
            .iter()
            .filter_map(|e| match &e.kind {
                cadcraft_doc::EntityKind::LwPolyline(p) if p.vertices.len() >= 8 => {
                    Some((e.handle.0, p.vertices.iter().map(|v| [v.p.x, v.p.y]).collect()))
                }
                _ => None,
            })
            .collect()
    };
    let tol = 0.02;
    let (a, b) = polys
        .iter()
        .find_map(|(handle, pts)| {
            let (a, b) = (pts[0], pts[pts.len() / 2]);
            let picks = |p: [f64; 2]| {
                matches!(solidcraft_engine::drawing::pick(&h.app.session, p, tol),
                    Some(solidcraft_engine::drawing::PaperPick::ViewEdge { handle: x, curve: CurveKind::Poly, .. }) if x == *handle)
            };
            (picks(a) && picks(b) && (a[0] - b[0]).hypot(a[1] - b[1]) > 0.3).then_some((a, b))
        })
        .expect("a rim polyline");
    alias(&mut h, "D");
    click_paper(&mut h, a);
    assert_eq!(tool(&h).picks[0].1, CurveKind::Poly);
    click_paper(&mut h, b);
    assert_eq!(tool(&h).picks.len(), 2);
    hover(&mut h, [16.0, 12.0]);
    click_paper(&mut h, [16.0, 12.0]);
    let dim = *tool(&h).placed.last().expect("placed");
    let picks = sent_picks(&h);
    assert_eq!(picks.len(), 2);
    assert!(picks.iter().all(|p| p["snap"] == json!("vertex")), "{picks:?}");
    assert_eq!(tool(&h).last_sent.as_ref().unwrap()["view"], json!(v));
    assert!(sick_dimensions(h.app.session.doc.drawing.as_deref().unwrap()).is_empty());
    assert!(dimension_text(h.app.session.doc.drawing.as_deref().unwrap(), dim).is_some());
}

#[test]
fn solidcraft_drawings_share_the_tool() {
    let (mut h, _) = front_view(Harness::new());
    h.app.start("drawing.dim.general");
    h.frames(2);
    let linear = dimension(&mut h, &[[10.0, 6.5]], [10.0, 5.8]);
    assert_eq!(text(&h, linear), "3.00");
    assert!(matches!(h.app.drawing.tool, Some(DrawingTool::Dimension(_))));
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert!(h.app.drawing.tool.is_none());
}

#[test]
fn the_live_dimension_is_fast_enough_for_hover() {
    let (h, v1) = front_view(inventor(Drawing));
    let params = json!({"view": v1, "picks": [{"at": [10.0, 6.5]}], "placement": [10.0, 5.8]});
    let t = std::time::Instant::now();
    for _ in 0..5 {
        let mut s = h.app.session.scratch();
        let dim = s.execute("drawing.dim.general", &params).unwrap()["handle"].as_u64().unwrap();
        assert!(!curves_of(&s, dim).is_empty());
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / 5.0;
    println!("live dimension: {ms:.2} ms per cell");
    // It runs once per 0.05 in cell (R89), not per frame.
    assert!(ms < 250.0, "the live dimension took {ms:.1} ms");
    // The real document is untouched.
    assert_eq!(h.app.session.undo.len(), 1);
}

#[test]
fn the_lists_cancel_closes_a_panel_over_the_tool() {
    // R72: Cancel and OK act on the open panel first, as Esc does; the tool under it goes on.
    let (mut h, _) = front_view(inventor(Drawing));
    h.app.drawing.dim_edit_when_created = true;
    alias(&mut h, "D");
    let hole = dimension(&mut h, &[[10.375, 8.0]], [11.5, 9.5]);
    assert_eq!(panel(&h).dim, hole);
    let undo = h.app.session.undo.len();
    right_click_paper(&mut h, [25.0, 15.0]);
    crate::context_menu::pick(&mut h.app, "ui.cancel").unwrap();
    h.frames(2);
    assert!(h.app.drawing.panel.is_none(), "Cancel closes the panel");
    assert!(matches!(h.app.drawing.tool, Some(DrawingTool::Dimension(_))), "the tool goes on");
    assert_eq!(h.app.session.undo.len(), undo);
}
