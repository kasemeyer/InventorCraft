#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Hostile parameters on `constraint.create` and `constraint.edit` (spec §8.4): each is refused
//! with its reason, adds no undo step and changes nothing.

use serde_json::{Value, json};
use solidcraft_doc::constraints::{PickWhat, geo_at};
use solidcraft_doc::joints::{Joint, JointExprs, JointKind, JointOrigin, Snap};
use solidcraft_geom::Vec3;

use crate::Session;
use crate::test_support::{blocks, occ, run};

fn face(name: &str, p: [f64; 3]) -> Value {
    json!({"occurrence": name, "face": p})
}

fn mate() -> Value {
    json!({"type": "mate", "a": face("A", [50.0, 25.0, 20.0]), "b": face("B", [50.0, 25.0, 40.0])})
}

/// `mate()` with `extra` merged over it.
fn with(mut p: Value, extra: Value) -> Value {
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    p
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
fn hostile_constraint_params_are_refused() {
    let mut s = blocks();
    let create = "constraint.create";
    // Numbers out of range, as numbers and as text.
    // A NaN number reaches a command as null: an offset refuses it (a limit's null clears it, spec §5).
    refused(&mut s, create, with(mate(), json!({"offset": f64::NAN})), "offset");
    for v in [json!("NaN"), json!("inf"), json!("-inf"), json!(1e300), json!(-1e300), json!("1e300 mm")] {
        for key in ["offset", "min", "max"] {
            refused(&mut s, create, with(mate(), json!({key: v.clone()})), "");
        }
    }
    refused(&mut s, create, with(mate(), json!({"offset": "1e300 mm"})), "not a number within");
    refused(&mut s, create, with(mate(), json!({"min": 5, "max": 1})), "min 5 is above max 1");
    // An expression cycle, and a parameter that is gone.
    let _ = s.execute("parameters.add", &json!({"name": "a", "expression": "b + 1 mm"}));
    let _ = s.execute("parameters.add", &json!({"name": "b", "expression": "a"}));
    refused(&mut s, create, with(mate(), json!({"offset": "a"})), "");
    run(&mut s, "parameters.add", json!({"name": "gone", "expression": "2 mm"}));
    run(&mut s, "parameters.delete", json!({"name": "gone"}));
    refused(&mut s, create, with(mate(), json!({"offset": "gone * 2"})), "gone");
    // Picks.
    refused(&mut s, create, json!({"type": "mate", "a": face("B", [50.0, 25.0, 60.0]), "b": face("B", [50.0, 25.0, 40.0])}), "both picks are on B");
    refused(&mut s, create, with(mate(), json!({"a": {"occurrence": 0, "face": [50.0, 25.0, 20.0]}})), "origin");
    refused(&mut s, create, with(mate(), json!({"a": {"occurrence": "A", "face": "NoSuchFace"}})), "NoSuchFace");
    refused(&mut s, create, with(mate(), json!({"a": {"occurrence": "A"}})), "face, edge, vertex or origin");
    refused(&mut s, create, with(mate(), json!({"a": {"occurrence": "A", "face": [1e300, 0, 0]}})), "");
    refused(&mut s, create, with(mate(), json!({"a": "A"})), "pick");
    refused(&mut s, create, with(mate(), json!({"b": {"occurrence": "B", "origin": "w"}})), "origin");
    refused(&mut s, create, json!({"type": "mate", "a": face("A", [50.0, 25.0, 20.0])}), "Mate takes 2 picks");
    refused(&mut s, create, with(mate(), json!({"ref": {"origin": "z"}})), "Mate takes 2 picks");
    refused(&mut s, create, with(mate(), json!({"type": "symmetry"})), "Symmetry comes in a later version");
    refused(&mut s, create, with(mate(), json!({"type": "weld"})), "`type`");
    refused(&mut s, create, with(mate(), json!({"solution": "inside"})), "Mate has no `inside` solution");
    refused(&mut s, create, with(mate(), json!({"solution": "opposed"})), "does not fit");
    refused(&mut s, create, with(mate(), json!({"lock": true})), "lock");
    // The Ball's sphere for Insert (no cone face exists to try).
    refused(
        &mut s,
        create,
        json!({"type": "insert", "a": {"occurrence": "Plate", "edge": [200.0, 0.0, 10.0]}, "b": face("Ball", [515.0, 0.0, 50.0])}),
        "Insert",
    );
    refused(&mut s, create, json!({"type": "tangent", "a": face("A", [50.0, 25.0, 20.0]), "b": face("B", [50.0, 25.0, 40.0])}), "Tangent");
    // Names.
    refused(&mut s, create, with(mate(), json!({"name": "x".repeat(10 << 20)})), "at most 128");
    refused(&mut s, create, with(mate(), json!({"name": ""})), "can't be empty");
    refused(&mut s, create, with(mate(), json!({"name": 7})), "name");
    let joint = s.doc_mut().allocate_id().unwrap();
    let frame = |o: u64| JointOrigin { occurrence: o, snap: Snap::Frame { origin: Vec3::ZERO, z: Vec3::Z, x: None } };
    let (drum, ball) = (frame(occ(&s, "Drum")), frame(occ(&s, "Ball")));
    s.doc_mut().assembly.joints.push(Joint {
        id: joint,
        name: "Hinge".into(),
        kind: JointKind::Rigid,
        a: drum,
        b: ball,
        values: Vec::new(),
        limits: Vec::new(),
        flip: false,
        offset: 0.0,
        angle: 0.0,
        suppressed: true,
        exprs: JointExprs::default(),
    });
    refused(&mut s, create, with(mate(), json!({"name": "Hinge"})), "already a joint's name");
    // A suppressed occurrence (its only feature suppressed: nothing left to pick) and a deleted one.
    let mut gone = blocks();
    let bottom = {
        let st = gone.model.state();
        let g = geo_at(&gone.doc, &st, occ(&gone, "B"), PickWhat::Face, Vec3::new(50.0, 25.0, 40.0)).unwrap();
        crate::constraints::pick_json(&g)
    };
    let feature = gone.doc.features.iter().find(|f| Some(f.component) == gone.doc.find_component("B")).unwrap().id;
    run(&mut gone, "timeline.suppress", json!({"feature": feature, "suppressed": true}));
    refused(&mut gone, create, mate(), "");
    refused(&mut gone, create, with(mate(), json!({"b": bottom})), "no longer exists");
    let mut deleted = blocks();
    let ball = occ(&deleted, "Ball");
    run(&mut deleted, "component.delete", json!({"component": "Ball"}));
    refused(&mut deleted, create, with(mate(), json!({"b": {"occurrence": ball, "face": [500, 0, 65]}})), &format!("no occurrence `{ball}`"));
    // Edit: the same checks on an existing constraint.
    run(&mut s, create, mate());
    let edit = "constraint.edit";
    refused(&mut s, edit, json!({"constraint": "Mate:1", "offset": 1e300}), "");
    refused(&mut s, edit, json!({"constraint": "Mate:1", "offset": "1e300 mm"}), "not a number within");
    refused(&mut s, edit, json!({"constraint": "Mate:1", "min": 5, "max": 1}), "min 5 is above max 1");
    refused(&mut s, edit, json!({"constraint": "Mate:1", "b": face("A", [50.0, 25.0, 0.0])}), "both picks are on A");
    refused(&mut s, edit, json!({"constraint": "Mate:1", "type": "symmetry"}), "Symmetry comes in a later version");
    refused(&mut s, edit, json!({"constraint": "Mate:1", "ref": {"origin": "z"}}), "Mate takes 2 picks");
    refused(&mut s, edit, json!({"constraint": "Mate:1", "name": "Hinge"}), "already a joint's name");
    refused(&mut s, edit, json!({"constraint": "Mate:1", "name": ""}), "can't be empty");
    refused(&mut s, edit, json!({"constraint": "Mate:1", "lock": true}), "lock");
    refused(&mut s, edit, json!({"constraint": "Mate:9"}), "no constraint `Mate:9`");
    refused(&mut s, edit, json!({"constraint": [1, 2]}), "constraint");
    refused(&mut s, "constraint.delete", json!({"constraint": ["Mate:1", "Mate:9"]}), "no constraint `Mate:9`");
    refused(&mut s, "constraint.suppress", json!({"constraint": {"x": 1}}), "constraint");
    refused(&mut s, "constraint.inspect", json!({"constraint": "Mate:9"}), "no constraint `Mate:9`");
    // The 10,001st constraint.
    let record = s.doc.assembly.constraints[0].clone();
    let doc = s.doc_mut();
    while doc.assembly.constraints.len() < solidcraft_doc::constraints::MAX_CONSTRAINTS {
        let mut c = record.clone();
        c.id = doc.allocate_id().unwrap();
        c.name = format!("Copy:{}", c.id);
        c.suppressed = true;
        doc.assembly.constraints.push(c);
    }
    refused(&mut s, create, mate(), "at most 10000 constraints");
}
