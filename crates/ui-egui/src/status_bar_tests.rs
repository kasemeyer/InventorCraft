#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The status bar: "Ready" or the latest message on the left, the sketch's constraint status and
//! the document count on the right, drawn only under a profile that asks for it.

use super::*;
use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::doc::DocumentKind;

fn part() -> Harness {
    let mut h = crate::test_support::inventor(DocumentKind::Part);
    h.app.preferences.look_at_anim = false;
    h
}

/// An open XY sketch holding a two-point rectangle from (10,10) to (40,30).
fn rectangle(h: &mut Harness) {
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[10,10],"p1":[40,30]})).unwrap();
}

fn dof(h: &Harness) -> usize {
    let st = h.app.session.world_state();
    st.sketch(h.app.session.active_sketch.unwrap()).unwrap().report.dof
}

#[test]
fn the_status_bar_says_ready_and_counts_documents() {
    let h = part();
    let s = texts(&h.app);
    assert_eq!(s.left, "Ready");
    assert_eq!(s.documents, 1);
}

#[test]
fn a_status_message_shows_on_the_left() {
    let mut h = part();
    h.app.set_status("Saved", false);
    assert_eq!(texts(&h.app).left, "Saved");
}

#[test]
fn the_sketch_status_counts_dimensions_needed() {
    let mut h = part();
    rectangle(&mut h);
    assert_eq!(dof(&h), 4);
    assert_eq!(texts(&h.app).sketch.as_deref(), Some("4 dimensions needed"));
}

#[test]
fn a_fully_dimensioned_sketch_says_so() {
    let mut h = part();
    rectangle(&mut h);
    h.app.run("sketch.constraint.fix", json!({"entities":["p1"]})).unwrap();
    h.app.run("sketch.dimension", json!({"entities":["l1"],"value":30})).unwrap();
    h.app.run("sketch.dimension", json!({"entities":["l2"],"value":20})).unwrap();
    assert_eq!(dof(&h), 0);
    assert_eq!(texts(&h.app).sketch.as_deref(), Some("Fully constrained"));
}

#[test]
fn no_sketch_no_sketch_status() {
    let mut h = part();
    assert!(texts(&h.app).sketch.is_none(), "outside a sketch");
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    assert!(h.app.session.active_sketch.is_some());
    assert!(texts(&h.app).sketch.is_none(), "a blank sketch has no status yet");
}

#[test]
fn only_profiles_that_ask_draw_it() {
    let mut h = part();
    h.frames(2);
    assert!(crate::scenario::rect_of("status_bar").is_some());
    let mut h = Harness::new();
    h.app.home.open = false;
    h.frames(2);
    assert!(crate::scenario::rect_of("status_bar").is_none());
}

/// Every right-hand cell sits between two rules: a trailing rule closes the last one (the
/// document count), as in the capture.
#[test]
fn a_trailing_rule_closes_the_last_cell() {
    let mut h = part();
    rectangle(&mut h);
    h.frames(2);
    let rect = |key: &str| crate::scenario::rect_of(key).unwrap_or_else(|| panic!("{key} drawn"));
    let bar = rect("status_bar");
    let (documents, sketch) = (rect("status_bar:cell:0"), rect("status_bar:cell:1"));
    let rules: Vec<_> = (0..3).map(|i| rect(&format!("status_bar:rule:{i}"))).collect();
    assert!(crate::scenario::rect_of("status_bar:rule:3").is_none(), "one rule per cell, plus the trailing one");
    assert!(rules[0].left() > documents.right() && rules[0].right() <= bar.right(), "the trailing rule is after the document count");
    assert!(rules[1].left() < documents.left() && rules[1].left() > sketch.right(), "a rule between the sketch status and the count");
    assert!(rules[2].right() < sketch.left(), "a rule before the sketch status");
}
