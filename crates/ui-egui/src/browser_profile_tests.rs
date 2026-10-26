#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::profile::RootNode::*;
use crate::scenario::Harness;
use crate::test_support::{click, inventor};
use solidcraft_engine::doc::DocumentKind::{Assembly, Part};

fn labels() -> Vec<String> {
    drawn_rows().into_iter().map(|(label, ..)| label).collect()
}

fn position(labels: &[String], label: &str) -> usize {
    labels.iter().position(|l| l == label).unwrap_or_else(|| panic!("no {label} row in {labels:?}"))
}

/// The seven rows after the Origin row (the folder expanded).
fn origin_children(h: &mut Harness) -> Vec<String> {
    h.app.tree.expanded.insert("origin".into());
    h.frames(2);
    let labels = labels();
    let at = position(&labels, "Origin");
    labels[at + 1..at + 8].to_vec()
}

/// One sketch (Profile) and the extrusion made from it (body Plate); the extrusion's name.
fn sketch_and_extrude(h: &mut Harness) -> String {
    h.app.run("sketch.create", json!({"plane":"XY","name":"Profile"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,15]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let extrude = h.app.run("solid.extrude", json!({"distance":10,"body_name":"Plate"})).unwrap()["feature"].as_u64().unwrap();
    h.app.session.doc.feature(extrude).unwrap().name.clone()
}

#[test]
fn inventor_part_root_is_flat_history_between_origin_and_end_of_part() {
    let mut h = inventor(Part);
    let name = sketch_and_extrude(&mut h);
    // Folded, so its body row does not sit between the root rows.
    h.app.tree.collapsed.insert("c0/bodies".into());
    h.frames(2);
    let labels = labels();
    assert_eq!(labels[1..], ["Representations", "Solid Bodies", "Origin", "Profile", name.as_str(), "End of Part"], "{labels:?}");
    for folder in ["fold:Sketches", "fold:Features", "fold:Named Views", "fold:Bodies"] {
        assert!(crate::scenario::handle_at(folder).is_none(), "{folder} is drawn: {labels:?}");
    }
    assert!(crate::scenario::handle_at("fold:Representations").is_some());
    assert!(crate::scenario::handle_at("fold:End of Part").is_none(), "End of Part does not fold");
}

#[test]
fn inventor_end_of_part_row_does_nothing_when_clicked() {
    let mut h = inventor(Part);
    sketch_and_extrude(&mut h);
    h.app.session.selection = vec![Sel::Plane { name: "XY".into() }];
    h.frames(2);
    let saved = h.app.session.doc.to_json();
    let (collapsed, expanded) = (h.app.tree.collapsed.clone(), h.app.tree.expanded.clone());
    click(&mut h, "row:End of Part");
    assert_eq!(h.app.session.selection, [Sel::Plane { name: "XY".into() }], "the selection stays");
    assert_eq!((&h.app.tree.collapsed, &h.app.tree.expanded), (&collapsed, &expanded));
    assert_eq!(h.app.session.doc.to_json(), saved);
}

#[test]
fn inventor_empty_assembly_draws_relationships() {
    let mut h = inventor(Assembly);
    assert!(h.app.session.doc.assembly.joints.is_empty());
    h.frames(2);
    let labels = labels();
    assert_eq!(labels[1..4], ["Representations", "Relationships", "Origin"], "{labels:?}");
    assert!(crate::scenario::handle_at("fold:Relationships").is_some());
}

#[test]
fn solidcraft_root_keeps_its_folders() {
    let mut h = Harness::new();
    let name = sketch_and_extrude(&mut h);
    h.app.tree.expanded.insert("c0/features".into());
    h.frames(2);
    let labels = labels();
    for folder in ["fold:Named Views", "fold:Bodies", "fold:Sketches", "fold:Features"] {
        assert!(crate::scenario::handle_at(folder).is_some(), "{folder} is not drawn: {labels:?}");
    }
    assert!(position(&labels, "Features") < position(&labels, &name), "{labels:?}");
    // An empty design has no Joints folder, and a part has no End of Part row.
    for absent in ["Joints", "End of Part", "Representations"] {
        assert!(!labels.iter().any(|l| l == absent), "{absent}: {labels:?}");
    }
}

#[test]
fn root_lists_parse_always_flags_and_reject_duplicates() {
    let inventor = crate::profile::get("inventor").unwrap();
    assert_eq!(inventor.browser.part_root, [NamedViews, Bodies, Origin, History, EndOfPart]);
    assert_eq!(inventor.browser.assembly_always, [Joints]);
    assert!(inventor.browser.part_always.is_empty());
    assert!(crate::profile::get("solidcraft").unwrap().browser.assembly_always.is_empty());
    let mut twice = inventor.clone();
    twice.browser.part_root.push(Origin);
    let problems = crate::profile::validate(&twice);
    assert_eq!(problems, ["browser.part_root: Origin appears twice (entry 5)"], "{problems:?}");
}

#[test]
fn inventor_origin_lists_planes_then_axes_then_center_point() {
    let mut h = inventor(Part);
    assert_eq!(origin_children(&mut h), ["YZ Plane", "XZ Plane", "XY Plane", "X Axis", "Y Axis", "Z Axis", "Center Point"]);
    click(&mut h, "row:XY Plane");
    assert_eq!(h.app.session.selection, [Sel::Plane { name: "XY".into() }]);
}

#[test]
fn inventor_assembly_root_starts_with_relationships() {
    let h = inventor(Assembly);
    assert_eq!(root_order(&h.app)[..3], [NamedViews, Joints, Origin]);
    assert_eq!(folder_label(&h.app, "joints"), "Relationships");
    assert_eq!(folder_label(&h.app, "bodies"), "Solid Bodies");
    assert_eq!(folder_label(&h.app, "named_views"), "Representations");
}

#[test]
fn solidcraft_browser_keeps_its_order_and_names() {
    let mut h = Harness::new();
    assert_eq!(origin_children(&mut h), ["O", "X", "Y", "Z", "XY", "XZ", "YZ"]);
    assert_eq!(folder_label(&h.app, "bodies"), "Bodies");
    assert_eq!(folder_label(&h.app, "named_views"), "Named Views");
}
