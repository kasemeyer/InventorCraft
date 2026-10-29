#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The constraint commands (Plan E-1 Task 4, spec §5): create, edit, suppress, delete, list and
//! inspect on the blocks fixture, one undo step each. Positions are world, to 1e-9 mm unless
//! stated (a circular edge's centre comes from its mesh polyline: 1e-6 mm).

use serde_json::{Value, json};
use solidcraft_doc::joints::{self, occurrence_world};
use solidcraft_doc::{Mat, apply_point, apply_vector};
use solidcraft_geom::Vec3;

use crate::Session;
use crate::test_support::{blocks, occ, run};

const TOL: f64 = 1e-9;
const RIM: f64 = 1e-6;

fn face(name: &str, p: [f64; 3]) -> Value {
    json!({"occurrence": name, "face": p})
}

fn edge(name: &str, p: [f64; 3]) -> Value {
    json!({"occurrence": name, "edge": p})
}

/// A's top on B's bottom.
fn mate_a_b() -> Value {
    json!({"type": "mate", "a": face("A", [50.0, 25.0, 20.0]), "b": face("B", [50.0, 25.0, 40.0])})
}

/// B's front face (y = 0) flush with A's: with `mate_a_b`, a loop that leaves B one slide.
fn flush_fronts(s: &Session) -> Value {
    let front = apply_point(&transform(s, "B"), Vec3::new(50.0, 0.0, 50.0));
    json!({"type": "mate", "solution": "flush", "a": face("A", [50.0, 0.0, 10.0]), "b": face("B", [front.x, front.y, front.z])})
}

/// The Insert of Task 3 test 6: the plate's hole rim and the pin's bottom rim.
fn insert_plate_pin() -> Value {
    json!({"type": "insert", "a": edge("Plate", [200.0, 0.0, 10.0]), "b": edge("Pin", [310.0, 0.0, 50.0])})
}

fn transform(s: &Session, name: &str) -> Mat {
    occurrence_world(&s.doc, occ(s, name))
}

fn same(a: &Mat, b: &Mat) -> bool {
    a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() <= TOL)
}

/// The z of B's bottom face (its component point (50, 25, 40)).
fn b_bottom(s: &Session) -> f64 {
    apply_point(&transform(s, "B"), Vec3::new(50.0, 25.0, 40.0)).z
}

fn dof_of(v: &Value, s: &Session, name: &str) -> u64 {
    v["dof"][occ(s, name).to_string()].as_u64().unwrap_or_else(|| panic!("no dof for {name} in {v}"))
}

fn names(s: &mut Session) -> Vec<String> {
    let l = run(s, "constraint.list", json!({}));
    l["constraints"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap().to_string()).collect()
}

fn undo(s: &mut Session) {
    run(s, "edit.undo", json!({}));
}

#[test]
fn create_mate_names_it_and_takes_one_step() {
    let mut s = blocks();
    let steps = s.undo.len();
    let r = run(&mut s, "constraint.create", mate_a_b());
    assert_eq!(r["name"], "Mate:1", "{r}");
    assert_eq!(r["status"], "ok");
    assert_eq!(dof_of(&r, &s, "B"), 3);
    assert_eq!(r["moved"], json!([occ(&s, "B")]));
    assert_eq!(s.undo.len(), steps + 1);
    assert_eq!(s.undo.last().unwrap().label, "Constrain");
    assert!((b_bottom(&s) - 20.0).abs() <= TOL);
    let p = flush_fronts(&s);
    assert_eq!(run(&mut s, "constraint.create", p)["name"], "Mate:2");
    assert_eq!(run(&mut s, "constraint.create", insert_plate_pin())["name"], "Insert:1");
    assert_eq!(s.undo.len(), steps + 3);
}

/// The gap between A's top and B's bottom.
fn gap(s: &Session) -> f64 {
    b_bottom(s) - 20.0
}

