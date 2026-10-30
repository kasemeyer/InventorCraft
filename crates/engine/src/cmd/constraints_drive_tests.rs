#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! `constraint.drive` (Plan E-1 Task 8, spec §5, §6.6): frames from start to end (and back),
//! repeated; stops at contact (bisected to 1e-4 unit), at a limit and where a loop opens; the
//! design, its revision and the constraint report are restored and no undo step is added (owner
//! Q17). Blocks fixture; positions world, to 1e-9 mm; angles to 1e-6°.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcraft_doc::joints::occurrence_world;
use solidcraft_doc::{Mat, apply_point, apply_vector};
use solidcraft_geom::Vec3;

use crate::Session;
use crate::test_support::{blocks, occ, run};

const TOL: f64 = 1e-9;
const ANG: f64 = 1e-6;

fn face(name: &str, p: [f64; 3]) -> Value {
    json!({"occurrence": name, "face": p})
}

/// A's top on B's bottom, with `extra` merged in.
fn mate_a_b(extra: Value) -> Value {
    let mut p = json!({"type": "mate", "a": face("A", [50.0, 25.0, 20.0]), "b": face("B", [50.0, 25.0, 40.0])});
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    p
}

fn drive(s: &mut Session, p: Value) -> Value {
    let (steps, doc, revision, report) = (s.undo.len(), s.doc.clone(), s.revision, s.constraint_report.clone());
    let r = run(s, "constraint.drive", p);
    // Q17: the drive leaves the design as it was, adds no step.
    assert_eq!(s.undo.len(), steps);
    assert!(*s.doc == *doc, "the design changed");
    assert_eq!(s.revision, revision);
    assert!(s.constraint_report == report, "the report changed");
    r
}

fn values(r: &Value) -> Vec<f64> {
    r["frames"].as_array().unwrap_or_else(|| panic!("{r}")).iter().map(|f| f["value"].as_f64().unwrap()).collect()
}

/// Occurrence `name`'s world placement in frame `i` (the design's when the frame leaves it).
fn world_in(r: &Value, i: usize, s: &Session, name: &str) -> Mat {
    let id = occ(s, name);
    let world: BTreeMap<u64, Mat> = serde_json::from_value(r["frames"][i]["world"].clone()).unwrap_or_else(|e| panic!("{e}: {r}"));
    world.get(&id).copied().unwrap_or_else(|| occurrence_world(&s.doc, id))
}

/// B's bottom z (its component point (50, 25, 40)) under `m`.
fn bottom(m: &Mat) -> f64 {
    apply_point(m, Vec3::new(50.0, 25.0, 40.0)).z
}

#[test]
fn drive_steps_and_restores() {
    let mut s = blocks();
    run(&mut s, "constraint.create", mate_a_b(json!({})));
    let r = drive(&mut s, json!({"constraint": "Mate:1", "end": 20, "steps": 10}));
    let v = values(&r);
    assert_eq!(v.len(), 11, "{r}");
    for (i, x) in v.iter().enumerate() {
        assert!((x - 2.0 * i as f64).abs() <= TOL, "{v:?}");
    }
    assert!((bottom(&world_in(&r, 5, &s, "B")) - 30.0).abs() <= TOL, "{r}");
    assert!((bottom(&world_in(&r, 10, &s, "B")) - 40.0).abs() <= TOL, "{r}");
    assert!(r.get("stopped_by_contact").is_none() && r.get("stopped_by_limit").is_none() && r.get("stopped_by_loop").is_none(), "{r}");
    // Restored: B where it was, the offset 0.
    assert!((bottom(&occurrence_world(&s.doc, occ(&s, "B"))) - 20.0).abs() <= TOL);
    assert_eq!(s.doc.assembly.constraints[0].offset, 0.0);
}

