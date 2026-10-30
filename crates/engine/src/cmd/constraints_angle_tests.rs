#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Loop equations (Plan E-1 Task 6, spec §3.5): Angle Undirected (a relation, or an equation with
//! limits), Directed and Explicit vector, Tangent between two parallel cylinders, and the
//! provisional `Free` member of a part held only by equations. Blocks fixture: A's front face is
//! y = 0 (normal -y), B's front the same 40 mm up. Angles in degrees to 1e-6, distances and normals' components to
//! 1e-9.

use serde_json::{Value, json};
use solidcraft_doc::constraints::{
    Constraint, ConstraintKind, ConstraintSolution, Geo, GeoRef, OriginItem, Prim, compile, compiled_id, resolve_world,
};
use solidcraft_doc::joints::{self, occurrence_world};
use solidcraft_doc::{Document, apply_vector};
use solidcraft_geom::Vec3;

use crate::Session;
use crate::test_support::{blocks, occ, run};

const TOL: f64 = 1e-9;
/// Angles, degrees: the loop solver closes an angle to 1e-10 rad (about 6e-9°).
const ANG: f64 = 1e-6;

fn face(name: &str, p: [f64; 3]) -> Value {
    json!({"occurrence": name, "face": p})
}

fn edge(name: &str, p: [f64; 3]) -> Value {
    json!({"occurrence": name, "edge": p})
}

/// An Angle from A's front to B's front, `solution`, `offset` degrees, with `extra` merged in.
fn fronts(solution: &str, offset: f64, extra: Value) -> Value {
    let mut p =
        json!({"type": "angle", "solution": solution, "a": face("A", [50.0, 0.0, 10.0]), "b": face("B", [50.0, 0.0, 50.0]), "offset": offset});
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    p
}

fn constraint(s: &Session, name: &str) -> Constraint {
    s.doc.assembly.constraint(name).cloned().unwrap_or_else(|| panic!("no constraint {name}"))
}

/// A stored pick's primitive in the world now.
fn now(s: &Session, g: &GeoRef) -> Prim {
    resolve_world(&s.doc, &s.model.state(), g).unwrap()
}

/// The world normals of constraint `name`'s first two picks (planes).
fn normals(s: &Session, name: &str) -> (Vec3, Vec3) {
    let c = constraint(s, name);
    (now(s, &c.picks[0]).plane().unwrap().1, now(s, &c.picks[1]).plane().unwrap().1)
}

/// The angle between two directions, degrees (from the sine and the cosine: no rounding loss).
fn deg(a: Vec3, b: Vec3) -> f64 {
    a.cross(b).len().atan2(a.dot(b)).to_degrees()
}

/// The angle from `a` to `b` about `k`, both projected square to it, degrees.
fn signed(a: Vec3, b: Vec3, k: Vec3) -> f64 {
    let k = k.normalized().unwrap();
    let (pa, pb) = (a - k * a.dot(k), b - k * b.dot(k));
    pa.cross(pb).dot(k).atan2(pa.dot(pb)).to_degrees()
}

fn dof_of(v: &Value, s: &Session, name: &str) -> u64 {
    v["dof"][occ(s, name).to_string()].as_u64().unwrap_or_else(|| panic!("no dof for {name} in {v}"))
}

/// Distance between the axes of constraint `name`'s first two picks (cylinders).
fn axes_apart(s: &Session, name: &str) -> f64 {
    let c = constraint(s, name);
    let ((p, d), (q, e)) = (now(s, &c.picks[0]).line().unwrap(), now(s, &c.picks[1]).line().unwrap());
    assert!(d.cross(e).len() <= 1e-9, "axes not parallel: {d:?} {e:?}");
    (q - p).cross(d).len()
}

/// `id p` fails with a message containing `want`; nothing changes, no undo step.
fn refused(s: &mut Session, id: &str, p: Value, want: &str) {
    let (steps, doc) = (s.undo.len(), s.doc.clone());
    let e = s.execute(id, &p).map(|v| panic!("{id} {p}: accepted: {v}")).unwrap_err().to_string();
    assert!(e.contains(want), "{id} {p}: `{e}` does not say `{want}`");
    assert_eq!(s.undo.len(), steps, "{id} {p}");
    assert!(*s.doc == *doc, "{id} {p}: the design changed");
}

