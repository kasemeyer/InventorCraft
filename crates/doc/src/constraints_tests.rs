#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use super::*;
use crate::Document;
use crate::joints::{Assembly, Joint};

fn face(occurrence: u64, name: &str) -> GeoRef {
    GeoRef { occurrence, geo: Geo::Face { name: name.into(), pick: Vec3::new(1.0, 2.0, 3.0) } }
}

fn origin(occurrence: u64, item: OriginItem) -> GeoRef {
    GeoRef { occurrence, geo: Geo::Origin { item } }
}

fn record(id: u64, name: &str, kind: ConstraintKind, picks: Vec<GeoRef>) -> Constraint {
    Constraint {
        id,
        name: name.into(),
        kind,
        solution: kind.default_solution(false),
        picks,
        offset: 0.0,
        limits: ConstraintLimits::default(),
        exprs: ConstraintExprs::default(),
        suppressed: false,
        lock: false,
        axis: None,
        state: Vec::new(),
    }
}

fn mate(id: u64, name: &str) -> Constraint {
    record(id, name, ConstraintKind::Mate, vec![face(1, "f1"), origin(0, OriginItem::Xy)])
}

const JOINT1: &str = r#"{"joints":[{"id":7,"name":"Joint1","kind":"rigid","a":{"occurrence":1,"snap":{"type":"frame","origin":[0.0,0.0,0.0],"z":[0.0,0.0,1.0]}},"b":{"occurrence":2,"snap":{"type":"frame","origin":[0.0,0.0,0.0],"z":[0.0,0.0,1.0]}},"values":[],"limits":[],"offset":0.0,"angle":0.0}]}"#;

fn joint1() -> Joint {
    let asm: Assembly = serde_json::from_str(JOINT1).unwrap();
    asm.joints[0].clone()
}

fn reason(r: crate::Result<()>) -> String {
    r.expect_err("expected an error").to_string()
}

#[test]
fn constraint_json_is_snake_case() {
    let c = mate(5, "Mate:1");
    let v = serde_json::to_value(&c).unwrap();
    assert_eq!(v["kind"], "mate");
    assert_eq!(v["solution"], "mate");
    assert_eq!(v["picks"][0], json!({"occurrence": 1, "geo": {"face": {"name": "f1", "pick": [1.0, 2.0, 3.0]}}}));
    assert_eq!(v["picks"][1], json!({"occurrence": 0, "geo": {"origin": {"item": "xy"}}}));
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    for absent in ["limits", "exprs", "suppressed", "lock", "axis", "state"] {
        assert!(!keys.contains(&absent), "{absent} written: {v}");
    }
    let back: Constraint = serde_json::from_value(v).unwrap();
    assert_eq!(back, c);
    assert_eq!(serde_json::to_value(ConstraintSolution::ExplicitVector).unwrap(), "explicit_vector");
    assert_eq!(serde_json::to_value(ConstraintSolution::MateFace).unwrap(), "mate");
}

#[test]
fn an_assembly_without_constraints_writes_as_before() {
    let asm: Assembly = serde_json::from_str(JOINT1).unwrap();
    assert!(asm.constraints.is_empty());
    assert_eq!(serde_json::to_string(&asm).unwrap(), JOINT1);
}

#[test]
fn names_number_per_type() {
    let mut asm = Assembly::default();
    assert_eq!(next_name(&asm, ConstraintKind::Mate), "Mate:1");
    asm.constraints.push(mate(1, "Mate:1"));
    asm.constraints.push(mate(2, "Mate:3"));
    asm.constraints.push(record(3, "Insert:1", ConstraintKind::Insert, vec![face(1, "a"), face(2, "b")]));
    assert_eq!(next_name(&asm, ConstraintKind::Mate), "Mate:4");
    assert_eq!(next_name(&asm, ConstraintKind::Insert), "Insert:2");
    assert_eq!(next_name(&asm, ConstraintKind::Angle), "Angle:1");
}

