#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::history::{drop_position, parents, top_rows};
use super::*;
use crate::profile::RootNode;
use crate::scenario::Harness;
use crate::test_support::inventor;
use solidcraft_engine::doc::DocumentKind::Part;
use std::collections::BTreeMap;

fn labels() -> Vec<String> {
    drawn_rows().into_iter().map(|(label, ..)| label).collect()
}

/// The labels from `first` on, `n` of them.
fn from(first: &str, n: usize) -> Vec<String> {
    let labels = labels();
    let at = labels.iter().position(|l| l == first).unwrap_or_else(|| panic!("no {first} row in {labels:?}"));
    labels[at..(at + n).min(labels.len())].to_vec()
}

fn id_of(h: &Harness, name: &str) -> u64 {
    h.app.session.doc.find_feature(name).unwrap().id
}

fn label_left(label: &str) -> f32 {
    crate::scenario::rect_of(&format!("row_label:{label}")).unwrap_or_else(|| panic!("no {label} label")).left()
}

/// Extrude the finished sketch, renamed `name`; its id.
fn extrude(h: &mut Harness, distance: f64, name: &str) -> u64 {
    let id = h.app.run("solid.extrude", json!({"distance": distance})).unwrap()["feature"].as_u64().unwrap();
    h.app.run("timeline.rename", json!({"feature": id, "name": name})).unwrap();
    id
}

