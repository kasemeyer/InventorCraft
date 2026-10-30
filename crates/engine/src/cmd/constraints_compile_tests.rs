#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Mate, Tangent and Insert compile to joints in every solve (Plan E-1 Task 3, spec §3.4, §3.6)
//! on the blocks fixture. Positions are world, to 1e-9 mm unless stated (a circular edge's centre
//! comes from its mesh polyline: 1e-6 mm, as in the pick tests).

use serde_json::json;
use solidcraft_doc::constraints::geo::{PickWhat, Prim, resolve_world};
use solidcraft_doc::constraints::{COMPILED_BASE, Constraint, GeoRef, compiled_id, geo_at};
use solidcraft_doc::joints::{self, occurrence_world};
use solidcraft_doc::{Mat, apply_point};
use solidcraft_geom::Vec3;

use crate::Session;
use crate::test_support::{add, blocks, occ, run};

const TOL: f64 = 1e-9;
const RIM: f64 = 1e-6;

const FACE: PickWhat = PickWhat::Face;
const EDGE: PickWhat = PickWhat::Edge;
const VERTEX: PickWhat = PickWhat::Vertex;

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}

/// A world pick of `name` at `p` resolved in the world as the design is now.
fn world(s: &Session, name: &str, what: PickWhat, p: Vec3) -> Prim {
    let st = s.model.state();
    let g = geo_at(&s.doc, &st, occ(s, name), what, p).unwrap_or_else(|e| panic!("{name} {what:?} at {p:?}: {e}"));
    resolve_world(&s.doc, &st, &g).unwrap()
}

/// A stored pick resolved in the world as the design is now (it follows its occurrence).
fn now(s: &Session, g: &GeoRef) -> Prim {
    resolve_world(&s.doc, &s.model.state(), g).unwrap()
}

fn plane(p: Prim) -> (Vec3, Vec3) {
    p.plane().unwrap_or_else(|| panic!("{p:?} is not a plane"))
}

fn line(p: Prim) -> (Vec3, Vec3) {
    p.line().unwrap_or_else(|| panic!("{p:?} is not a line"))
}

/// Where occurrence `name` puts its component point `p` now.
fn placed(s: &Session, name: &str, p: Vec3) -> Vec3 {
    apply_point(&occurrence_world(&s.doc, occ(s, name)), p)
}

fn transform(s: &Session, name: &str) -> Mat {
    occurrence_world(&s.doc, occ(s, name))
}

fn constraint(s: &Session, id: u64) -> Constraint {
    s.doc.assembly.constraints.iter().find(|c| c.id == id).cloned().unwrap()
}

fn solution(s: &Session) -> joints::Solution {
    joints::solve(&s.doc, &s.model.state())
}

fn dof(s: &Session, name: &str) -> usize {
    solution(s).dof[&occ(s, name)]
}

/// Distance between two parallel lines.
fn apart((p, d): (Vec3, Vec3), (q, _): (Vec3, Vec3)) -> f64 {
    (q - p).cross(d).len()
}

fn ground(s: &mut Session, name: &str) {
    let o = occ(s, name);
    run(s, "occurrence.ground", json!({"occurrence": o, "grounded": true}));
}

/// The Mate of test 1: A's top on B's bottom.
fn mate_a_b(s: &mut Session, offset: f64) -> u64 {
    add(s, "mate", "mate", &[("A", FACE, [50.0, 25.0, 20.0]), ("B", FACE, [50.0, 25.0, 40.0])], offset, false)
}

#[test]
fn mate_planes_coincide() {
    for (offset, z) in [(0.0, 20.0), (5.0, 25.0)] {
        let mut s = blocks();
        let id = mate_a_b(&mut s, offset);
        let c = constraint(&s, id);
        let (p, n) = plane(now(&s, &c.picks[1]));
        let (_, m) = plane(now(&s, &c.picks[0]));
        assert!((p.z - z).abs() <= TOL, "offset {offset}: B's bottom at {p:?}");
        assert!((n.dot(m) + 1.0).abs() <= 1e-12, "normals {n:?} {m:?}");
        let sol = solution(&s);
        assert_eq!(sol.dof[&occ(&s, "B")], 3);
        assert_eq!(sol.conflicts, vec![]);
    }
}

