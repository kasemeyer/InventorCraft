#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Extrude's presets and Between extent (F-1 item 4, R197; the clean-room theme document
//! §Property panels): Extrude remembers No Preset and Last Used. Inventor reopens with Last
//! Used; the profile opens with No Preset until the owner rules (owner decision 7, "template
//! defaults, not last values"), and Last Used is one pick away. The Behavior row's Between is
//! From Object + To Object, which the engine has.

use super::*;
use crate::scenario::Harness;
use solidcraft_engine::doc::DocumentKind;

/// A finished 60 x 40 rectangle on XY and the Extrude panel open on it.
fn open_extrude(h: &mut Harness) {
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[60,40]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.start("solid.extrude");
    h.frames(2);
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Extrude { .. })), "the Extrude panel is open");
}

fn inv() -> Harness {
    let mut h = crate::test_support::inventor(DocumentKind::Part);
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;
    h
}

/// (distance, direction, taper) of the open Extrude panel.
fn values(h: &Harness) -> (String, usize, String) {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Extrude { distance, direction, taper, .. }) => (distance.clone(), *direction, taper.clone()),
        _ => panic!("no Extrude panel"),
    }
}

fn set(h: &mut Harness, d: &str, dir: usize, t: &str) {
    if let Some(Kind::Extrude { distance, direction, taper, taper_present, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *distance = d.into();
        *direction = dir;
        *taper = t.into();
        *taper_present = true;
    }
    h.frames(2);
}

fn ok(h: &mut Harness) {
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(4);
    assert!(h.app.dialog.is_none(), "{:?}", h.app.status);
}

fn choose(h: &mut Harness, option: &str) {
    h.step(&json!({"click": {"handle": "dialog:Presets"}})).unwrap();
    h.step(&json!({"click": {"handle": format!("dialog:Presets:{option}")}})).unwrap();
    h.frames(2);
}

#[test]
fn extrude_remembers_last_used_and_no_preset_gives_the_template() {
    let mut h = inv();
    open_extrude(&mut h);
    let template = values(&h);
    assert_eq!(template, ("10 mm".to_string(), 0, "0 deg".to_string()), "the first extrude starts from the template");
    set(&mut h, "25 mm", 1, "3 deg");
    ok(&mut h);
    // Reopened: No Preset (owner decision 7), with Last Used offered.
    open_extrude(&mut h);
    assert_eq!(values(&h), template, "the panel opens with the template's values");
    assert!(h.widget("Presets").is_some(), "the Presets row is drawn");
    choose(&mut h, "Last Used");
    assert_eq!(values(&h), ("25 mm".to_string(), 1, "3 deg".to_string()), "Last Used restores the last extrude's values");
    choose(&mut h, "No Preset");
    assert_eq!(values(&h), template, "No Preset: the template's values");
    // Remembered per profile in the preferences, so it survives a restart.
    assert!(h.app.preferences.feature_presets["inventor"].contains_key("solid.extrude"));
    assert!(!crate::profile::active(&h.app).workflow.preset_opens_last_used, "owner decision 7 stands");
}

/// Inventor's own default, one profile value away: a panel that opens with Last Used.
#[test]
fn opening_with_last_used_restores_the_last_extrude() {
    let mut h = inv();
    open_extrude(&mut h);
    set(&mut h, "25 mm", 1, "3 deg");
    ok(&mut h);
    open_extrude(&mut h);
    let mut fresh = h.app.dialog.clone().unwrap();
    assert!(use_last_used(&h.app, &mut fresh), "there is a Last Used");
    *h.app.dialog.as_mut().unwrap() = fresh;
    h.frames(2);
    assert_eq!(values(&h), ("25 mm".to_string(), 1, "3 deg".to_string()));
}

#[test]
fn editing_an_extrude_neither_restores_nor_remembers() {
    let mut h = inv();
    open_extrude(&mut h);
    set(&mut h, "20 mm", 0, "0 deg");
    ok(&mut h);
    let feature = h.app.session.doc.features.iter().rev().find(|f| matches!(f.kind, solidcraft_engine::doc::FeatureKind::Extrude { .. })).unwrap().id;
    h.app.edit_feature(feature);
    h.frames(2);
    assert_eq!(values(&h).0, "20 mm", "an edit shows the feature's own values");
    set(&mut h, "40 mm", 0, "0 deg");
    ok(&mut h);
    open_extrude(&mut h);
    choose(&mut h, "Last Used");
    assert_eq!(values(&h).0, "20 mm", "the edit did not change Last Used");
}

#[test]
fn solidcraft_extrude_keeps_its_defaults() {
    let mut h = Harness::new();
    h.app.home.open = false;
    open_extrude(&mut h);
    let first = values(&h);
    set(&mut h, "25 mm", 1, "0 deg");
    ok(&mut h);
    open_extrude(&mut h);
    assert_eq!(values(&h), first, "no Last Used under SolidCraft");
    assert!(h.widget("Presets").is_none());
    assert!(h.app.preferences.feature_presets.is_empty(), "nothing remembered");
}

#[test]
fn between_is_from_object_and_to_object() {
    let mut h = inv();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [200, 0, 0]})).unwrap();
    open_extrude(&mut h);
    let keys: Vec<_> = dense::extent_glyphs().iter().map(|g| g.key).collect();
    assert_eq!(keys, ["Distance", "To", "Through All", "Between"]);
    h.step(&json!({"click": {"handle": "dialog:Extents:Between"}})).unwrap();
    h.frames(2);
    let d = h.app.dialog.as_ref().unwrap();
    assert!(
        to_object_on(&d.inputs) && object_on(&d.inputs, FROM_OBJECT),
        "Between asks for both faces: {:?}",
        d.inputs.iter().map(|i| i.label).collect::<Vec<_>>()
    );
    assert_eq!(d.inputs[d.active].label, FROM_OBJECT, "the start face is picked first");
    // Back to Distance: both picks go.
    h.step(&json!({"click": {"handle": "dialog:Extents:Distance"}})).unwrap();
    h.frames(2);
    let d = h.app.dialog.as_ref().unwrap();
    assert!(!to_object_on(&d.inputs) && !object_on(&d.inputs, FROM_OBJECT));
}

/// Last Used is offered disabled, with the reason on hover, until an extrude has been made.
#[test]
fn last_used_is_disabled_until_something_is_stored() {
    let mut h = inv();
    open_extrude(&mut h);
    h.step(&json!({"click": {"handle": "dialog:Presets"}})).unwrap();
    h.frames(1);
    let disabled = |h: &Harness| {
        h.output
            .accesskit_update
            .as_ref()
            .and_then(|tree| tree.nodes.iter().find_map(|(_, n)| (n.label() == Some("Last Used")).then(|| n.is_disabled())))
    };
    assert_eq!(disabled(&h), Some(true), "Last Used is greyed with nothing stored");
    h.step(&json!({"click": {"handle": "dialog:Presets:Last Used"}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.dialog.as_ref().unwrap().extra.get("preset"), None, "still No Preset");
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(1);
    if h.app.dialog.is_some() {
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(4);
    }
    // Once an extrude is made, it is live.
    let _ = h.app.dialog.take();
    open_extrude(&mut h);
    ok(&mut h);
    open_extrude(&mut h);
    h.step(&json!({"click": {"handle": "dialog:Presets"}})).unwrap();
    h.frames(1);
    assert_eq!(disabled(&h), Some(false), "Last Used is live once something is stored");
}