#[test]
fn expressions_follow_parameters() {
    let mut s = blocks();
    run(&mut s, "parameters.add", json!({"name": "t", "expression": "3 mm"}));
    let mut p = mate_a_b();
    p["offset"] = json!("t * 2");
    run(&mut s, "constraint.create", p);
    assert!((gap(&s) - 6.0).abs() <= TOL, "{}", gap(&s));
    run(&mut s, "parameters.change", json!({"name": "t", "expression": "4 mm"}));
    assert!((gap(&s) - 8.0).abs() <= TOL, "{}", gap(&s));
    run(&mut s, "parameters.rename", json!({"name": "t", "new_name": "u"}));
    let l = run(&mut s, "constraint.list", json!({}));
    assert_eq!(l["constraints"][0]["expressions"]["offset"], "u * 2", "{l}");
    assert!((l["constraints"][0]["offset"].as_f64().unwrap() - 8.0).abs() <= TOL, "{l}");
    let steps = s.undo.len();
    let e = s.execute("parameters.delete", &json!({"name": "u"})).unwrap_err().to_string();
    assert!(e.contains("constraint Mate:1 (offset)"), "{e}");
    assert_eq!(s.undo.len(), steps);
    // R129: a constraint is listed as a constraint.
    let users = run(&mut s, "parameters.users", json!({"name": "u"}));
    assert_eq!(users["users"], json!([{"type": "constraint", "constraint": s.doc.assembly.constraints[0].id, "name": "Mate:1", "input": "offset"}]));
}

#[test]
fn predict_measures_and_does_not_move() {
    let mut s = blocks();
    let b = occ(&s, "B");
    run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": [0, 0, -13]}));
    assert!((b_bottom(&s) - 27.0).abs() <= TOL);
    let before = transform(&s, "B");
    let r = run(
        &mut s,
        "constraint.create",
        json!({"type": "mate", "a": face("A", [50.0, 25.0, 20.0]), "b": face("B", [50.0, 25.0, 27.0]), "predict": true}),
    );
    let l = run(&mut s, "constraint.list", json!({}));
    let row = &l["constraints"][0];
    assert!((row["offset"].as_f64().unwrap() - 7.0).abs() <= TOL, "{l}");
    assert_eq!(row["solution"], "mate");
    assert!(same(&transform(&s, "B"), &before), "{r}");
    run(
        &mut s,
        "constraint.create",
        json!({"type": "mate", "a": face("A", [50.0, 25.0, 20.0]), "b": face("B", [50.0, 25.0, 47.0]), "predict": true}),
    );
    let l = run(&mut s, "constraint.list", json!({}));
    let row = &l["constraints"][1];
    assert_eq!(row["solution"], "flush", "{l}");
    assert!((row["offset"].as_f64().unwrap() - 27.0).abs() <= TOL, "{l}");
    assert!(same(&transform(&s, "B"), &before));
    // The pure measure, through the public API.
    let c = &s.doc.assembly.constraints[0];
    let pr = crate::constraints::predict(&s, c.kind, &c.picks[0], &c.picks[1]).unwrap();
    assert!((pr.offset - 7.0).abs() <= TOL);
    assert_eq!(pr.solution, solidcraft_doc::constraints::ConstraintSolution::MateFace);
}