#[test]
fn flush_planes_face_the_same_way() {
    let mut s = blocks();
    let id = add(&mut s, "mate", "flush", &[("A", FACE, [50.0, 25.0, 20.0]), ("B", FACE, [50.0, 25.0, 60.0])], 0.0, false);
    let c = constraint(&s, id);
    let (p, n) = plane(now(&s, &c.picks[1]));
    let (_, m) = plane(now(&s, &c.picks[0]));
    assert!((p.z - 20.0).abs() <= TOL, "B's top at {p:?}");
    assert!((n.dot(m) - 1.0).abs() <= 1e-12, "normals {n:?} {m:?}");
}

#[test]
fn mate_axes_opposed_aligned_undirected() {
    // (solution, the pin turned first about y by, the axes' dot product after)
    for (solution, turn, dot) in
        [("opposed", None, -1.0), ("aligned", None, 1.0), ("undirected", Some("170 deg"), -1.0), ("undirected", Some("10 deg"), 1.0)]
    {
        let mut s = blocks();
        ground(&mut s, "Drum");
        if let Some(angle) = turn {
            let pin = occ(&s, "Pin");
            run(&mut s, "occurrence.move", json!({"occurrence": pin, "axis": [0, 1, 0], "angle": angle}));
        }
        // The pin wall where the turn left it.
        let wall = placed(&s, "Pin", v(310.0, 0.0, 80.0));
        let id = add(&mut s, "mate", solution, &[("Drum", FACE, [415.0, 0.0, 20.0]), ("Pin", FACE, [wall.x, wall.y, wall.z])], 0.0, false);
        let c = constraint(&s, id);
        let (drum, pin) = (line(now(&s, &c.picks[0])), line(now(&s, &c.picks[1])));
        assert!(apart(drum, pin) <= TOL, "{solution} {turn:?}: axes {drum:?} {pin:?}");
        assert!((drum.1.dot(pin.1) - dot).abs() <= 1e-12, "{solution} {turn:?}: dot {}", drum.1.dot(pin.1));
        assert_eq!(dof(&s, "Pin"), 2, "{solution} {turn:?}");
    }
}

#[test]
fn mate_point_and_line_pairs_leave_the_counted_freedom() {
    let corner = ("B", VERTEX, [0.0, 0.0, 40.0]);
    // (second pick, grounded drum, the dof B keeps)
    let cases =
        [(("A", FACE, [50.0, 25.0, 20.0]), false, 5), (("Drum", FACE, [415.0, 0.0, 20.0]), true, 4), (("A", VERTEX, [0.0, 0.0, 20.0]), false, 3)];
    for (other, drum, want) in cases {
        let mut s = blocks();
        if drum {
            ground(&mut s, "Drum");
        }
        add(&mut s, "mate", "mate", &[other, corner], 0.0, false);
        assert_eq!(dof(&s, "B"), want, "{other:?}");
        assert_eq!(solution(&s).conflicts, vec![], "{other:?}");
    }
    let mut s = blocks();
    add(&mut s, "mate", "mate", &[("A", FACE, [50.0, 25.0, 20.0]), ("B", EDGE, [50.0, 0.0, 40.0])], 0.0, false);
    assert_eq!(dof(&s, "B"), 4);
    // The edge lies on A's top.
    let (p, d) = line(world(&s, "B", EDGE, placed(&s, "B", v(50.0, 0.0, 40.0))));
    assert!((p.z - 20.0).abs() <= TOL && d.z.abs() <= 1e-12, "B's edge {p:?} {d:?}");
}