#[test]
fn names_are_unique_across_constraints_and_joints() {
    let mut asm = Assembly::default();
    asm.joints.push(joint1());
    asm.constraints.push(mate(9, "Mate:1"));
    assert!(reason(check_name(&asm, "Joint1", None)).contains("already a joint's name"));
    assert_eq!(reason(check_name(&asm, "Joint1", None)), "`Joint1` is already a joint's name");
    assert_eq!(reason(check_name(&asm, "Mate:1", None)), "`Mate:1` is already a constraint's name");
    assert_eq!(reason(check_name(&asm, "", None)), "a name can't be empty");
    assert_eq!(reason(check_name(&asm, "   ", None)), "a name can't be empty");
    assert_eq!(reason(check_name(&asm, &"x".repeat(129), None)), "a name is at most 128 characters");
    check_name(&asm, &"x".repeat(128), None).unwrap();
    check_name(&asm, "Mate:1", Some(9)).unwrap();
    check_name(&asm, "Mate:2", None).unwrap();
    assert_eq!(asm.constraint("Mate:1").map(|c| c.id), Some(9));
    assert_eq!(asm.constraint("9").map(|c| c.name.as_str()), Some("Mate:1"));
    assert!(asm.constraint("Joint1").is_none());
}

#[test]
fn validate_refuses_hostile_numbers() {
    let ok = mate(1, "Mate:1");
    ok.validate().unwrap();
    for offset in [f64::NAN, f64::INFINITY, 1e300, 1.000_000_1e7] {
        let c = Constraint { offset, ..ok.clone() };
        assert!(c.validate().is_err(), "offset {offset} accepted");
    }
    Constraint { offset: -1e7, ..ok.clone() }.validate().unwrap();
    let angle = Constraint { offset: 1.000_000_1e6, ..record(2, "Angle:1", ConstraintKind::Angle, vec![face(1, "a"), face(2, "b")]) };
    assert!(angle.validate().is_err());
    Constraint { offset: 1e6, ..angle.clone() }.validate().unwrap();
    let crossed = Constraint { limits: ConstraintLimits { min: Some(5.0), max: Some(1.0) }, ..ok.clone() };
    assert!(crossed.validate().is_err());
    for limits in [ConstraintLimits { min: Some(f64::NAN), max: None }, ConstraintLimits { min: None, max: Some(2e7) }] {
        assert!(Constraint { limits, ..ok.clone() }.validate().is_err(), "{limits:?} accepted");
    }
    assert!(Constraint { picks: vec![face(1, "a")], ..ok.clone() }.validate().is_err());
    assert!(Constraint { picks: vec![face(1, "a"), face(2, "b"), face(3, "c"), face(4, "d")], ..ok.clone() }.validate().is_err());
    let insert = record(3, "Insert:1", ConstraintKind::Insert, vec![face(1, "a"), face(2, "b")]);
    insert.validate().unwrap();
    assert!(Constraint { solution: ConstraintSolution::Flush, ..insert }.validate().is_err());
    assert!(Constraint { axis: Some(Vec3::new(f64::NAN, 0.0, 1.0)), ..ok.clone() }.validate().is_err());
    assert!(Constraint { name: " ".into(), ..ok.clone() }.validate().is_err());
    assert!(Constraint { name: "x".repeat(129), ..ok.clone() }.validate().is_err());
    // A state that does not fit is re-seeded by the solver, never refused (spec §4, R112).
    Constraint { state: vec![f64::NAN, 1.0, 2.0, 3.0, 4.0], ..ok }.validate().unwrap();
}

