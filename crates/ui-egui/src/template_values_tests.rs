#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The inch template: new panels open in inches in an inch design under a profile with template
//! values (Inventor), the Hole offers ANSI Unified threads, a new document carries its own
//! length precision, and any field takes any unit or a parameter.

use super::*;
use crate::context_menu::{Target, items, run_item};
use crate::prefs::{DOC_SETTINGS_KEY, apply_template_decimals, document_length_decimals, shown_length_decimals};
use crate::scenario::Harness;
use crate::test_support::inventor;
use solidcraft_engine::doc::{DocumentKind, TappedThreadFamily, parse_tapped_thread};

fn with_rectangle(h: &mut Harness, at: f64) {
    h.app.preferences.look_at_anim = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[at, 0],"p1":[at + 2.0, 1.5]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
}

fn part_in(units: &str) -> Harness {
    let mut h = inventor(DocumentKind::Part);
    crate::prefs::apply_new_design_units(&mut h.app, units);
    with_rectangle(&mut h, 0.0);
    h
}

fn inch_part() -> Harness {
    part_in("in")
}

fn open(h: &mut Harness, command: &str) -> Kind {
    h.app.dialog = None;
    h.app.start(command);
    h.frames(2);
    h.app.dialog.as_ref().unwrap_or_else(|| panic!("{command} opened no panel")).kind.clone()
}

fn extrude_distance(h: &mut Harness) -> String {
    match open(h, "solid.extrude") {
        Kind::Extrude { distance, .. } => distance,
        k => panic!("not an extrude: {k:?}"),
    }
}

fn hole(h: &mut Harness) -> (String, String, HoleOpts) {
    match open(h, "solid.hole") {
        Kind::Hole { diameter, depth, opts, .. } => (diameter, depth, opts),
        k => panic!("not a hole: {k:?}"),
    }
}

fn fillet_radius(h: &mut Harness, command: &str) -> String {
    match open(h, command) {
        Kind::Fillet { radius, .. } => radius,
        k => panic!("not a fillet: {k:?}"),
    }
}

#[test]
fn an_inch_part_opens_extrude_at_one_inch() {
    let mut h = inch_part();
    let Kind::Extrude { distance, distance2, wall, wall2, .. } = open(&mut h, "solid.extrude") else { panic!("extrude") };
    assert_eq!(distance, "1 in");
    assert_eq!(distance2, "1 in");
    assert_eq!(wall, "0.1 in");
    assert_eq!(wall2, "0.05 in");
}

#[test]
fn an_inch_part_opens_fillet_chamfer_and_hole_in_inches() {
    let mut h = inch_part();
    assert_eq!(fillet_radius(&mut h, "solid.fillet"), "0.125 in");
    assert_eq!(fillet_radius(&mut h, "solid.chamfer"), "0.0625 in");
    let (diameter, depth, opts) = hole(&mut h);
    assert_eq!(diameter, "0.25 in");
    assert_eq!(depth, "0.5 in");
    assert_eq!(opts.thread, "1/4-20 UNC");
}

#[test]
fn the_inch_hole_offers_ansi_unified_threads() {
    assert!(crate::dialogs_thread::THREAD_PROFILES.contains(&"UNC"));
    assert!(crate::dialogs_thread::THREAD_PROFILES.contains(&"UNF"));
    let mut h = inch_part();
    let (_, _, opts) = hole(&mut h);
    assert_eq!(parse_tapped_thread(&opts.thread).expect("a catalogued thread").family, TappedThreadFamily::Unc);
}

#[test]
fn a_mm_part_keeps_the_millimetre_defaults() {
    let mut h = part_in("mm");
    assert_eq!(extrude_distance(&mut h), "10 mm");
    assert_eq!(fillet_radius(&mut h, "solid.fillet"), "2 mm");
    assert_eq!(hole(&mut h).2.thread, "M5");
}

#[test]
fn solidcraft_inch_design_keeps_upstream_literals() {
    let mut h = Harness::new();
    crate::prefs::apply_new_design_units(&mut h.app, "in");
    with_rectangle(&mut h, 0.0);
    assert_eq!(h.app.session.doc.units, "in");
    assert_eq!(extrude_distance(&mut h), "10 mm");
    assert_eq!(hole(&mut h).2.thread, "M5");
    apply_template_decimals(&mut h.app);
    assert_eq!(document_length_decimals(&h.app.session.doc), None);
    assert_eq!(shown_length_decimals(&h.app), h.app.preferences.length_decimals);
}

