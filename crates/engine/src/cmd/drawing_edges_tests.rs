#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Edge identity through the hidden-line removal (Task 5): every projected line names the
//! B-rep edge or the silhouette it came from.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use solidcraft_doc::naming;
use solidcraft_geom::{Vec2, Vec3};
use solidcraft_render::{Camera, StandardView};

use crate::Session;
use crate::cmd::view_dxf::meshes::{self, Keep};
use crate::cmd::view_dxf::projection::{self, EdgeKind};
use crate::drawing::edges::{EdgeSource, SourcedLine, dihedral_tangent, sourced_lines};

const TOLERANCE: f64 = 0.01;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// The drawing tests' block (`drawing_fixture::block_part`'s commands), kept in the session.
fn block() -> Session {
    let mut s = Session::default();
    run(&mut s, "document.new_part", json!({}));
    run(&mut s, "document.units", json!({"units": "in"}));
    run(
        &mut s,
        "solid.box",
        json!({"length": "4 in", "width": "1 in", "height": "3 in", "corner": [-50.8, 0, 0], "name": "Block", "body_name": "Block"}),
    );
    run(&mut s, "solid.hole", json!({"position": [0, 0, 38.1], "direction": [0, 1, 0], "diameter": "0.75 in", "name": "Drill"}));
    run(&mut s, "solid.fillet", json!({"edges": [[-50.8, 12.7, 0], [50.8, 12.7, 0], [-50.8, 12.7, 76.2], [50.8, 12.7, 76.2]], "radius": "0.5 in"}));
    s
}

/// An orthographic standard view centred on `target`.
fn camera(view: StandardView, target: Vec3) -> Camera {
    let mut c = Camera { target, fov: 0., distance: 500., ..Default::default() };
    c.set_view(view);
    c
}
/// The block's front view, drawn in model x (right) and z (up), in mm.
fn front() -> Camera {
    camera(StandardView::Front, Vec3::new(0., 12.7, 0.))
}
/// The block's top view, drawn in model x (right) and y (up), in mm.
fn top() -> Camera {
    camera(StandardView::Top, Vec3::new(0., 0., 38.1))
}

fn ids(lines: &[SourcedLine]) -> BTreeSet<String> {
    lines.iter().map(|l| l.source.model_edge_id()).collect()
}

fn through(l: &SourcedLine, p: Vec2, tol: f64) -> bool {
    let d = l.b - l.a;
    let t = ((p - l.a).dot(d) / d.len2()).clamp(0., 1.);
    (l.a + d * t).dist(p) <= tol
}

fn block_names(s: &Session) -> (Vec<String>, Vec<String>) {
    let state = s.model.state();
    let body = state.body("Block").unwrap();
    (naming::edge_names(body), naming::face_names(body).to_vec())
}

