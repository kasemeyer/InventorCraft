#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Text, Leader Text, Center Mark, Centerline Bisector and Hole and Thread Note (Plan D-2 Task 9,
//! spec §2.8, §7.1 item 10): each tool places its annotation through one command and one undo
//! step; the text panel sends the text, its height and justification (R82, R83) and shows bold,
//! italic, underline and the parameter row disabled (R65); a double-click on a note opens Edit
//! Note, whose OK sends only what changed; Esc and Done end each tool.

use egui::Rect;
use serde_json::{Value, json};
use solidcraft_engine::doc::DocumentKind::Drawing;
use solidcraft_engine::drawing::{PaperPick, note_at};

use super::*;
use crate::drawing_ui::{DrawingPanel, DrawingTool};
use crate::scenario::{Harness, count_of, handle_at};
use crate::sheet_canvas::screen_of;
use crate::test_support::{block_part, click, inventor};

/// A drawing of the block: VIEW1 its front at [10, 8] (hidden lines, R91 D8) and VIEW3 projected
/// to the right at [20, 8]. The harness, the model's path, VIEW1 and VIEW3.
fn sheet(mut h: Harness) -> (Harness, String, u64, u64) {
    h.app.home.open = false;
    let block = block_part(&h.dir);
    h.app.run("drawing.new", json!({"model": block})).unwrap();
    let id = |r: Value| r["view"].as_u64().unwrap();
    let v1 = id(h
        .app
        .run("drawing.view.base", json!({"model": block, "orientation": "front", "lines": "hidden", "scale": 1, "position": [10, 8]}))
        .unwrap());
    let v3 = id(h.app.run("drawing.view.projected", json!({"parent": v1, "position": [20, 8]})).unwrap());
    h.frames(3);
    (h, block, v1, v3)
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

fn key(h: &mut Harness, k: &str) {
    h.step(&json!({"key": k})).unwrap();
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

fn type_text(h: &mut Harness, handle: &str, text: &str) {
    click(h, handle);
    h.step(&json!({"text": text})).unwrap();
    h.frames(2);
}

fn tool(h: &Harness) -> &NoteTool {
    match &h.app.drawing.tool {
        Some(DrawingTool::Note(t)) => t,
        _ => panic!("no note tool is running"),
    }
}

fn panel(h: &Harness) -> &TextPanel {
    match &h.app.drawing.panel {
        Some(DrawingPanel::Text(p)) => p,
        _ => panic!("the text panel is not open"),
    }
}

fn sent(h: &Harness) -> (&'static str, Value) {
    h.app.drawing.notes_last_sent.clone().expect("nothing was sent")
}

fn near(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
    (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol
}

fn at_of(v: &Value) -> [f64; 2] {
    [v[0].as_f64().unwrap(), v[1].as_f64().unwrap()]
}

/// The active sheet's entities as (handle, kind, layer).
fn entities(h: &Harness) -> Vec<(u64, cadcraft_doc::EntityKind, String)> {
    let m = h.app.session.doc.drawing.as_deref().unwrap();
    let layout = m.paper.layout(&m.sheets[0].layout).unwrap();
    layout.entities.iter().map(|e| (e.handle.0, e.kind.clone(), e.common.layer.clone())).collect()
}

fn count(h: &Harness, f: impl Fn(&cadcraft_doc::EntityKind, &str) -> bool) -> usize {
    entities(h).iter().filter(|(_, k, layer)| f(k, layer)).count()
}

fn center_lines(h: &Harness) -> usize {
    count(h, |k, layer| matches!(k, cadcraft_doc::EntityKind::Line(_)) && layer == "CENTER")
}

fn mtexts(h: &Harness) -> Vec<(u64, cadcraft_doc::MText)> {
    entities(h)
        .into_iter()
        .filter_map(|(handle, k, _)| match k {
            cadcraft_doc::EntityKind::MText(t) => Some((handle, t)),
            _ => None,
        })
        .collect()
}

fn leaders(h: &Harness) -> Vec<(u64, cadcraft_doc::Leader)> {
    entities(h)
        .into_iter()
        .filter_map(|(handle, k, _)| match k {
            cadcraft_doc::EntityKind::Leader(l) => Some((handle, l)),
            _ => None,
        })
        .collect()
}

/// The MText that is new since `before`.
fn new_text(h: &Harness, before: &[(u64, cadcraft_doc::MText)]) -> (u64, cadcraft_doc::MText) {
    let mut new: Vec<_> = mtexts(h).into_iter().filter(|(handle, _)| before.iter().all(|(b, _)| b != handle)).collect();
    assert_eq!(new.len(), 1, "one new text");
    new.remove(0)
}

fn undo(h: &Harness) -> usize {
    h.app.session.undo.len()
}

#[test]
fn each_note_tool_makes_one_entity() {
    let (mut h, _, v1, v3) = sheet(inventor(Drawing));
    let cam = h.app.cam;
    let start = undo(&h);

    // Text: a click, the panel, OK.
    alias(&mut h, "T");
    assert_eq!(crate::status_bar::texts(&h.app).left, crate::i18n::tr("ui.drawing.prompt_text"));
    let before = mtexts(&h);
    click_paper(&mut h, [2.0, 2.0]);
    assert_eq!(panel(&h).justify, Some(Justify::Left), "a text note starts left");
    assert_eq!(undo(&h), start, "the panel is not a command");
    type_text(&mut h, "drawing_text:text", "NOTES:");
    click(&mut h, "dialog:ok");
    assert!(h.app.drawing.panel.is_none());
    let (note, t) = new_text(&h, &before);
    let info = note_at(h.app.session.doc.drawing.as_deref().unwrap(), note).unwrap();
    assert_eq!((info.text.as_str(), info.justify, info.height), ("NOTES:", "left", 0.12));
    assert!(near([t.insert.x, t.insert.y], [2.0, 2.0], 1e-3), "{:?}", t.insert);
    assert_eq!(undo(&h), start + 1);
    assert_eq!(h.app.session.undo.last().unwrap().label, "Text");
    assert!(matches!(tool(&h), NoteTool::Text { position: None, corner: None }), "the tool goes on");

    // Leader Text: the arrow on the bottom edge, a bend, Enter, the text.
    alias(&mut h, "LE");
    assert!(matches!(tool(&h), NoteTool::Leader { .. }), "LE ends Text and starts Leader Text");
    let before = mtexts(&h);
    let leaders_before = leaders(&h).len();
    click_paper(&mut h, [10.5, 6.5]);
    click_paper(&mut h, [12.0, 5.0]);
    assert!(!h.app.drawing.preview.is_empty(), "the rubber band follows the cursor");
    assert_eq!(undo(&h), start + 1);
    key(&mut h, "Enter");
    assert_eq!(panel(&h).justify, None, "a leader's text starts with no justification (R83)");
    type_text(&mut h, "drawing_text:text", "BREAK EDGES");
    click(&mut h, "dialog:ok");
    let (cmd, params) = sent(&h);
    assert_eq!(cmd, "drawing.note.leader");
    assert!(params.get("justify").is_none(), "{params}");
    assert_eq!(params["attach"]["view"], json!(v1));
    assert!(near(at_of(&params["attach"]["edge"]["at"]), [10.5, 6.5], 1e-3), "{params}");
    assert_eq!(params["text"], json!("BREAK EDGES"));
    assert_eq!(leaders(&h).len(), leaders_before + 1);
    let (_, l) = leaders(&h).pop().unwrap();
    assert!((l.vertices[0].y - 6.5).abs() < 1e-9 && (l.vertices[0].x - 10.5).abs() < 1e-3, "the arrow on the edge: {:?}", l.vertices);
    assert_eq!(new_text(&h, &before).1.contents, "BREAK EDGES");
    assert_eq!(undo(&h), start + 2);
    assert_eq!(h.app.session.undo.last().unwrap().label, "Leader Text");

    // Center Mark on the hole: two CENTER lines; Esc ends it.
    alias(&mut h, "CM");
    let lines = center_lines(&h);
    click_paper(&mut h, [10.375, 8.0]);
    assert_eq!(center_lines(&h), lines + 2);
    assert_eq!(undo(&h), start + 3);
    assert_eq!(h.app.session.undo.last().unwrap().label, "Center Mark");
    key(&mut h, "Escape");
    assert!(h.app.drawing.tool.is_none());

    // Centerline Bisector between VIEW3's two hidden lines of the hole.
    alias(&mut h, "CB");
    click_paper(&mut h, [20.0, 7.625]);
    assert_eq!(undo(&h), start + 3, "one edge is not a centreline yet");
    click_paper(&mut h, [20.0, 8.375]);
    assert_eq!(center_lines(&h), lines + 3);
    let (cmd, params) = sent(&h);
    assert_eq!(cmd, "drawing.centerline.bisector");
    assert_eq!(params["view"], json!(v3));
    assert!(near(at_of(&params["a"]["at"]), [20.0, 7.625], 1e-3) && near(at_of(&params["b"]["at"]), [20.0, 8.375], 1e-3), "{params}");
    assert_eq!(undo(&h), start + 4);
    assert_eq!(h.app.session.undo.last().unwrap().label, "Centerline Bisector");
    key(&mut h, "Escape");
    assert!(h.app.drawing.tool.is_none() && h.app.drawing.preview.is_empty() && h.app.drawing.selection.is_empty());
    assert_eq!(h.app.cam, cam, "the camera never moves");
}

#[test]
fn the_hole_note_reads_the_feature() {
    let (mut h, _, v1, _) = sheet(inventor(Drawing));
    let start = undo(&h);
    alias(&mut h, "HN");
    assert_eq!(crate::status_bar::texts(&h.app).left, crate::i18n::tr("ui.drawing.prompt_hole"));
    // A fillet is not a hole: refused, nothing kept.
    click_paper(&mut h, [8.146, 6.646]);
    assert!(matches!(tool(&h), NoteTool::HoleNote { edge: None }));
    let before = mtexts(&h);
    click_paper(&mut h, [10.375, 8.0]);
    assert!(matches!(tool(&h), NoteTool::HoleNote { edge: Some((v, _)) } if *v == v1));
    hover(&mut h, [12.0, 10.0]);
    assert!(!h.app.drawing.preview.is_empty());
    assert_eq!(undo(&h), start);
    click_paper(&mut h, [12.0, 10.0]);
    let (note, t) = new_text(&h, &before);
    let info = note_at(h.app.session.doc.drawing.as_deref().unwrap(), note).unwrap();
    assert_eq!(info.text, "⌀.75 THRU");
    assert!(info.hole);
    assert!(near([t.insert.x, t.insert.y], [12.0, 10.0], 1e-3), "placed at the click: {:?}", t.insert);
    assert_eq!(undo(&h), start + 1);
    assert_eq!(h.app.session.undo.last().unwrap().label, "Hole and Thread Note");
    assert!(matches!(tool(&h), NoteTool::HoleNote { edge: None }), "the tool goes on");
}

#[test]
fn leader_back_and_single_segment() {
    let (mut h, _, _, _) = sheet(inventor(Drawing));
    alias(&mut h, "LE");
    for p in [[10.5, 6.5], [12.0, 5.0], [13.0, 5.0]] {
        click_paper(&mut h, p);
    }
    right_click_paper(&mut h, [25.0, 15.0]);
    let menu = crate::context_menu::describe(&h.app);
    let ids: Vec<&str> = menu["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["ui.done", "ui.drawing.back", "ui.drawing.singleSegment", "ui.cancel"]);
    assert!(menu["radial"].as_array().is_none_or(|r| r.is_empty()), "a list, never a ring: {menu}");
    let icons: Vec<&str> = menu["items"].as_array().unwrap()[1..3].iter().map(|i| i["icon"].as_str().unwrap()).collect();
    assert_eq!(icons, ["rollback", "line"]);
    assert_eq!(menu["items"][2]["shortcut"], "", "Single-Segment starts unchecked");
    crate::context_menu::pick(&mut h.app, "ui.drawing.back").unwrap();
    h.frames(2);
    assert!(matches!(tool(&h), NoteTool::Leader { points, .. } if points.len() == 2), "Back drops the last point");
    right_click_paper(&mut h, [25.0, 15.0]);
    crate::context_menu::pick(&mut h.app, "ui.drawing.back").unwrap();
    h.frames(2);
    assert!(matches!(tool(&h), NoteTool::Leader { points, attach: Some(_), .. } if points.len() == 1));
    // Single-segment on: the leader's second click ends it and opens the panel.
    right_click_paper(&mut h, [25.0, 15.0]);
    crate::context_menu::pick(&mut h.app, "ui.drawing.singleSegment").unwrap();
    h.frames(2);
    assert!(matches!(tool(&h), NoteTool::Leader { single: true, .. }));
    assert!(h.app.drawing.panel.is_none());
    // Final wave item 24: the row shows its check mark while on ("current" → `checked`).
    right_click_paper(&mut h, [25.0, 15.0]);
    let menu = crate::context_menu::describe(&h.app);
    assert_eq!(menu["items"][2]["id"], "ui.drawing.singleSegment");
    assert_eq!(menu["items"][2]["shortcut"], "current");
    h.app.viewport.context_menu = None;
    h.frames(2);
    click_paper(&mut h, [11.5, 5.5]);
    assert!(matches!(tool(&h), NoteTool::Leader { points, .. } if points.len() == 2));
    assert_eq!(panel(&h).justify, None);
    type_text(&mut h, "drawing_text:text", "A");
    let start = undo(&h);
    click(&mut h, "dialog:ok");
    assert_eq!(undo(&h), start + 1);
    assert_eq!(sent(&h).1["points"].as_array().unwrap().len(), 2);
    // The list's Done is Continue; Cancel ends the tool.
    click_paper(&mut h, [14.0, 4.0]);
    assert!(h.app.drawing.panel.is_none(), "one point is no leader yet");
    right_click_paper(&mut h, [25.0, 15.0]);
    crate::context_menu::pick(&mut h.app, "ui.cancel").unwrap();
    h.frames(2);
    assert!(h.app.drawing.tool.is_none() && h.app.drawing.preview.is_empty());
}

#[test]
fn center_mark_repeats_until_esc() {
    let (mut h, _, _, _) = sheet(inventor(Drawing));
    let start = undo(&h);
    alias(&mut h, "CM");
    assert_eq!(crate::status_bar::texts(&h.app).left, crate::i18n::tr("ui.drawing.prompt_centermark"));
    let lines = center_lines(&h);
    click_paper(&mut h, [10.375, 8.0]);
    // A straight edge is no circle: nothing.
    click_paper(&mut h, [10.0, 6.5]);
    click_paper(&mut h, [8.146, 6.646]);
    assert_eq!(center_lines(&h), lines + 4);
    assert_eq!(undo(&h), start + 2);
    assert!(matches!(tool(&h), NoteTool::CenterMark { .. }));
    let marks: Vec<Value> = {
        let (cmd, params) = sent(&h);
        assert_eq!(cmd, "drawing.centermark");
        vec![params]
    };
    assert!(near(at_of(&marks[0]["edge"]["at"]), [8.146, 6.646], 1e-3));
    // The hole's mark: two lines through its centre, 0.375 + 0.125 each side.
    let m = h.app.session.doc.drawing.as_deref().unwrap();
    let layout = m.paper.layout(&m.sheets[0].layout).unwrap();
    let hole: Vec<f64> = layout
        .entities
        .iter()
        .filter(|e| e.common.layer == "CENTER")
        .filter_map(|e| match &e.kind {
            cadcraft_doc::EntityKind::Line(l) if ((l.a.x + l.b.x) / 2.0 - 10.0).abs() < 1e-6 && ((l.a.y + l.b.y) / 2.0 - 8.0).abs() < 1e-6 => {
                Some((l.b.x - l.a.x).hypot(l.b.y - l.a.y))
            }
            _ => None,
        })
        .collect();
    assert_eq!(hole.len(), 2, "two legs through the hole's centre");
    assert!(hole.iter().all(|len| (len - 1.0).abs() < 1e-6), "{hole:?}");
    key(&mut h, "Escape");
    assert!(h.app.drawing.tool.is_none(), "R44: Esc ends it");
    assert_eq!(undo(&h), start + 2);
}

#[test]
fn text_by_two_corners_sets_the_box() {
    let (mut h, _, _, _) = sheet(inventor(Drawing));
    let start = undo(&h);
    alias(&mut h, "T");
    let (from, to) = (screen(&h, [2.0, 3.0]), screen(&h, [6.0, 2.0]));
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
    assert!(matches!(tool(&h), NoteTool::Text { position: Some(_), corner: Some(_) }));
    assert_eq!(panel(&h).justify, Some(Justify::Left));
    let before = mtexts(&h);
    type_text(&mut h, "drawing_text:text", "BOXED");
    click(&mut h, "dialog:ok");
    let (_, t) = new_text(&h, &before);
    assert!((t.width - 4.0).abs() < 1e-3, "the box is 4 wide: {}", t.width);
    assert!(near([t.insert.x, t.insert.y], [2.0, 3.0], 1e-3), "{:?}", t.insert);
    assert_eq!(undo(&h), start + 1);
}

#[test]
fn the_text_panel_sends_height_and_justify() {
    let (mut h, _, _, _) = sheet(inventor(Drawing));
    alias(&mut h, "T");
    click_paper(&mut h, [2.0, 2.0]);
    assert_eq!((panel(&h).height_text.as_str(), panel(&h).height), ("0.12 in", 0.12));
    // R65: bold, italic, underline and the parameter row are shown, disabled.
    for name in ["bold", "italic", "underline", "parameter"] {
        assert_eq!(count_of(&format!("drawing_text:{name}:enabled")), Some(0.0), "{name}");
    }
    assert!(handle_at("drawing_text:underline").is_some() && handle_at("drawing_text:parameter").is_some(), "shown");
    // A height that does not read keeps OK from sending.
    let edit = |h: &mut Harness, text: &str| {
        click(h, "drawing_text:height");
        h.step(&json!({"key": "End"})).unwrap();
        for _ in 0..12 {
            h.step(&json!({"key": "Backspace"})).unwrap();
        }
        h.step(&json!({"text": text})).unwrap();
        h.frames(2);
    };
    edit(&mut h, "3 mm");
    assert!((panel(&h).height - 3.0 / 25.4).abs() < 1e-9, "{}", panel(&h).height);
    edit(&mut h, "0.25 in");
    assert_eq!(panel(&h).height, 0.25);
    click(&mut h, "drawing_text:justify:center");
    assert_eq!(panel(&h).justify, Some(Justify::Center));
    type_text(&mut h, "drawing_text:text", "A");
    click(&mut h, "dialog:ok");
    let (cmd, params) = sent(&h);
    assert_eq!(cmd, "drawing.note.text");
    assert_eq!(params["height"], json!(0.25));
    assert_eq!(params["justify"], json!("center"));
    for never in ["bold", "italic", "underline", "precision"] {
        assert!(params.get(never).is_none(), "{params}");
    }
    let m = h.app.session.doc.drawing.as_deref().unwrap();
    let note = mtexts(&h).into_iter().find(|(_, t)| t.contents == "A").unwrap().0;
    let info = note_at(m, note).unwrap();
    assert_eq!((info.height, info.justify), (0.25, "center"));
    assert_eq!(parse_height("0"), None);
    assert_eq!(parse_height("11 in"), None);
    assert_eq!(parse_height("0.5"), Some(0.5));
}

#[test]
fn double_click_opens_edit_note() {
    let (mut h, _, _, _) = sheet(inventor(Drawing));
    let note = h.app.run("drawing.note.text", json!({"position": [2, 2], "text": "NOTES:"})).unwrap()["handle"].as_u64().unwrap();
    h.frames(2);
    let start = undo(&h);
    double_click_paper(&mut h, [2.2, 1.95]);
    let p = panel(&h);
    assert_eq!((p.edit, p.text.as_str(), p.height, p.justify, p.hole), (Some(note), "NOTES:", 0.12, Some(Justify::Left), false));
    assert!(h.app.drawing.tool.is_none());
    // OK with nothing changed: nothing sent.
    click(&mut h, "dialog:ok");
    assert!(h.app.drawing.panel.is_none());
    assert_eq!(undo(&h), start);
    double_click_paper(&mut h, [2.2, 1.95]);
    let mut p = panel(&h).clone();
    assert_eq!(edits(&p), None);
    p.justify = Some(Justify::Right);
    assert_eq!(edits(&p), Some(json!({"handle": note, "justify": "right"})), "only what changed");
    type_text(&mut h, "drawing_text:text", " 1");
    assert_eq!(panel(&h).text, "NOTES: 1");
    click(&mut h, "dialog:ok");
    assert_eq!(undo(&h), start + 1);
    assert_eq!(h.app.session.undo.last().unwrap().label, "Edit Note");
    let info = note_at(h.app.session.doc.drawing.as_deref().unwrap(), note).unwrap();
    assert_eq!((info.text.as_str(), info.justify, info.height), ("NOTES: 1", "left", 0.12));
    // Esc closes Edit Note and sends nothing.
    double_click_paper(&mut h, [2.2, 1.95]);
    assert!(h.app.drawing.panel.is_some());
    key(&mut h, "Escape");
    assert!(h.app.drawing.panel.is_none());
    assert_eq!(undo(&h), start + 1);
}

#[test]
fn the_leader_follows_its_edge_when_the_model_changes() {
    let (mut h, block, v1, _) = sheet(inventor(Drawing));
    let edge = match solidcraft_engine::drawing::pick(&h.app.session, [10.5, 6.5], 0.01) {
        Some(PaperPick::ViewEdge { edge, view, .. }) if view == v1 => edge,
        other => panic!("the bottom edge: {other:?}"),
    };
    alias(&mut h, "LE");
    click_paper(&mut h, [10.5, 6.5]);
    click_paper(&mut h, [12.0, 5.0]);
    key(&mut h, "Enter");
    type_text(&mut h, "drawing_text:text", "BREAK EDGES");
    click(&mut h, "dialog:ok");
    let (leader, l) = leaders(&h).pop().unwrap();
    let arrow = [l.vertices[0].x, l.vertices[0].y];
    // The block 1 in taller: its bottom edge moves on the sheet.
    let mut s = solidcraft_engine::Session::default();
    s.execute("doc.open", &json!({"path": block})).unwrap();
    s.execute("timeline.edit", &json!({"feature": "Block", "set": {"height": "4 in"}})).unwrap();
    s.execute("file.save", &json!({"path": block})).unwrap();
    h.app.run("drawing.update", json!({})).unwrap();
    h.frames(2);
    let l = leaders(&h).into_iter().find(|(handle, _)| *handle == leader).unwrap().1;
    let moved = [l.vertices[0].x, l.vertices[0].y];
    assert!(!near(moved, arrow, 1e-3), "the arrow moved with its edge: {moved:?}");
    match solidcraft_engine::drawing::pick(&h.app.session, moved, 1e-6) {
        Some(PaperPick::ViewEdge { edge: now, .. }) => assert_eq!(now, edge, "still on the bottom edge"),
        other => panic!("the arrow is off its edge: {other:?}"),
    }
}

#[test]
fn the_lists_cancel_closes_the_text_panel_over_the_tool() {
    // R72 (Task 8 review carry): Cancel acts on the open panel first; the tool goes on.
    let (mut h, _, _, _) = sheet(inventor(Drawing));
    let start = undo(&h);
    alias(&mut h, "T");
    click_paper(&mut h, [2.0, 2.0]);
    assert!(h.app.drawing.panel.is_some());
    right_click_paper(&mut h, [25.0, 15.0]);
    crate::context_menu::pick(&mut h.app, "ui.cancel").unwrap();
    h.frames(2);
    assert!(h.app.drawing.panel.is_none(), "Cancel closes the panel");
    assert!(matches!(tool(&h), NoteTool::Text { position: None, .. }), "the tool goes on, its note dropped");
    assert_eq!(undo(&h), start);
    // Done from the list ends it.
    right_click_paper(&mut h, [25.0, 15.0]);
    crate::context_menu::pick(&mut h.app, "ui.done").unwrap();
    h.frames(2);
    assert!(h.app.drawing.tool.is_none());
}

#[test]
fn solidcraft_drawings_share_the_tools() {
    let (mut h, _, _, _) = sheet(Harness::new());
    h.app.start("drawing.centermark");
    h.frames(2);
    let lines = center_lines(&h);
    click_paper(&mut h, [10.375, 8.0]);
    assert_eq!(center_lines(&h), lines + 2);
    h.app.start("drawing.note.text");
    h.frames(2);
    click_paper(&mut h, [2.0, 2.0]);
    type_text(&mut h, "drawing_text:text", "NOTES:");
    click(&mut h, "dialog:ok");
    assert!(mtexts(&h).iter().any(|(_, t)| t.contents == "NOTES:"));
    key(&mut h, "Escape");
    assert!(h.app.drawing.tool.is_none());
}

#[test]
fn edit_note_on_a_metric_height_sends_nothing_unchanged() {
    let (mut h, _, _, _) = sheet(inventor(Drawing));
    let height = 3.0 / 25.4;
    let note = h.app.run("drawing.note.text", json!({"position": [2, 2], "text": "NOTES:", "height": height})).unwrap()["handle"].as_u64().unwrap();
    h.frames(2);
    let start = undo(&h);
    h.app.drawing.notes_last_sent = None;
    open_edit(&mut h.app, note);
    h.frames(2);
    assert_eq!(panel(&h).height_text, "0.1181 in");
    assert_eq!(edits(panel(&h)), None, "the rounded box is not a change");
    click(&mut h, "dialog:ok");
    assert!(h.app.drawing.panel.is_none());
    assert_eq!(undo(&h), start, "no command, no undo step");
    let info = note_at(h.app.session.doc.drawing.as_deref().unwrap(), note).unwrap();
    assert!((info.height - height).abs() < 1e-12);
}

#[test]
fn the_leader_list_respects_the_open_panel() {
    let (mut h, _, _, _) = sheet(inventor(Drawing));
    alias(&mut h, "LE");
    click_paper(&mut h, [3.0, 3.0]);
    // One point: Continue has nothing to continue.
    let row = |h: &Harness, id: &str| crate::marking_menu::item(&h.app, id, crate::profile_workflow::MenuContext::Tool);
    assert!(!row(&h, "ui.done").enabled);
    assert!(row(&h, "ui.drawing.back").enabled && row(&h, "ui.drawing.singleSegment").enabled);
    click_paper(&mut h, [5.0, 4.0]);
    assert!(row(&h, "ui.done").enabled);
    continue_leader(&mut h.app);
    h.frames(2);
    type_text(&mut h, "drawing_text:text", "KEEP");
    for id in ["ui.done", "ui.drawing.back", "ui.drawing.singleSegment"] {
        assert!(!row(&h, id).enabled, "{id} with the panel open");
    }
    // Even when sent, the rows leave the panel and its points alone.
    run_menu(&mut h.app, "ui.drawing.singleSegment");
    run_menu(&mut h.app, "ui.drawing.back");
    assert_eq!(panel(&h).text, "KEEP");
    assert!(matches!(tool(&h), NoteTool::Leader { points, single: false, .. } if points.len() == 2));
    // Ending the tool takes its prompt off the status line.
    key(&mut h, "Escape");
    key(&mut h, "Escape");
    assert!(h.app.drawing.tool.is_none());
    assert_eq!(crate::status_bar::texts(&h.app).left, crate::i18n::tr("ui.status_ready"));
}

#[test]
fn the_leader_attaches_within_the_engines_at_tolerance() {
    // Final wave item 25: zoomed far out (a screen pick reaches past 0.1 in), a click 0.15 in
    // off the block's bottom edge does not attach; one 0.05 in off does, and the leader is placed
    // attached (the engine resolves the same `{at}`).
    let (mut h, ..) = sheet(inventor(Drawing));
    h.app.cam.distance *= 8.0;
    h.frames(2);
    alias(&mut h, "LE");
    click_paper(&mut h, [10.0, 6.35]);
    assert!(matches!(tool(&h), NoteTool::Leader { attach: None, .. }), "0.15 in off: free");
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    alias(&mut h, "LE");
    click_paper(&mut h, [10.0, 6.45]);
    assert!(matches!(tool(&h), NoteTool::Leader { attach: Some(_), .. }), "0.05 in off: attached");
    assert!(solidcraft_engine::drawing::AT_TOLERANCE_IN > 0.05 && solidcraft_engine::drawing::AT_TOLERANCE_IN < 0.15);
}
