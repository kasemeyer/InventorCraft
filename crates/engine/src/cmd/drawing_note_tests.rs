#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Notes, leaders, hole notes, centre marks and centrelines (Task 11, spec §2.8): text notes on
//! NOTE, leaders with a closed filled arrow, hole notes read from the model's hole feature,
//! centre marks and centreline bisectors on CENTER that regenerate with their edges, and moving
//! and deleting annotations. VIEW1 is the block's front view at [10, 8] (hole r .375 at [10, 8],
//! box [8, 6.5]-[12, 9.5]); VIEW3 the right view at [20, 8] (the hole's hidden lines at y 7.625
//! and 8.375, x 19.5 to 20.5).

use cadcraft_doc::{EntityKind, Handle, Leader, MText};
use cadcraft_geom::Vec2;
use cadcraft_render::Arrowhead;
use serde_json::{Value, json};
use solidcraft_doc::DrawingModel;

use crate::cmd::drawing::drawing_fixture::{block_drawing, edge_at, edit_model, front_view};
use crate::drawing::mapper::{Seg, xkey_of};
use crate::drawing::{
    LEADER_ARROW, MARK_OVERSHOOT_IN, NOTE_HEIGHT_IN, NoteInfo, PaperPick, dimension_text, note_at, pick, sick_dimensions, suppressed_annotations,
};
use crate::{EngineError, Session};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn model(s: &Session) -> &DrawingModel {
    s.doc.drawing.as_deref().unwrap()
}

fn bad_message(r: crate::Result<Value>) -> String {
    match r {
        Err(EngineError::BadParams { msg, .. }) => msg,
        other => panic!("expected bad params, got {other:?}"),
    }
}

fn handle(r: &Value) -> u64 {
    r["handle"].as_u64().unwrap_or_else(|| panic!("no handle in {r}"))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6
}

fn near(p: Vec2, q: [f64; 2]) -> bool {
    close(p.x, q[0]) && close(p.y, q[1])
}

/// The block drawing with VIEW1; (session, block path, view id).
fn front(name: &str) -> (Session, String, u64) {
    let (mut s, block) = block_drawing(name);
    let v = front_view(&mut s, &block);
    (s, block, v)
}

/// Entity `h`: (layer, kind); None when it is gone.
fn entity(s: &Session, h: u64) -> Option<(String, EntityKind)> {
    model(s).paper.entity(Handle(h)).map(|e| (e.common.layer.clone(), e.kind.clone()))
}

fn mtext(s: &Session, h: u64) -> MText {
    match entity(s, h) {
        Some((layer, EntityKind::MText(t))) => {
            assert_eq!(layer, "NOTE");
            t
        }
        other => panic!("entity {h} is not an MText: {other:?}"),
    }
}

/// The leader `h` and its text (the MText the note's xdata names).
fn leader(s: &Session, h: u64) -> (Leader, MText) {
    let Some((layer, EntityKind::Leader(l))) = entity(s, h) else { panic!("entity {h} is not a leader") };
    assert_eq!(layer, "NOTE");
    let text = crate::drawing::notes::text_of(model(s), Handle(h)).unwrap_or_else(|| panic!("leader {h} has no text"));
    (l, mtext(s, text.0))
}

/// The MText lines of a stored text.
fn lines(t: &MText) -> Vec<&str> {
    t.contents.split("\\P").collect()
}

/// Every Line of the sheet on CENTER: (handle, a, b, its xdata key).
fn center_lines(s: &Session) -> Vec<(u64, Vec2, Vec2, crate::drawing::mapper::XKey)> {
    let m = model(s);
    let layout = m.paper.layout(&m.sheets[0].layout).unwrap();
    layout
        .entities
        .iter()
        .filter(|e| e.common.layer == "CENTER")
        .map(|e| match &e.kind {
            EntityKind::Line(l) => (e.handle.0, l.a.xy(), l.b.xy(), xkey_of(e).unwrap_or_else(|| panic!("centre line {} has no key", e.handle.0))),
            other => panic!("not a line on CENTER: {other:?}"),
        })
        .collect()
}

fn has_cm_tag(s: &Session, h: u64) -> bool {
    model(s).paper.entity(Handle(h)).unwrap().common.xdata.iter().any(|t| t.code == 1000 && t.value == "cm")
}

fn group_has(s: &Session, view: u64, h: u64) -> bool {
    let m = model(s);
    let name = crate::drawing::mapper::group_name(m.views[&view].group);
    m.paper.groups.iter().find(|g| g.name == name).is_some_and(|g| g.members.contains(&Handle(h)))
}

#[test]
fn a_text_note_at_a_point() {
    let (mut s, _, _) = front("a_text_note_at_a_point");
    let undo = s.undo.len();
    let h = handle(&run(&mut s, "drawing.note.text", json!({"position": [2, 2], "text": "NOTES:"})));
    assert_eq!(s.undo.len(), undo + 1);
    let t = mtext(&s, h);
    assert_eq!((t.contents.as_str(), t.height, t.width), ("NOTES:", 0.12, 0.0));
    assert_eq!(NOTE_HEIGHT_IN, 0.12);
    assert!(near(t.insert.xy(), [2.0, 2.0]), "{t:?}");
    // Two corners: a box 4 in wide, from its upper-left corner.
    let boxed = handle(&run(&mut s, "drawing.note.text", json!({"position": [2, 2], "corner": [6, 1], "text": "NOTES:"})));
    let t = mtext(&s, boxed);
    assert!(close(t.width, 4.0) && near(t.insert.xy(), [2.0, 2.0]), "{t:?}");
    let tall = handle(&run(&mut s, "drawing.note.text", json!({"position": [2, 4], "text": "A\nB", "height": 0.25})));
    let t = mtext(&s, tall);
    assert_eq!((t.contents.as_str(), t.height), ("A\\PB", 0.25));
    assert_eq!(bad_message(s.execute("drawing.note.text", &json!({"position": [2, 2], "text": ""}))), "`text` must be some text");
}

