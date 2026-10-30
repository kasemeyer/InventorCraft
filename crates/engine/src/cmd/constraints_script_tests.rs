#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The owner's first minute as a CLI script (Plan E-1 Task 9, spec §2.4, R120):
//! `examples/assembly/first-minute.json`, run by `solidcraft-cli run --results`, builds the blocks
//! fixture's components by commands, places the five constraints of spec §2.4 in an order where
//! each record is solvable and everything is chained to grounded A (Mate:1 A top / B bottom; Mate:2
//! Flush A bottom / Plate bottom; Insert:1 the plate's hole and the pin; Mate:3 the pin's and the
//! drum's axes; Angle:1 A front / B front at 30° about A's vertical edge), lists them and saves the
//! assembly. Here it runs through `Session::run_script` with its file name put in a temp folder.

use serde_json::{Value, json};
use solidcraft_doc::joints::{Analysis, Held};
use solidcraft_geom::Vec3;

use crate::Session;
use crate::test_support::{at, occ, run};

const SCRIPT: &str = include_str!("../../../../examples/assembly/first-minute.json");

const TOL: f64 = 1e-9;

/// The script with every `path` put in `dir` (the CLI takes it from the working folder).
fn script_in(dir: &std::path::Path) -> Value {
    let mut script: Value = serde_json::from_str(SCRIPT).unwrap();
    for step in script["commands"].as_array_mut().unwrap() {
        if let Some(name) = step["params"]["path"].as_str() {
            assert!(!name.contains(['/', '\\']), "the script names files in its own folder: {name}");
            step["params"]["path"] = json!(dir.join(name).to_string_lossy());
        }
    }
    script
}

/// The result of the script's (last) step running `command`.
fn result_of<'a>(script: &Value, results: &'a [Value], command: &str) -> &'a Value {
    let steps = script["commands"].as_array().unwrap();
    steps.iter().zip(results).filter(|(c, _)| c["command"] == command).map(|(_, r)| r).next_back().unwrap_or_else(|| panic!("no {command} step"))
}

/// How far `p` is from the line through `a` along `d` (a unit vector).
fn off_line(p: Vec3, a: Vec3, d: Vec3) -> f64 {
    (p - a).cross(d).len()
}

#[test]
fn the_first_minute_script_places_five_constraints() {
    let dir = std::env::temp_dir().join(format!("tcicad-con-{}-the_first_minute_script_places_five_constraints", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = script_in(&dir);
    let mut s = Session::default();
    let results = s.run_script(&script).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(results.len(), script["commands"].as_array().unwrap().len());

    // The five records, every one met.
    let list = result_of(&script, &results, "constraint.list");
    let rows = list["constraints"].as_array().unwrap();
    let names: Vec<&str> = rows.iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Mate:1", "Mate:2", "Insert:1", "Mate:3", "Angle:1"], "{list}");
    assert!(rows.iter().all(|c| c["status"] == "ok"), "{list}");
    // The script's Angle names no solution: it gets the type's default (ANGLE_DEFAULT).
    let angle = s.doc.assembly.constraint("Angle:1").unwrap();
    assert_eq!(angle.solution, solidcraft_doc::constraints::ANGLE_DEFAULT);
    assert_eq!(angle.solution, solidcraft_doc::constraints::ConstraintSolution::ExplicitVector);
    let sol = crate::cmd::joints::solution(&s, &Held::new(), Analysis::DegreesOfFreedom);
    assert_eq!(sol.conflicts, vec![]);
    assert_eq!(sol.errors, vec![]);

    // DOF from the chain: B keeps x, y (the Angle took Mate:1's turn); Plate the Flush's 3; the
    // pin Plate's 3 and the Insert's turn; the drum the pin's 4 and its slide and turn.
    let dof = |name: &str| list["dof"][occ(&s, name).to_string()].as_u64().unwrap_or_else(|| panic!("no dof for {name} in {list}"));
    assert_eq!(["B", "Plate", "Pin", "Drum"].map(dof), [2, 3, 4, 6], "{list}");

    // The pin stands in the plate's hole: its bottom centre on the hole's top rim centre.
    let bottom = at(&s, "Pin", Vec3::new(300.0, 0.0, 50.0));
    assert!(bottom.dist(Vec3::new(190.0, 0.0, 10.0)) <= TOL, "pin bottom centre at {bottom:?}");
    // The drum is placed, not free: its axis on the pin's.
    let (p0, p1) = (bottom, at(&s, "Pin", Vec3::new(300.0, 0.0, 110.0)));
    let d = (p1 - p0).normalized().unwrap();
    let (q0, q1) = (at(&s, "Drum", Vec3::new(400.0, 0.0, 0.0)), at(&s, "Drum", Vec3::new(400.0, 0.0, 40.0)));
    assert!(off_line(q0, p0, d) <= TOL && off_line(q1, p0, d) <= TOL, "drum axis {q0:?}-{q1:?} off the pin's {p0:?} along {d:?}");

    // The saved file reopens with the same five records and poses.
    let path = dir.join("first-minute.tciasm");
    assert!(path.is_file(), "the script saved {}", path.display());
    let mut reopened = Session::default();
    run(&mut reopened, "doc.open", json!({"path": path.to_string_lossy()}));
    let _ = std::fs::remove_dir_all(&dir);
    let names: Vec<&str> = reopened.doc.assembly.constraints.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Mate:1", "Mate:2", "Insert:1", "Mate:3", "Angle:1"]);
    assert_eq!(serde_json::to_string(&reopened.doc.assembly).unwrap(), serde_json::to_string(&s.doc.assembly).unwrap());
    assert_eq!(serde_json::to_string(&reopened.doc.occurrences).unwrap(), serde_json::to_string(&s.doc.occurrences).unwrap());
}