#[test]
fn solution_rules() {
    use ConstraintKind::*;
    use ConstraintSolution::*;
    assert_eq!([Mate, Angle, Tangent, Insert, Symmetry].map(ConstraintKind::word), ["Mate", "Angle", "Tangent", "Insert", "Symmetry"]);
    assert_eq!(ConstraintKind::parse("mate"), Some(Mate));
    assert_eq!(ConstraintKind::parse("INSERT"), Some(Insert));
    assert_eq!(ConstraintKind::parse("Symmetry"), Some(Symmetry));
    assert_eq!(ConstraintKind::parse("flush"), None);
    assert_eq!(ConstraintSolution::parse("explicit_vector"), Some(ExplicitVector));
    assert_eq!(ConstraintSolution::parse("mate"), Some(MateFace));
    assert_eq!(ConstraintSolution::parse("bogus"), None);

    assert_eq!(Mate.default_solution(false), MateFace);
    assert_eq!(Mate.default_solution(true), Opposed);
    assert_eq!(Angle.default_solution(false), ANGLE_DEFAULT);
    assert_eq!(ANGLE_DEFAULT, Undirected);
    assert_eq!(Tangent.default_solution(false), Outside);
    assert_eq!(Insert.default_solution(false), Opposed);
    assert_eq!(Symmetry.default_solution(false), Opposed);

    assert_eq!(Angle.picks(ExplicitVector), 3);
    assert_eq!(Angle.picks(Directed), 2);
    assert_eq!(Angle.picks(Undirected), 2);
    assert_eq!(Symmetry.picks(Opposed), 3);
    assert_eq!(Mate.picks(MateFace), 2);
    assert_eq!(Tangent.picks(Inside), 2);
    assert_eq!(Insert.picks(Aligned), 2);

    assert!(Angle.is_angle());
    assert!(!Mate.is_angle());

    let all = [MateFace, Flush, Opposed, Aligned, Undirected, Directed, ExplicitVector, Inside, Outside];
    let fits = |k: ConstraintKind| all.iter().copied().filter(|s| s.fits(k)).collect::<Vec<_>>();
    assert_eq!(fits(Mate), [MateFace, Flush, Opposed, Aligned, Undirected]);
    assert_eq!(fits(Angle), [Undirected, Directed, ExplicitVector]);
    assert_eq!(fits(Tangent), [Inside, Outside]);
    assert_eq!(fits(Insert), [Opposed, Aligned]);
    assert_eq!(fits(Symmetry), [Opposed, Aligned]);
    let words: Vec<Value> = all.iter().map(|s| serde_json::to_value(s).unwrap()).collect();
    assert_eq!(words, ["mate", "flush", "opposed", "aligned", "undirected", "directed", "explicit_vector", "inside", "outside"]);
}

#[test]
fn expressions_follow_constraints() {
    let mut asm = Assembly::default();
    assert!(asm.is_empty());
    let mut c = mate(11, "Mate:1");
    c.exprs = ConstraintExprs { offset: Some("t * 2".into()), min: None, max: Some("u".into()) };
    asm.constraints.push(c);
    assert!(!asm.is_empty());
    assert_eq!(asm.exprs(), vec![(11, "offset".to_string(), "t * 2"), (11, "max".to_string(), "u")]);
    for e in asm.exprs_mut() {
        *e = crate::expr::rename_reference(&crate::expr::rename_reference(e, "t", "v"), "u", "w");
    }
    assert_eq!(asm.constraints[0].exprs, ConstraintExprs { offset: Some("v * 2".into()), min: None, max: Some("w".into()) });

    // A parameter's users name the constraint (no joint has its id).
    let mut doc = Document::new("A");
    doc.assembly = asm;
    let users = doc.param_users("v");
    assert!(
        users.iter().any(|u| matches!(u, crate::ParamUser::Constraint { constraint: 11, name, input } if name == "Mate:1" && input == "offset")),
        "{users:?}"
    );
}

#[test]
fn format_pending_lists_assembly_constraints() {
    assert!(crate::format::pending_names().iter().any(|n| n == "assembly-constraints"), "{:?}", crate::format::pending_names());
}

#[test]
fn removing_a_component_takes_its_constraints() {
    let mut doc = Document::new("A");
    let a = doc.add_component(Some("A"), 0).unwrap();
    let b = doc.add_component(Some("B"), 0).unwrap();
    let c = doc.add_component(Some("C"), 0).unwrap();
    let occ = |doc: &Document, comp: u64| doc.occurrences.iter().find(|o| o.component == comp).unwrap().id;
    let (oa, ob, oc) = (occ(&doc, a), occ(&doc, b), occ(&doc, c));
    doc.assembly.constraints.push(record(100, "Mate:1", ConstraintKind::Mate, vec![face(oa, "f"), face(ob, "g")]));
    doc.assembly.constraints.push(record(101, "Mate:2", ConstraintKind::Mate, vec![face(oa, "f"), face(oc, "h")]));
    doc.remove_component_tree(c).unwrap();
    let left: Vec<&str> = doc.assembly.constraints.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(left, ["Mate:1"]);
}

