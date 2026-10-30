#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Constraints follow a component that is extracted to its own design or pasted as a new one
//! (R133): those wholly inside go with it, remapped; one that crosses the boundary stays as a
//! Lost record, named in the notices; undo brings everything back.

use serde_json::{Value, json};
use solidcraft_doc::joints::occurrence_world;
use solidcraft_doc::{Mat, mat_inverse, mat_mul};

use crate::Session;
use crate::test_support::run;

const TOL: f64 = 1e-9;

/// Sub (grounded, root level) holds P (grounded) and Q; Other sits at the root. Mate:1 puts Q's
/// bottom on P's top (inside Sub); Mate:2 puts Other's bottom on Q's top (across Sub's boundary).
/// Returns the session and the occurrences of P, Q and Other.
fn design() -> (Session, u64, u64, u64) {
    let mut s = Session::default();
    let sub = run(&mut s, "component.create", json!({"name": "Sub"}))["component"].as_u64().unwrap();
    let p = run(&mut s, "component.create", json!({"name": "P", "parent": sub}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [0, 0, 0], "body_name": "PBody"}));
    run(&mut s, "component.activate", json!({"component": sub}));
    let q = run(&mut s, "component.create", json!({"name": "Q", "parent": sub}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 30], "body_name": "QBody"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let other = run(&mut s, "component.create", json!({"name": "Other"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [100, 0, 0], "body_name": "OtherBody"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.ground", json!({"occurrence": p, "grounded": true}));
    let r = run(
        &mut s,
        "constraint.create",
        json!({"type": "mate", "a": {"occurrence": p, "face": [10, 10, 10]}, "b": {"occurrence": q, "face": [5, 5, 30]}}),
    );
    assert_eq!(r["moved"], json!([q]), "{r}");
    run(
        &mut s,
        "constraint.create",
        json!({"type": "mate", "a": {"occurrence": q, "face": [5, 5, 20]}, "b": {"occurrence": other, "face": [105, 5, 0]}}),
    );
    (s, p, q, other)
}

/// Q's placement relative to P's.
fn relative(doc: &solidcraft_doc::Document, p: u64, q: u64) -> Mat {
    mat_mul(&mat_inverse(&occurrence_world(doc, p)).unwrap(), &occurrence_world(doc, q))
}

fn same(a: &Mat, b: &Mat) -> bool {
    a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() <= TOL)
}

fn names(s: &Session) -> Vec<String> {
    s.doc.assembly.constraints.iter().map(|c| c.name.clone()).collect()
}

#[test]
fn extract_carries_inner_constraints_and_keeps_crossing_ones() {
    let dir = std::env::temp_dir().join(format!("tcicad-con-{}-extract_carries_inner_constraints_and_keeps_crossing_ones", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (mut s, p, q, _) = design();
    run(&mut s, "file.save", json!({"path": dir.join("asm.solidcraft")}));
    let pose = relative(&s.doc, p, q);
    let before = s.doc.assembly.constraints.clone();
    let steps = s.undo.len();
    let file = dir.join("sub.solidcraft");
    let out = run(&mut s, "component.extract", json!({"component": "Sub", "path": file}));
    let notices: Vec<String> = out["notices"].as_array().unwrap().iter().map(|n| n.as_str().unwrap().to_string()).collect();
    assert!(notices.iter().any(|n| n.contains("constraint `Mate:2`")), "{out}");
    assert!(!notices.iter().any(|n| n.contains("Mate:1")), "{out}");
    // This design keeps the crossing Mate, now Lost (its Q is gone), and not the inner one.
    assert_eq!(names(&s), ["Mate:2"]);
    let i = run(&mut s, "constraint.inspect", json!({"constraint": "Mate:2"}));
    assert!(i["constraints"][0]["lost"].is_string(), "{i}");
    // The new design has the Mate on its own occurrences, and solves to the same pose.
    let mut part = Session::default();
    run(&mut part, "doc.open", json!({"path": file}));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(names(&part), ["Mate:1"]);
    let c = &part.doc.assembly.constraints[0];
    assert_eq!(c.picks.iter().map(|g| g.occurrence).collect::<Vec<_>>(), [p, q], "the inner placements keep their ids");
    assert_ne!(c.id, before[0].id, "a new id");
    run(&mut part, "joint.solve", json!({}));
    let i = run(&mut part, "constraint.inspect", json!({}));
    assert!(i["constraints"][0]["lost"].is_null(), "{i}");
    assert!(i["constraints"][0]["gaps"]["mm"].as_f64().unwrap() <= TOL, "{i}");
    assert!(same(&relative(&part.doc, p, q), &pose));
    // Undo: one step brings both constraints and the history back.
    assert_eq!(s.undo.len(), steps + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.assembly.constraints, before);
    assert!(same(&relative(&s.doc, p, q), &pose));
}

#[test]
fn paste_new_copies_inner_constraints() {
    let (mut s, p, q, _) = design();
    let pose = relative(&s.doc, p, q);
    let before = s.doc.assembly.constraints.clone();
    let out: Value = run(&mut s, "component.paste_new", json!({"component": "Sub", "translate": [0, 200, 0]}));
    assert!(out["notices"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("constraint `Mate:2`")), "{out}");
    assert_eq!(names(&s), ["Mate:1", "Mate:2", "Mate:3"]);
    let copy = &s.doc.assembly.constraints[2];
    let (cp, cq) = (copy.picks[0].occurrence, copy.picks[1].occurrence);
    assert!(cp != p && cq != q && cp != cq, "{copy:?}");
    assert!(s.doc.occurrences.iter().any(|o| o.id == cp) && s.doc.occurrences.iter().any(|o| o.id == cq));
    assert!(same(&relative(&s.doc, cp, cq), &pose));
    let i = run(&mut s, "constraint.inspect", json!({"constraint": "Mate:3"}));
    assert!(i["constraints"][0]["gaps"]["mm"].as_f64().unwrap() <= TOL, "{i}");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.assembly.constraints, before);
}

/// Final wave item 8: a constraint between the extracted component's own occurrence (its XY
/// plane) and an inner one (P's bottom) goes with it onto the new design's root (occurrence 0).
#[test]
fn extract_carries_a_constraint_on_its_own_occurrence_onto_the_root() {
    let dir = std::env::temp_dir().join(format!("tcicad-con-{}-extract_carries_a_constraint_on_its_own_occurrence", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (mut s, p, _, _) = design();
    let sub = s.doc.occurrences.iter().find(|o| o.name.starts_with("Sub")).unwrap().id;
    let r = run(
        &mut s,
        "constraint.create",
        json!({"type": "mate", "a": {"occurrence": sub, "origin": "xy"}, "b": {"occurrence": p, "face": [10, 10, 0]}, "offset": 0}),
    );
    assert_eq!(r["status"], "ok", "{r}");
    run(&mut s, "file.save", json!({"path": dir.join("asm.solidcraft")}));
    let file = dir.join("sub.solidcraft");
    let out = run(&mut s, "component.extract", json!({"component": "Sub", "path": file}));
    let notices = out["notices"].to_string();
    assert!(notices.contains("Mate:2") && !notices.contains("Mate:3"), "{out}");
    // It goes with the component: this design keeps only the crossing Mate:2.
    assert_eq!(names(&s), ["Mate:2"]);
    let mut part = Session::default();
    run(&mut part, "doc.open", json!({"path": file}));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(names(&part), ["Mate:1", "Mate:3"]);
    let c = part.doc.assembly.constraint("Mate:3").unwrap();
    assert_eq!(c.picks.iter().map(|g| g.occurrence).collect::<Vec<_>>(), [0, p], "Sub's own occurrence is the new design's root");
    assert_eq!(c.picks[0].geo, solidcraft_doc::constraints::Geo::Origin { item: solidcraft_doc::constraints::OriginItem::Xy });
    run(&mut part, "joint.solve", json!({}));
    let l = run(&mut part, "constraint.list", json!({}));
    assert!(l["constraints"].as_array().unwrap().iter().all(|c| c["status"] == "ok"), "{l}");
}