/// R65: `justify` lines a text note up left (the default), centred or right, `position` then
/// being its upper left, upper centre or upper right (MText attachment 1, 2, 3); with a box, the
/// insert is the box's top left, top centre or top right.
#[test]
fn a_text_note_is_justified() {
    let (mut s, _, _) = front("a_text_note_is_justified");
    for (justify, attach) in [(Value::Null, 1), (json!("left"), 1), (json!("center"), 2), (json!("right"), 3)] {
        let h = handle(&run(&mut s, "drawing.note.text", json!({"position": [2, 2], "text": "A\nBB", "justify": justify})));
        let t = mtext(&s, h);
        assert_eq!((t.attach, t.height, t.width), (attach, 0.12, 0.0), "{justify}");
        assert!(near(t.insert.xy(), [2.0, 2.0]), "{justify}: {t:?}");
    }
    for (justify, attach, x) in [("left", 1, 2.0), ("center", 2, 4.0), ("right", 3, 6.0)] {
        let h =
            handle(&run(&mut s, "drawing.note.text", json!({"position": [6, 1], "corner": [2, 2], "text": "A", "justify": justify, "height": 0.2})));
        let t = mtext(&s, h);
        assert_eq!((t.attach, t.height), (attach, 0.2), "{justify}");
        assert!(close(t.width, 4.0) && near(t.insert.xy(), [x, 2.0]), "{justify}: {t:?}");
    }
    let undo = s.undo.len();
    for bad in [json!("middle"), json!(1), json!(true)] {
        let r = s.execute("drawing.note.text", &json!({"position": [2, 2], "text": "A", "justify": bad}));
        assert_eq!(bad_message(r), "`justify` must be left, center or right");
    }
    assert_eq!(s.undo.len(), undo);
}

/// R65: a leader's text takes `height` and `justify`: it starts at (middle left, attachment 4),
/// centres on (5) or ends at (6) the last point; by default by the leader's direction.
#[test]
fn a_leader_note_is_justified() {
    let (mut s, _, _) = front("a_leader_note_is_justified");
    for (points, justify, attach) in [
        (json!([[5, 5], [6, 6]]), Value::Null, 4),
        (json!([[7, 5], [6, 6]]), Value::Null, 6),
        (json!([[7, 5], [6, 6]]), json!("left"), 4),
        (json!([[5, 5], [6, 6]]), json!("center"), 5),
        (json!([[5, 5], [6, 6]]), json!("right"), 6),
    ] {
        let r = run(&mut s, "drawing.note.leader", json!({"points": points, "text": "X", "justify": justify}));
        let (_, t) = leader(&s, handle(&r));
        assert_eq!((t.attach, t.height), (attach, 0.12), "{points} {justify}");
        assert!(near(t.insert.xy(), [6.0, 6.0]), "{t:?}");
    }
    let r = run(&mut s, "drawing.note.leader", json!({"points": [[5, 5], [6, 6]], "text": "X", "height": 0.25}));
    assert_eq!(leader(&s, handle(&r)).1.height, 0.25);
    let r = s.execute("drawing.note.leader", &json!({"points": [[5, 5], [6, 6]], "text": "X", "height": 0}));
    assert_eq!(bad_message(r), "`height` must be a number of inches above 0, at most 10");
    let r = s.execute("drawing.note.leader", &json!({"points": [[5, 5], [6, 6]], "text": "X", "justify": "up"}));
    assert_eq!(bad_message(r), "`justify` must be left, center or right");
}

#[test]
fn a_leader_note_has_a_filled_arrow() {
    let (mut s, _, _) = front("a_leader_note_has_a_filled_arrow");
    let undo = s.undo.len();
    let r = run(&mut s, "drawing.note.leader", json!({"points": [[5, 5], [6, 6], [7, 6]], "text": "BREAK EDGES"}));
    assert_eq!(s.undo.len(), undo + 1);
    let (l, t) = leader(&s, handle(&r));
    let points: Vec<Vec2> = l.vertices.iter().map(|v| v.xy()).collect();
    assert_eq!(points.len(), 3);
    assert!(near(points[0], [5.0, 5.0]) && near(points[1], [6.0, 6.0]) && near(points[2], [7.0, 6.0]), "{points:?}");
    // A closed filled arrowhead (owner default; §8.4), DIMASZ 0.125 of ANSI_INCH, at the first point.
    assert!(l.arrow);
    assert_eq!(LEADER_ARROW, "closed_filled");
    assert_eq!(Arrowhead::parse(LEADER_ARROW), Arrowhead::ClosedFilled);
    let st = model(&s).paper.dim_style(&l.style).unwrap();
    assert_eq!((l.style.as_str(), Arrowhead::parse(&st.leader_arrow_block), st.arrow_size), ("ANSI_INCH", Arrowhead::ClosedFilled, 0.125));
    let dir = (points[0] - points[1]).normalized();
    let g = cadcraft_render::arrowhead(Arrowhead::ClosedFilled, points[0], dir, st.arrow_size);
    assert!(!g.tris.is_empty());
    let corners: Vec<Vec2> = g.tris.clone();
    assert!(corners.iter().any(|p| near(*p, [5.0, 5.0])), "{corners:?}");
    let back = corners.iter().map(|p| (*p - points[0]).dot(-dir)).fold(0.0, f64::max);
    assert!(close(back, 0.125), "{back}");
    assert_eq!((t.contents.as_str(), t.height), ("BREAK EDGES", 0.12));
    assert!(near(t.insert.xy(), [7.0, 6.0]), "{t:?}");
    let undo = s.undo.len();
    assert_eq!(bad_message(s.execute("drawing.note.leader", &json!({"points": [[5, 5]], "text": "X"}))), "a leader needs at least 2 points");
    assert_eq!(s.undo.len(), undo);
}

#[test]
fn an_attached_leader_follows_its_edge() {
    let (mut s, _, v) = front("an_attached_leader_follows_its_edge");
    let bottom = edge_at(&s, v, [10.0, 6.5]);
    let r = run(
        &mut s,
        "drawing.note.leader",
        json!({"points": [[10, 6.5], [10.5, 5.5], [11, 5.5]], "text": "EDGE", "attach": {"view": v, "edge": bottom}}),
    );
    let h = handle(&r);
    assert!(near(leader(&s, h).0.vertices[0].xy(), [10.0, 6.5]));
    run(&mut s, "drawing.view.set_position", json!({"view": v, "position": [11, 8]}));
    let (l, t) = leader(&s, h);
    assert!(near(l.vertices[0].xy(), [11.0, 6.5]), "{:?}", l.vertices);
    // Only the arrow end is attached: the rest of the note stays where it was put.
    assert!(near(l.vertices[1].xy(), [10.5, 5.5]) && near(t.insert.xy(), [11.0, 5.5]));
    run(&mut s, "edit.undo", json!({}));
    assert!(near(leader(&s, h).0.vertices[0].xy(), [10.0, 6.5]));
}

