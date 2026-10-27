#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Ribbon split buttons remember the last pick under the Inventor profile (Plan B-2 fix wave 5):
//! a row picked from a split button's list runs and becomes its face; the face then runs it; the
//! pick survives a restart; a `fixed_face` drop-down keeps its face; SolidCraft never changes one.

use serde_json::json;
use solidcraft_engine::doc::DocumentKind::Part;
use solidcraft_engine::find_command;

use super::*;
use crate::dialogs::Kind;
use crate::scenario::{Harness, handle_at};
use crate::test_support::{click, inventor};

/// An Inventor Part on its 3D Model tab, the Start page closed.
fn model() -> Harness {
    let mut h = inventor(Part);
    h.app.home.open = false;
    assert_eq!(h.call("ui.set", json!({"tab": "MODEL"}))["ok"], json!(true));
    h.frames(3);
    h
}

fn plane(h: &Harness) -> &'static Dropdown {
    dropdown(&h.app, "MODEL", "WORK FEATURES", "construct.plane.offset").expect("Plane is a split button")
}

/// Open a split button's list and pick a row of it, then close whatever the pick opened.
fn pick_row(h: &mut Harness, split: &str, command: &str) {
    click(h, &format!("split:{split}"));
    click(h, &format!("dropdown:{command}"));
}

fn close(h: &mut Harness) {
    h.app.dialog = None;
    h.app.tool = None;
    h.frames(2);
}

fn kind(h: &Harness) -> String {
    format!("{:?}", h.app.dialog.as_ref().map(|d| &d.kind))
}

/// The face's command id, its label and its icon.
fn face_of(h: &Harness, d: &Dropdown) -> (&'static str, &'static str, &'static str) {
    let c = face(&h.app, d).expect("a face command");
    (c.id, face_label(&h.app, d), c.icon)
}

#[test]
fn picking_offset_plane_from_the_plane_list_makes_it_the_face_and_the_face_runs_it() {
    let mut h = model();
    let d = plane(&h);
    let offset = find_command("construct.plane.offset").unwrap();
    assert_eq!(face_of(&h, d), ("construct.plane.offset", "Plane", offset.icon), "before any pick: the profile's Plane face");
    pick_row(&mut h, "work_plane", "construct.plane.offset");
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::OffsetPlane { .. })), "the pick runs Offset Plane: {}", kind(&h));
    assert_eq!(face_of(&h, d), ("construct.plane.offset", offset.label, offset.icon), "the face is Offset Plane's own");
    assert_ne!(offset.label, "Plane");
    assert_eq!(h.app.preferences.split_faces["inventor"]["work_plane"], "construct.plane.offset");
    close(&mut h);
    click(&mut h, "toolbar:construct.plane.offset");
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::OffsetPlane { .. })), "the face runs it: {}", kind(&h));
}

#[test]
fn another_pick_updates_the_face_and_the_face_runs_the_new_pick() {
    let mut h = model();
    let d = plane(&h);
    pick_row(&mut h, "work_plane", "construct.plane.offset");
    close(&mut h);
    pick_row(&mut h, "work_plane", "construct.plane.midplane");
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Midplane)), "{}", kind(&h));
    let mid = find_command("construct.plane.midplane").unwrap();
    assert_eq!(face_of(&h, d), ("construct.plane.midplane", mid.label, mid.icon));
    close(&mut h);
    assert!(handle_at("button:construct.plane.midplane").is_some(), "the face is drawn as Midplane");
    click(&mut h, "toolbar:construct.plane.offset");
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Midplane)), "the face runs Midplane now: {}", kind(&h));
}

#[test]
fn a_small_split_button_follows_its_pick_too() {
    let mut h = model();
    let axis = dropdown(&h.app, "MODEL", "WORK FEATURES", "construct.axis.edge").expect("Axis is a split button");
    assert_eq!(face_label(&h.app, axis), "Axis");
    pick_row(&mut h, "work_axis", "construct.axis.two_points");
    close(&mut h);
    let two = find_command("construct.axis.two_points").unwrap();
    assert_eq!(face_of(&h, axis), ("construct.axis.two_points", two.label, two.icon));
    assert!(handle_at("button:construct.axis.two_points").is_some(), "the small face is drawn as the pick");
    // Plane keeps its own face: each drop-down remembers its own pick.
    assert_eq!(face_of(&h, plane(&h)).0, "construct.plane.offset");
    assert_eq!(face_label(&h.app, plane(&h)), "Plane");
}

#[test]
fn the_pick_survives_a_restart() {
    let mut h = model();
    pick_row(&mut h, "work_plane", "construct.plane.midplane");
    close(&mut h);
    let saved = h.app.prefs();
    let mut restored = crate::SolveApp::new(Default::default(), Default::default());
    restored.load_prefs(&saved);
    restored.session.doc_mut().kind = Part;
    assert_eq!(crate::profile::active(&restored).id, "inventor");
    let d = dropdown(&restored, "MODEL", "WORK FEATURES", "construct.plane.offset").unwrap();
    let mid = find_command("construct.plane.midplane").unwrap();
    assert_eq!(face(&restored, d).map(|c| c.id), Some("construct.plane.midplane"));
    assert_eq!(face_label(&restored, d), mid.label);
}

#[test]
fn a_fixed_face_drop_down_keeps_its_face() {
    let mut h = inventor(Part);
    h.app.home.open = false;
    assert_eq!(h.call("ui.set", json!({"tab": "SKETCH"}))["ok"], json!(true));
    h.frames(3);
    let d = dropdown(&h.app, "SKETCH", "CREATE", "sketch.fillet").expect("sketch Fillet is a split button");
    assert!(d.fixed_face);
    let before = face_label(&h.app, d);
    pick_row(&mut h, "sketch_fillet", "sketch.chamfer.two_distance");
    // Outside a sketch the pick starts a sketch first, then that exact tool.
    assert_eq!(h.app.dialog.as_ref().and_then(|d| d.then_start), Some("sketch.chamfer.two_distance"), "the pick ran");
    assert_eq!(face(&h.app, d).map(|c| c.id), Some("sketch.fillet"), "the face stays Fillet");
    assert_eq!(face_label(&h.app, d), before);
    assert!(!h.app.preferences.split_faces.get("inventor").is_some_and(|m| m.contains_key("sketch_fillet")), "nothing remembered");
}

#[test]
fn solidcraft_never_changes_a_face_and_writes_no_picks() {
    let mut h = Harness::new();
    assert!(!crate::profile::active(&h.app).workflow.sticky_split_buttons);
    let ribbon = crate::profile::ribbon(&h.app);
    assert!(ribbon.tabs.iter().flat_map(|t| &t.panels).all(|p| p.dropdowns.is_empty()), "SolidCraft has no split buttons");
    h.app.start("construct.plane.midplane");
    close(&mut h);
    assert!(h.app.preferences.split_faces.is_empty());
    assert!(!h.app.prefs().contains("split_faces"), "SolidCraft's prefs file is left as it was");
    // Even handed the Inventor drop-down, a pick under SolidCraft remembers nothing.
    let d = crate::profile::get("inventor")
        .unwrap()
        .ribbon(Part)
        .tab("MODEL")
        .unwrap()
        .panel("WORK FEATURES")
        .unwrap()
        .dropdown("construct.plane.offset")
        .unwrap();
    pick(&mut h.app, d, "construct.plane.midplane");
    assert!(h.app.preferences.split_faces.is_empty());
    assert_eq!(face(&h.app, d).map(|c| c.id), Some("construct.plane.offset"));
}
