#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Constraint limits (Plan E-1 Task 7, spec §3.4 limit rows, §3.5 bounded): a Mate between planes
//! with limits compiles to `Parallel` with its z bounded, an Insert to `Cylindrical` with its
//! distance bounded, an Undirected Angle to Task 6's bounded equation. The offset is then a start,
//! not a hold. Blocks fixture; positions world, to 1e-9 mm (a circular edge's centre: 1e-6 mm),
//! angles to 1e-6°.

use serde_json::{Value, json};
use solidcraft_doc::joints::occurrence_world;
use solidcraft_doc::{Mat, apply_point, apply_vector};
use solidcraft_geom::Vec3;

use solidcraft_doc::constraints::{ConstraintKind, ConstraintSolution, PickWhat, geo_at};

use crate::Session;
use crate::constraints::{Draft, preview};
use crate::test_support::{blocks, occ, run};

const TOL: f64 = 1e-9;
const RIM: f64 = 1e-6;
const ANG: f64 = 1e-6;

fn face(name: &str, p: [f64; 3]) -> Value {
    json!({"occurrence": name, "face": p})
}

fn edge(name: &str, p: [f64; 3]) -> Value {
    json!({"occurrence": name, "edge": p})
}

/// A's top on B's bottom, with `extra` merged in.
fn mate_a_b(extra: Value) -> Value {
    let mut p = json!({"type": "mate", "a": face("A", [50.0, 25.0, 20.0]), "b": face("B", [50.0, 25.0, 40.0])});
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    p
}

fn transform(s: &Session, name: &str) -> Mat {
    occurrence_world(&s.doc, occ(s, name))
}

fn same(a: &Mat, b: &Mat) -> bool {
    a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() <= TOL)
}

/// The gap between A's top (z = 20) and B's bottom (its component point (50, 25, 40)).
fn gap(s: &Session) -> f64 {
    apply_point(&transform(s, "B"), Vec3::new(50.0, 25.0, 40.0)).z - 20.0
}

fn dof_of(v: &Value, s: &Session, name: &str) -> u64 {
    v["dof"][occ(s, name).to_string()].as_u64().unwrap_or_else(|| panic!("no dof for {name} in {v}"))
}