#[test]
fn undirected_angle_holds_thirty_degrees() {
    let mut s = blocks();
    let r = run(&mut s, "constraint.create", fronts("undirected", 30.0, json!({})));
    assert_eq!(r["status"], "ok", "{r}");
    let (na, nb) = normals(&s, "Angle:1");
    assert!((deg(na, nb) - 30.0).abs() <= ANG, "normals {na:?} {nb:?}: {}", deg(na, nb));
    // An angle relation takes one turn away.
    assert_eq!(dof_of(&r, &s, "B"), 5);
}

#[test]
fn explicit_vector_angles_mirror() {
    let mut made = Vec::new();
    for offset in [30.0, -30.0] {
        let mut s = blocks();
        let p = fronts("explicit_vector", offset, json!({"ref": edge("A", [0.0, 0.0, 10.0])}));
        let r = run(&mut s, "constraint.create", p);
        assert_eq!(r["status"], "ok", "{r}");
        let (na, nb) = normals(&s, "Angle:1");
        assert!((deg(na, nb) - 30.0).abs() <= ANG, "{offset}: {}", deg(na, nb));
        made.push(nb);
    }
    assert!((made[0].x - 0.5).abs() <= TOL, "+30: {:?}", made[0]);
    assert!((made[1].x + 0.5).abs() <= TOL, "-30: {:?}", made[1]);
    assert!((made[0].y - made[1].y).abs() <= TOL, "{made:?}");
}

#[test]
fn directed_thirty_and_three_thirty_differ() {
    let placed = |solution: &str, offset: f64| {
        let mut s = blocks();
        let r = run(&mut s, "constraint.create", fronts(solution, offset, json!({})));
        assert_eq!(r["status"], "ok", "{solution} {offset}: {r}");
        normals(&s, "Angle:1").1
    };
    let (d30, d330) = (placed("directed", 30.0), placed("directed", 330.0));
    assert!(d30.dot(d330) < 0.9, "{d30:?} {d330:?}");
    let (u30, u330) = (placed("undirected", 30.0), placed("undirected", 330.0));
    assert!((u30 - u330).len() <= TOL, "{u30:?} {u330:?}");
}

#[test]
fn cylinders_touch_at_the_radius_sum() {
    let tangent = |s: &mut Session, solution: &str| {
        let pin = occ(s, "Pin");
        run(s, "occurrence.ground", json!({"occurrence": pin, "grounded": true}));
        let p = json!({"type": "tangent", "solution": solution, "a": face("Pin", [310.0, 0.0, 80.0]), "b": face("Drum", [385.0, 0.0, 20.0])});
        run(s, "constraint.create", p)
    };
    let mut s = blocks();
    let r = tangent(&mut s, "outside");
    assert_eq!(r["status"], "ok", "{r}");
    assert!((axes_apart(&s, "Tangent:1") - 25.0).abs() <= TOL, "{}", axes_apart(&s, "Tangent:1"));
    assert_eq!(dof_of(&r, &s, "Drum"), 3);
    let mut inside = blocks();
    tangent(&mut inside, "inside");
    assert!((axes_apart(&inside, "Tangent:1") - 5.0).abs() <= TOL, "{}", axes_apart(&inside, "Tangent:1"));
    run(&mut s, "timeline.edit", json!({"feature": "PinBody", "set": {"radius": "12 mm"}}));
    assert!((axes_apart(&s, "Tangent:1") - 27.0).abs() <= TOL, "{}", axes_apart(&s, "Tangent:1"));
}