#[test]
fn tangent_follows_the_radius() {
    let top = ("A", FACE, [50.0, 25.0, 20.0]);
    for (solution, z) in [("outside", 30.0), ("inside", 10.0)] {
        let mut s = blocks();
        let id = add(&mut s, "tangent", solution, &[top, ("Pin", FACE, [310.0, 0.0, 80.0])], 0.0, false);
        let (p, d) = line(now(&s, &constraint(&s, id).picks[1]));
        assert!(d.z.abs() <= 1e-12 && (p.z - z).abs() <= TOL, "{solution}: pin axis {p:?} {d:?}");
    }
    let mut s = blocks();
    let id = add(&mut s, "tangent", "outside", &[top, ("Ball", FACE, [515.0, 0.0, 50.0])], 0.0, false);
    let c = now(&s, &constraint(&s, id).picks[1]).point().unwrap();
    assert!((c.z - 35.0).abs() <= TOL, "ball centre {c:?}");

    let mut s = blocks();
    let id = add(&mut s, "tangent", "outside", &[top, ("Pin", FACE, [310.0, 0.0, 80.0])], 0.0, false);
    run(&mut s, "timeline.edit", json!({"feature": "PinBody", "set": {"radius": "12 mm"}}));
    let (p, _) = line(now(&s, &constraint(&s, id).picks[1]));
    assert!((p.z - 32.0).abs() <= TOL, "pin axis after the radius edit {p:?}");
}

#[test]
fn insert_pin_in_hole() {
    let rims = [("Plate", EDGE, [200.0, 0.0, 10.0]), ("Pin", EDGE, [310.0, 0.0, 50.0])];
    // (solution, offset, the pin's bottom centre z, its top centre z)
    for (solution, offset, bottom, top) in [("opposed", 0.0, 10.0, 70.0), ("aligned", 0.0, 10.0, -50.0), ("opposed", 3.0, 13.0, 73.0)] {
        let mut s = blocks();
        add(&mut s, "insert", solution, &rims, offset, false);
        let (b, t) = (placed(&s, "Pin", v(300.0, 0.0, 50.0)), placed(&s, "Pin", v(300.0, 0.0, 110.0)));
        assert!(b.dist(v(190.0, 0.0, bottom)) <= RIM, "{solution} {offset}: bottom centre {b:?}");
        assert!(t.dist(v(190.0, 0.0, top)) <= RIM, "{solution} {offset}: top centre {t:?}");
        assert_eq!(dof(&s, "Pin"), 1, "{solution} {offset}");
    }
    let mut free = blocks();
    add(&mut free, "insert", "opposed", &rims, 0.0, false);
    let mut locked = blocks();
    let id = add(&mut locked, "insert", "opposed", &rims, 0.0, true);
    assert_eq!(dof(&locked, "Pin"), 0);
    let (a, b) = (transform(&free, "Pin"), transform(&locked, "Pin"));
    assert!(a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() <= TOL), "the lock turned the pin: {a:?} {b:?}");
    assert_eq!(constraint(&locked, id).state.len(), 1, "a locked Insert keeps its turn");
}

#[test]
fn three_mates_fix_a_block_in_any_order() {
    // (solution, A's face, B's face in B's own coordinates: picked where B is when it is added)
    let mates = [
        ("mate", v(50.0, 25.0, 20.0), v(50.0, 25.0, 40.0)),
        ("flush", v(0.0, 25.0, 10.0), v(0.0, 25.0, 50.0)),
        ("flush", v(50.0, 0.0, 10.0), v(50.0, 0.0, 50.0)),
    ];
    let orders = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]];
    let mut first: Option<Mat> = None;
    for order in orders {
        let mut s = blocks();
        let b = occ(&s, "B");
        run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": [7, -3, 5]}));
        for k in order {
            let (solution, a, b) = mates[k];
            let b = placed(&s, "B", b);
            add(&mut s, "mate", solution, &[("A", FACE, [a.x, a.y, a.z]), ("B", FACE, [b.x, b.y, b.z])], 0.0, false);
        }
        let corner = placed(&s, "B", v(0.0, 0.0, 40.0));
        assert!(corner.dist(v(0.0, 0.0, 20.0)) <= TOL, "{order:?}: B's corner {corner:?}");
        assert_eq!(dof(&s, "B"), 0, "{order:?}");
        assert_eq!(solution(&s).conflicts, vec![], "{order:?}");
        let t = transform(&s, "B");
        match &first {
            None => first = Some(t),
            Some(f) => assert!(f.iter().flatten().zip(t.iter().flatten()).all(|(x, y)| (x - y).abs() <= TOL), "{order:?}: {t:?} vs {f:?}"),
        }
    }
}