fn compiled_kind(s: &mut Session, name: &str) -> String {
    let i = run(s, "constraint.inspect", json!({"constraint": name}));
    i["constraints"][0]["compiled"][0]["kind"].as_str().unwrap_or_else(|| panic!("{i}")).to_string()
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
fn a_limited_mate_slides_within_its_range() {
    let mut s = blocks();
    let r = run(&mut s, "constraint.create", mate_a_b(json!({"min": 0, "max": 10})));
    assert_eq!(r["status"], "ok", "{r}");
    // Parallel: x, y, z (bounded) and a turn.
    assert_eq!(dof_of(&r, &s, "B"), 4, "{r}");
    assert_eq!(compiled_kind(&mut s, "Mate:1"), "parallel");
    assert!(gap(&s).abs() <= TOL, "{}", gap(&s));
    let r = run(&mut s, "constraint.edit", json!({"constraint": "Mate:1", "offset": 6}));
    assert!((gap(&s) - 6.0).abs() <= TOL, "{}", gap(&s));
    assert!(r.get("warnings").is_none(), "{r}");
    // Past the max: the start is clamped at it (as joint.create clamps values), and the answer
    // says so.
    let r = run(&mut s, "constraint.edit", json!({"constraint": "Mate:1", "offset": 12}));
    assert!((gap(&s) - 10.0).abs() <= TOL, "{}", gap(&s));
    assert_eq!(r["status"], "ok", "{r}");
    let w = r["warnings"][0].as_str().unwrap_or_else(|| panic!("no warning: {r}"));
    assert!(w.contains("Mate:1") && w.contains("10 mm") && w.contains("12 mm"), "{w}");
}

#[test]
fn adding_limits_keeps_the_pose() {
    let mut s = blocks();
    run(&mut s, "constraint.create", mate_a_b(json!({"offset": 3})));
    assert!((gap(&s) - 3.0).abs() <= TOL, "{}", gap(&s));
    let before = transform(&s, "B");
    let steps = s.undo.len();
    run(&mut s, "constraint.edit", json!({"constraint": "Mate:1", "min": 0, "max": 10}));
    assert!(same(&transform(&s, "B"), &before), "{:?} vs {before:?}", transform(&s, "B"));
    assert_eq!(s.doc.assembly.constraints[0].state.len(), 4);
    assert_eq!(s.undo.len(), steps + 1);
    assert_eq!(compiled_kind(&mut s, "Mate:1"), "parallel");
    run(&mut s, "constraint.edit", json!({"constraint": "Mate:1", "min": null, "max": null}));
    assert_eq!(compiled_kind(&mut s, "Mate:1"), "planar");
    assert!(same(&transform(&s, "B"), &before), "{:?} vs {before:?}", transform(&s, "B"));
    assert_eq!(s.doc.assembly.constraints[0].state.len(), 3);
}

#[test]
fn a_limited_insert_slides_and_turns() {
    let mut s = blocks();
    let p = json!({"type": "insert", "a": edge("Plate", [200.0, 0.0, 10.0]), "b": edge("Pin", [310.0, 0.0, 50.0]), "min": 0, "max": 10});
    let r = run(&mut s, "constraint.create", p);
    assert_eq!(r["status"], "ok", "{r}");
    assert_eq!(dof_of(&r, &s, "Pin"), 2, "{r}");
    assert_eq!(compiled_kind(&mut s, "Insert:1"), "cylindrical");
    run(&mut s, "constraint.edit", json!({"constraint": "Insert:1", "offset": 4}));
    // The pin's bottom (its component point (300, 0, 50)) 4 off the plate's top (190, 0, 10),
    // along the plate's top normal.
    let (plate, pin) = (transform(&s, "Plate"), transform(&s, "Pin"));
    let up = apply_vector(&plate, Vec3::Z);
    let apart = apply_point(&pin, Vec3::new(300.0, 0.0, 50.0)) - apply_point(&plate, Vec3::new(190.0, 0.0, 10.0));
    assert!((apart.dot(up) - 4.0).abs() <= RIM, "{apart:?} along {up:?}");
    assert!(apart.cross(up).len() <= RIM, "{apart:?} off the axis {up:?}");
}

/// The angle between A's and B's front normals (A's front is y = 0 in its component, B's too).
fn fronts_angle(s: &Session) -> f64 {
    let (a, b) = (apply_vector(&transform(s, "A"), Vec3::new(0.0, -1.0, 0.0)), apply_vector(&transform(s, "B"), Vec3::new(0.0, -1.0, 0.0)));
    a.cross(b).len().atan2(a.dot(b)).to_degrees()
}

#[test]
fn angle_limits_rest_at_the_bound() {
    let mut s = blocks();
    let b = occ(&s, "B");
    // B, a free part (R118), turned 30° about z first: inside [10, 40], so the Angle holds it
    // where it is.
    run(&mut s, "occurrence.move", json!({"occurrence": b, "axis": [0, 0, 1], "origin": [0, 0, 0], "angle": "30 deg"}));
    let front = apply_point(&transform(&s, "B"), Vec3::new(50.0, 0.0, 50.0));
    let p = json!({"type": "angle", "solution": "undirected", "a": face("A", [50.0, 0.0, 10.0]),
        "b": face("B", [front.x, front.y, front.z]), "offset": 30, "min": 10, "max": 40});
    let r = run(&mut s, "constraint.create", p);
    assert_eq!(r["status"], "ok", "{r}");
    assert!((fronts_angle(&s) - 30.0).abs() <= ANG, "{}", fronts_angle(&s));
    // 20° more (to 50°): the command's solve brings it back to the max.
    run(&mut s, "occurrence.move", json!({"occurrence": b, "axis": [0, 0, 1], "origin": [0, 0, 0], "angle": "20 deg"}));
    assert!((fronts_angle(&s) - 40.0).abs() <= ANG, "{}", fronts_angle(&s));
}

/// R153: a limited Angle's offset is a start, as a Mate's.
#[test]
fn a_limited_angle_starts_at_its_offset() {
    let mut s = blocks();
    // From parallel fronts: it turns to its offset, not to the nearer limit.
    let p = json!({"type": "angle", "solution": "undirected", "a": face("A", [50.0, 0.0, 10.0]), "b": face("B", [50.0, 0.0, 50.0]),
        "offset": 30, "min": 10, "max": 40});
    let r = run(&mut s, "constraint.create", p);
    assert_eq!(r["status"], "ok", "{r}");
    assert!(r.get("warnings").is_none(), "{r}");
    assert!((fronts_angle(&s) - 30.0).abs() <= ANG, "{}", fronts_angle(&s));
    // An offset edit inside the limits moves it there.
    run(&mut s, "constraint.edit", json!({"constraint": "Angle:1", "offset": 25}));
    assert!((fronts_angle(&s) - 25.0).abs() <= ANG, "{}", fronts_angle(&s));
    // Past the max: it starts at the max, and says so.
    let r = run(&mut s, "constraint.edit", json!({"constraint": "Angle:1", "offset": 50}));
    assert!((fronts_angle(&s) - 40.0).abs() <= ANG, "{}", fronts_angle(&s));
    let w = r["warnings"][0].as_str().unwrap_or_else(|| panic!("no warning: {r}"));
    assert!(w.contains("Angle:1") && w.contains("40 deg") && w.contains("50 deg"), "{w}");
}

/// The angle between A's and B's front normals under the placements `world`.
fn fronts_angle_in(s: &Session, world: &std::collections::BTreeMap<u64, Mat>) -> f64 {
    let front = |name: &str| apply_vector(&world[&occ(s, name)], Vec3::new(0.0, -1.0, 0.0));
    let (a, b) = (front("A"), front("B"));
    a.cross(b).len().atan2(a.dot(b)).to_degrees()
}

/// R160: the preview of a limited Angle takes R153's start step, so it shows the pose the
/// command makes, and carries the command's clamp warning.
#[test]
fn a_limited_angle_previews_where_it_commits() {
    let mut s = blocks();
    let st = s.model.state();
    let pick = |name: &str, p: Vec3| geo_at(&s.doc, &st, occ(&s, name), PickWhat::Face, p).unwrap();
    let draft = Draft {
        kind: ConstraintKind::Angle,
        solution: Some(ConstraintSolution::Undirected),
        a: Some(pick("A", Vec3::new(50.0, 0.0, 10.0))),
        b: Some(pick("B", Vec3::new(50.0, 0.0, 50.0))),
        offset: Some("30".into()),
        min: Some("10".into()),
        max: Some("40".into()),
        ..Draft::default()
    };
    drop(st);
    let pv = preview(&s, &draft);
    assert_eq!(pv.refused, None);
    assert!((fronts_angle_in(&s, &pv.world) - 30.0).abs() <= ANG, "{}", fronts_angle_in(&s, &pv.world));
    assert_eq!(pv.warning, None);
    // Past the max: the preview starts at the max and says what the command says.
    let past = preview(&s, &Draft { offset: Some("50".into()), ..draft.clone() });
    assert!((fronts_angle_in(&s, &past.world) - 40.0).abs() <= ANG, "{}", fronts_angle_in(&s, &past.world));
    assert_eq!(past.warning.as_deref(), Some("Angle:1 starts at 40 deg, inside its limits [10, 40], not at its offset 50 deg: the part moved there"));
    // The command makes the pose the preview showed.
    let r = run(&mut s, "constraint.create", draft.params());
    assert_eq!(r["status"], "ok", "{r}");
    assert!((fronts_angle(&s) - 30.0).abs() <= ANG, "{}", fronts_angle(&s));
    // An offset edit previews the same way.
    let id = r["id"].as_u64().unwrap();
    let record = s.doc.assembly.constraints.iter().find(|c| c.id == id).unwrap().clone();
    let edit = preview(&s, &Draft { offset: Some("50".into()), ..Draft::edit(&record) });
    assert!((fronts_angle_in(&s, &edit.world) - 40.0).abs() <= ANG, "{}", fronts_angle_in(&s, &edit.world));
    let w = edit.warning.unwrap_or_default();
    assert!(w.contains("Angle:1") && w.contains("40 deg") && w.contains("50 deg"), "{w}");
}

/// Final wave item 11: an Undirected angle is unsigned (0..180), so an offset of 350 is 10
/// before it is clamped into [10, 40]: it starts at 10 and does not warn, not at 40.
#[test]
fn an_undirected_offset_is_folded_before_it_is_clamped() {
    let mut s = blocks();
    let p = json!({"type": "angle", "solution": "undirected", "a": face("A", [50.0, 0.0, 10.0]), "b": face("B", [50.0, 0.0, 50.0]),
        "offset": 30, "min": 10, "max": 40});
    run(&mut s, "constraint.create", p);
    let r = run(&mut s, "constraint.edit", json!({"constraint": "Angle:1", "offset": 350}));
    assert!((fronts_angle(&s) - 10.0).abs() <= ANG, "{}", fronts_angle(&s));
    assert!(r.get("warnings").is_none(), "{r}");
    // 200 is 160: past the max, it starts at 40 and says so in the folded value.
    let r = run(&mut s, "constraint.edit", json!({"constraint": "Angle:1", "offset": 200}));
    assert!((fronts_angle(&s) - 40.0).abs() <= ANG, "{}", fronts_angle(&s));
    assert_eq!(r["warnings"][0], "Angle:1 starts at 40 deg, inside its limits [10, 40], not at its offset 200 deg: the part moved there", "{r}");
}

#[test]
fn limits_refuse_hostile_values() {
    let mut s = blocks();
    run(&mut s, "constraint.create", mate_a_b(json!({})));
    let edit = "constraint.edit";
    refused(&mut s, edit, json!({"constraint": "Mate:1", "min": 5, "max": 1}), "min 5 is above max 1");
    refused(&mut s, edit, json!({"constraint": "Mate:1", "max": "1e300 mm"}), "not a number within ±10000000 mm");
    // A NaN number reaches a command as null (which clears a limit); as text it is no number.
    refused(&mut s, edit, json!({"constraint": "Mate:1", "min": "NaN"}), "unknown parameter `NaN`");
    // A limit typed as an expression follows its parameter.
    let mut s = blocks();
    run(&mut s, "parameters.add", json!({"name": "t", "expression": "2 mm"}));
    run(&mut s, "constraint.create", mate_a_b(json!({"min": "t", "max": 10})));
    assert!((gap(&s) - 2.0).abs() <= TOL, "{}", gap(&s));
    run(&mut s, "parameters.change", json!({"name": "t", "expression": "5 mm"}));
    assert!((gap(&s) - 5.0).abs() <= TOL, "{}", gap(&s));
}