#[test]
fn the_hole_note_reads_the_feature() {
    let (mut s, block, v) = front("the_hole_note_reads_the_feature");
    let hole = edge_at(&s, v, [10.375, 8.0]);
    let undo = s.undo.len();
    let r = run(&mut s, "drawing.note.hole", json!({"view": v, "edge": hole, "placement": [12, 10]}));
    assert_eq!(s.undo.len(), undo + 1);
    assert_eq!(r["text"].as_str(), Some("⌀.75 THRU"));
    let h = handle(&r);
    let (l, t) = leader(&s, h);
    assert_eq!(t.contents, "⌀.75 THRU");
    // The arrow is on the hole's rim, towards the text.
    let k = 0.375 / 2f64.sqrt();
    assert_eq!(l.vertices.len(), 2);
    assert!(near(l.vertices[0].xy(), [10.0 + k, 8.0 + k]) && near(l.vertices[1].xy(), [12.0, 10.0]), "{:?}", l.vertices);
    assert!(near(t.insert.xy(), [12.0, 10.0]));
    let k = 0.5 * std::f64::consts::FRAC_1_SQRT_2;
    let fillet = edge_at(&s, v, [11.5 + k, 9.0 + k]);
    assert!(fillet.contains("blend"), "{fillet}");
    let undo = s.undo.len();
    assert_eq!(bad_message(s.execute("drawing.note.hole", &json!({"view": v, "edge": fillet, "placement": [13, 10]}))), "pick the edge of a hole");
    assert_eq!(s.undo.len(), undo);
    edit_model(&block, &[("timeline.edit", json!({"feature": "Drill", "set": {"depth": "0.5 in"}}))]);
    run(&mut s, "drawing.update", json!({}));
    let (_, t) = leader(&s, h);
    assert_eq!(t.contents, "⌀.75 ↧.50");
    // A tapped hole: its thread on the second line (a 1/4-20 UNC needs a hole near its tap drill).
    edit_model(&block, &[("timeline.edit", json!({"feature": "Drill", "set": {"diameter": "0.201 in", "thread": "1/4-20 UNC"}}))]);
    run(&mut s, "drawing.update", json!({}));
    let (l, t) = leader(&s, h);
    assert_eq!(lines(&t), ["⌀.20 ↧.50", "1/4-20 UNC"]);
    // The arrow follows the smaller rim.
    let k = 0.1005 / 2f64.sqrt();
    assert!(near(l.vertices[0].xy(), [10.0 + k, 8.0 + k]), "{:?}", l.vertices);
}

#[test]
fn a_center_mark_spans_the_circle() {
    let (mut s, _, v) = front("a_center_mark_spans_the_circle");
    let hole = edge_at(&s, v, [10.375, 8.0]);
    let undo = s.undo.len();
    let r = run(&mut s, "drawing.centermark", json!({"view": v, "edge": hole}));
    assert_eq!(s.undo.len(), undo + 1);
    let marks = center_lines(&s);
    assert_eq!(marks.len(), 2);
    assert_eq!(r["handles"], json!(marks.iter().map(|m| m.0).collect::<Vec<_>>()));
    assert_eq!(MARK_OVERSHOOT_IN, 0.125);
    for (h, a, b, key) in &marks {
        assert!(close(a.dist(*b), 1.0), "{a:?} {b:?}");
        assert!(near(a.mid(*b), [10.0, 8.0]), "{a:?} {b:?}");
        assert_eq!((key.view, key.edge.as_str(), &key.seg), (v, hole.as_str(), &Seg::Mark));
        assert!(has_cm_tag(&s, *h));
        assert!(group_has(&s, v, *h));
    }
    // One across, one up.
    assert!(close(marks[0].1.y, marks[0].2.y) && close(marks[1].1.x, marks[1].2.x), "{marks:?}");
    let bottom = edge_at(&s, v, [10.0, 6.5]);
    assert_eq!(bad_message(s.execute("drawing.centermark", &json!({"view": v, "edge": bottom}))), "a center mark needs a circle or an arc");
}

/// Final wave 18: rescaling a view fits its centre mark again: the legs cross the scaled circle
/// and run the same 0.125 in past it (2 × (.1875 + .125) = .625 at half scale, not .5);
/// centrelines are fitted the same way. Handles are kept.
#[test]
fn rescaling_keeps_the_mark_overshoot() {
    let (mut s, _, v) = front("rescaling_keeps_the_mark_overshoot");
    let hole = edge_at(&s, v, [10.375, 8.0]);
    run(&mut s, "drawing.centermark", json!({"view": v, "edge": hole}));
    let before: Vec<u64> = center_lines(&s).iter().map(|m| m.0).collect();
    run(&mut s, "drawing.view.set_scale", json!({"view": v, "scale": 0.5}));
    let marks = center_lines(&s);
    assert_eq!(marks.iter().map(|m| m.0).collect::<Vec<_>>(), before);
    for (_, a, b, _) in &marks {
        assert!(close(a.dist(*b), 0.625) && near(a.mid(*b), [10.0, 8.0]), "{a:?} {b:?}");
    }
    assert!(close(marks[0].1.y, marks[0].2.y) && close(marks[1].1.x, marks[1].2.x), "{marks:?}");
    run(&mut s, "drawing.view.set_scale", json!({"view": v, "scale": 2}));
    for (_, a, b, _) in &center_lines(&s) {
        assert!(close(a.dist(*b), 2.0 * (0.75 + 0.125)) && near(a.mid(*b), [10.0, 8.0]), "{a:?} {b:?}");
    }
}