#[test]
fn a_new_mate_moves_b_least() {
    let mut s = blocks();
    let b = occ(&s, "B");
    run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": [30, 10, -19.5]}));
    let before = placed(&s, "B", v(0.0, 0.0, 40.0));
    add(&mut s, "mate", "mate", &[("A", FACE, [50.0, 25.0, 20.0]), ("B", FACE, [80.0, 35.0, 20.5])], 0.0, false);
    let after = placed(&s, "B", v(0.0, 0.0, 40.0));
    assert!(after.dist(v(30.0, 10.0, 20.0)) <= TOL, "B's corner {after:?}");
    assert!(after.dist(before) <= 0.5 + TOL, "moved {}", after.dist(before));
}

#[test]
fn the_free_side_moves_when_the_second_pick_is_grounded() {
    let mut s = blocks();
    let a = transform(&s, "A");
    let id = add(&mut s, "mate", "mate", &[("B", FACE, [50.0, 25.0, 40.0]), ("A", FACE, [50.0, 25.0, 20.0])], 0.0, false);
    assert_eq!(transform(&s, "A"), a, "grounded A moved");
    let (p, _) = plane(now(&s, &constraint(&s, id).picks[0]));
    assert!((p.z - 20.0).abs() <= TOL, "B's bottom at {p:?}");
}

#[test]
fn dragging_a_constrained_part_is_refused() {
    let mut s = blocks();
    mate_a_b(&mut s, 0.0);
    let before = transform(&s, "B");
    let b = occ(&s, "B");
    let e = s.execute("occurrence.drag", &json!({"occurrence": b, "at": [50, 25, 30], "point": [80, 25, 30]})).unwrap_err().to_string();
    assert!(e.contains("held by Mate:1"), "{e}");
    assert_eq!(transform(&s, "B"), before);
}

