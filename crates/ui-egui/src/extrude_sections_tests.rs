#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Extrude's panel under a profile that sections it (Inventor's): Behavior (Direction, Extents,
//! distances), Output (the boolean, once there is a body to join) and Advanced.

use super::*;
use crate::scenario::Harness;
use solidcraft_engine::doc::DocumentKind;

/// The Inventor profile, optionally with a box away from the sketch, then a 60 x 40 rectangle
/// on XY and the Extrude panel open.
fn extrude(with_box: bool) -> Harness {
    let mut h = crate::test_support::inventor(DocumentKind::Part);
    h.app.preferences.look_at_anim = false;
    if with_box {
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [200, 0, 0]})).unwrap();
    }
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[60,40]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.start("solid.extrude");
    h.frames(2);
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Extrude { .. })), "the Extrude panel is open");
    h
}

fn direction(h: &Harness) -> usize {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Extrude { direction, .. }) => *direction,
        _ => panic!("no Extrude panel"),
    }
}

fn operation(h: &Harness) -> (usize, bool) {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Extrude { operation, auto_op, .. }) => (*operation, *auto_op),
        _ => panic!("no Extrude panel"),
    }
}

fn click(h: &mut Harness, text: &str) {
    h.step(&json!({"widget": text})).unwrap();
    h.frames(2);
}

#[test]
fn inventor_extrude_shows_behavior_output_and_advanced() {
    let h = extrude(true);
    for text in ["Flipped", "Asymmetric", "Through All", "New Solid", "Behavior", "Output"] {
        assert!(h.widget(text).is_some(), "{text} is drawn");
    }
}

#[test]
fn direction_buttons_map_to_directions() {
    let mut h = extrude(true);
    click(&mut h, "Asymmetric");
    assert_eq!(direction(&h), TWO_SIDES);
    assert!(h.widget("Distance B").is_some(), "Asymmetric asks for the second distance");
    click(&mut h, "Flipped");
    assert_eq!(direction(&h), 1);
    assert!(h.widget("Distance B").is_none());
    click(&mut h, "Default");
    assert_eq!(direction(&h), 0);
}

#[test]
fn output_buttons_map_to_operations() {
    let mut h = extrude(true);
    click(&mut h, "Cut");
    assert_eq!(operation(&h), (2, false));
    click(&mut h, "New Solid");
    assert_eq!(operation(&h).0, 0);
}

#[test]
fn no_output_row_on_the_first_feature() {
    let h = extrude(false);
    assert!(h.widget("Behavior").is_some(), "the sectioned panel is drawn");
    assert!(h.widget("New Solid").is_none());
}

#[test]
fn solidcraft_extrude_keeps_its_rows() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[60,40]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.start("solid.extrude");
    h.frames(2);
    assert!(h.widget("Asymmetric").is_none());
    assert!(h.widget("Extent Type").is_some());
}