/// Opening a file runs `validate` on each constraint; a state that does not fit still loads (the
/// solver re-seeds it, spec §4, R112).
#[test]
fn loading_validates_constraints() {
    let file = |constraints: Value| {
        let mut v: Value = serde_json::from_str(&Document::new("A").to_json()).unwrap();
        v["assembly"] = json!({ "constraints": constraints });
        v.to_string()
    };
    let good = serde_json::to_value(mate(1, "Mate:1")).unwrap();
    let opened = Document::from_json(&file(json!([good.clone()]))).unwrap();
    assert_eq!(opened.assembly.constraints, vec![mate(1, "Mate:1")]);

    let mut bad = good.clone();
    bad["offset"] = json!(2e7);
    let err = Document::from_json(&file(json!([bad]))).unwrap_err().to_string();
    assert!(err.starts_with("constraint Mate:1: "), "{err}");

    // serde_json writes a NaN as `null`.
    let mut corrupt = good.clone();
    corrupt["state"] = json!([null, 1.0, 2.0]);
    let opened = Document::from_json(&file(json!([corrupt]))).unwrap();
    assert_eq!(opened.assembly.constraints[0].state.len(), 3);
    assert!(opened.assembly.constraints[0].state[0].is_nan());

    let many: Vec<Value> = (0..=MAX_CONSTRAINTS as u64)
        .map(|i| {
            let mut c = good.clone();
            c["id"] = json!(i + 1);
            c["name"] = json!(format!("Mate:{}", i + 1));
            c
        })
        .collect();
    assert_eq!(Document::from_json(&file(Value::from(many))).unwrap_err().to_string(), "too many constraints");
}

/// R128: a saved record has exactly the picks its solution takes, an id below `COMPILED_BASE`, and an
/// id and name no other constraint has.
#[test]
fn loading_refuses_wrong_picks_ids_and_names() {
    let file = |constraints: Vec<Constraint>| {
        let mut v: Value = serde_json::from_str(&Document::new("A").to_json()).unwrap();
        v["assembly"] = json!({ "constraints": constraints });
        v.to_string()
    };
    let open_err = |constraints: Vec<Constraint>| Document::from_json(&file(constraints)).unwrap_err().to_string();

    let short_vector =
        Constraint { solution: ConstraintSolution::ExplicitVector, ..record(1, "Angle:1", ConstraintKind::Angle, vec![face(1, "a"), face(2, "b")]) };
    assert_eq!(short_vector.validate().unwrap_err().to_string(), "Angle takes 3 picks, not 2");
    assert_eq!(open_err(vec![short_vector]), "constraint Angle:1: Angle takes 3 picks, not 2");

    let long_mate = record(2, "Mate:1", ConstraintKind::Mate, vec![face(1, "a"), face(2, "b"), face(3, "c")]);
    assert_eq!(long_mate.validate().unwrap_err().to_string(), "Mate takes 2 picks, not 3");
    assert_eq!(open_err(vec![long_mate]), "constraint Mate:1: Mate takes 2 picks, not 3");
    let symmetry = record(3, "Symmetry:1", ConstraintKind::Symmetry, vec![face(1, "a"), face(2, "b")]);
    assert_eq!(symmetry.validate().unwrap_err().to_string(), "Symmetry takes 3 picks, not 2");
    Constraint { picks: vec![face(1, "a"), face(2, "b"), face(3, "c")], ..symmetry }.validate().unwrap();

    assert_eq!(open_err(vec![mate(4, "Mate:1"), mate(4, "Mate:2")]), "constraint Mate:2: id 4 is already another constraint's");
    assert_eq!(open_err(vec![mate(4, "Mate:1"), mate(5, "Mate:1")]), "constraint Mate:1: the name is already another constraint's");
    assert_eq!(
        open_err(vec![mate(COMPILED_BASE, "Mate:1")]),
        format!("constraint Mate:1: id {COMPILED_BASE} is in the range kept for compiled joints")
    );
    Document::from_json(&file(vec![mate(COMPILED_BASE - 1, "Mate:1"), mate(5, "Mate:2")])).unwrap();
}