#[test]
fn center_marks_follow_their_circle() {
    let (mut s, block, v) = front("center_marks_follow_their_circle");
    let hole = edge_at(&s, v, [10.375, 8.0]);
    run(&mut s, "drawing.centermark", json!({"view": v, "edge": hole}));
    let before: Vec<u64> = center_lines(&s).iter().map(|m| m.0).collect();
    edit_model(&block, &[("timeline.edit", json!({"feature": "Drill", "set": {"diameter": "1 in"}}))]);
    run(&mut s, "drawing.update", json!({}));
    let marks = center_lines(&s);
    assert_eq!(marks.iter().map(|m| m.0).collect::<Vec<_>>(), before);
    for (h, a, b, _) in &marks {
        assert!(close(a.dist(*b), 1.25) && near(a.mid(*b), [10.0, 8.0]), "{a:?} {b:?}");
        assert!(group_has(&s, v, *h));
    }
    // The hole gone: its centre mark goes too (the orphan is removed).
    edit_model(&block, &[("timeline.delete", json!({"feature": "Drill"}))]);
    run(&mut s, "drawing.update", json!({}));
    assert!(center_lines(&s).is_empty());
    assert!(before.iter().all(|h| !group_has(&s, v, *h)));
}

#[test]
fn a_centerline_bisects_two_edges() {
    let (mut s, block, v1) = front("a_centerline_bisects_two_edges");
    let v3 = run(&mut s, "drawing.view.projected", json!({"parent": v1, "position": [20, 8]}))["view"].as_u64().unwrap();
    let a = edge_at(&s, v3, [20.0, 7.625]);
    let b = edge_at(&s, v3, [20.0, 8.375]);
    assert_ne!(a, b);
    let undo = s.undo.len();
    let r = run(&mut s, "drawing.centerline.bisector", json!({"view": v3, "a": a, "b": b}));
    assert_eq!(s.undo.len(), undo + 1);
    let found = center_lines(&s);
    assert_eq!(found.len(), 1);
    let (h, p, q, key) = found[0].clone();
    assert_eq!(r["handle"].as_u64(), Some(h));
    assert!(close(p.dist(q), 1.25) && close(p.y, 8.0) && close(q.y, 8.0) && near(p.mid(q), [20.0, 8.0]), "{p:?} {q:?}");
    let mut sorted = [a.clone(), b.clone()];
    sorted.sort();
    assert_eq!((key.view, key.edge, key.seg), (v3, format!("{}|{}", sorted[0], sorted[1]), Seg::Mark));
    assert!(has_cm_tag(&s, h));
    let hole = edge_at(&s, v1, [10.375, 8.0]);
    let bottom = edge_at(&s, v1, [10.0, 6.5]);
    assert_eq!(
        bad_message(s.execute("drawing.centerline.bisector", &json!({"view": v1, "a": hole, "b": bottom}))),
        "a centerline bisector needs two straight edges"
    );
    // It follows the hole, and goes with it.
    edit_model(&block, &[("timeline.edit", json!({"feature": "Drill", "set": {"diameter": "1 in"}}))]);
    run(&mut s, "drawing.update", json!({}));
    let found = center_lines(&s);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, h);
    assert!(close(found[0].1.dist(found[0].2), 1.25) && near(found[0].1.mid(found[0].2), [20.0, 8.0]));
    edit_model(&block, &[("timeline.delete", json!({"feature": "Drill"}))]);
    run(&mut s, "drawing.update", json!({}));
    assert!(center_lines(&s).is_empty());
}

#[test]
fn annotations_move_and_delete() {
    let (mut s, _, v) = front("annotations_move_and_delete");
    let note = handle(&run(&mut s, "drawing.note.text", json!({"position": [2, 2], "text": "NOTES:"})));
    let undo = s.undo.len();
    run(&mut s, "drawing.annotation.move", json!({"handle": note, "placement": [3, 3]}));
    assert_eq!(s.undo.len(), undo + 1);
    assert!(near(mtext(&s, note).insert.xy(), [3.0, 3.0]));
    let r = run(&mut s, "drawing.annotation.delete", json!({"handles": [note]}));
    assert_eq!(r["deleted"], json!([note]));
    assert!(entity(&s, note).is_none());
    // A leader and its text go together, whichever is named.
    let l = handle(&run(&mut s, "drawing.note.leader", json!({"points": [[5, 5], [6, 6]], "text": "X"})));
    let text = crate::drawing::notes::text_of(model(&s), Handle(l)).unwrap().0;
    run(&mut s, "drawing.annotation.move", json!({"handle": text, "placement": [7, 7]}));
    let (ld, t) = leader(&s, l);
    assert!(near(t.insert.xy(), [7.0, 7.0]) && near(ld.vertices[0].xy(), [6.0, 6.0]) && near(ld.vertices[1].xy(), [7.0, 7.0]), "{ld:?}");
    run(&mut s, "drawing.annotation.delete", json!({"handles": [text]}));
    assert!(entity(&s, l).is_none() && entity(&s, text).is_none());
    // A centre mark's two lines go together.
    let hole = edge_at(&s, v, [10.375, 8.0]);
    let marks = run(&mut s, "drawing.centermark", json!({"view": v, "edge": hole}));
    let first = marks["handles"][0].as_u64().unwrap();
    run(&mut s, "drawing.annotation.delete", json!({"handles": [first]}));
    assert!(center_lines(&s).is_empty());
    // View entities move with their view.
    let bottom = edge_at(&s, v, [10.0, 6.5]);
    let line = {
        let m = model(&s);
        m.paper.layout(&m.sheets[0].layout).unwrap().entities.iter().find(|e| xkey_of(e).is_some_and(|k| k.edge == bottom)).unwrap().handle.0
    };
    let undo = s.undo.len();
    let message = "VIEW1 entities move with their view (drawing.view.set_position)";
    assert_eq!(bad_message(s.execute("drawing.annotation.move", &json!({"handle": line, "placement": [1, 1]}))), message);
    assert_eq!(bad_message(s.execute("drawing.annotation.delete", &json!({"handles": [line]}))), message);
    assert_eq!(s.undo.len(), undo);
}