#[test]
fn compiled_joints_stay_out_of_joint_lists() {
    let mut s = blocks();
    let id = mate_a_b(&mut s, 0.0);
    let list = run(&mut s, "joint.list", json!({}));
    assert_eq!(list["joints"].as_array().map(Vec::len), Some(0), "{list}");
    let solved = run(&mut s, "joint.solve", json!({}));
    let ids: Vec<u64> =
        ["conflicts", "errors"].iter().flat_map(|k| solved[k].as_array().cloned().unwrap_or_default()).filter_map(|c| c["joint"].as_u64()).collect();
    assert!(ids.iter().all(|i| *i < COMPILED_BASE), "{solved}");
    let dir = std::env::temp_dir().join(format!("tcicad-con-{}-compiled_joints_stay_out_of_joint_lists", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mate.tciasm");
    run(&mut s, "file.save", json!({"path": path.to_string_lossy(), "format": "json"}));
    let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let joints = saved.pointer("/assembly/joints").and_then(|j| j.as_array()).map_or(0, Vec::len);
    assert_eq!(joints, 0, "saved joints");
    assert_eq!(constraint(&s, id).state.len(), 3);
}

#[test]
fn a_wrong_length_state_reseeds_without_a_jump() {
    let mut made = blocks();
    let id = mate_a_b(&mut made, 0.0);
    let record = constraint(&made, id);
    for state in [vec![1.0, 2.0], vec![f64::NAN, 0.0, 0.0]] {
        let mut s = blocks();
        let before = placed(&s, "B", v(0.0, 0.0, 40.0));
        s.doc_mut().assembly.constraints.push(Constraint { state: state.clone(), ..record.clone() });
        crate::cmd::joints::resolve(&mut s);
        let (p, _) = plane(now(&s, &record.picks[1]));
        assert!((p.z - 20.0).abs() <= TOL, "{state:?}: B's bottom at {p:?}");
        let after = placed(&s, "B", v(0.0, 0.0, 40.0));
        assert!(after.dist(before) <= 20.0 + TOL, "{state:?}: B jumped {}", after.dist(before));
        let kept = constraint(&s, id).state;
        assert!(kept.len() == 3 && kept.iter().all(|x| x.is_finite()), "{state:?}: state {kept:?}");
    }
    // A saved NaN (`null`) re-seeds on open (spec §8.4, R112).
    let dir = std::env::temp_dir().join(format!("tcicad-con-{}-a_wrong_length_state_reseeds_without_a_jump", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mate.tciasm");
    run(&mut made, "file.save", json!({"path": path.to_string_lossy(), "format": "json"}));
    let mut file: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    *file.pointer_mut("/assembly/constraints/0/state").unwrap() = json!([null, 0, 0]);
    std::fs::write(&path, file.to_string()).unwrap();
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path.to_string_lossy()}));
    let _ = std::fs::remove_dir_all(&dir);
    let kept = constraint(&s, id).state;
    assert!(kept.len() == 3 && kept.iter().all(|x| x.is_finite()), "reopened state {kept:?}");
    let (p, _) = plane(now(&s, &record.picks[1]));
    assert!((p.z - 20.0).abs() <= TOL, "reopened: B's bottom at {p:?}");
}

#[test]
fn a_constraints_only_assembly_resolves_after_edits() {
    let mut s = blocks();
    assert!(!s.doc.assembly.solves(), "an assembly with neither joints nor constraints");
    let id = add(&mut s, "tangent", "outside", &[("A", FACE, [50.0, 25.0, 20.0]), ("Pin", FACE, [310.0, 0.0, 80.0])], 0.0, false);
    assert!(s.doc.assembly.joints.is_empty() && s.doc.assembly.solves());
    // A part edit: `Session::refresh` alone re-solves.
    run(&mut s, "timeline.edit", json!({"feature": "PinBody", "set": {"radius": "12 mm"}}));
    let (p, _) = line(now(&s, &constraint(&s, id).picks[1]));
    assert!((p.z - 32.0).abs() <= TOL, "after the radius edit {p:?}");
    // A parameter the offset names.
    run(&mut s, "parameters.add", json!({"name": "gap", "expression": "5 mm"}));
    if let Some(c) = s.doc_mut().assembly.constraints.iter_mut().find(|c| c.id == id) {
        c.exprs.offset = Some("gap".into());
    }
    crate::cmd::joints::resolve(&mut s);
    let (p, _) = line(now(&s, &constraint(&s, id).picks[1]));
    assert!((p.z - 37.0).abs() <= TOL, "at gap 5 {p:?}");
    run(&mut s, "parameters.change", json!({"name": "gap", "expression": "8 mm"}));
    let (p, _) = line(now(&s, &constraint(&s, id).picks[1]));
    assert!((p.z - 40.0).abs() <= TOL, "at gap 8 {p:?}");
}

#[test]
fn seeded_tree_values_reach_state() {
    let mut s = blocks();
    let id = mate_a_b(&mut s, 0.0);
    let sol = solution(&s);
    assert!(sol.groups.is_empty(), "no loop: the Mate is a tree member");
    let state = constraint(&s, id).state;
    assert_eq!(state.len(), 3);
    assert_eq!(sol.compiled.get(&compiled_id(id, 0)), Some(&state));
}

// Fix round 1 (R132): a state is reused only for the compile it was solved for.

/// The pose a constraint of `solution` re-seeded from scratch gives: the constraint deleted and
/// made again with the same picks from where the parts are now.
fn reseeded(mut s: Session, id: u64, solution: &str, name: &str) -> Mat {
    let mut c = constraint(&s, id);
    s.doc_mut().assembly.constraints.retain(|x| x.id != id);
    c.solution = solidcraft_doc::constraints::ConstraintSolution::parse(solution).unwrap();
    c.state = Vec::new();
    c.signatures = Vec::new();
    s.doc_mut().assembly.constraints.push(c);
    crate::cmd::joints::resolve(&mut s);
    transform(&s, name)
}

/// A session on a copy of `s`'s design (its model evaluated again).
fn blocks_like(s: &Session) -> Session {
    Session::new((*s.doc).clone())
}

fn same(a: &Mat, b: &Mat) -> bool {
    a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() <= TOL)
}

fn signature(s: &Session, id: u64) -> Option<solidcraft_doc::constraints::StateSignature> {
    constraint(s, id).signatures.first().copied()
}

