#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The session's pick cache (R156) on the blocks fixture: solves read it; a part edit re-keys the
//! component, so the constraint follows the new face, and undo brings the old one back; a pick
//! that no longer resolves is still Lost; two occurrences of one component share its
//! component-space entry and get their own world primitives; the name index finds what the
//! nearest-match search finds.

use serde_json::json;
use solidcraft_doc::constraints::{ConstraintStatus, PickCache, PickWhat, Prim, geo_at, resolve};
use solidcraft_geom::Vec3;

use crate::Session;
use crate::constraints::report;
use crate::test_support::{at, blocks, occ, run};

const TOL: f64 = 1e-9;

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}

fn status(s: &Session, name: &str) -> ConstraintStatus {
    report(s).rows.iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no row {name}")).status.clone()
}

/// The z of B's bottom face (its component point (50, 25, 40)).
fn b_bottom(s: &Session) -> f64 {
    at(s, "B", v(50.0, 25.0, 40.0)).z
}

/// The first feature of component `name` (its box).
fn first_feature(s: &Session, name: &str) -> u64 {
    let c = s.doc.find_component(name).unwrap();
    s.doc.features.iter().find(|f| f.component == c).unwrap().id
}

#[test]
fn a_part_edit_moves_the_cached_pick_and_undo_restores_it() {
    let mut s = blocks();
    let p = json!({"type": "mate", "a": {"occurrence": "A", "face": at(&s, "A", v(50.0, 25.0, 20.0))},
        "b": {"occurrence": "B", "face": at(&s, "B", v(50.0, 25.0, 40.0))}, "offset": 0});
    run(&mut s, "constraint.create", p);
    assert!((b_bottom(&s) - 20.0).abs() <= TOL, "B on A's top: {}", b_bottom(&s));
    // Solves read the cache: nothing is resolved again while the parts are as they were.
    let misses = s.pick_cache.misses();
    assert!(misses > 0 && !s.pick_cache.is_empty());
    run(&mut s, "joint.solve", json!({}));
    run(&mut s, "constraint.list", json!({}));
    assert_eq!(s.pick_cache.misses(), misses, "a re-solve of the same geometry finds no pick again");

    // A taller A: its top (the picked face) moves up 10 and B follows it.
    let a_box = first_feature(&s, "A");
    run(&mut s, "timeline.edit", json!({"feature": a_box, "set": {"height": "30 mm"}}));
    assert!((b_bottom(&s) - 30.0).abs() <= TOL, "B follows A's new top: {}", b_bottom(&s));
    assert!(s.pick_cache.misses() > misses, "A's geometry changed: its pick is resolved again");
    assert_eq!(status(&s, "Mate:1"), ConstraintStatus::Ok);
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "joint.solve", json!({}));
    assert!((b_bottom(&s) - 20.0).abs() <= TOL, "undo: B back on A's old top: {}", b_bottom(&s));
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, "joint.solve", json!({}));
    assert!((b_bottom(&s) - 30.0).abs() <= TOL, "redo: B on A's new top again: {}", b_bottom(&s));
}

#[test]
fn a_lost_pick_is_still_lost_through_the_cache() {
    let mut s = blocks();
    let insert =
        json!({"type": "insert", "a": {"occurrence": "Plate", "edge": [200.0, 0.0, 10.0]}, "b": {"occurrence": "Pin", "edge": [310.0, 0.0, 50.0]}});
    run(&mut s, "constraint.create", insert);
    assert_eq!(status(&s, "Insert:1"), ConstraintStatus::Ok);
    // The plate cut short of its hole: the rim is gone, every solve after says so.
    run(&mut s, "timeline.edit", json!({"feature": "Plate", "set": {"length": "25 mm"}}));
    for _ in 0..2 {
        match status(&s, "Insert:1") {
            ConstraintStatus::Lost { reason } => assert!(reason.contains("no longer exists"), "{reason}"),
            other => panic!("Insert:1 is {other:?}"),
        }
        run(&mut s, "joint.solve", json!({}));
    }
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "joint.solve", json!({}));
    assert_eq!(status(&s, "Insert:1"), ConstraintStatus::Ok);
}