/// Task 10 review: moving a dimension moves its text and dimension line, never its attached
/// points, so it still follows the model.
#[test]
fn a_moved_dimension_still_follows_the_model() {
    let (mut s, block, v) = front("a_moved_dimension_still_follows_the_model");
    let hole = edge_at(&s, v, [10.375, 8.0]);
    let left = edge_at(&s, v, [8.0, 8.0]);
    let r = run(&mut s, "drawing.dim.general", json!({"view": v, "picks": [{"edge": left}, {"edge": hole}], "placement": [9, 10.5]}));
    let h = handle(&r);
    assert_eq!(r["text"].as_str(), Some("2.00"));
    run(&mut s, "drawing.annotation.move", json!({"handle": h, "placement": [9.5, 11]}));
    let EntityKind::Dimension(d) = entity(&s, h).unwrap().1 else { panic!("not a dimension") };
    assert!(close(d.text_mid.x, 9.5) && close(d.text_mid.y, 11.0) && close(d.defpt.y, 11.0), "{d:?}");
    assert!(close(d.p13.x, 8.0) && close(d.p14.x, 10.0), "{d:?}");
    // The hole 0.5 in further right.
    edit_model(&block, &[("timeline.edit", json!({"feature": "Drill", "set": {"position": [12.7, 0, 38.1]}}))]);
    let r = run(&mut s, "drawing.update", json!({}));
    assert_eq!(r["orphans"].as_u64(), Some(0));
    assert_eq!(dimension_text(model(&s), h).as_deref(), Some("2.50"));
    assert!(sick_dimensions(model(&s)).is_empty());
}

/// R57 (Task 11 part): a deleted view takes its centre marks and its attached notes; notes
/// not attached to it stay.
#[test]
fn deleting_a_view_deletes_its_notes_and_marks() {
    let (mut s, _, v) = front("deleting_a_view_deletes_its_notes_and_marks");
    let hole = edge_at(&s, v, [10.375, 8.0]);
    let bottom = edge_at(&s, v, [10.0, 6.5]);
    run(&mut s, "drawing.centermark", json!({"view": v, "edge": hole}));
    let hn = handle(&run(&mut s, "drawing.note.hole", json!({"view": v, "edge": hole, "placement": [12, 10]})));
    let attached =
        handle(&run(&mut s, "drawing.note.leader", json!({"points": [[10, 6.5], [11, 5.5]], "text": "E", "attach": {"view": v, "edge": bottom}})));
    let free = handle(&run(&mut s, "drawing.note.text", json!({"position": [2, 2], "text": "NOTES:"})));
    let texts = [hn, attached].map(|h| crate::drawing::notes::text_of(model(&s), Handle(h)).unwrap().0);
    let undo = s.undo.len();
    run(&mut s, "drawing.view.delete", json!({"view": v}));
    assert_eq!(s.undo.len(), undo + 1);
    assert!(center_lines(&s).is_empty());
    for h in [hn, attached, texts[0], texts[1]] {
        assert!(entity(&s, h).is_none(), "{h} survived");
    }
    assert!(entity(&s, free).is_some());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(center_lines(&s).len(), 2);
    assert!(entity(&s, hn).is_some());
}

#[test]
fn note_commands_need_a_drawing() {
    let mut s = Session::default();
    run(&mut s, "document.new_part", json!({}));
    for (id, p) in [
        ("drawing.note.text", json!({"position": [1, 1], "text": "A"})),
        ("drawing.note.leader", json!({"points": [[1, 1], [2, 2]], "text": "A"})),
        ("drawing.note.hole", json!({"view": 1, "edge": "e", "placement": [1, 1]})),
        ("drawing.centermark", json!({"view": 1, "edge": "e"})),
        ("drawing.centerline.bisector", json!({"view": 1, "a": "e", "b": "f"})),
        ("drawing.annotation.move", json!({"handle": 1, "placement": [1, 1]})),
        ("drawing.annotation.delete", json!({"handles": [1]})),
        ("drawing.note.edit", json!({"handle": 1, "text": "A"})),
    ] {
        let e = s.execute(id, &p).unwrap_err().to_string();
        assert!(e.contains("not a drawing"), "{id}: {e}");
    }
}

/// View `id`'s entities on HIDDEN: handle and whether it is visible.
fn hidden_curves(s: &Session, id: u64) -> Vec<(u64, bool)> {
    let m = model(s);
    let layout = m.paper.layout(&m.sheets[0].layout).unwrap();
    layout
        .entities
        .iter()
        .filter(|e| xkey_of(e).is_some_and(|k| k.view == id && k.seg != Seg::Mark) && e.common.layer == "HIDDEN")
        .map(|e| (e.handle.0, e.common.visible))
        .collect()
}

fn visible(s: &Session, h: u64) -> bool {
    model(s).paper.entity(Handle(h)).unwrap_or_else(|| panic!("entity {h} is gone")).common.visible
}