#[test]
fn a_mate_edited_to_flush_lands_without_a_jump() {
    let mut s = blocks();
    let id = mate_a_b(&mut s, 0.0);
    assert_eq!(signature(&s, id).map(|g| (g.kind, g.flip)), Some((joints::JointKind::Planar, false)));
    let want = reseeded(blocks_like(&s), id, "flush", "B");
    run(&mut s, "constraint.edit", json!({"constraint": "Mate:1", "solution": "flush"}));
    assert!(same(&transform(&s, "B"), &want), "B is not where a fresh Flush puts it: {:?} vs {want:?}", transform(&s, "B"));
    let c = constraint(&s, id);
    let (p, n) = plane(now(&s, &c.picks[1]));
    let (_, m) = plane(now(&s, &c.picks[0]));
    // B turns over about its bottom's centre: the centre stays where the Mate put it.
    assert!(p.dist(v(50.0, 25.0, 20.0)) <= 1e-6, "B's bottom centre {p:?}");
    assert!((n.dot(m) - 1.0).abs() <= 1e-12, "normals {n:?} {m:?}");
    assert_eq!(c.signatures.first().map(|g| g.flip), Some(true), "state re-seeded for Flush");
    assert_eq!(solution(&s).conflicts, vec![]);
}

#[test]
fn an_insert_edited_to_aligned_lands_without_a_jump() {
    let mut s = blocks();
    let id = add(&mut s, "insert", "opposed", &[("Plate", EDGE, [200.0, 0.0, 10.0]), ("Pin", EDGE, [310.0, 0.0, 50.0])], 0.0, false);
    let name = constraint(&s, id).name;
    let want = reseeded(blocks_like(&s), id, "aligned", "Pin");
    run(&mut s, "constraint.edit", json!({"constraint": name, "solution": "aligned"}));
    assert!(same(&transform(&s, "Pin"), &want), "the pin is not where a fresh Aligned puts it: {:?} vs {want:?}", transform(&s, "Pin"));
    let (b, t) = (placed(&s, "Pin", v(300.0, 0.0, 50.0)), placed(&s, "Pin", v(300.0, 0.0, 110.0)));
    assert!(b.dist(v(190.0, 0.0, 10.0)) <= RIM, "bottom centre {b:?}");
    assert!(t.dist(v(190.0, 0.0, -50.0)) <= RIM, "top centre {t:?}");
    assert_eq!(signature(&s, id).map(|g| (g.kind, g.flip)), Some((joints::JointKind::Revolute, true)));
}

#[test]
fn an_undirected_sense_change_reseeds() {
    let mut s = blocks();
    ground(&mut s, "Drum");
    let id = add(&mut s, "mate", "undirected", &[("Drum", FACE, [415.0, 0.0, 20.0]), ("Pin", FACE, [310.0, 0.0, 80.0])], 0.0, false);
    assert_eq!(signature(&s, id).map(|g| g.flip), Some(true), "the axes start aligned");
    // An outside move turns the pin over (170° about y through its middle): the sense flips.
    let centre = placed(&s, "Pin", v(300.0, 0.0, 80.0));
    let pin = occ(&s, "Pin");
    run(
        &mut s,
        "occurrence.move",
        json!({"occurrence": pin, "space": "world", "axis": [0, 1, 0], "angle": "170 deg", "origin": [centre.x, centre.y, centre.z]}),
    );
    crate::cmd::joints::resolve(&mut s);
    let c = constraint(&s, id);
    let (drum, axis) = (line(now(&s, &c.picks[0])), line(now(&s, &c.picks[1])));
    assert!((drum.1.dot(axis.1) + 1.0).abs() <= 1e-12, "dot {}", drum.1.dot(axis.1));
    assert_eq!(c.signatures.first().map(|g| g.flip), Some(false), "re-seeded for the opposed sense");
    let after = placed(&s, "Pin", v(300.0, 0.0, 80.0));
    assert!(after.dist(centre) <= 1.0, "the pin jumped from {centre:?} to {after:?}");
}

#[test]
fn a_file_from_before_signatures_reseeds_once() {
    let mut made = blocks();
    let id = mate_a_b(&mut made, 0.0);
    let dir = std::env::temp_dir().join(format!("tcicad-con-{}-a_file_from_before_signatures_reseeds_once", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mate.tciasm");
    run(&mut made, "file.save", json!({"path": path.to_string_lossy(), "format": "json"}));
    let mut file: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let record = file.pointer_mut("/assembly/constraints/0").and_then(|c| c.as_object_mut()).unwrap();
    assert!(record.remove("signatures").is_some(), "the save wrote the signatures");
    std::fs::write(&path, file.to_string()).unwrap();
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path.to_string_lossy()}));
    let _ = std::fs::remove_dir_all(&dir);
    let c = constraint(&s, id);
    assert_eq!(c.signatures.first().map(|g| (g.kind, g.flip)), Some((joints::JointKind::Planar, false)), "signed on open");
    assert_eq!(c.state.len(), 3);
    let (p, _) = plane(now(&s, &c.picks[1]));
    assert!((p.z - 20.0).abs() <= TOL, "B's bottom at {p:?}");
}