#[test]
fn two_occurrences_share_the_component_entry_with_their_own_world_prims() {
    let mut s = blocks();
    run(&mut s, "occurrence.copy", json!({"component": "B", "translate": [0, 0, 100]}));
    let comp = s.doc.find_component("B").unwrap();
    let (b1, b2) = {
        let o: Vec<u64> = s.doc.occurrences.iter().filter(|o| o.component == comp).map(|o| o.id).collect();
        assert_eq!(o.len(), 2, "two occurrences of B");
        (o[0], o[1])
    };
    assert_eq!(b1, occ(&s, "B"));
    let st = s.model.state();
    let top = |o: u64| {
        let w = solidcraft_doc::apply_point(&solidcraft_doc::joints::occurrence_world(&s.doc, o), v(50.0, 25.0, 60.0));
        geo_at(&s.doc, &st, o, PickWhat::Face, w).unwrap()
    };
    let (g1, g2) = (top(b1), top(b2));
    assert_eq!(g1.geo, g2.geo, "one face of one component");
    let cache = PickCache::default();
    let picks = cache.picks(&s.doc, &st);
    let (p1, p2) = (picks.resolve(&g1).unwrap(), picks.resolve(&g2).unwrap());
    assert_eq!(p1, p2, "the component's primitive");
    assert_eq!(p1, resolve(&s.doc, &st, &g1).unwrap(), "as the uncached resolve gives it");
    assert_eq!((cache.misses(), cache.len()), (1, 1), "one entry for both occurrences");
    let (Prim::Plane { p: w1, n: n1 }, Prim::Plane { p: w2, n: n2 }) = (picks.resolve_world(&g1).unwrap(), picks.resolve_world(&g2).unwrap()) else {
        panic!("B's top is a plane")
    };
    assert!(n1.dist(v(0.0, 0.0, 1.0)) <= TOL && n2.dist(n1) <= TOL);
    assert!((w2 - w1).dist(v(0.0, 0.0, 100.0)) <= TOL, "each at its own occurrence: {w1:?} {w2:?}");
    assert_eq!(cache.misses(), 1);
}

/// Every pick of the blocks fixture through the cache's name index is what the uncached
/// nearest-match resolve gives: faces (plane, cylinder, sphere), edges (line, circle), vertices.
#[test]
fn the_name_index_finds_what_the_search_finds() {
    let s = blocks();
    let st = s.model.state();
    let cache = PickCache::default();
    let picks = cache.picks(&s.doc, &st);
    let cases = [
        ("A", PickWhat::Face, v(50.0, 25.0, 20.0)),
        ("A", PickWhat::Edge, v(50.0, 0.0, 20.0)),
        ("A", PickWhat::Vertex, v(0.0, 0.0, 20.0)),
        ("Plate", PickWhat::Edge, v(200.0, 0.0, 10.0)),
        ("Plate", PickWhat::Face, v(200.0, 0.0, 5.0)),
        ("Pin", PickWhat::Face, v(310.0, 0.0, 80.0)),
        ("Pin", PickWhat::Edge, v(310.0, 0.0, 50.0)),
        ("Ball", PickWhat::Face, v(515.0, 0.0, 50.0)),
    ];
    for (name, what, p) in cases {
        let g = geo_at(&s.doc, &st, occ(&s, name), what, at(&s, name, p)).unwrap_or_else(|e| panic!("{name} {what:?}: {e}"));
        assert_eq!(picks.resolve(&g).unwrap(), resolve(&s.doc, &st, &g).unwrap(), "{name} {g:?}");
    }
    assert_eq!(cache.misses(), cases.len());
    // A name no face has: Lost, as the search says.
    let mut g = geo_at(&s.doc, &st, occ(&s, "A"), PickWhat::Face, at(&s, "A", v(50.0, 25.0, 20.0))).unwrap();
    if let solidcraft_doc::constraints::Geo::Face { name, .. } = &mut g.geo {
        name.push_str("-gone");
    }
    assert_eq!(picks.resolve(&g).map_err(|e| e.to_string()), resolve(&s.doc, &st, &g).map_err(|e| e.to_string()));
}