/// R58, R61: annotations on the hole's hidden lines in VIEW3 (a centreline, a dimension, an
/// attached leader note) are SUPPRESSED by Hidden Line Removed: kept with their handles,
/// invisible, not sick, not picked; the hidden curves stay as invisible entities with their
/// handles; Hidden Line brings everything back as it was.
#[test]
fn style_suppressed_annotations_are_kept_not_orphaned() {
    let (mut s, _, v1) = front("style_suppressed_annotations_are_kept_not_orphaned");
    let v3 = run(&mut s, "drawing.view.projected", json!({"parent": v1, "position": [20, 8]}))["view"].as_u64().unwrap();
    let a = edge_at(&s, v3, [20.0, 7.625]);
    let b = edge_at(&s, v3, [20.0, 8.375]);
    let cl = handle(&run(&mut s, "drawing.centerline.bisector", json!({"view": v3, "a": a, "b": b})));
    let dim = handle(&run(&mut s, "drawing.dim.general", json!({"view": v3, "picks": [{"edge": a}], "placement": [20, 6]})));
    assert_eq!(dimension_text(model(&s), dim).as_deref(), Some("1.00"));
    let r = run(&mut s, "drawing.note.leader", json!({"points": [[20, 8.375], [21.5, 9.5]], "text": "SEE NOTE", "attach": {"view": v3, "edge": b}}));
    let (ld, txt) = (handle(&r), r["text_handle"].as_u64().unwrap());
    let curves = hidden_curves(&s, v3);
    assert_eq!(curves.len(), 2);
    assert!(curves.iter().all(|(_, v)| *v));
    let annotations = [cl, dim, ld, txt];
    assert!(suppressed_annotations(model(&s)).is_empty());

    let r = run(&mut s, "drawing.view.set_style", json!({"view": v3, "lines": "removed"}));
    assert_eq!(r["suppressed"], 4);
    // The hidden curves are kept, invisible, with their handles.
    assert_eq!(hidden_curves(&s, v3), curves.iter().map(|(h, _)| (*h, false)).collect::<Vec<_>>());
    // Every annotation is kept with its handle, not drawn, not sick.
    for h in annotations {
        assert!(!visible(&s, h), "{h} is still drawn");
    }
    assert_eq!(suppressed_annotations(model(&s)).into_iter().collect::<Vec<_>>(), {
        let mut v = annotations.to_vec();
        v.sort();
        v
    });
    assert!(sick_dimensions(model(&s)).is_empty());
    // A suppressed curve is not picked (pick skips invisible entities) nor dimensioned.
    assert_eq!(pick(&s, [20.0, 7.625], 0.05), Some(PaperPick::View { view: v3 }));
    assert_eq!(
        bad_message(s.execute("drawing.dim.general", &json!({"view": v3, "picks": [{"edge": a}], "placement": [20, 6]}))),
        format!("view {v3} shows no edge {a}")
    );
    // An update keeps them suppressed, not orphaned.
    let r = run(&mut s, "drawing.update", json!({}));
    assert_eq!(r["orphans"], 0);
    assert_eq!(suppressed_annotations(model(&s)).len(), 4);

    let r = run(&mut s, "drawing.view.set_style", json!({"view": v3, "lines": "hidden"}));
    assert_eq!(r["suppressed"], 0);
    assert_eq!(hidden_curves(&s, v3), curves);
    for h in annotations {
        assert!(visible(&s, h), "{h} did not come back");
    }
    assert_eq!(dimension_text(model(&s), dim).as_deref(), Some("1.00"));
    assert!(sick_dimensions(model(&s)).is_empty());
    let found = center_lines(&s);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, cl);
    assert!(close(found[0].1.dist(found[0].2), 1.25) && near(found[0].1.mid(found[0].2), [20.0, 8.0]));
    assert!(near(leader(&s, ld).0.vertices[0].xy(), [20.0, 8.375]));
}

/// Final wave 19: `notes::remove_attached` returns how many notes it removed, also when a
/// leader has lost its text; a free note stays.
#[test]
fn remove_attached_counts_the_notes() {
    use std::collections::BTreeSet;
    let (mut s, _, v) = front("remove_attached_counts_the_notes");
    let bottom = edge_at(&s, v, [10.0, 6.5]);
    let a =
        handle(&run(&mut s, "drawing.note.leader", json!({"points": [[10, 6.5], [11, 5.5]], "text": "A", "attach": {"view": v, "edge": bottom}})));
    let b = run(&mut s, "drawing.note.leader", json!({"points": [[9.5, 6.5], [9, 5.5]], "text": "B", "attach": {"view": v, "edge": bottom}}));
    run(&mut s, "drawing.note.text", json!({"position": [2, 2], "text": "NOTES:"}));
    let mut paper = model(&s).paper.clone();
    // The second leader's text gone: the note is still one.
    paper.remove_entity(Handle(b["text_handle"].as_u64().unwrap()));
    let views: BTreeSet<u64> = [v].into_iter().collect();
    assert_eq!(crate::drawing::notes::remove_attached(&mut paper, &views), 2);
    assert!(paper.entity(Handle(a)).is_none() && paper.entity(Handle(handle(&b))).is_none());
    let others: BTreeSet<u64> = [v + 100].into_iter().collect();
    assert_eq!(crate::drawing::notes::remove_attached(&mut paper, &others), 0);
}

/// Final wave 28: the hole is found by the edge's own faces, split on the body's face names. A
/// round on the hole's rim names the bore in its face name (`F4:blend:F1:-y|F2:bore:side:0`),
/// so the rim the front view shows after it, `F1:+y|F4:blend:F1:-y|F2:bore:side:0`, held `F2:`
/// and read as the hole; its faces are the back face and the round, so it is not a hole's edge.
#[test]
fn a_round_on_the_rim_is_not_the_hole() {
    let (mut s, block, v) = front("a_round_on_the_rim_is_not_the_hole");
    edit_model(&block, &[("solid.fillet", json!({"edges": [[9.525, 0, 38.1]], "radius": 1}))]);
    run(&mut s, "drawing.update", json!({}));
    let rim = edge_at(&s, v, [10.375, 8.0]);
    assert_eq!(rim, "0/Block/e/F1:+y|F4:blend:F1:-y|F2:bore:side:0");
    let undo = s.undo.len();
    assert_eq!(bad_message(s.execute("drawing.note.hole", &json!({"view": v, "edge": rim, "placement": [12, 10]}))), "pick the edge of a hole");
    assert_eq!(s.undo.len(), undo);
}

#[test]
fn edge_names_split_on_whole_face_names() {
    use crate::drawing::notes::edge_faces;
    let known: std::collections::BTreeSet<&str> = ["F1:-y", "F2:bore:side:0", "F4:blend:F1:-y|F2:bore:side:0"].into_iter().collect();
    assert_eq!(edge_faces("F1:-y|F4:blend:F1:-y|F2:bore:side:0", &known), ["F1:-y", "F4:blend:F1:-y|F2:bore:side:0"]);
    assert_eq!(edge_faces("F2:bore:side:0|F4:blend:F1:-y|F2:bore:side:0~2", &known), ["F2:bore:side:0", "F4:blend:F1:-y|F2:bore:side:0"]);
    assert_eq!(edge_faces("F1:-y|F2:bore:side:0", &known), ["F1:-y", "F2:bore:side:0"]);
    assert!(edge_faces("F1:+z|F9:x", &known).is_empty());
}

/// A note as `note_at` reads it: (text, height, justify).
fn read_note(s: &Session, h: u64) -> (String, f64, &'static str) {
    let n = note_at(model(s), h).unwrap_or_else(|| panic!("{h} is not a note"));
    (n.text, n.height, n.justify)
}

fn undo_label(s: &Session) -> Option<&str> {
    s.undo.last().map(|u| u.label.as_str())
}