#[test]
fn every_block_line_names_its_edge() {
    let s = block();
    let lines = sourced_lines(&s, &front(), TOLERANCE).unwrap();
    let (edges, faces) = block_names(&s);
    assert!(!lines.is_empty());
    for l in &lines {
        let id = l.source.model_edge_id();
        assert!(id.starts_with("0/Block/"), "{id}");
        match &l.source {
            EdgeSource::Brep { component_path, body_name, edge_name } => {
                assert!(component_path.is_empty() && body_name == "Block");
                let i = l.mesh_edge.unwrap() as usize;
                assert_eq!(edges.get(i), Some(edge_name), "{id}");
                assert_eq!(id, format!("0/Block/e/{edge_name}"));
            }
            EdgeSource::Silhouette { component_path, body_name, face_name, k } => {
                assert!(component_path.is_empty() && body_name == "Block");
                assert!(l.mesh_edge.is_none() && !l.smooth, "{id}");
                assert!(faces.contains(face_name), "{id}");
                assert_eq!(id, format!("0/Block/s/{face_name}/{k}"));
            }
        }
    }
    // From the front, the B-rep lines are the front face's ten edges, all visible, one line per
    // straight edge and one per chord of a curved one (the back face's edges are merged away).
    let mut breps: BTreeMap<String, usize> = BTreeMap::new();
    for l in lines.iter().filter(|l| matches!(l.source, EdgeSource::Brep { .. })) {
        assert!(!l.hidden && !l.smooth, "{l:?}");
        *breps.entry(l.source.model_edge_id()).or_default() += 1;
    }
    let expected: BTreeMap<String, usize> = [
        ("F1:+x|F1:-y", 1),
        ("F1:+z|F1:-y", 1),
        ("F1:-x|F1:-y", 1),
        ("F1:-y|F1:-z", 1),
        ("F1:-y|F2:bore:side:0", 54),
        ("F1:-y|F2:bore:side:1", 54),
        ("F1:-y|F3:blend:F1:+x|F1:+z", 32),
        ("F1:-y|F3:blend:F1:+x|F1:-z", 32),
        ("F1:-y|F3:blend:F1:+z|F1:-x", 32),
        ("F1:-y|F3:blend:F1:-x|F1:-z", 32),
    ]
    .into_iter()
    .map(|(name, n)| (format!("0/Block/e/{name}"), n))
    .collect();
    assert_eq!(breps, expected);
    // The silhouettes are those of the curved faces seen edge on: the bore (its two halves are
    // one surface) and the four blends.
    let surfaces: BTreeSet<_> =
        lines.iter().filter_map(|l| if let EdgeSource::Silhouette { face_name, .. } = &l.source { Some(face_name.as_str()) } else { None }).collect();
    assert_eq!(
        surfaces,
        BTreeSet::from(["F2:bore:side:0", "F3:blend:F1:+x|F1:+z", "F3:blend:F1:+x|F1:-z", "F3:blend:F1:+z|F1:-x", "F3:blend:F1:-x|F1:-z"])
    );
    // The hole's front rim: a circle of radius 9.525 mm about (0, 38.1). Its rightmost point is
    // on visible chords of one B-rep edge only (the back rim behind it is merged away).
    let rim = Vec2::new(9.525, 38.1);
    let at_rim: Vec<_> = lines.iter().filter(|l| through(l, rim, 0.02)).collect();
    assert!(!at_rim.is_empty());
    assert!(at_rim.iter().all(|l| !l.hidden && matches!(l.source, EdgeSource::Brep { .. })), "{at_rim:?}");
    let rim_ids: BTreeSet<_> = at_rim.iter().map(|l| l.source.model_edge_id()).collect();
    let rim_edges: BTreeSet<_> = at_rim.iter().map(|l| l.mesh_edge).collect();
    assert_eq!((rim_ids.len(), rim_edges.len()), (1, 1), "{rim_ids:?}");
    assert_eq!(rim_ids.first().unwrap(), "0/Block/e/F1:-y|F2:bore:side:1");
    assert_eq!(edges.iter().position(|e| e == "F1:-y|F2:bore:side:1").map(|i| i as u32), *rim_edges.first().unwrap());
    // Every line of that edge lies on the rim circle.
    let rim_id = rim_ids.first().unwrap();
    for l in lines.iter().filter(|l| &l.source.model_edge_id() == rim_id) {
        for p in [l.a, l.b] {
            assert!(((p - Vec2::new(0., 38.1)).len() - 9.525).abs() < 0.02, "{p:?}");
        }
    }
}

#[test]
fn cylinder_silhouettes_are_numbered_per_face() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"diameter": 20, "height": 40}));
    let lines = sourced_lines(&s, &camera(StandardView::Front, Vec3::new(0., 0., 20.)), TOLERANCE).unwrap();
    let silhouettes: BTreeMap<EdgeSource, Vec<&SourcedLine>> =
        lines.iter().filter(|l| matches!(l.source, EdgeSource::Silhouette { .. })).fold(BTreeMap::new(), |mut m, l| {
            m.entry(l.source.clone()).or_insert_with(Vec::new).push(l);
            m
        });
    assert_eq!(silhouettes.len(), 2, "{silhouettes:?}");
    let faces: BTreeSet<_> =
        silhouettes.keys().filter_map(|k| if let EdgeSource::Silhouette { face_name, .. } = k { Some(face_name.clone()) } else { None }).collect();
    // The kernel splits the side into two halves joined across hidden seams: one surface.
    assert_eq!(faces, BTreeSet::from(["F1:side#1".to_string()]));
    let x = |k: u32| {
        let (_, lines) = silhouettes.iter().find(|(s, _)| matches!(s, EdgeSource::Silhouette { k: n, .. } if *n == k)).unwrap();
        assert_eq!(lines.len(), 1);
        let l = lines[0];
        assert!(!l.hidden && (l.a.y - l.b.y).abs() > 39.9, "{l:?}");
        (l.a.x + l.b.x) / 2.
    };
    let (left, right) = (x(0), x(1));
    assert!(left < right && (left + 10.).abs() < 0.1 && (right - 10.).abs() < 0.1, "{left} {right}");
}