#[test]
fn a_foot_design_uses_the_inch_template() {
    let mut h = part_in("ft");
    assert_eq!(h.app.session.doc.units, "ft");
    assert_eq!(extrude_distance(&mut h), "1 in");
}

#[test]
fn a_pattern_type_switch_keeps_the_template() {
    let mut h = inch_part();
    h.app.run("solid.box", json!({"length":"1 in","width":"1 in","height":"1 in"})).unwrap();
    let Kind::PatternRect { spacing, spacing2, .. } = open(&mut h, "solid.pattern.rectangular") else { panic!("rectangular pattern") };
    assert_eq!((spacing.as_str(), spacing2.as_str()), ("1 in", "1 in"));
    // The Type row's re-init, to Path and back to Rectangular.
    let Kind::PathPattern { spacing, .. } = pattern_start(&h.app, 2).0 else { panic!("path pattern") };
    assert_eq!(spacing, "1 in");
    let Kind::PatternRect { spacing, spacing2, .. } = pattern_start(&h.app, 0).0 else { panic!("rectangular pattern") };
    assert_eq!((spacing.as_str(), spacing2.as_str()), ("1 in", "1 in"));
}

/// Applies an Extrude of the newest sketch's profile at `distance` and returns what it stored.
fn extrude_typed(h: &mut Harness, at: f64, distance: &str) -> String {
    with_rectangle(h, at);
    let sketch = h.app.session.model.state().sketches.last().unwrap().feature;
    h.app.dialog = None;
    h.app.start("solid.extrude");
    h.frames(2);
    let d = h.app.dialog.as_mut().expect("the Extrude panel");
    d.inputs[0].items = vec![Sel::Profile { sketch, index: 0 }];
    if let Kind::Extrude { distance: value, operation, auto_op, .. } = &mut d.kind {
        *value = distance.into();
        *operation = 0;
        *auto_op = false;
    }
    d.requested = Some(PanelAction::Ok);
    h.frames(3);
    assert!(h.app.dialog.is_none(), "OK applied {distance}");
    let FeatureKind::Extrude { extent, .. } =
        &h.app.session.doc.features.iter().rev().find(|f| matches!(f.kind, FeatureKind::Extrude { .. })).expect("an extrude").kind
    else {
        panic!("extrude")
    };
    extent.distance.to_string()
}

#[test]
fn feature_fields_take_any_unit_or_parameter() {
    let mut h = inch_part();
    h.app.run("parameters.add", json!({"name":"w","expression":"2 in"})).unwrap();
    assert_eq!(extrude_typed(&mut h, 10.0, "w/2"), "w/2");
    assert!((h.app.session.doc.eval("w/2", ValueKind::Length).unwrap() - 25.4).abs() < 1e-9);
    assert_eq!(extrude_typed(&mut h, 20.0, "12 mm"), "12 mm");
    assert_eq!(extrude_typed(&mut h, 30.0, "2"), "2 in", "a bare number is the design's unit");
}

#[test]
fn the_new_part_template_reaches_the_panels() {
    let mut h = inventor(DocumentKind::Part);
    crate::documents::new_document(&mut h.app, DocumentKind::Part, Some("in"));
    with_rectangle(&mut h, 0.0);
    assert_eq!(extrude_distance(&mut h), "1 in");
}

fn new_part(units: &str) -> Harness {
    let mut h = inventor(DocumentKind::Part);
    h.app.preferences.length_decimals = 4;
    crate::documents::new_document(&mut h.app, DocumentKind::Part, Some(units));
    h
}

#[test]
fn a_new_inch_part_shows_three_decimals() {
    let h = new_part("in");
    assert_eq!(h.app.session.doc.units, "in");
    assert_eq!(document_length_decimals(&h.app.session.doc), Some(3));
    assert_eq!(shown_length_decimals(&h.app), 3);
    assert_eq!(crate::prefs::show_mm_bare(&h.app, 25.4), "1.000");
    assert!(!h.app.session.is_dirty(), "the template's precision is not a change to save");
    assert!(h.app.session.undo.is_empty(), "nor an undo step");
    assert_eq!(h.app.preferences.length_decimals, 4, "the user's preference is untouched");
}

#[test]
fn a_new_mm_part_shows_two_decimals() {
    let h = new_part("mm");
    assert_eq!(document_length_decimals(&h.app.session.doc), Some(2));
    assert_eq!(shown_length_decimals(&h.app), 2);
    assert_eq!(crate::prefs::show_mm_bare(&h.app, 1.0), "1.00");
    assert_eq!(h.app.preferences.length_decimals, 4);
}