/// R74: `drawing.note.edit` changes a note's text, height and justification in one undo step;
/// a leader note is edited through its leader or its text; the same values add no step.
#[test]
fn edit_note_changes_text_height_and_justify() {
    let (mut s, _, _) = front("edit_note_changes_text_height_and_justify");
    let h = handle(&run(&mut s, "drawing.note.text", json!({"position": [2, 2], "text": "NOTES:"})));
    let undo = s.undo.len();
    let r = run(&mut s, "drawing.note.edit", json!({"handle": h, "text": "NOTES: 1", "height": 0.25, "justify": "center"}));
    assert_eq!(r, json!({"handle": h}));
    assert_eq!((s.undo.len(), undo_label(&s)), (undo + 1, Some("Edit Note")));
    assert_eq!(read_note(&s, h), ("NOTES: 1".to_string(), 0.25, "center"));
    let t = mtext(&s, h);
    assert_eq!((t.attach, t.contents.as_str()), (2, "NOTES: 1"));
    assert!(near(t.insert.xy(), [2.0, 2.0]), "{t:?}");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(read_note(&s, h), ("NOTES:".to_string(), 0.12, "left"));
    assert_eq!(mtext(&s, h).attach, 1);
    // Two lines: stored as \P (braces escaped), read back with a new line.
    run(&mut s, "drawing.note.edit", json!({"handle": h, "text": "A\nB{1}"}));
    assert_eq!(mtext(&s, h).contents, "A\\PB\\{1\\}");
    assert_eq!(read_note(&s, h).0, "A\nB{1}");
    // A leader note through its leader: its text entity changes, attachment 4/5/6.
    let r = run(&mut s, "drawing.note.leader", json!({"points": [[5, 5], [6, 6]], "text": "X"}));
    let (lh, th) = (handle(&r), r["text_handle"].as_u64().unwrap());
    let undo = s.undo.len();
    let r = run(&mut s, "drawing.note.edit", json!({"handle": lh, "text": "Y", "justify": "right"}));
    assert_eq!(r, json!({"handle": th}));
    assert_eq!(s.undo.len(), undo + 1);
    let t = mtext(&s, th);
    assert_eq!((t.contents.as_str(), t.attach, t.height), ("Y", 6, 0.12));
    assert!(near(t.insert.xy(), [6.0, 6.0]), "{t:?}");
    assert_eq!(note_at(model(&s), lh).map(|n| n.text_handle), Some(th));
    // Through its text.
    run(&mut s, "drawing.note.edit", json!({"handle": th, "justify": "center", "height": 0.2}));
    let t = mtext(&s, th);
    assert_eq!((t.contents.as_str(), t.attach, t.height), ("Y", 5, 0.2));
    // The same values again: no step.
    let undo = s.undo.len();
    let r = run(&mut s, "drawing.note.edit", json!({"handle": lh, "text": "Y", "justify": "center", "height": 0.2}));
    assert_eq!(r, json!({"handle": th}));
    run(&mut s, "drawing.note.edit", json!({"handle": h, "text": "A\nB{1}", "justify": "left"}));
    assert_eq!(s.undo.len(), undo);
}

/// A boxed text note keeps its box when its justification changes: the insert moves to the
/// box's upper centre or right.
#[test]
fn edit_note_keeps_a_boxed_note_in_its_box() {
    let (mut s, _, _) = front("edit_note_keeps_a_boxed_note_in_its_box");
    let h = handle(&run(&mut s, "drawing.note.text", json!({"position": [2, 2], "corner": [6, 1], "text": "A"})));
    run(&mut s, "drawing.note.edit", json!({"handle": h, "justify": "right"}));
    let t = mtext(&s, h);
    assert!(t.attach == 3 && close(t.width, 4.0) && near(t.insert.xy(), [6.0, 2.0]), "{t:?}");
    run(&mut s, "drawing.note.edit", json!({"handle": h, "justify": "center"}));
    let t = mtext(&s, h);
    assert!(t.attach == 2 && near(t.insert.xy(), [4.0, 2.0]), "{t:?}");
}

/// The label entity (the identifier line) of view `v`.
fn view_label(s: &Session, v: u64) -> u64 {
    let m = model(s);
    let layout = m.paper.layout(&m.sheets[0].layout).unwrap();
    layout
        .entities
        .iter()
        .find(|e| crate::drawing::mapper::view_of(e) == Some(v) && xkey_of(e).is_none())
        .unwrap_or_else(|| panic!("view {v} has no label"))
        .handle
        .0
}

/// R74: what is not a note is refused, naming its command; nothing is added to the undo
/// history. A hole note takes a height (its text comes from its hole) and keeps it through an
/// update.
#[test]
fn edit_note_refuses_what_is_not_a_note() {
    let (mut s, _, v) = front("edit_note_refuses_what_is_not_a_note");
    let hole = edge_at(&s, v, [10.375, 8.0]);
    let note = handle(&run(&mut s, "drawing.note.text", json!({"position": [2, 2], "text": "NOTES:"})));
    let dim = handle(&run(&mut s, "drawing.dim.general", json!({"view": v, "picks": [{"edge": hole}], "placement": [11, 9.5]})));
    let mark = run(&mut s, "drawing.centermark", json!({"view": v, "edge": hole}))["handles"][0].as_u64().unwrap();
    run(&mut s, "drawing.view.set_label", json!({"view": v, "label_visible": true}));
    let label = view_label(&s, v);
    let hn = handle(&run(&mut s, "drawing.note.hole", json!({"view": v, "edge": hole, "placement": [12, 10]})));
    let m = model(&s);
    let curve =
        m.paper.layout(&m.sheets[0].layout).unwrap().entities.iter().find(|e| xkey_of(e).is_some_and(|k| k.seg != Seg::Mark)).unwrap().handle.0;
    let not_a_note = format!("{curve} is not a note");
    let undo = s.undo.len();
    for (p, msg) in [
        (json!({"handle": note}), "name text, height or justify"),
        (json!({"handle": note, "text": null}), "name text, height or justify"),
        (json!({"handle": dim, "text": "A"}), "a dimension: use drawing.dim.edit"),
        (json!({"handle": mark, "height": 0.2}), "a centre mark has no text"),
        (json!({"handle": label, "text": "A"}), "a view's label: use drawing.view.set_label"),
        (json!({"handle": hn, "text": "A"}), "a hole note's text comes from its hole"),
        (json!({"handle": note, "height": 0}), "`height` must be a number of inches above 0, at most 10"),
        (json!({"handle": note, "height": 11}), "`height` must be a number of inches above 0, at most 10"),
        (json!({"handle": note, "justify": "up"}), "`justify` must be left, center or right"),
        (json!({"handle": note, "text": " "}), "`text` must be some text"),
        (json!({"handle": "x", "text": "A"}), "`handle` must be an entity's handle"),
        (json!({"handle": 999_999, "text": "A"}), "no entity 999999"),
        (json!({"handle": curve, "text": "A"}), not_a_note.as_str()),
    ] {
        assert_eq!(bad_message(s.execute("drawing.note.edit", &p)), msg, "{p}");
    }
    assert_eq!(s.undo.len(), undo);
    // A hole note's height: its text stays the hole's, at 0.2, through an update.
    let th = run(&mut s, "drawing.note.edit", json!({"handle": hn, "height": 0.2}))["handle"].as_u64().unwrap();
    assert_eq!(s.undo.len(), undo + 1);
    run(&mut s, "drawing.update", json!({"views": [v]}));
    let t = mtext(&s, th);
    assert_eq!((t.contents.as_str(), t.height), ("⌀.75 THRU", 0.2));
}