#[test]
fn edit_suppress_delete_are_one_step_each() {
    let mut s = blocks();
    run(&mut s, "constraint.create", mate_a_b());
    let base = s.undo.len();
    let pose0 = transform(&s, "B");
    let r = run(&mut s, "constraint.edit", json!({"constraint": "Mate:1", "offset": 5}));
    assert_eq!(r["name"], "Mate:1", "{r}");
    assert!((gap(&s) - 5.0).abs() <= TOL, "{}", gap(&s));
    assert_eq!(s.undo.len(), base + 1);
    assert_eq!(s.undo.last().unwrap().label, "Edit Constraint");
    let pose5 = transform(&s, "B");
    run(&mut s, "constraint.edit", json!({"constraint": "Mate:1", "offset": 5}));
    assert_eq!(s.undo.len(), base + 1, "an edit that changes nothing adds no step");
    let r = run(&mut s, "constraint.suppress", json!({"constraint": "Mate:1"}));
    assert_eq!(r["constraints"], json!([{"name": "Mate:1", "suppressed": true, "status": "suppressed"}]));
    assert!(same(&transform(&s, "B"), &pose5), "B keeps its pose");
    let l = run(&mut s, "constraint.list", json!({}));
    assert_eq!(dof_of(&l, &s, "B"), 6);
    assert_eq!(l["constraints"][0]["status"], "suppressed");
    let r = run(&mut s, "constraint.suppress", json!({"constraint": "Mate:1"}));
    assert_eq!(r["constraints"][0]["suppressed"], false);
    assert_eq!(r["constraints"][0]["status"], "ok");
    assert_eq!(run(&mut s, "constraint.delete", json!({"constraint": "Mate:1"}))["deleted"], 1);
    assert_eq!(s.undo.len(), base + 4);
    assert!(names(&mut s).is_empty());
    undo(&mut s);
    assert_eq!(names(&mut s), ["Mate:1"]);
    assert!(!s.doc.assembly.constraints[0].suppressed);
    undo(&mut s);
    assert!(s.doc.assembly.constraints[0].suppressed);
    undo(&mut s);
    assert!(!s.doc.assembly.constraints[0].suppressed);
    assert!((gap(&s) - 5.0).abs() <= TOL);
    undo(&mut s);
    assert_eq!(s.doc.assembly.constraints[0].offset, 0.0);
    assert!(same(&transform(&s, "B"), &pose0));
    let p = flush_fronts(&s);
    run(&mut s, "constraint.create", p);
    let steps = s.undo.len();
    assert_eq!(run(&mut s, "constraint.delete", json!({"constraint": ["Mate:1", "Mate:2"]}))["deleted"], 2);
    assert_eq!(s.undo.len(), steps + 1);
    assert!(s.doc.assembly.constraints.is_empty());
}

#[test]
fn origin_items_are_picks() {
    let mut s = blocks();
    let b = occ(&s, "B");
    run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": [0, 0, 13]}));
    let r = run(&mut s, "constraint.create", json!({"type": "mate", "a": {"occurrence": "B", "origin": "xy"}, "b": {"origin": "xy"}}));
    let t = transform(&s, "B");
    assert!(apply_point(&t, Vec3::ZERO).z.abs() <= TOL, "{t:?}");
    assert!((apply_vector(&t, Vec3::Z).z.abs() - 1.0).abs() <= TOL, "{t:?}");
    assert_eq!(dof_of(&r, &s, "B"), 3);
    assert_eq!(s.doc.assembly.constraints[0].picks[1].occurrence, 0);
}

