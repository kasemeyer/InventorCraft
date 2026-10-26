#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The fast extrude preview in a design that is not in millimetres: a length with its unit, and
//! a bare number in the design's unit, preview at once; a unit-less expression keeps the engine
//! preview.

use super::*;
use crate::{dialogs::Kind, scenario::Harness};
use serde_json::json;

fn inch_box_preview(distance: &str) -> Option<Result<Built, String>> {
    let mut harness = Harness::new();
    crate::prefs::apply_new_design_units(&mut harness.app, "in");
    assert_eq!(harness.app.session.doc.units, "in");
    harness.app.run("solid.box", json!({"length":"4 mm","width":"3 mm","height":"5 mm","body_name":"Stock"})).unwrap();
    let state = harness.app.session.world_state();
    let face = state
        .body("Stock")
        .unwrap()
        .body
        .faces(0.001)
        .unwrap()
        .into_iter()
        .find(|face| face.plane_normal.is_some_and(|normal| normal.z > 0.99))
        .unwrap();
    harness.app.session.selection = vec![Sel::Face { body: "Stock".into(), index: face.index, point: face.centroid }];
    harness.app.start("solid.extrude");
    if let Kind::Extrude { distance: value, operation, auto_op, .. } = &mut harness.app.dialog.as_mut().unwrap().kind {
        *value = distance.into();
        *operation = 0;
        *auto_op = false;
    }
    let commands = crate::dialogs::apply_commands(&harness.app, harness.app.dialog.as_ref().unwrap()).unwrap();
    StraightPreview::default().compute(&harness.app, &commands, Colors::from_tokens(&crate::theme::Tokens::get()))
}

#[test]
fn the_fast_preview_runs_for_a_distance_with_a_unit() {
    let Some(Ok(built)) = inch_box_preview("1 in") else { panic!("no fast preview for 1 in") };
    let volume = super::tests::mesh_volume(&built.scene);
    assert!((volume - 12.0 * 25.4).abs() < 1e-3, "volume {volume}");
}

#[test]
fn the_fast_preview_reads_a_bare_number_in_the_design_unit() {
    let Some(Ok(built)) = inch_box_preview("2") else { panic!("no fast preview for a bare 2") };
    let volume = super::tests::mesh_volume(&built.scene);
    assert!((volume - 12.0 * 50.8).abs() < 1e-3, "volume {volume}: 2 in, as the engine builds it");
}

#[test]
fn a_unitless_expression_in_an_inch_design_uses_the_engine_preview() {
    assert!(inch_box_preview("1+1").is_none());
}
