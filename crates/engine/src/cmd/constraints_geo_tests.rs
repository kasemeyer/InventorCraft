#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Picks resolve to primitives (Plan E-1 Task 2, spec §3.3) on the blocks fixture.

use serde_json::json;
use solidcraft_doc::constraints::geo::{PickWhat, Prim, PrimKind, geo_at, resolve, resolve_world};
use solidcraft_doc::constraints::{Geo, GeoRef, OriginItem};
use solidcraft_geom::Vec3;

use crate::Session;
use crate::test_support::{at, blocks, occ, run};

const TOL: f64 = 1e-9;

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}

fn close(a: Vec3, b: Vec3, tol: f64) -> bool {
    a.dist(b) <= tol
}

/// `d` is `want` or its opposite.
fn along(d: Vec3, want: Vec3) -> bool {
    close(d, want, 1e-9) || close(d, -want, 1e-9)
}

/// Resolve the pick of `what` at world `p` on component `name`.
fn pick(s: &Session, name: &str, what: PickWhat, p: Vec3) -> (GeoRef, Prim) {
    let st = s.model.state();
    let g = geo_at(&s.doc, &st, occ(s, name), what, p).unwrap_or_else(|e| panic!("{name} {what:?} at {p:?}: {e}"));
    let prim = resolve(&s.doc, &st, &g).unwrap_or_else(|e| panic!("{name} {g:?}: {e}"));
    (g, prim)
}

#[test]
fn faces_resolve_to_planes_cylinders_and_spheres() {
    let s = blocks();
    let (g, top) = pick(&s, "A", PickWhat::Face, v(50.0, 25.0, 20.0));
    assert!(matches!(g.geo, Geo::Face { .. }), "{g:?}");
    assert_eq!(top.kind(), PrimKind::Plane);
    let Prim::Plane { p, n } = top else { panic!("{top:?}") };
    assert!(close(n, v(0.0, 0.0, 1.0), TOL), "A's top normal {n:?}");
    assert!((p.z - 20.0).abs() <= TOL, "A's top at {p:?}");

    let (_, wall) = pick(&s, "Pin", PickWhat::Face, v(310.0, 0.0, 80.0));
    let Prim::Cylinder { p, d, r } = wall else { panic!("the pin wall is {wall:?}") };
    assert!((r - 10.0).abs() <= TOL, "pin radius {r}");
    assert!(along(d, v(0.0, 0.0, 1.0)), "pin axis {d:?}");
    assert!((p.x - 300.0).abs() <= TOL && p.y.abs() <= TOL, "pin axis through {p:?}");
    assert_eq!(wall.line().map(|(_, d)| d), Some(d));

    let (_, ball) = pick(&s, "Ball", PickWhat::Face, v(515.0, 0.0, 50.0));
    let Prim::Sphere { c, r } = ball else { panic!("the ball is {ball:?}") };
    assert!(close(c, v(500.0, 0.0, 50.0), TOL), "ball centre {c:?}");
    assert!((r - 15.0).abs() <= TOL, "ball radius {r}");
    assert_eq!(ball.point(), Some(c));
}

#[test]
fn edges_resolve_to_lines_and_circles() {
    let s = blocks();
    let (g, edge) = pick(&s, "A", PickWhat::Edge, v(50.0, 0.0, 20.0));
    assert!(matches!(g.geo, Geo::Edge { .. }), "{g:?}");
    let Prim::Line { p, d } = edge else { panic!("A's edge is {edge:?}") };
    assert!(along(d, v(1.0, 0.0, 0.0)), "A's edge runs {d:?}");
    assert!(close(p, v(50.0, 0.0, 20.0), TOL), "A's edge middle {p:?}");

    let (_, rim) = pick(&s, "Plate", PickWhat::Edge, v(200.0, 0.0, 10.0));
    let Prim::Circle { c, axis, r } = rim else { panic!("the hole's rim is {rim:?}") };
    assert!(close(c, v(190.0, 0.0, 10.0), 1e-6), "rim centre {c:?}");
    assert!((r - 10.0).abs() <= 1e-6, "rim radius {r}");
    assert!(close(axis, v(0.0, 0.0, 1.0), TOL), "rim axis {axis:?} (the plate's top normal)");
    assert_eq!(rim.plane(), Some((c, axis)));
    assert_eq!(rim.point(), Some(c));

    let (_, bottom) = pick(&s, "Pin", PickWhat::Edge, v(310.0, 0.0, 50.0));
    let Prim::Circle { c, axis, r } = bottom else { panic!("the pin's bottom rim is {bottom:?}") };
    assert!(close(c, v(300.0, 0.0, 50.0), 1e-6), "pin rim centre {c:?}");
    assert!((r - 10.0).abs() <= 1e-6, "pin rim radius {r}");
    assert!(close(axis, v(0.0, 0.0, -1.0), TOL), "pin rim axis {axis:?} (its bottom face's normal)");
}