/// R75: `note_at` reads a text, leader or hole note (by either handle of a leader note) and
/// nothing else.
#[test]
fn note_at_reads_notes_only() {
    let (mut s, _, v) = front("note_at_reads_notes_only");
    let hole = edge_at(&s, v, [10.375, 8.0]);
    let note = handle(&run(&mut s, "drawing.note.text", json!({"position": [2, 2], "text": "NOTES:"})));
    assert_eq!(note_at(model(&s), note), Some(NoteInfo { text_handle: note, text: "NOTES:".into(), height: 0.12, justify: "left", hole: false }));
    let r = run(&mut s, "drawing.note.leader", json!({"points": [[7, 5], [6, 6]], "text": "X\nY"}));
    let (lh, th) = (handle(&r), r["text_handle"].as_u64().unwrap());
    let by_leader = note_at(model(&s), lh).unwrap();
    assert_eq!(by_leader, NoteInfo { text_handle: th, text: "X\nY".into(), height: 0.12, justify: "right", hole: false });
    assert_eq!(note_at(model(&s), th), Some(by_leader));
    let hn = handle(&run(&mut s, "drawing.note.hole", json!({"view": v, "edge": hole, "placement": [12, 10]})));
    let n = note_at(model(&s), hn).unwrap();
    assert_eq!((n.text.as_str(), n.hole, n.justify), ("⌀.75 THRU", true, "left"));
    assert_eq!(note_at(model(&s), n.text_handle), Some(n.clone()));
    let dim = handle(&run(&mut s, "drawing.dim.general", json!({"view": v, "picks": [{"edge": hole}], "placement": [11, 9.5]})));
    let mark = run(&mut s, "drawing.centermark", json!({"view": v, "edge": hole}))["handles"][0].as_u64().unwrap();
    run(&mut s, "drawing.view.set_label", json!({"view": v, "label_visible": true}));
    let label = view_label(&s, v);
    for h in [dim, mark, label, 999_999] {
        assert_eq!(note_at(model(&s), h), None, "{h}");
    }
}

/// A dimension's (p15, defpt, text_mid).
fn radial_points(s: &Session, h: u64) -> (Vec2, Vec2, Vec2) {
    match &model(s).paper.entity(Handle(h)).unwrap().kind {
        EntityKind::Dimension(d) => (d.p15.xy(), d.defpt.xy(), d.text_mid.xy()),
        other => panic!("{h} is not a dimension: {other:?}"),
    }
}

fn within(p: Vec2, q: [f64; 2], tol: f64) -> bool {
    (p.x - q[0]).abs() <= tol && (p.y - q[1]).abs() <= tol
}

/// R80: moving a radius or diameter dimension turns its leader to the text: `p15` (and a
/// diameter's `defpt`, opposite) on the circle towards the placement, kept by an update.
#[test]
fn moving_a_radius_reaims_its_leader() {
    let (mut s, _, v) = front("moving_a_radius_reaims_its_leader");
    let r = run(&mut s, "drawing.dim.general", json!({"view": v, "picks": [{"at": [8.146, 6.646]}], "placement": [7.2, 6.0]}));
    assert_eq!((r["kind"].as_str(), r["text"].as_str()), (Some("radius"), Some("R.50")));
    let h = handle(&r);
    let undo = s.undo.len();
    let r = run(&mut s, "drawing.annotation.move", json!({"handle": h, "placement": [8.0, 5.5]}));
    assert_eq!(r["handle"], json!(h));
    assert_eq!(s.undo.len(), undo + 1);
    let n = 2.5f64.sqrt();
    let want = [8.5 + 0.5 * (-0.5 / n), 7.0 + 0.5 * (-1.5 / n)];
    let (p, defpt, text) = radial_points(&s, h);
    assert!(within(p, want, 1e-9), "{p:?} {want:?}");
    assert!(within(defpt, [8.5, 7.0], 1e-9), "{defpt:?}");
    assert!(within(text, [8.0, 5.5], 1e-12), "{text:?}");
    assert_eq!(dimension_text(model(&s), h).as_deref(), Some("R.50"));
    assert!(!sick_dimensions(model(&s)).contains(&h));
    run(&mut s, "drawing.update", json!({"views": [v]}));
    let (p, ..) = radial_points(&s, h);
    assert!(within(p, want, 1e-9), "{p:?} {want:?}");
    assert!(!sick_dimensions(model(&s)).contains(&h));
    // A diameter: both ends on the circle, along the text's direction.
    let hole = edge_at(&s, v, [10.375, 8.0]);
    let d = handle(&run(&mut s, "drawing.dim.general", json!({"view": v, "picks": [{"edge": hole}], "placement": [9, 9.5]})));
    run(&mut s, "drawing.annotation.move", json!({"handle": d, "placement": [12, 10]}));
    let k = 0.375 * std::f64::consts::FRAC_1_SQRT_2;
    let (p15, defpt, _) = radial_points(&s, d);
    assert!(within(p15, [10.0 + k, 8.0 + k], 1e-9), "{p15:?}");
    assert!(within(defpt, [10.0 - k, 8.0 - k], 1e-9), "{defpt:?}");
    assert_eq!(dimension_text(model(&s), d).as_deref(), Some("⌀.75"));
    assert!(!sick_dimensions(model(&s)).contains(&d));
}