#[test]
fn edge_ids_survive_a_recompute() {
    let mut s = block();
    for cam in [front(), top()] {
        let before = ids(&sourced_lines(&s, &cam, TOLERANCE).unwrap());
        run(&mut s, "timeline.compute_all", json!({}));
        let after = ids(&sourced_lines(&s, &cam, TOLERANCE).unwrap());
        assert!(before.len() > 4);
        assert_eq!(before, after);
    }
}

#[test]
fn block_top_view_hides_the_bottom_and_back_edges() {
    let s = block();
    let lines = sourced_lines(&s, &top(), TOLERANCE).unwrap();
    let hidden = |brep: bool| -> BTreeMap<String, usize> {
        lines.iter().filter(|l| l.hidden && matches!(l.source, EdgeSource::Brep { .. }) == brep).fold(BTreeMap::new(), |mut m, l| {
            *m.entry(l.source.model_edge_id()).or_default() += 1;
            m
        })
    };
    let expected: BTreeMap<String, usize> = [
        ("F1:+y|F1:-z", 1),
        ("F1:+y|F2:bore:side:0", 54),
        ("F1:+y|F2:bore:side:1", 54),
        ("F1:+y|F3:blend:F1:+x|F1:-z", 32),
        ("F1:+y|F3:blend:F1:-x|F1:-z", 32),
        ("F1:-z|F3:blend:F1:+x|F1:-z", 1),
        ("F1:-z|F3:blend:F1:-x|F1:-z", 1),
    ]
    .into_iter()
    .map(|(name, n)| (format!("0/Block/e/{name}"), n))
    .collect();
    assert_eq!(hidden(true), expected);
    // The bore's two silhouettes run front to back, hidden, numbered left to right. They lie on
    // the bore's two halves (two B-rep faces joined across a hidden seam), and both name the
    // whole surface after the half whose name sorts first (R49).
    let scene = meshes::prepare(&s, TOLERANCE, Keep::Identity).unwrap();
    let raw = projection::drawing(&scene, &[], &top()).unwrap();
    let (_, faces) = block_names(&s);
    let halves: BTreeSet<&str> = raw
        .lines
        .iter()
        .filter(|l| l.hidden)
        .filter_map(|l| if let EdgeKind::Silhouette { face } = l.source.kind { faces.get(face as usize).map(String::as_str) } else { None })
        .collect();
    assert_eq!(halves, BTreeSet::from(["F2:bore:side:0", "F2:bore:side:1"]));
    assert_eq!(hidden(false).into_keys().collect::<Vec<_>>(), ["0/Block/s/F2:bore:side:0/0", "0/Block/s/F2:bore:side:0/1"]);
    for (k, x) in [(0, -9.523), (1, 9.523)] {
        let l = lines.iter().find(|l| matches!(&l.source, EdgeSource::Silhouette { k: n, .. } if *n == k)).unwrap();
        assert!(l.hidden && (l.a.x - x).abs() < 1e-3 && (l.b.x - x).abs() < 1e-3 && (l.a.dist(l.b) - 25.4).abs() < 1e-3, "{l:?}");
    }
    // Every visible B-rep line of the top face is drawn, the top face's four straight edges
    // included.
    let visible: BTreeSet<_> = lines.iter().filter(|l| !l.hidden).map(|l| l.source.model_edge_id()).collect();
    for name in ["F1:+y|F1:+z", "F1:+z|F1:-y", "F1:+z|F3:blend:F1:+x|F1:+z", "F1:+z|F3:blend:F1:+z|F1:-x"] {
        assert!(visible.contains(&format!("0/Block/e/{name}")), "{name}");
    }
}