/// The session's DOF memo (R130) answers as a fresh solve after a ground toggle, a part edit and
/// a constraint edit, on a loop (Mate and Flush on B).
#[test]
fn the_dof_memo_follows_grounding_part_and_constraint_edits() {
    let mut s = blocks();
    let b = occ(&s, "B");
    mate_a_b(&mut s, 0.0);
    add(&mut s, "mate", "flush", &[("A", FACE, [50.0, 0.0, 10.0]), ("B", FACE, [50.0, 0.0, 30.0])], 0.0, false);
    let check = |s: &mut Session, what: &str, want: usize| {
        let memo = crate::cmd::joints::resolve(s).dof;
        assert_eq!(memo, solution(s).dof, "{what}");
        assert_eq!(memo[&b], want, "{what}");
    };
    check(&mut s, "a loop", 1);
    run(&mut s, "occurrence.ground", json!({"occurrence": b, "grounded": true}));
    check(&mut s, "grounded", 0);
    run(&mut s, "occurrence.ground", json!({"occurrence": b, "grounded": false}));
    check(&mut s, "ungrounded", 1);
    let comp = s.doc.occurrences.iter().find(|o| o.id == b).unwrap().component;
    let feature = s.doc.features.iter().find(|f| f.component == comp && matches!(f.kind, solidcraft_doc::FeatureKind::Box { .. })).unwrap().id;
    run(&mut s, "timeline.edit", json!({"feature": feature, "set": {"height": "25 mm"}}));
    check(&mut s, "a part edit", 1);
    run(&mut s, "constraint.edit", json!({"constraint": "Mate:2", "offset": 2}));
    check(&mut s, "a constraint edit", 1);
}

/// Plan E-1 Task 6: a file written with Task 3's single `signature` reads it as a one-member
/// list: the state keeps its meaning (no re-seed, B does not move), and the next save writes the
/// list.
#[test]
fn a_file_with_one_signature_keeps_its_state() {
    let mut made = blocks();
    let id = mate_a_b(&mut made, 0.0);
    let before = transform(&made, "B");
    let dir = std::env::temp_dir().join(format!("tcicad-con-{}-a_file_with_one_signature_keeps_its_state", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mate.tciasm");
    run(&mut made, "file.save", json!({"path": path.to_string_lossy(), "format": "json"}));
    let mut file: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let record = file.pointer_mut("/assembly/constraints/0").and_then(|c| c.as_object_mut()).unwrap();
    let list = record.remove("signatures").unwrap();
    assert_eq!(list.as_array().map(Vec::len), Some(1), "{list}");
    record.insert("signature".into(), list[0].clone());
    // A state that only the reused values give: B a little along x from where the Mate left it.
    let state = record.get_mut("state").and_then(|v| v.as_array_mut()).unwrap();
    let x = state[0].as_f64().unwrap();
    state[0] = json!(x + 7.0);
    std::fs::write(&path, file.to_string()).unwrap();
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path.to_string_lossy()}));
    let c = constraint(&s, id);
    assert_eq!(c.signatures.iter().map(|g| (g.kind, g.flip)).collect::<Vec<_>>(), vec![(joints::JointKind::Planar, false)]);
    assert!((c.state[0] - (x + 7.0)).abs() <= TOL, "the state was re-seeded: {:?}", c.state);
    let moved = transform(&s, "B");
    let shift = Vec3::new(moved[3][0] - before[3][0], moved[3][1] - before[3][1], moved[3][2] - before[3][2]).len();
    assert!((shift - 7.0).abs() <= TOL, "B moved {shift} mm, not by the saved state's 7");
    run(&mut s, "file.save", json!({"path": path.to_string_lossy(), "format": "json"}));
    let saved = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(saved.contains("\"signatures\"") && !saved.contains("\"signature\""), "{saved}");
}
