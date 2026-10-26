#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use serde_json::{Value, json};
use solidcraft_engine::Session;

use crate::profile::{self, History};
use crate::{Services, SolveApp};

fn inventor() -> &'static crate::profile::Profile {
    profile::get(profile::INVENTOR).expect("the Inventor profile is built in")
}

fn inventor_layouts() -> &'static BTreeMap<MenuContext, MenuLayout> {
    inventor().workflow.marking_menu.as_ref().expect("Inventor lays out its marking menus")
}

fn strings(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

/// Inventor's workflow with its Face layout replaced by this ring and overflow.
fn face_only(ring: &[&str], overflow: &[&str]) -> Workflow {
    let mut w = inventor().workflow.clone();
    let layout = MenuLayout { ring: strings(ring), overflow: strings(overflow), ..MenuLayout::default() };
    w.marking_menu.as_mut().unwrap().insert(MenuContext::Face, layout);
    w
}

const EIGHT: [&str; 8] =
    ["ui.lookAt", "solid.extrude", "ui.selectOther", "ui.tangentChain", "sketch.create", "construct.plane.offset", "edit.undo", "inspect.measure"];

#[test]
fn solidcraft_workflow_is_upstream() {
    let w = &profile::get("solidcraft").unwrap().workflow;
    assert_eq!(*w, Workflow::default());
    assert_eq!(w.marking_menu, None);
    assert_eq!(w.inch_length_decimals, None);
    assert_eq!(w.mm_length_decimals, None);
    assert!(w.escape_twice_finishes_sketch && w.sketch_palette);
    let others = [w.look_at_on_sketch, w.restore_view_after_sketch, w.template_values, w.apply_and_continue, w.status_bar, w.sectioned_extrude];
    assert_eq!(others, [false; 6]);
}

#[test]
fn inventor_turns_the_workflow_on() {
    let p = inventor();
    let w = &p.workflow;
    let on = [w.look_at_on_sketch, w.restore_view_after_sketch, w.template_values, w.apply_and_continue, w.status_bar, w.sectioned_extrude];
    assert_eq!(on, [true; 6]);
    assert!(!w.escape_twice_finishes_sketch);
    assert!(!w.sketch_palette);
    assert_eq!(w.inch_length_decimals, Some(3));
    assert_eq!(w.mm_length_decimals, Some(2));
    assert_eq!(p.defaults.units.as_deref(), Some("in"));
    assert_eq!(p.defaults.history, Some(History::Browser));
}

#[test]
fn inventor_face_ring_follows_the_capture() {
    let face = &inventor_layouts()[&MenuContext::Face];
    assert_eq!(face.ring, strings(&EIGHT));
    assert_eq!(face.overflow, strings(&["ui.repeat", "-", "ui.previousView", "ui.home", "-", "ui.properties"]));
    assert_eq!(face.evidence.as_deref(), Some("docs/evidence/screenshots/part/marking-menu.png"));
}

#[test]
fn every_context_has_a_layout() {
    let layouts = inventor_layouts();
    for ctx in MenuContext::ALL {
        assert!(layouts.contains_key(&ctx), "Inventor lacks a {ctx} marking menu");
    }
    assert_eq!(layouts.len(), MenuContext::ALL.len());

    let mut json: Value = serde_json::from_str(include_str!("../../../assets/profiles/inventor.json")).unwrap();
    json["workflow"]["marking_menu"]["browser"] = json!({"ring": EIGHT});
    let err = profile::parse(&json.to_string(), &profile::solidcraft()).expect_err("browser rows are list menus");
    assert!(err.contains("browser"), "{err}");
}

#[test]
fn a_ring_needs_eight_slots() {
    let lines = validate_workflow(&face_only(&EIGHT[..7], &[]));
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("face ring has 7 slots"), "{lines:?}");
}

#[test]
fn marking_ids_must_be_commands_or_actions() {
    let mut ring = EIGHT;
    ring[2] = "ui.bogus";
    assert_eq!(validate_workflow(&face_only(&ring, &[])), ["face marking menu: ui.bogus is not a registry command or marking action"]);

    ring[2] = "-";
    assert_eq!(validate_workflow(&face_only(&ring, &[])), ["face marking menu: - is not a registry command or marking action"]);

    ring[2] = "";
    assert_eq!(validate_workflow(&face_only(&ring, &["ui.repeat", "-", "ui.home"])), Vec::<String>::new());
}

#[test]
fn a_layout_set_names_every_context() {
    let mut w = inventor().workflow.clone();
    w.marking_menu.as_mut().unwrap().remove(&MenuContext::Panel);
    assert_eq!(validate_workflow(&w), ["marking menu lacks panel"]);
}

#[test]
fn validate_reports_workflow_problems() {
    let mut p = inventor().clone();
    let face = p.workflow.marking_menu.as_mut().unwrap().get_mut(&MenuContext::Face).unwrap();
    face.ring.pop();
    let lines = profile::validate(&p);
    assert!(lines.iter().any(|l| l.contains("face ring has 7 slots")), "{lines:?}");
}

#[test]
fn first_run_prefers_the_profiles_units() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    assert_ne!(app.preferences.default_units, "in", "the test needs a non-inch start");
    profile::first_run(&mut app);
    assert_eq!(app.preferences.default_units, "in");
    assert_eq!(app.session.doc.units, "in");
    assert!(!app.session.is_dirty());
    profile::choose(&mut app, "solidcraft").unwrap();
    assert_eq!(app.preferences.default_units, "in");
}

#[test]
fn uncaptured_marking_menus_are_flagged() {
    // Every graphics-window context Inventor lays out is read from a capture (Task 4).
    assert_eq!(workflow_warnings(&inventor().workflow), Vec::<String>::new());
    for (ctx, layout) in inventor_layouts() {
        let evidence = layout.evidence.as_deref().unwrap_or_default();
        assert!(evidence.starts_with("docs/evidence/screenshots/"), "{ctx}: {evidence:?}");
    }
    // A layout without one is flagged as unverified.
    let mut w = inventor().workflow.clone();
    w.marking_menu.as_mut().unwrap().get_mut(&MenuContext::Panel).unwrap().evidence = None;
    assert_eq!(workflow_warnings(&w), vec!["panel marking menu has no capture (unverified)".to_string()]);
    assert_eq!(workflow_warnings(&profile::get("solidcraft").unwrap().workflow), Vec::<String>::new());
}