#[test]
fn vertices_and_origin_items_resolve() {
    let s = blocks();
    let (g, corner) = pick(&s, "A", PickWhat::Vertex, v(100.0, 50.0, 20.0));
    assert!(matches!(g.geo, Geo::Vertex { .. }), "{g:?}");
    assert_eq!(corner.kind(), PrimKind::Point);
    assert!(corner.point().is_some_and(|p| close(p, v(100.0, 50.0, 20.0), TOL)), "corner {corner:?}");

    let st = s.model.state();
    let origin = |o: u64, item: OriginItem| GeoRef { occurrence: o, geo: Geo::Origin { item } };
    let b = occ(&s, "B");
    let xz = resolve_world(&s.doc, &st, &origin(b, OriginItem::Xz)).unwrap();
    assert_eq!(xz, Prim::Plane { p: at(&s, "B", Vec3::ZERO), n: v(0.0, 1.0, 0.0) });
    let z = resolve(&s.doc, &st, &origin(0, OriginItem::Z)).unwrap();
    assert_eq!(z, Prim::Line { p: Vec3::ZERO, d: v(0.0, 0.0, 1.0) });
    let centre = resolve(&s.doc, &st, &origin(0, OriginItem::Center)).unwrap();
    assert_eq!(centre, Prim::Point { p: Vec3::ZERO });
    assert_eq!(resolve(&s.doc, &st, &origin(0, OriginItem::Xy)).unwrap(), Prim::Plane { p: Vec3::ZERO, n: v(0.0, 0.0, 1.0) });
    assert_eq!(resolve(&s.doc, &st, &origin(0, OriginItem::Yz)).unwrap(), Prim::Plane { p: Vec3::ZERO, n: v(1.0, 0.0, 0.0) });
    assert_eq!(resolve(&s.doc, &st, &origin(0, OriginItem::X)).unwrap(), Prim::Line { p: Vec3::ZERO, d: v(1.0, 0.0, 0.0) });
    assert_eq!(resolve(&s.doc, &st, &origin(0, OriginItem::Y)).unwrap(), Prim::Line { p: Vec3::ZERO, d: v(0.0, 1.0, 0.0) });
}

#[test]
fn a_moved_occurrence_resolves_in_the_world() {
    let mut s = blocks();
    let b = occ(&s, "B");
    run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": [0, 0, 7]}));
    let (g, _) = pick(&s, "B", PickWhat::Face, v(50.0, 25.0, 47.0));
    let st = s.model.state();
    let Prim::Plane { p, n } = resolve(&s.doc, &st, &g).unwrap() else { panic!("B's bottom is not a plane") };
    assert!((p.z - 40.0).abs() <= TOL, "B's bottom in its component at {p:?}");
    assert!(close(n, v(0.0, 0.0, -1.0), TOL), "B's bottom normal {n:?}");
    let Prim::Plane { p, n } = resolve_world(&s.doc, &st, &g).unwrap() else { panic!("B's bottom is not a plane") };
    assert!((p.z - 47.0).abs() <= TOL, "B's bottom in the world at {p:?}");
    assert!(close(n, v(0.0, 0.0, -1.0), TOL), "B's bottom world normal {n:?}");
}