/// Two tangent cubic arcs extruded 10 mm: the kernel certifies their two straight joins
/// tangent (`Mesh::smooth_edges`).
fn tube() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.spline.control_point", json!({"points": [[10, 0], [10, 13.333], [-10, 13.333], [-10, 0]]}));
    run(&mut s, "sketch.spline.control_point", json!({"points": [[-10, 0], [-10, -6], [10, -6], [10, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10, "body_name": "Tube"}));
    s
}

#[test]
fn tangent_edges_reach_the_drawing_path_flagged() {
    // The kernel certifies only joins of ruled B-spline faces tangent and flags none of the
    // block's (R50); the dihedral rule (R64) finds its eight fillet joins, where each round
    // meets its two planes with their normals equal.
    let block = block();
    let state = block.model.state();
    let body = state.body("Block").unwrap();
    assert!(body.mesh().smooth_edges.iter().all(|f| !f));
    let names = naming::edge_names(body);
    let tangent: BTreeSet<String> = dihedral_tangent(&body.mesh()).iter().zip(&names).filter(|(t, _)| **t).map(|(_, n)| n.clone()).collect();
    let mut joins = BTreeSet::new();
    for (p, q) in [("F1:+x", "F1:+z"), ("F1:+z", "F1:-x"), ("F1:+x", "F1:-z"), ("F1:-x", "F1:-z")] {
        joins.insert(format!("{p}|F3:blend:{p}|{q}"));
        joins.insert(format!("{q}|F3:blend:{p}|{q}"));
    }
    // The bore's two seams (where the kernel halves it) are tangent too; a seam is never drawn.
    let seams = ["F2:bore:side:0|F2:bore:side:1~1", "F2:bore:side:0|F2:bore:side:1~2"];
    assert_eq!(tangent, joins.iter().cloned().chain(seams.map(String::from)).collect::<BTreeSet<_>>());
    // From the top, the top face's joins with the rounds (x = -38.1 and 38.1, front to back) are
    // drawn flagged smooth; the joins with the sides lie on the outline, and are marked so (R54).
    let lines = sourced_lines(&block, &top(), TOLERANCE).unwrap();
    assert!(lines.iter().filter(|l| l.smooth).all(|l| matches!(&l.source, EdgeSource::Brep { edge_name, .. } if joins.contains(edge_name))));
    let mut inside: Vec<_> = lines.iter().filter(|l| l.smooth && !l.hidden && !l.silhouette).collect();
    inside.sort_by(|a, b| a.a.x.total_cmp(&b.a.x));
    assert_eq!(inside.len(), 2, "{inside:#?}");
    for (l, (x, name)) in inside.iter().zip([(-38.1, "F1:+z|F3:blend:F1:+z|F1:-x"), (38.1, "F1:+z|F3:blend:F1:+x|F1:+z")]) {
        assert!((l.a.x - x).abs() < 1e-6 && (l.b.x - x).abs() < 1e-6 && (l.a.dist(l.b) - 25.4).abs() < 1e-6, "{l:?}");
        assert_eq!(l.source.model_edge_id(), format!("0/Block/e/{name}"));
    }
    let s = tube();
    let state = s.model.state();
    let body = state.body("Tube").unwrap();
    let flagged: BTreeSet<u32> = body.mesh().smooth_edges.iter().enumerate().filter(|(_, f)| **f).map(|(i, _)| i as u32).collect();
    assert_eq!(flagged.len(), 2);
    // The dihedral rule agrees with the kernel on the tube: its two joins and nothing else.
    let by_dihedral: BTreeSet<u32> = dihedral_tangent(&body.mesh()).iter().enumerate().filter(|(_, f)| **f).map(|(i, _)| i as u32).collect();
    assert_eq!(by_dihedral, flagged);
    // From the front the joins are the tube's outline, x = -10 and 10, 10 mm tall.
    let cam = camera(StandardView::Front, Vec3::new(0., 0., 5.));
    let lines = sourced_lines(&s, &cam, TOLERANCE).unwrap();
    let mut smooth: Vec<_> = lines.iter().filter(|l| l.smooth).collect();
    smooth.sort_by(|a, b| a.a.x.total_cmp(&b.a.x));
    assert_eq!(smooth.len(), 2, "{smooth:?}");
    let names = naming::edge_names(body);
    for (l, x) in smooth.iter().zip([-10., 10.]) {
        assert!(!l.hidden && (l.a.x - x).abs() < 1e-6 && (l.b.x - x).abs() < 1e-6 && (l.a.dist(l.b) - 10.).abs() < 1e-6, "{l:?}");
        let i = l.mesh_edge.unwrap();
        assert!(flagged.contains(&i));
        assert!(matches!(&l.source, EdgeSource::Brep { edge_name, .. } if Some(edge_name) == names.get(i as usize)), "{l:?}");
        assert!(l.source.model_edge_id().starts_with("0/Tube/e/F2:side:s1|F2:side:s2~"), "{l:?}");
    }
    // An export never draws them: there the outline is the curved faces' silhouettes.
    let scene = meshes::prepare(&s, TOLERANCE, Keep::Export).unwrap();
    let export = projection::drawing(&scene, &[], &cam).unwrap();
    let exported: BTreeSet<u32> = export.lines.iter().filter_map(|l| if let EdgeKind::Brep(i) = l.source.kind { Some(i) } else { None }).collect();
    assert!(!exported.is_empty() && exported.is_disjoint(&flagged), "{exported:?}");
    for x in [-10., 10.] {
        assert!(
            export
                .lines
                .iter()
                .any(|l| matches!(l.source.kind, EdgeKind::Silhouette { .. }) && (l.a.x - x).abs() < 1e-6 && (l.a.dist(l.b) - 10.).abs() < 1e-6)
        );
    }
}

/// R53: an occluder that splits one silhouette into pieces does not renumber the silhouettes:
/// `k` ranks whole silhouettes (a surface's collinear end-to-end pieces, hidden or not).
#[test]
fn silhouette_ids_survive_an_occluder() {
    let cam = camera(StandardView::Front, Vec3::new(0., 0., 20.));
    let silhouettes = |s: &Session| -> BTreeMap<String, Vec<SourcedLine>> {
        sourced_lines(s, &cam, TOLERANCE).unwrap().into_iter().filter(|l| matches!(l.source, EdgeSource::Silhouette { .. })).fold(
            BTreeMap::new(),
            |mut m, l| {
                m.entry(l.source.model_edge_id()).or_insert_with(Vec::new).push(l);
                m
            },
        )
    };
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"diameter": 20, "height": 40}));
    let before = silhouettes(&s);
    let ids = ["0/Body1/s/F1:side#1/0", "0/Body1/s/F1:side#1/1"];
    assert_eq!(before.keys().map(String::as_str).collect::<Vec<_>>(), ids);
    // A box in front of the left silhouette's middle (x -15..-5, z 15..25) hides that part.
    run(&mut s, "solid.box", json!({"length": 10, "width": 5, "height": 10, "corner": [-15, -30, 15], "body_name": "Cover"}));
    let after = silhouettes(&s);
    assert_eq!(after.keys().map(String::as_str).collect::<Vec<_>>(), ids, "{after:?}");
    let left = &after[ids[0]];
    assert_eq!(left.len(), 3, "{left:?}");
    assert_eq!(left.iter().filter(|l| l.hidden).count(), 1, "{left:?}");
    assert!(left.iter().all(|l| (l.a.x + 10.).abs() < 0.1 && (l.b.x + 10.).abs() < 0.1), "{left:?}");
    let right = &after[ids[1]];
    assert!(right.len() == 1 && !right[0].hidden && (right[0].a.x - 10.).abs() < 0.1, "{right:?}");
}
