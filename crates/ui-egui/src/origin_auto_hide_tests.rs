#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The Origin hides itself once a design is no longer empty, under the profile's
//! `workflow.origin_auto_hide` (Plan B-2 fix wave 6): shown in a new Part, hidden after its first
//! sketch, shown again while a plane is picked, and the browser's eyes win in both directions.

use serde_json::json;
use solidcraft_engine::doc::DocumentKind::{Assembly, Part};

use crate::scenario::{Harness, handle_at, rect_of};
use crate::test_support::inventor;

/// A new inch Part from Inventor's template, browser shown.
fn inch_part() -> Harness {
    let mut h = inventor(Part);
    h.app.home.open = false;
    crate::documents::new_document(&mut h.app, Part, Some("in"));
    h.frames(3);
    h
}

/// The origin planes and axes the view draws now.
fn drawn(h: &Harness) -> (usize, usize) {
    (crate::viewport::origin_planes(&h.app).len(), crate::viewport::origin_axes(&h.app).len())
}

/// Create Sketch, pick `plane` in the view, draw a rectangle, Finish Sketch.
fn sketch_on(h: &mut Harness, plane: &str) {
    h.app.start("sketch.create");
    h.frames(2);
    assert!(h.app.picking_plane(), "Create Sketch waits for a plane");
    assert!(h.app.origin_visible(), "the Origin is shown while a plane is picked");
    assert_eq!(drawn(h), (3, 3), "every origin plane and axis is drawn for the pick");
    h.step(&json!({"click": {"plane": plane}})).unwrap();
    h.frames(3);
    assert!(h.app.session.active_sketch.is_some(), "the pick opens a sketch on {plane}");
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [1, 1]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.frames(3);
}

/// Click a browser row's label (fold or select it).
fn click_row(h: &mut Harness, label: &str) {
    let r = rect_of(&format!("row_label:{label}")).unwrap_or_else(|| panic!("no {label} row"));
    h.step(&json!({"click": [r.center().x, r.center().y]})).unwrap();
    h.frames(2);
}

fn click_eye(h: &mut Harness, label: &str) {
    assert!(handle_at(&format!("eye:{label}")).is_some(), "no eye on {label}");
    h.step(&json!({"click": {"handle": format!("eye:{label}")}})).unwrap();
    h.frames(2);
}

#[test]
fn a_new_inch_part_shows_the_origin_until_its_first_sketch_then_only_for_a_plane_pick() {
    let mut h = inch_part();
    assert_eq!(h.app.session.doc.units, "in");
    assert!(h.app.origin_shown() && h.app.origin_visible(), "an empty Part shows its Origin");
    assert_eq!(drawn(&h), (3, 3));
    sketch_on(&mut h, "XY");
    assert!(!h.app.origin_shown() && !h.app.origin_visible(), "the first finished sketch hides the Origin");
    assert_eq!(drawn(&h), (0, 0));
    assert_eq!(h.app.session.visibility.origin_hidden, None, "hidden by the rule, not saved as a toggle");
    // Start Sketch again: shown for the pick (asserted in sketch_on), hidden once it is made.
    sketch_on(&mut h, "XZ");
    assert!(!h.app.origin_visible());
    assert_eq!(drawn(&h), (0, 0));
}

#[test]
fn the_browser_origin_eye_wins_over_auto_hide_both_ways_and_is_saved() {
    // On after the rule hid it: stays on across a feature.
    let mut h = inch_part();
    sketch_on(&mut h, "XY");
    assert!(!h.app.origin_shown());
    click_eye(&mut h, "Origin");
    assert!(h.app.origin_shown());
    assert_eq!(h.app.session.visibility.origin_hidden, Some(false), "the toggle is saved with the design");
    sketch_on(&mut h, "XZ");
    h.app.run("solid.box", json!({"length": 1, "width": 1, "height": 1})).unwrap();
    h.frames(2);
    assert!(h.app.origin_visible(), "a toggled-on Origin stays on across features");
    assert_eq!(drawn(&h), (3, 3));

    // Off in an empty Part: stays off although the design is empty.
    let mut h = inch_part();
    click_eye(&mut h, "Origin");
    assert_eq!(h.app.session.visibility.origin_hidden, Some(true));
    assert!(!h.app.origin_visible());
    assert_eq!(drawn(&h), (0, 0));
}

#[test]
fn an_origin_item_eye_turned_on_after_auto_hide_shows_that_item_alone() {
    let mut h = inch_part();
    sketch_on(&mut h, "XY");
    click_row(&mut h, "Origin");
    let xz = crate::browser::drawn_rows().into_iter().map(|(l, ..)| l).find(|l| l.starts_with("XZ")).expect("the XZ Plane row");
    click_eye(&mut h, &xz);
    assert!(h.app.origin_shown(), "an item turned on shows the Origin");
    let planes: Vec<&str> = crate::viewport::origin_planes(&h.app).into_iter().map(|(n, ..)| n).collect();
    assert_eq!(planes, ["XZ"], "only the item turned on is drawn");
    assert!(crate::viewport::origin_axes(&h.app).is_empty());
    // Its own eye then hides it again, as before.
    click_eye(&mut h, &xz);
    assert!(crate::viewport::origin_planes(&h.app).is_empty());
}

#[test]
fn an_assembly_hides_its_origin_once_a_component_is_placed() {
    let mut h = inventor(Assembly);
    h.app.home.open = false;
    h.frames(2);
    assert!(h.app.origin_visible(), "an empty Assembly shows its Origin");
    h.app.run("solid.box", json!({"length": 1, "width": 1, "height": 1})).unwrap();
    assert!(h.app.origin_visible(), "a feature is not a placed component");
    h.app.run("component.create", json!({"name": "Bracket"})).unwrap();
    h.frames(2);
    assert!(!h.app.session.doc.occurrences.is_empty(), "a component is placed");
    assert!(!h.app.origin_visible(), "a placed component hides the Origin");
}

#[test]
fn solidcraft_keeps_its_origin_after_a_sketch() {
    let mut h = Harness::new();
    h.app.home.open = false;
    assert!(!crate::profile::active(&h.app).workflow.origin_auto_hide);
    assert!(h.app.origin_visible());
    sketch_on(&mut h, "XY");
    assert!(h.app.origin_visible() && !h.app.origin_auto_hidden(), "SolidCraft never hides the Origin by itself");
    assert_eq!(drawn(&h), (3, 3));
}