#[test]
fn a_part_held_only_by_an_equation_still_solves() {
    let mut s = blocks();
    let r = run(&mut s, "constraint.create", fronts("directed", 30.0, json!({})));
    assert_eq!(r["status"], "ok", "{r}");
    let c = constraint(&s, "Angle:1");
    let axis = apply_vector(&occurrence_world(&s.doc, occ(&s, "A")), c.axis.expect("a Directed Angle freezes its axis"));
    let (na, nb) = normals(&s, "Angle:1");
    assert!((signed(na, nb, axis) - 30.0).abs() <= ANG, "{na:?} {nb:?} about {axis:?}: {}", signed(na, nb, axis));
    assert_eq!(dof_of(&r, &s, "B"), 5);
    // The Free member is the solver's own: never a joint, never saved, never answered.
    assert_eq!(run(&mut s, "joint.list", json!({}))["joints"], json!([]));
    assert!(s.doc.assembly.joints.is_empty());
    let solved = run(&mut s, "joint.solve", json!({}));
    assert!(!solved.to_string().to_lowercase().contains("free"), "{solved}");
    let dir = std::env::temp_dir().join(format!("tcicad-con-{}-a_part_held_only_by_an_equation_still_solves", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("angle.tciasm");
    run(&mut s, "file.save", json!({"path": path.to_string_lossy(), "format": "json"}));
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!text.to_lowercase().contains("\"free\""), "{text}");
    let file: Value = serde_json::from_str(&text).unwrap();
    assert!(file.pointer("/assembly/joints").is_none_or(|j| j == &json!([])), "{text}");
}

#[test]
fn angle_equations_obey_caps() {
    // 70 empty parts, each held to the next only by a Directed Angle between their XZ origin
    // planes: one loop group of 70 Free members (420 values), over the solver's cap.
    let mut doc = Document::new("chain");
    let occs: Vec<u64> = (0..70)
        .map(|i| {
            let comp = doc.add_component(Some(&format!("P{i}")), 0).unwrap();
            doc.occurrences.iter().find(|o| o.component == comp).unwrap().id
        })
        .collect();
    for o in doc.occurrences.iter_mut() {
        o.grounded = false;
    }
    let plane = |occurrence: u64| GeoRef { occurrence, geo: Geo::Origin { item: OriginItem::Xz } };
    for (i, w) in occs.windows(2).enumerate() {
        let id = doc.allocate_id().unwrap();
        let c = Constraint {
            id,
            name: format!("Angle:{}", i + 1),
            kind: ConstraintKind::Angle,
            solution: ConstraintSolution::Directed,
            picks: vec![plane(w[0]), plane(w[1])],
            offset: 10.0,
            limits: Default::default(),
            exprs: Default::default(),
            suppressed: false,
            lock: false,
            axis: Some(Vec3::Z),
            state: Vec::new(),
            signatures: Vec::new(),
        };
        c.validate().unwrap();
        doc.assembly.constraints.push(c);
    }
    let mut s = Session::new(doc);
    let st = s.model.state();
    let compiled = compile(&s.doc, &st, &s.doc.assembly);
    assert_eq!(compiled.equations.len(), 69);
    let sol = joints::solve(&s.doc, &st);
    let first = compiled_id(s.doc.assembly.constraints[0].id, 0);
    let group = sol.groups.iter().find(|g| g.members.contains(&first)).expect("the equations close loops");
    assert!(group.capped, "{group:?}");
    assert_eq!(group.members.len(), 69);
    let l = run(&mut s, "constraint.list", json!({}));
    for c in l["constraints"].as_array().unwrap() {
        assert_eq!(c["status"], "unsolved", "{c}");
    }
}

/// Undirected with limits is an equation: B goes to the nearer bound and no further.
#[test]
fn undirected_limits_hold_the_nearer_bound() {
    let mut s = blocks();
    let r = run(&mut s, "constraint.create", fronts("undirected", 30.0, json!({"min": 20, "max": 40})));
    assert_eq!(r["status"], "ok", "{r}");
    let (na, nb) = normals(&s, "Angle:1");
    // Any angle inside the limits meets them: B stops just inside the nearer one.
    assert!((20.0 - ANG..=20.001).contains(&deg(na, nb)), "{}", deg(na, nb));
    assert_eq!(dof_of(&r, &s, "B"), 5);
    let st = s.model.state();
    let compiled = compile(&s.doc, &st, &s.doc.assembly);
    assert_eq!((compiled.joints.len(), compiled.equations.len()), (0, 1));
}

/// Each new row over-constrains B held flush by a Mate (its normal fixed): refused, nothing changes.
#[test]
fn an_angle_or_tangent_that_disagrees_is_refused() {
    let flush = json!({"type": "mate", "solution": "flush", "a": face("A", [50.0, 0.0, 10.0]), "b": face("B", [50.0, 0.0, 50.0])});
    for (what, extra) in [
        ("undirected", json!({})),
        ("undirected", json!({"min": 20, "max": 40})),
        ("directed", json!({})),
        ("explicit_vector", json!({"ref": edge("A", [0.0, 0.0, 10.0])})),
    ] {
        let mut s = blocks();
        run(&mut s, "constraint.create", flush.clone());
        refused(&mut s, "constraint.create", fronts(what, 30.0, extra), "over-constrains B: Angle:1 off by");
    }
    // Tangent: the drum held on the pin's axis by a Mate, then told to touch it.
    let mut s = blocks();
    let pin = occ(&s, "Pin");
    run(&mut s, "occurrence.ground", json!({"occurrence": pin, "grounded": true}));
    run(
        &mut s,
        "constraint.create",
        json!({"type": "mate", "solution": "aligned", "a": face("Pin", [310.0, 0.0, 80.0]), "b": face("Drum", [415.0, 0.0, 20.0])}),
    );
    let at = crate::test_support::at(&s, "Drum", Vec3::new(385.0, 0.0, 20.0));
    let p = json!({"type": "tangent", "solution": "outside", "a": face("Pin", [310.0, 0.0, 80.0]), "b": face("Drum", [at.x, at.y, at.z])});
    refused(&mut s, "constraint.create", p, "over-constrains Drum: Tangent:1 off by 25.000 mm");
}

/// Two cylinders whose axes are not parallel can't be tangent in v1: refused at create, Lost in
/// a solve.
#[test]
fn skew_cylinders_come_later() {
    let mut s = blocks();
    let pin = occ(&s, "Pin");
    run(&mut s, "occurrence.move", json!({"occurrence": pin, "axis": [0, 1, 0], "angle": "30 deg"}));
    let wall = crate::test_support::at(&s, "Pin", Vec3::new(310.0, 0.0, 80.0));
    let p = json!({"type": "tangent", "a": face("Pin", [wall.x, wall.y, wall.z]), "b": face("Drum", [385.0, 0.0, 20.0])});
    refused(&mut s, "constraint.create", p, "skew cylinders come in a later version");
}

/// Directed freezes a × b in A's frame at create; an edit that makes an Angle Directed freezes it
/// then; `axis` sets it (world, turned into A's frame); a solution that is not Directed drops it.
#[test]
fn directed_freezes_its_axis() {
    let mut s = blocks();
    run(&mut s, "constraint.create", fronts("undirected", 30.0, json!({})));
    assert_eq!(constraint(&s, "Angle:1").axis, None);
    run(&mut s, "constraint.edit", json!({"constraint": "Angle:1", "solution": "directed"}));
    let c = constraint(&s, "Angle:1");
    let axis = c.axis.expect("frozen on the edit");
    // B sat 30° off A about some axis: a × b, square to both normals.
    let (na, nb) = normals(&s, "Angle:1");
    assert!(axis.dot(na).abs() <= 1e-9 && axis.dot(nb).abs() <= 1e-9, "{axis:?}");
    assert!((signed(na, nb, axis) - 30.0).abs() <= ANG, "{}", signed(na, nb, axis));
    // An explicit axis, then back to Undirected.
    run(&mut s, "constraint.edit", json!({"constraint": "Angle:1", "axis": [0, 0, -1]}));
    let c = constraint(&s, "Angle:1");
    assert!((c.axis.unwrap() - Vec3::new(0.0, 0.0, -1.0)).len() <= 1e-12, "{:?}", c.axis);
    let (na, nb) = normals(&s, "Angle:1");
    assert!((signed(na, nb, Vec3::new(0.0, 0.0, -1.0)) - 30.0).abs() <= ANG);
    run(&mut s, "constraint.edit", json!({"constraint": "Angle:1", "solution": "undirected"}));
    assert_eq!(constraint(&s, "Angle:1").axis, None);
}

/// The session's DOF memo answers as a fresh solve after an Angle edit (equations in its key).
#[test]
fn the_dof_memo_follows_an_angle_edit() {
    let mut s = blocks();
    let b = occ(&s, "B");
    run(&mut s, "constraint.create", fronts("directed", 30.0, json!({})));
    let bottom = crate::test_support::at(&s, "B", Vec3::new(50.0, 25.0, 40.0));
    run(&mut s, "constraint.create", json!({"type": "mate", "a": face("A", [50.0, 25.0, 20.0]), "b": face("B", [bottom.x, bottom.y, bottom.z])}));
    let check = |s: &mut Session, what: &str, want: usize| {
        let memo = crate::cmd::joints::resolve(s).dof;
        assert_eq!(memo, joints::solve(&s.doc, &s.model.state()).dof, "{what}");
        assert_eq!(memo[&b], want, "{what}");
    };
    check(&mut s, "angle and mate", 2);
    run(&mut s, "constraint.edit", json!({"constraint": "Angle:1", "offset": 45}));
    check(&mut s, "edited", 2);
    let (na, nb) = normals(&s, "Angle:1");
    let axis = apply_vector(&occurrence_world(&s.doc, occ(&s, "A")), constraint(&s, "Angle:1").axis.unwrap());
    assert!((signed(na, nb, axis) - 45.0).abs() <= ANG, "{}", signed(na, nb, axis));
    run(&mut s, "constraint.suppress", json!({"constraint": "Angle:1"}));
    check(&mut s, "suppressed", 3);
}