#[test]
fn list_and_inspect_report_what_was_made() {
    let mut s = blocks();
    run(&mut s, "constraint.create", insert_plate_pin());
    let (plate, pin) = (occ(&s, "Plate"), occ(&s, "Pin"));
    let l = run(&mut s, "constraint.list", json!({}));
    let row = &l["constraints"][0];
    assert_eq!(row["type"], "insert", "{l}");
    assert_eq!(row["solution"], "opposed");
    assert_eq!(row["unit"], "mm");
    assert_eq!(row["occurrences"], json!([plate, pin]));
    assert_eq!(row["suppressed"], false);
    assert_eq!(row["status"], "ok");
    assert_eq!(dof_of(&l, &s, "Pin"), 1);
    // The occurrence filter.
    let none = run(&mut s, "constraint.list", json!({"occurrence": "A"}));
    assert_eq!(none["constraints"], json!([]));
    let i = run(&mut s, "constraint.inspect", json!({"constraint": "Insert:1"}));
    let c = &i["constraints"][0];
    assert_eq!(c["compiled"][0]["kind"], "revolute", "{i}");
    assert_eq!(c["compiled"][0]["id"], solidcraft_doc::constraints::compiled_id(s.doc.assembly.constraints[0].id, 0));
    let rim = &c["picks"][0]["resolved"];
    assert_eq!(rim["kind"], "circle", "{i}");
    let at: Vec<f64> = rim["c"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!(Vec3::new(at[0], at[1], at[2]).dist(Vec3::new(190.0, 0.0, 10.0)) <= RIM, "{rim}");
    assert!(c["gaps"]["mm"].as_f64().unwrap() <= RIM, "{i}");
    assert!(c["gaps"]["deg"].as_f64().unwrap() <= RIM, "{i}");
    // Every constraint when none is named; picks are written back as the command takes them.
    let all = run(&mut s, "constraint.inspect", json!({}));
    assert_eq!(all["constraints"].as_array().map(Vec::len), Some(1));
    let mut again = insert_plate_pin();
    again["a"] = c["picks"][0].clone();
    again["b"] = c["picks"][1].clone();
    let r = run(&mut s, "constraint.create", again);
    assert_eq!(r["name"], "Insert:2", "{r}");
    assert_eq!(s.doc.assembly.constraints[1].picks, s.doc.assembly.constraints[0].picks);
}

#[test]
fn the_first_part_is_grounded_and_hardware_is_not() {
    let dir = std::env::temp_dir().join(format!("tcicad-con-{}-the_first_part_is_grounded_and_hardware_is_not", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Block.solidcraft");
    let mut part = Session::default();
    run(&mut part, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [5, 5, 5]}));
    run(&mut part, "file.save_as", json!({"path": path.to_string_lossy()}));
    let mut s = Session::default();
    run(&mut s, "document.new_assembly", json!({}));
    let first = run(&mut s, "component.insert_design", json!({"path": path.to_string_lossy()}));
    let second = run(&mut s, "component.insert_design", json!({"path": path.to_string_lossy()}));
    let _ = std::fs::remove_dir_all(&dir);
    let placed = |s: &Session, c: &Value| s.doc.occurrence_of(c["component"].as_u64().unwrap()).cloned().unwrap();
    let a = placed(&s, &first);
    assert!(a.grounded, "the first part is grounded");
    assert_eq!(a.transform, solidcraft_doc::IDENTITY);
    assert!(!placed(&s, &second).grounded, "a second part starts free");
    let mut h = Session::default();
    run(&mut h, "document.new_assembly", json!({}));
    let bolt = run(&mut h, "parts.fastener", json!({"family": "socket_head_cap_screw", "size": "M6", "length": 20}));
    assert!(!placed(&h, &bolt).grounded, "hardware placed first starts free");
}

#[test]
fn deleting_a_component_removes_its_constraints() {
    let mut s = blocks();
    run(&mut s, "constraint.create", mate_a_b());
    run(&mut s, "constraint.create", insert_plate_pin());
    let pin = transform(&s, "Pin");
    let steps = s.undo.len();
    run(&mut s, "component.delete", json!({"component": "Pin"}));
    assert_eq!(names(&mut s), ["Mate:1"]);
    assert_eq!(s.undo.len(), steps + 1);
    undo(&mut s);
    assert_eq!(names(&mut s), ["Mate:1", "Insert:1"]);
    assert!(same(&transform(&s, "Pin"), &pin));
}

#[test]
fn commands_refuse_a_drawing_and_an_empty_assembly() {
    let ids = ["constraint.create", "constraint.edit", "constraint.suppress", "constraint.delete", "constraint.list", "constraint.inspect"];
    for (new, p) in [("drawing.new", json!({})), ("document.new_assembly", json!({}))] {
        let mut s = Session::default();
        run(&mut s, new, p);
        for id in ids {
            let e = s.execute(id, &json!({"constraint": "Mate:1", "type": "mate"})).unwrap_err().to_string();
            assert!(e.contains("needs an assembly with components"), "{new} {id}: {e}");
        }
        assert!(!crate::find_command("constraint.create").unwrap().info(&s).enabled, "{new}");
    }
    // An assembly with components but no constraints: suppress and delete have nothing to act on.
    let s = blocks();
    assert!(crate::find_command("constraint.create").unwrap().info(&s).enabled);
    let info = crate::find_command("constraint.delete").unwrap().info(&s);
    assert_eq!(info.disabled_reason.as_deref(), Some("no constraints yet"));
}

#[test]
fn selection_delete_takes_constraints() {
    let mut s = blocks();
    run(&mut s, "constraint.create", mate_a_b());
    let steps = s.undo.len();
    let dry = run(&mut s, "selection.delete", json!({"constraints": ["Mate:1"], "occurrences": [], "dry_run": true}));
    assert_eq!(dry["deleted_constraints"], 1, "{dry}");
    assert_eq!(s.doc.assembly.constraints.len(), 1, "a dry run deletes nothing");
    let r = run(&mut s, "selection.delete", json!({"constraints": ["Mate:1"], "occurrences": []}));
    assert_eq!(r["deleted_constraints"], 1, "{r}");
    assert!(s.doc.assembly.constraints.is_empty());
    assert_eq!(s.undo.len(), steps + 1);
}

/// The DOF a fresh solve (no memo) counts.
fn fresh_dof(s: &Session) -> std::collections::BTreeMap<u64, usize> {
    joints::solve(&s.doc, &s.model.state()).dof
}

fn listed_dof(s: &mut Session) -> std::collections::BTreeMap<u64, usize> {
    let l = run(s, "constraint.list", json!({}));
    l["dof"].as_object().unwrap().iter().map(|(k, v)| (k.parse().unwrap(), v.as_u64().unwrap() as usize)).collect()
}

/// The session's DOF memo (R130) never answers for a design it was not counted on: after every
/// create, edit, suppress, delete, ground and undo, `constraint.list` (through the memo) gives
/// what a fresh solve gives, on a loop (two Mates on B).
#[test]
fn the_dof_memo_follows_every_change() {
    let mut s = blocks();
    let b = occ(&s, "B");
    let check = |s: &mut Session, what: &str, want: usize| {
        let listed = listed_dof(s);
        assert_eq!(listed, fresh_dof(s), "{what}");
        assert_eq!(listed[&b], want, "{what}");
    };
    run(&mut s, "constraint.create", mate_a_b());
    check(&mut s, "create", 3);
    let p = flush_fronts(&s);
    let r = run(&mut s, "constraint.create", p);
    assert_eq!(dof_of(&r, &s, "B"), 1, "{r}");
    check(&mut s, "a loop", 1);
    run(&mut s, "constraint.edit", json!({"constraint": "Mate:2", "offset": 2}));
    check(&mut s, "edit", 1);
    run(&mut s, "constraint.suppress", json!({"constraint": "Mate:2"}));
    check(&mut s, "suppress", 3);
    run(&mut s, "constraint.suppress", json!({"constraint": "Mate:2"}));
    check(&mut s, "unsuppress", 1);
    run(&mut s, "occurrence.ground", json!({"occurrence": b, "grounded": true}));
    check(&mut s, "ground", 0);
    undo(&mut s);
    check(&mut s, "undo ground", 1);
    run(&mut s, "constraint.delete", json!({"constraint": "Mate:2"}));
    check(&mut s, "delete", 3);
    undo(&mut s);
    check(&mut s, "undo delete", 1);
}

#[test]
fn joint_inspect_skips_compiled_joints() {
    let mut s = blocks();
    run(&mut s, "constraint.create", mate_a_b());
    let i = run(&mut s, "joint.inspect", json!({}));
    assert_eq!(i["joints"], json!([]), "{i}");
    let id = solidcraft_doc::constraints::compiled_id(s.doc.assembly.constraints[0].id, 0);
    assert!(s.execute("joint.inspect", &json!({"joint": id})).is_err());
}