#[test]
fn a_legacy_document_shows_the_user_preference() {
    let mut h = inventor(DocumentKind::Part);
    h.app.preferences.length_decimals = 4;
    let mut s = Session::default();
    s.execute("document.units", &json!({"units":"in"})).unwrap();
    s.mark_saved();
    crate::documents::adopt(&mut h.app, s);
    assert_eq!(h.app.session.doc.units, "in");
    assert!(h.app.session.doc.metadata.get(DOC_SETTINGS_KEY).is_none());
    assert_eq!(document_length_decimals(&h.app.session.doc), None);
    assert_eq!(shown_length_decimals(&h.app), 4);
}

#[test]
fn document_decimals_are_editable_per_document() {
    let mut h = new_part("in");
    let preference = h.app.preferences.length_decimals;
    let list: Vec<_> = items(&h.app, &Target::Units).into_iter().filter(|i| i.id == "ui.docDecimals").collect();
    let numbers: Vec<u64> = list.iter().map(|i| i.params.as_ref().unwrap()["decimals"].as_u64().unwrap()).collect();
    assert_eq!(numbers, [0, 1, 2, 3, 4, 5, 6]);
    let current: Vec<u64> =
        list.iter().filter(|i| i.shortcut == "current").map(|i| i.params.as_ref().unwrap()["decimals"].as_u64().unwrap()).collect();
    assert_eq!(current, [3]);
    let undo = h.app.session.undo.len();
    let four = list.iter().find(|i| i.params.as_ref().unwrap()["decimals"] == json!(4)).unwrap().clone();
    run_item(&mut h.app, &four, egui::Pos2::ZERO);
    assert_eq!(document_length_decimals(&h.app.session.doc), Some(4));
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(document_length_decimals(&h.app.session.doc), Some(3));
    assert_eq!(h.app.preferences.length_decimals, preference);
}

#[test]
fn a_fresh_install_design_carries_the_inch_template_precision() {
    let mut app = SolveApp::new(Session::default(), crate::Services::default());
    crate::profile::first_run(&mut app);
    assert_eq!(app.session.doc.units, "in");
    assert_eq!(document_length_decimals(&app.session.doc), Some(3));
    assert!(!app.session.is_dirty());
}

#[test]
fn editing_a_hole_keeps_its_stored_values() {
    let mut h = inch_part();
    h.app.run("solid.box", json!({"length":"40 mm","width":"30 mm","height":"20 mm"})).unwrap();
    let id = h.app.run("solid.hole", json!({"position":[20, 15, 20],"diameter":"5 mm","depth":"10 mm"})).unwrap()["feature"].as_u64().unwrap();
    h.app.edit_feature(id);
    h.frames(2);
    let Some(Dialog { kind: Kind::Hole { diameter, depth, .. }, .. }) = h.app.dialog.as_ref() else { panic!("the Hole panel") };
    assert_eq!(diameter, "5 mm");
    assert_eq!(depth, "10 mm");
}

#[test]
fn a_home_mm_tile_under_an_inch_preference_takes_the_mm_precision() {
    let mut h = inventor(DocumentKind::Part);
    h.app.preferences.default_units = "in".into();
    h.app.home.show(0);
    h.frames(3);
    h.step(&json!({"click": {"handle": "home:new_mm"}})).unwrap();
    h.frames(3);
    assert!(!h.app.home.open);
    assert_eq!(h.app.session.doc.units, "mm");
    assert_eq!(document_length_decimals(&h.app.session.doc), Some(2), "the mm template's precision, not the inch one");
    assert_eq!(h.app.preferences.default_units, "in");
}

#[test]
fn a_units_preference_change_on_a_blank_inch_design_moves_its_precision() {
    let mut h = new_part("in");
    assert_eq!(document_length_decimals(&h.app.session.doc), Some(3));
    assert!(crate::documents::active_is_blank(&h.app));
    h.app.preferences.default_units = "mm".into();
    crate::prefs::default_units_changed(&mut h.app);
    assert_eq!(h.app.session.doc.units, "mm");
    assert_eq!(document_length_decimals(&h.app.session.doc), Some(2));
    assert!(!h.app.session.is_dirty() && h.app.session.undo.is_empty());
}

#[test]
fn a_design_with_work_keeps_its_precision() {
    let mut h = part_in("in");
    h.app.run("document.metadata.set", json!({"key": DOC_SETTINGS_KEY, "value": {"length_decimals": 5}})).unwrap();
    apply_template_decimals(&mut h.app);
    assert_eq!(document_length_decimals(&h.app.session.doc), Some(5));
}