/// S1 (an XY rectangle), E1 (extruded from S1), S2 (an XZ circle, unused): (harness, S1, E1, S2).
fn history_part() -> (Harness, u64, u64, u64) {
    let mut h = inventor(Part);
    h.app.run("sketch.create", json!({"plane":"XY","name":"S1"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,15]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let e1 = extrude(&mut h, 10.0, "E1");
    h.app.run("sketch.create", json!({"plane":"XZ","name":"S2"})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[0,0],"radius":5})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let (s1, s2) = (id_of(&h, "S1"), id_of(&h, "S2"));
    h.app.tree.collapsed.remove("c0");
    h.frames(2);
    (h, s1, e1, s2)
}

#[test]
fn the_history_folds_consumed_sketches_collapsed() {
    let (mut h, _, e1, _) = history_part();
    assert_eq!(from("E1", 3), ["E1", "S2", "End of Part"]);
    let labels = labels();
    for absent in ["S1", "Sketches", "Features"] {
        assert!(!labels.iter().any(|l| l == absent), "{absent} is drawn: {labels:?}");
    }
    h.app.tree.expanded.insert(format!("history/{e1}"));
    h.frames(2);
    assert_eq!(from("E1", 4), ["E1", "S1", "S2", "End of Part"]);
    assert!(label_left("S1") > label_left("E1"), "S1 is indented under E1");
}

#[test]
fn work_features_nest_under_their_consumer() {
    let mut h = inventor(Part);
    h.app.run("construct.plane.offset", json!({"base":"XY","offset":30,"name":"WP"})).unwrap();
    h.app.run("sketch.create", json!({"plane":"WP","name":"S3"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let e2 = extrude(&mut h, 5.0, "E2");
    let (wp, s3) = (id_of(&h, "WP"), id_of(&h, "S3"));
    let doc = &h.app.session.doc;
    assert_eq!(parents(doc, None), BTreeMap::from([(s3, e2), (wp, s3)]));
    assert_eq!(top_rows(doc).into_iter().map(|(_, id)| id).collect::<Vec<_>>(), [e2]);
    h.app.tree.expanded.insert(format!("history/{e2}"));
    h.app.tree.expanded.insert(format!("history/{s3}"));
    h.frames(2);
    assert_eq!(from("E2", 4), ["E2", "S3", "WP", "End of Part"]);
    assert!(label_left("S3") > label_left("E2"), "S3 is indented under E2");
    assert!(label_left("WP") > label_left("S3"), "WP is indented under S3");
}

#[test]
fn end_of_part_follows_the_marker() {
    let (mut h, ..) = history_part();
    h.app.run_heavy("timeline.roll_to", json!({"position": 2})).unwrap();
    h.frames(3);
    assert_eq!(h.app.session.doc.marker, Some(2));
    assert_eq!(from("E1", 3), ["E1", "End of Part", "S2"]);
}

#[test]
fn a_rolled_back_consumer_releases_its_sketch() {
    let (mut h, ..) = history_part();
    h.app.run_heavy("timeline.roll_to", json!({"position": 1})).unwrap();
    h.frames(3);
    assert_eq!(from("S1", 4), ["S1", "End of Part", "E1", "S2"]);
    assert!(parents(&h.app.session.doc, Some(1)).is_empty());
}

#[test]
fn drop_position_lands_between_top_rows() {
    let rows = [(1, 100.0), (2, 120.0)];
    assert_eq!(drop_position(&rows, 90.0), 0);
    assert_eq!(drop_position(&rows, 110.0), 2);
    assert_eq!(drop_position(&rows, 130.0), 3);
}

#[test]
fn end_of_part_menu_moves_it_to_top_and_end() {
    let (mut h, ..) = history_part();
    let items = crate::context_menu::items(&h.app, &Target::EndOfPart);
    assert_eq!(items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["ui.eopTop", "ui.eopEnd"]);
    crate::context_menu::run_item(&mut h.app, &items[0], pos2(0.0, 0.0));
    h.frames(3);
    assert_eq!(h.app.session.doc.marker, Some(0));
    crate::context_menu::run_item(&mut h.app, &items[1], pos2(0.0, 0.0));
    h.frames(3);
    assert_eq!(h.app.session.doc.marker, None);
}

#[test]
fn double_clicking_the_consumed_sketch_edits_it() {
    let (mut h, s1, e1, _) = history_part();
    h.app.tree.expanded.insert(format!("history/{e1}"));
    h.frames(2);
    h.step(&json!({"click":{"handle": format!("row:history:{s1}")},"double":true})).unwrap();
    h.frames(3);
    assert_eq!(h.app.session.active_sketch, Some(s1));
}

#[test]
fn the_feature_menu_offers_edit_sketch() {
    let (h, s1, e1, _) = history_part();
    let items = crate::context_menu::items(&h.app, &Target::Feature { id: e1 });
    let edit = items.iter().find(|i| i.id == "ui.editSketch").expect("Edit Sketch");
    assert_eq!(edit.params, Some(json!({"sketch": s1})));
    assert!(!items.iter().any(|i| i.id == "ui.findTimeline"), "Inventor has no timeline to find in");

    let mut plain = Harness::new();
    plain.app.run("sketch.create", json!({"plane":"XY","name":"S1"})).unwrap();
    plain.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,15]})).unwrap();
    plain.app.run("sketch.finish", json!({})).unwrap();
    let e1 = extrude(&mut plain, 10.0, "E1");
    let items = crate::context_menu::items(&plain.app, &Target::Feature { id: e1 });
    assert!(items.iter().any(|i| i.id == "ui.findTimeline"), "SolidCraft keeps Find in Timeline");
    assert!(!items.iter().any(|i| i.id == "ui.editSketch"), "SolidCraft's feature menu is unchanged");
}

#[test]
fn inventor_hides_the_timeline_panel() {
    let h = inventor(Part);
    assert!(!crate::timeline::shown(&h.app));
    assert!(crate::timeline::shown(&Harness::new().app));
}

#[test]
fn solidcraft_root_has_no_history_node() {
    assert!(!root_order(&Harness::new().app).contains(&RootNode::History));
}

#[test]
fn a_history_root_needs_an_end_of_part_label() {
    let mut profile = crate::profile::get("inventor").unwrap().clone();
    assert!(!crate::profile::validate(&profile).iter().any(|p| p.contains("end_of_part")));
    profile.browser.folder_labels.remove("end_of_part");
    let problems = crate::profile::validate(&profile);
    assert!(problems.iter().any(|p| p == "part browser: history needs folder_labels.end_of_part"), "{problems:?}");
}