#[test]
fn lost_picks_say_which() {
    let mut s = blocks();
    let a = occ(&s, "A");
    let gone = GeoRef { occurrence: a, geo: Geo::Face { name: "nope".into(), pick: v(50.0, 25.0, 20.0) } };
    let e = resolve(&s.doc, &s.model.state(), &gone).unwrap_err().to_string();
    assert!(e.contains("no longer exists") && e.contains("`nope`"), "{e}");
    let edge = GeoRef { occurrence: a, geo: Geo::Edge { name: "nope".into(), pick: v(50.0, 0.0, 20.0) } };
    let e = resolve(&s.doc, &s.model.state(), &edge).unwrap_err().to_string();
    assert!(e.contains("the edge `nope` no longer exists"), "{e}");

    run(&mut s, "component.create", json!({"name": "Ring", "grounded": false}));
    run(&mut s, "solid.torus", json!({"major": 20, "minor": 5, "center": [600, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let st = s.model.state();
    let g = geo_at(&s.doc, &st, occ(&s, "Ring"), PickWhat::Face, v(625.0, 0.0, 0.0)).unwrap();
    let e = resolve(&s.doc, &st, &g).unwrap_err().to_string();
    assert!(e.contains("not a v1 pick"), "{e}");
    assert!(e.contains("torus"), "{e}");
}

/// A face name resolves only on its own occurrence's component: A's top named on B is lost, not
/// A's plane (Task 2 review).
#[test]
fn a_face_resolves_on_its_own_occurrence_only() {
    let s = blocks();
    let (a_top, _) = pick(&s, "A", PickWhat::Face, v(50.0, 25.0, 20.0));
    let Geo::Face { name, .. } = a_top.geo else { panic!("{a_top:?}") };
    let on_b = GeoRef { occurrence: occ(&s, "B"), geo: Geo::Face { name: name.clone(), pick: v(50.0, 25.0, 20.0) } };
    let e = resolve(&s.doc, &s.model.state(), &on_b).unwrap_err().to_string();
    assert!(e.contains("no longer exists") && e.contains(&name), "{e}");
}

/// A vertex whose stored end is now the edge's other end (its polyline reversed by a rebuild)
/// resolves to the end nearer the stored pick.
#[test]
fn a_reversed_edge_keeps_the_picked_vertex() {
    let s = blocks();
    let (g, _) = pick(&s, "A", PickWhat::Vertex, v(100.0, 50.0, 20.0));
    let Geo::Vertex { edge, end, pick: at } = g.geo else { panic!("{g:?}") };
    let other = match end {
        solidcraft_doc::constraints::EdgeEnd::Start => solidcraft_doc::constraints::EdgeEnd::End,
        solidcraft_doc::constraints::EdgeEnd::End => solidcraft_doc::constraints::EdgeEnd::Start,
    };
    let reversed = GeoRef { occurrence: g.occurrence, geo: Geo::Vertex { edge, end: other, pick: at } };
    let p = resolve(&s.doc, &s.model.state(), &reversed).unwrap();
    assert!(p.point().is_some_and(|p| close(p, v(100.0, 50.0, 20.0), TOL)), "reversed end {p:?}");
}

/// Final wave item 5: a component of two bodies (a 10 mm box and a copy of it moved 30 mm along
/// x, whose names carry the copy's `@F23.1`). An edge only the second body has (the name index
/// looks at the first body's edges only) resolves through the search. A face name two bodies
/// have exactly (only a damaged state has one: here the model state with the box's body twice)
/// is not taken from the index (two hits): the pick is found by the nearest-match search. Each
/// as the uncached `resolve` finds it.
#[test]
fn a_two_body_component_resolves_through_the_search() {
    let mut s = blocks();
    run(&mut s, "component.create", json!({"name": "Twin", "grounded": true}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 300, 0]}));
    let r = run(&mut s, "solid.move", json!({"bodies": ["Body7"], "translate": [30, 0, 0], "copy": true}));
    assert_eq!(r["created_bodies"], json!(["Body7 (2)"]), "{r}");
    run(&mut s, "component.activate", json!({"component": "root"}));
    let st = s.model.state();
    let twin = occ(&s, "Twin");
    let body = |name: &str| st.bodies.iter().find(|b| b.name == name).unwrap();
    let (first, second) = (body("Body7"), body("Body7 (2)"));

    // The copy's top front edge: the second body's only.
    let g = geo_at(&s.doc, &st, twin, PickWhat::Edge, v(35.0, 300.0, 10.0)).unwrap();
    let Geo::Edge { name: edge_name, .. } = &g.geo else { panic!("{g:?}") };
    let has = |b| solidcraft_doc::naming::edge_names(b).contains(edge_name);
    assert_eq!((has(first), has(second)), (false, true), "{edge_name}");
    let cache = solidcraft_doc::constraints::PickCache::default();
    let edge = cache.picks(&s.doc, &st).resolve(&g).unwrap_or_else(|e| panic!("{g:?}: {e}"));
    assert_eq!(edge, resolve(&s.doc, &st, &g).unwrap());
    match edge {
        Prim::Line { p, d } => assert!(along(d, Vec3::X) && (p.y - 300.0).abs() <= TOL && (p.z - 10.0).abs() <= TOL, "{p:?} {d:?}"),
        other => panic!("{other:?}"),
    }

    // The box's body twice in one component: `F22:+z` is an exact name of both.
    let mut twice = (*st).clone();
    let mut again = first.clone();
    again.name = "Body7 (again)".into();
    twice.bodies.push(again);
    let tops = |b| solidcraft_doc::naming::face_names(b).iter().filter(|n| *n == "F22:+z").count();
    assert_eq!(twice.bodies.iter().filter(|b| b.component == first.component).map(tops).sum::<usize>(), 2);
    let top = GeoRef { occurrence: twin, geo: Geo::Face { name: "F22:+z".into(), pick: v(5.0, 305.0, 10.0) } };
    let found = cache.picks(&s.doc, &twice).resolve(&top).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(found, resolve(&s.doc, &twice, &top).unwrap());
    match found {
        Prim::Plane { p, n } => assert!(along(n, Vec3::Z) && (p.z - 10.0).abs() <= TOL, "{p:?} {n:?}"),
        other => panic!("{other:?}"),
    }
}
