#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! `joint.reach` through the joints constraints compile to (R157), on the blocks fixture: B
//! Mated on grounded A's top slides and turns in that plane to a point, its constraint's state
//! keeps the pose, and the Mate stays met.

use serde_json::json;
use solidcraft_doc::constraints::ConstraintStatus;
use solidcraft_geom::Vec3;

use crate::constraints::report;
use crate::test_support::{at, blocks, occ, run};

const TOL: f64 = 1e-6;

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}

#[test]
fn a_part_mated_on_a_plane_reaches_through_its_constraint() {
    let mut s = blocks();
    let p = json!({"type": "mate", "a": {"occurrence": "A", "face": at(&s, "A", v(50.0, 25.0, 20.0))},
        "b": {"occurrence": "B", "face": at(&s, "B", v(50.0, 25.0, 40.0))}, "offset": 0});
    run(&mut s, "constraint.create", p);
    let state = |s: &crate::Session| s.doc.assembly.constraints[0].state.clone();
    let before = state(&s);
    // B's bottom corner (component point (0, 0, 40)) to (30, -20, 20) on A's top plane.
    let from = at(&s, "B", v(0.0, 0.0, 40.0));
    assert!((from.z - 20.0).abs() <= TOL, "{from:?}");
    let to = v(30.0, -20.0, 20.0);
    let b = occ(&s, "B");
    let r = run(&mut s, "joint.reach", json!({"occurrence": b, "at": [from.x, from.y, from.z], "point": [to.x, to.y, to.z]}));
    assert_eq!(r["reached"], json!(true), "{r}");
    assert!(at(&s, "B", v(0.0, 0.0, 40.0)).dist(to) <= 1e-4, "{r}");
    assert_ne!(state(&s), before, "the Mate's state holds the new pose");
    assert_eq!(report(&s).rows[0].status, ConstraintStatus::Ok);
    // Still on A's top, and a re-solve keeps it there.
    run(&mut s, "joint.solve", json!({}));
    assert!(at(&s, "B", v(0.0, 0.0, 40.0)).dist(to) <= 1e-4);
    assert!((at(&s, "B", v(100.0, 50.0, 40.0)).z - 20.0).abs() <= TOL);
    // Undo puts B back.
    run(&mut s, "edit.undo", json!({}));
    assert!(at(&s, "B", v(0.0, 0.0, 40.0)).dist(from) <= TOL);
}