#[test]
fn dragging_end_of_part_rolls_the_history_to_where_it_lands() {
    let (mut h, _, e1, s2) = history_part();
    let end = crate::scenario::handle_at("row:End of Part").unwrap();
    let (e1_at, s2_at) =
        (crate::scenario::handle_at(&format!("row:history:{e1}")).unwrap(), crate::scenario::handle_at(&format!("row:history:{s2}")).unwrap());
    // Between E1 and S2: after E1 (feature 2 of 3).
    let between = (e1_at.y + s2_at.y) / 2.0;
    h.step(&json!({"drag": [[end.x, end.y], [end.x, between]], "steps": 6})).unwrap();
    h.frames(3);
    assert_eq!(h.app.session.doc.marker, Some(2));
    assert_eq!(from("E1", 3), ["E1", "End of Part", "S2"]);
    // Back below S2: the end.
    let end = crate::scenario::handle_at("row:End of Part").unwrap();
    let below = crate::scenario::handle_at(&format!("row:history:{s2}")).unwrap().y + 6.0;
    h.step(&json!({"drag": [[end.x, end.y], [end.x, below]], "steps": 6})).unwrap();
    h.frames(3);
    assert_eq!(h.app.session.doc.marker, None);
}

/// History part plus an unused work plane WP: (harness, S1, WP, body name).
fn find_targets(mut h: Harness) -> (Harness, u64, u64, String) {
    h.app.run("sketch.create", json!({"plane":"XY","name":"S1"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,15]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    extrude(&mut h, 10.0, "E1");
    h.app.run("construct.plane.offset", json!({"base":"XY","offset":30,"name":"WP"})).unwrap();
    let body = h.app.session.model.state().bodies[0].name.clone();
    let (s1, wp) = (id_of(&h, "S1"), id_of(&h, "WP"));
    (h, s1, wp, body)
}

fn menus(h: &Harness, s1: u64, wp: u64, body: &str) -> Vec<(&'static str, Vec<Item>)> {
    vec![
        ("sketch", crate::context_menu::items(&h.app, &Target::Sketch { id: s1 })),
        ("plane", crate::context_menu::items(&h.app, &Target::Plane { id: wp })),
        ("body", crate::context_menu::items(&h.app, &Target::Body { name: body.into() })),
    ]
}

#[test]
fn find_in_timeline_is_offered_only_where_there_is_a_timeline() {
    let (h, s1, wp, body) = find_targets(inventor(Part));
    for (what, items) in menus(&h, s1, wp, &body) {
        assert!(!items.is_empty(), "{what} has a menu");
        assert!(!items.iter().any(|i| i.id == "ui.findTimeline"), "Inventor's {what} menu has no Find in Timeline");
        assert_ne!(items.last().map(|i| i.id.as_str()), Some("-"), "the {what} menu does not end on a separator");
    }
    let (h, s1, wp, body) = find_targets(Harness::new());
    for (what, items) in menus(&h, s1, wp, &body) {
        assert!(items.iter().any(|i| i.id == "ui.findTimeline"), "SolidCraft's {what} menu keeps Find in Timeline");
    }
}

#[test]
fn f2_on_a_history_plane_row_renames_the_plane() {
    let (mut h, _, wp, _) = find_targets(inventor(Part));
    h.app.home.open = false;
    h.frames(2);
    h.step(&json!({"click":{"handle": format!("row:history:{wp}")}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.selection, [Sel::Plane { name: "WP".into() }]);
    h.step(&json!({"key": "F2"})).unwrap();
    h.frames(2);
    let rename = h.app.menu.rename.as_ref().expect("F2 opens the Rename box");
    assert_eq!(rename.what, crate::context_menu::RenameWhat::Feature(wp));
    assert_eq!(rename.text, "WP");
}