#[test]
fn drive_stops_on_contact() {
    let mut s = blocks();
    run(&mut s, "constraint.create", mate_a_b(json!({})));
    // C, grounded, 13 mm above B's top (z = 40).
    run(&mut s, "component.create", json!({"name": "C", "grounded": true}));
    run(&mut s, "solid.box", json!({"length": 100, "width": 50, "height": 20, "corner": [0, 0, 53]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let (b, c) = (occ(&s, "B"), occ(&s, "C"));
    run(&mut s, "contact.create", json!({"occurrences": [b, c]}));
    let r = drive(&mut s, json!({"constraint": "Mate:1", "end": 20, "steps": 10, "stop_on_contact": true}));
    // R119: contact is an overlap deeper than the collision depth threshold, 1e-4 × the larger
    // body's size (motion.rs `overlap`: ≈ 0.011 mm for these 100 × 50 × 20 blocks), so the stop
    // is 13.000 within ±0.02.
    let stop = &r["stopped_by_contact"];
    let at = stop["value"].as_f64().unwrap_or_else(|| panic!("{r}"));
    assert!((at - 13.0).abs() <= 0.02, "{r}");
    let names = |id: u64| s.doc.occurrences.iter().find(|o| o.id == id).unwrap().name.clone();
    assert_eq!(stop["pair"], json!([names(b), names(c)]), "{r}");
    let v = values(&r);
    let last = *v.last().unwrap();
    assert!(last <= 13.02 && (last - at).abs() <= TOL, "{v:?}");
    assert_eq!(&v[..7], &[0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0], "{v:?}");
    // The last frame is free of C (B's top at 40 + the value, below C's bottom at 53).
    assert!((bottom(&world_in(&r, v.len() - 1, &s, "B")) - (20.0 + last)).abs() <= TOL, "{r}");
}

#[test]
fn drive_cycles_and_repeats() {
    let mut s = blocks();
    run(&mut s, "constraint.create", mate_a_b(json!({})));
    let r = drive(&mut s, json!({"constraint": "Mate:1", "end": 2, "increment": 1, "cycle": "start_end_start", "repeat": 2}));
    assert_eq!(values(&r), vec![0.0, 1.0, 2.0, 1.0, 0.0, 0.0, 1.0, 2.0, 1.0, 0.0], "{r}");
    // The defaults: start the current value, end start + 10, increment 1, once.
    let r = drive(&mut s, json!({"constraint": "Mate:1"}));
    assert_eq!(values(&r), (0..=10).map(f64::from).collect::<Vec<_>>(), "{r}");
}

#[test]
fn drive_stops_at_a_limit() {
    let mut s = blocks();
    run(&mut s, "constraint.create", mate_a_b(json!({"min": 0, "max": 10})));
    let r = drive(&mut s, json!({"constraint": "Mate:1", "end": 20, "steps": 10}));
    assert_eq!(r["stopped_by_limit"], json!({"value": 10.0, "limit": "max"}), "{r}");
    let v = values(&r);
    assert_eq!(v, vec![0.0, 2.0, 4.0, 6.0, 8.0, 10.0], "{r}");
    assert!((bottom(&world_in(&r, 5, &s, "B")) - 30.0).abs() <= TOL, "{r}");
    // A step past the bound adds a frame at it.
    let r = drive(&mut s, json!({"constraint": "Mate:1", "start": 1, "end": 20, "increment": 3}));
    assert_eq!(values(&r), vec![1.0, 4.0, 7.0, 10.0], "{r}");
    assert_eq!(r["stopped_by_limit"], json!({"value": 10.0, "limit": "max"}), "{r}");
    let r = drive(&mut s, json!({"constraint": "Mate:1", "start": 5, "end": -5, "steps": 5}));
    assert_eq!(values(&r), vec![5.0, 3.0, 1.0, 0.0], "{r}");
    assert_eq!(r["stopped_by_limit"], json!({"value": 0.0, "limit": "min"}), "{r}");
}

#[test]
fn an_explicit_angle_never_flips_on_a_drive() {
    let mut s = blocks();
    let p = json!({"type": "angle", "solution": "explicit_vector", "a": face("A", [50.0, 0.0, 10.0]), "b": face("B", [50.0, 0.0, 50.0]),
        "ref": {"occurrence": "A", "edge": [0.0, 0.0, 10.0]}, "offset": 30});
    assert_eq!(run(&mut s, "constraint.create", p)["status"], "ok");
    let r = drive(&mut s, json!({"constraint": "Angle:1", "start": 0, "end": 90, "increment": 5}));
    let v = values(&r);
    assert_eq!(v.len(), 19, "{r}");
    // A's front normal (-y) to B's, signed about the reference edge (+z, canonical).
    let a = apply_vector(&occurrence_world(&s.doc, occ(&s, "A")), Vec3::new(0.0, -1.0, 0.0));
    for (i, want) in v.iter().enumerate() {
        let b = apply_vector(&world_in(&r, i, &s, "B"), Vec3::new(0.0, -1.0, 0.0));
        let (pa, pb) = (Vec3::new(a.x, a.y, 0.0), Vec3::new(b.x, b.y, 0.0));
        let got = pa.cross(pb).z.atan2(pa.dot(pb)).to_degrees();
        assert!((got - want).abs() <= ANG, "frame {i}: {got} for {want}");
    }
}

/// `constraint.drive p` fails with a message containing `want`; nothing changes, no step.
fn refused(s: &mut Session, p: Value, want: &str) {
    let (steps, doc, revision) = (s.undo.len(), s.doc.clone(), s.revision);
    let e = s.execute("constraint.drive", &p).map(|v| panic!("{p}: accepted: {v}")).unwrap_err().to_string();
    assert!(e.contains(want), "{p}: `{e}` does not say `{want}`");
    assert_eq!(s.undo.len(), steps, "{p}");
    assert!(*s.doc == *doc, "{p}: the design changed");
    assert_eq!(s.revision, revision, "{p}");
}

#[test]
fn drive_refuses_hostile_counts() {
    let mut s = blocks();
    run(&mut s, "constraint.create", mate_a_b(json!({})));
    let m = |extra: Value| {
        let mut p = json!({"constraint": "Mate:1"});
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        p
    };
    for steps in [json!(0), json!(1e9), json!(-1), json!(2.5), json!("ten")] {
        refused(&mut s, m(json!({"steps": steps})), "`steps` must be a whole number from 1 to 10000");
    }
    for repeat in [json!(-1), json!(0), json!(1001), json!(1.5), json!(true)] {
        refused(&mut s, m(json!({"repeat": repeat})), "`repeat` must be a whole number from 1 to 1000");
    }
    for inc in [json!(0), json!(-1), json!("0 mm")] {
        refused(&mut s, m(json!({"increment": inc})), "`increment` must be above 0");
    }
    // A NaN number reaches a command as null.
    refused(&mut s, m(json!({"end": f64::NAN})), "`end` must be a number or an expression");
    refused(&mut s, m(json!({"start": "NaN"})), "unknown parameter `NaN`");
    refused(&mut s, m(json!({"end": 1e300})), "not a number within ±10000000 mm");
    refused(&mut s, m(json!({"end": 10000, "increment": 1})), "at most 10,000 frames");
    refused(&mut s, m(json!({"end": 10, "increment": 1e-9})), "at most 10,000 frames");
    refused(&mut s, m(json!({"steps": 10000, "repeat": 2})), "at most 10,000 frames");
    refused(&mut s, m(json!({"steps": 5000, "cycle": "start_end_start"})), "at most 10,000 frames");
    refused(&mut s, m(json!({"steps": 2, "increment": 1})), "`increment` or `steps`, not both");
    refused(&mut s, m(json!({"cycle": "loop"})), "`cycle` must be start_end or start_end_start");
    refused(&mut s, m(json!({"stop_on_contact": "yes"})), "`stop_on_contact` must be true or false");
    refused(&mut s, json!({"constraint": "Mate:9"}), "no constraint `Mate:9`");
    refused(&mut s, json!({}), "`constraint` must be an id or a name");
    run(&mut s, "constraint.suppress", json!({"constraint": "Mate:1"}));
    refused(&mut s, m(json!({})), "Mate:1 is suppressed");
}
