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

#[test]
fn inventor_part_root_lists_bodies_before_origin() {
    let mut h = inventor(Part);
    h.app.run("sketch.create", json!({"plane":"XY","name":"Profile"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,15]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let extrude = h.app.run("solid.extrude", json!({"distance":10,"body_name":"Plate"})).unwrap()["feature"].as_u64().unwrap();
    let name = h.app.session.doc.feature(extrude).unwrap().name.clone();
    h.app.tree.expanded.insert("c0/features".into());
    h.frames(2);
    let labels = labels();
    let (views, bodies, origin) = (position(&labels, "Named Views"), position(&labels, "Solid Bodies"), position(&labels, "Origin"));
    assert!(views < bodies && bodies < origin, "{labels:?}");
    assert!(position(&labels, &name) > origin, "{labels:?}");
    assert!(!labels.iter().any(|l| l == "Bodies"), "{labels:?}");
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
}

#[test]
fn solidcraft_browser_keeps_its_order_and_names() {
    let mut h = Harness::new();
    assert_eq!(origin_children(&mut h), ["O", "X", "Y", "Z", "XY", "XZ", "YZ"]);
    assert_eq!(folder_label(&h.app, "bodies"), "Bodies");
}
