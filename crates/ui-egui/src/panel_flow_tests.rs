#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Feature panels: Enter is OK, "+" applies and keeps the panel for the next one, and OK or "+"
//! can be asked for from outside the footer (the marking menu).

use super::*;
use crate::scenario::Harness;
use solidcraft_engine::doc::DocumentKind;

fn with_rectangle(mut h: Harness) -> Harness {
    h.app.preferences.look_at_anim = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[60,40]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h
}

fn rect_part(kind: DocumentKind) -> Harness {
    with_rectangle(crate::test_support::inventor(kind))
}

fn extrudes(h: &Harness) -> usize {
    h.app.session.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::Extrude { .. })).count()
}

fn extrude_panel(h: &mut Harness) {
    h.app.start("solid.extrude");
    h.frames(2);
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Extrude { .. })), "the Extrude panel is open");
}

fn request(h: &mut Harness, action: PanelAction) {
    h.app.dialog.as_mut().expect("a panel").requested = Some(action);
    h.frames(3);
}

#[test]
fn enter_is_ok_in_the_extrude_panel_under_inventor() {
    let mut h = rect_part(DocumentKind::Part);
    extrude_panel(&mut h);
    h.step(&json!({"key":"Enter"})).unwrap();
    h.frames(3);
    assert_eq!(extrudes(&h), 1);
    assert!(h.app.dialog.is_none(), "Enter closed the panel");
}

#[test]
fn the_plus_button_applies_and_keeps_the_panel() {
    let mut h = rect_part(DocumentKind::Part);
    extrude_panel(&mut h);
    assert!(crate::scenario::rect_of("dialog:apply").is_some(), "the panel offers +");
    h.step(&json!({"click":{"handle":"dialog:apply"}})).unwrap();
    h.frames(3);
    assert_eq!(extrudes(&h), 1);
    let d = h.app.dialog.as_ref().expect("the panel stays open for the next one");
    assert!(matches!(d.kind, Kind::Extrude { .. }));
    assert!(d.inputs[0].items.is_empty(), "the next one starts with nothing picked");
    assert!(h.app.session.selection.is_empty());
}

#[test]
fn plus_with_nothing_picked_adds_nothing() {
    let mut h = rect_part(DocumentKind::Part);
    extrude_panel(&mut h);
    h.app.dialog.as_mut().unwrap().inputs[0].items.clear();
    request(&mut h, PanelAction::ApplyAndContinue);
    assert_eq!(extrudes(&h), 0);
    let d = h.app.dialog.as_ref().expect("the panel stays");
    assert!(d.error.is_some(), "it says what is missing");
}

#[test]
fn a_requested_ok_applies_like_the_button() {
    let mut h = rect_part(DocumentKind::Part);
    extrude_panel(&mut h);
    request(&mut h, PanelAction::Ok);
    assert_eq!(extrudes(&h), 1);
    assert!(h.app.dialog.is_none());
}

#[test]
fn panels_open_with_template_defaults_not_last_values() {
    let mut h = rect_part(DocumentKind::Part);
    extrude_panel(&mut h);
    let Some(Kind::Extrude { distance, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) else { panic!("extrude") };
    *distance = "33 mm".into();
    request(&mut h, PanelAction::Ok);
    assert_eq!(extrudes(&h), 1);
    extrude_panel(&mut h);
    let Some(Kind::Extrude { distance, .. }) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("extrude") };
    assert_eq!(distance, "10 mm");
}

#[test]
fn editing_a_feature_offers_no_plus() {
    let mut h = rect_part(DocumentKind::Part);
    extrude_panel(&mut h);
    h.step(&json!({"key":"Enter"})).unwrap();
    h.frames(3);
    let id = h.app.session.doc.features.iter().find(|f| matches!(f.kind, FeatureKind::Extrude { .. })).expect("an extrude").id;
    h.app.edit_feature(id);
    h.frames(2);
    assert!(h.app.dialog.as_ref().is_some_and(|d| d.editing.is_some()), "the extrude is being edited");
    assert!(crate::scenario::rect_of("dialog:apply").is_none());
}

#[test]
fn solidcraft_extrude_has_no_plus() {
    let mut h = with_rectangle(Harness::new());
    extrude_panel(&mut h);
    assert!(crate::scenario::rect_of("dialog:apply").is_none());
}
