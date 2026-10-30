#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Constraint status (Plan E-1 Task 5, spec §3.7, §5, §6.8): sick, lost and redundant
//! constraints, the over-constraint refusal at create, edit and unsuppress, the session's report,
//! Show / Show Problems / Hide All and the dry-run preview, on the blocks fixture. Positions are
//! world, to 1e-9 mm.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_doc::Mat;
use solidcraft_doc::apply_point;
use solidcraft_doc::constraints::{ConstraintKind, ConstraintSolution, ConstraintStatus, Geo, GeoRef, PickWhat, PrimKind, geo_at};
use solidcraft_doc::joints::occurrence_world;
use solidcraft_geom::Vec3;

use crate::Session;
use crate::constraints::{Draft, Prediction, accepts, classify, preview, report};
use crate::test_support::{at, blocks, occ, run};

const TOL: f64 = 1e-9;

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}

/// A face pick at a world point.
fn face(name: &str, p: Vec3) -> Value {
    json!({"occurrence": name, "face": [p.x, p.y, p.z]})
}

/// A face pick on `name` at its component point `p` (where the part is now).
fn on(s: &Session, name: &str, p: Vec3) -> Value {
    face(name, at(s, name, p))
}

fn transform(s: &Session, name: &str) -> Mat {
    occurrence_world(&s.doc, occ(s, name))
}

fn same(a: &Mat, b: &Mat) -> bool {
    a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() <= TOL)
}

/// The z of B's bottom face (its component point (50, 25, 40)).
fn b_bottom(s: &Session) -> f64 {
    at(s, "B", v(50.0, 25.0, 40.0)).z
}

/// A Mate of A's top on B's bottom, picked at component point (x, y) of each.
fn mate_top_bottom(s: &Session, x: f64, y: f64, offset: Value) -> Value {
    json!({"type": "mate", "a": on(s, "A", v(x, y, 20.0)), "b": on(s, "B", v(x, y, 40.0)), "offset": offset})
}

/// Task 3 test 7's three constraints, made with `constraint.create`: B fixed on A.
fn three_mates(s: &mut Session) {
    let p = mate_top_bottom(s, 50.0, 25.0, json!(0));
    run(s, "constraint.create", p);
    let p = json!({"type": "mate", "solution": "flush", "a": on(s, "A", v(0.0, 25.0, 10.0)), "b": on(s, "B", v(0.0, 25.0, 50.0))});
    run(s, "constraint.create", p);
    let p = json!({"type": "mate", "solution": "flush", "a": on(s, "A", v(50.0, 0.0, 10.0)), "b": on(s, "B", v(50.0, 0.0, 50.0))});
    run(s, "constraint.create", p);
}

/// The status of constraint `name` in the session's report.
fn status(s: &Session, name: &str) -> ConstraintStatus {
    report(s).rows.iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no row {name}: {:?}", report(s).rows)).status.clone()
}

fn id_of(s: &Session, name: &str) -> u64 {
    s.doc.assembly.constraint(name).unwrap().id
}

/// `id p` fails with a message containing `want`; nothing changes, no undo step.
fn refused(s: &mut Session, id: &str, p: Value, want: &str) {
    let (steps, doc) = (s.undo.len(), s.doc.clone());
    let e = s.execute(id, &p).map(|v| panic!("{id} {p}: accepted: {v}")).unwrap_err().to_string();
    assert!(e.contains(want), "{id} {p}: `{e}` does not say `{want}`");
    assert_eq!(s.undo.len(), steps, "{id} {p}");
    assert!(*s.doc == *doc, "{id} {p}: the design changed");
}

/// The Insert of the plate's hole rim and the pin's bottom rim.
fn insert_plate_pin() -> Value {
    json!({"type": "insert", "a": {"occurrence": "Plate", "edge": [200.0, 0.0, 10.0]}, "b": {"occurrence": "Pin", "edge": [310.0, 0.0, 50.0]}})
}

/// The plate cut short of its hole (x 150-175; the hole is x 180-200): the rim is gone.
fn cut_the_plate_short(s: &mut Session) {
    run(s, "timeline.edit", json!({"feature": "Plate", "set": {"length": "25 mm"}}));
}

const OVER: &str = "over-constrains B: Mate:4 off by 2.000 mm";

#[test]
fn a_fourth_mate_that_disagrees_is_refused() {
    let mut s = blocks();
    three_mates(&mut s);
    let before = transform(&s, "B");
    let p = mate_top_bottom(&s, 20.0, 10.0, json!(2));
    refused(&mut s, "constraint.create", p, OVER);
    assert!(same(&transform(&s, "B"), &before));
    assert_eq!(s.doc.assembly.constraints.len(), 3);
}

#[test]
fn a_redundant_mate_is_accepted() {
    let mut s = blocks();
    three_mates(&mut s);
    let p = mate_top_bottom(&s, 20.0, 10.0, json!(0));
    let r = run(&mut s, "constraint.create", p);
    assert_eq!(r["name"], "Mate:4", "{r}");
    assert_eq!(r["status"], "ok", "{r}");
    assert_eq!(status(&s, "Mate:4"), ConstraintStatus::Ok, "the report never says redundant");
    let i = run(&mut s, "constraint.inspect", json!({"constraint": "Mate:4"}));
    assert_eq!(i["constraints"][0]["status"], "redundant", "{i}");
    // A constraint that takes freedom away is not redundant.
    let mut t = blocks();
    let p = mate_top_bottom(&t, 50.0, 25.0, json!(0));
    run(&mut t, "constraint.create", p);
    let i = run(&mut t, "constraint.inspect", json!({"constraint": "Mate:1"}));
    assert_eq!(i["constraints"][0]["status"], "ok", "{i}");
}

#[test]
fn a_part_edit_makes_it_sick_or_lost_and_restores() {
    let mut s = blocks();
    run(&mut s, "constraint.create", insert_plate_pin());
    assert_eq!(status(&s, "Insert:1"), ConstraintStatus::Ok);
    cut_the_plate_short(&mut s);
    match status(&s, "Insert:1") {
        ConstraintStatus::Lost { reason } => assert!(reason.contains("no longer exists"), "{reason}"),
        other => panic!("Insert:1 is {other:?}"),
    }
    let l = run(&mut s, "constraint.list", json!({}));
    assert_eq!(l["constraints"][0]["status"], "lost", "{l}");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(status(&s, "Insert:1"), ConstraintStatus::Ok);

    // Two Mates on one pair: B's tree member is Mate:1, Mate:2 closes the loop (R110).
    let mut s = blocks();
    let p = mate_top_bottom(&s, 50.0, 25.0, json!(0));
    run(&mut s, "constraint.create", p);
    let p = mate_top_bottom(&s, 20.0, 10.0, json!(0));
    run(&mut s, "constraint.create", p);
    refused(&mut s, "constraint.edit", json!({"constraint": "Mate:1", "offset": 2}), "over-constrains B: Mate:2 off by 2.000 mm");
    run(&mut s, "parameters.add", json!({"name": "t", "expression": "0 mm"}));
    run(&mut s, "constraint.edit", json!({"constraint": "Mate:1", "offset": "t"}));
    // After creation a parameter only flags it.
    run(&mut s, "parameters.change", json!({"name": "t", "expression": "2 mm"}));
    match status(&s, "Mate:2") {
        ConstraintStatus::Sick { gap_mm, gap_deg, with } => {
            assert!((gap_mm - 2.0).abs() <= 1e-6, "{gap_mm}");
            assert!(gap_deg.abs() <= 1e-6, "{gap_deg}");
            assert_eq!(with, Vec::<String>::new());
        }
        other => panic!("Mate:2 is {other:?}"),
    }
    assert_eq!(status(&s, "Mate:1"), ConstraintStatus::Ok);
    assert!(report(&s).any_sick);
    let l = run(&mut s, "constraint.list", json!({}));
    assert_eq!(l["constraints"][1]["status"], "sick", "{l}");
}

#[test]
fn unsuppressing_into_a_conflict_is_refused() {
    let mut s = blocks();
    let p = mate_top_bottom(&s, 50.0, 25.0, json!(0));
    run(&mut s, "constraint.create", p);
    let mut p = mate_top_bottom(&s, 20.0, 10.0, json!(2));
    p["suppressed"] = json!(true);
    let r = run(&mut s, "constraint.create", p);
    assert_eq!(r["status"], "suppressed", "{r}");
    refused(&mut s, "constraint.suppress", json!({"constraint": "Mate:2", "suppressed": false}), "over-constrains B: Mate:2 off by 2.000 mm");
    assert!(s.doc.assembly.constraint("Mate:2").unwrap().suppressed);
    assert_eq!(status(&s, "Mate:2"), ConstraintStatus::Suppressed);
    // Toggling it is refused the same way.
    refused(&mut s, "constraint.suppress", json!({"constraint": "Mate:2"}), "over-constrains");
}

#[test]
fn show_sick_and_hide_all() {
    let mut s = blocks();
    let p = mate_top_bottom(&s, 50.0, 25.0, json!(0));
    run(&mut s, "constraint.create", p);
    run(&mut s, "constraint.create", insert_plate_pin());
    let show_sick = crate::find_command("constraint.show_sick").unwrap();
    let info = show_sick.info(&s);
    assert!(!info.enabled);
    assert_eq!(info.disabled_reason.as_deref(), Some("nothing is sick"));
    cut_the_plate_short(&mut s);
    let steps = s.undo.len();
    assert!(show_sick.info(&s).enabled);
    let r = run(&mut s, "constraint.show_sick", json!({}));
    assert_eq!(r["shown"], json!(["Insert:1"]), "{r}");
    assert_eq!(s.shown_constraints, BTreeSet::from([id_of(&s, "Insert:1")]));
    let r = run(&mut s, "constraint.hide_all", json!({}));
    assert_eq!(r["shown"], json!([]), "{r}");
    assert!(s.shown_constraints.is_empty());
    assert_eq!(s.undo.len(), steps, "view state is no undo step");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(status(&s, "Insert:1"), ConstraintStatus::Ok);
    let info = show_sick.info(&s);
    assert!(!info.enabled);
    assert_eq!(info.disabled_reason.as_deref(), Some("nothing is sick"));
    let steps = s.undo.len();
    let r = run(&mut s, "constraint.show", json!({"occurrences": ["B"]}));
    assert_eq!(r["shown"], json!(["Mate:1"]), "{r}");
    let pin = occ(&s, "Pin");
    let r = run(&mut s, "constraint.show", json!({"occurrences": [pin]}));
    assert_eq!(r["shown"], json!(["Mate:1", "Insert:1"]), "Show adds: {r}");
    let r = run(&mut s, "constraint.show", json!({"occurrences": []}));
    assert_eq!(r["shown"], json!([]), "an empty list clears: {r}");
    assert_eq!(s.undo.len(), steps);
    refused(&mut s, "constraint.show", json!({"occurrences": "B"}), "`occurrences` must be a list of occurrence ids or names");
    refused(&mut s, "constraint.show", json!({}), "`occurrences` must be a list of occurrence ids or names");
    refused(&mut s, "constraint.show", json!({"occurrences": ["Nobody"]}), "no occurrence `Nobody`");
    refused(&mut s, "constraint.show", json!({"occurrences": [{"x": 1}]}), "an occurrence must be an id or a name");
    refused(&mut s, "constraint.show", json!({"occurrences": vec![json!("B"); 100_001]}), "at most 100000 occurrences");
}

#[test]
fn undo_restores_pose_state_and_report() {
    let mut s = blocks();
    let p = mate_top_bottom(&s, 50.0, 25.0, json!(0));
    run(&mut s, "constraint.create", p);
    run(&mut s, "constraint.show", json!({"occurrences": ["B"]}));
    assert_eq!(s.shown_constraints.len(), 1);
    assert!((b_bottom(&s) - 20.0).abs() <= TOL);
    run(&mut s, "edit.undo", json!({}));
    assert!((b_bottom(&s) - 40.0).abs() <= TOL, "{}", b_bottom(&s));
    assert!(report(&s).rows.is_empty(), "{:?}", report(&s).rows);
    assert!(s.shown_constraints.is_empty(), "the glyph of a constraint that no longer exists is dropped");
    run(&mut s, "edit.redo", json!({}));
    assert!((b_bottom(&s) - 20.0).abs() <= TOL, "{}", b_bottom(&s));
    assert_eq!(s.doc.assembly.constraints[0].state.len(), 3);
    let rows = &report(&s).rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, ConstraintStatus::Ok);
    assert_eq!(rows[0].kind, ConstraintKind::Mate);
    assert_eq!(rows[0].occurrences, vec![occ(&s, "A"), occ(&s, "B")]);
    let [pa, pb] = rows[0].anchors.unwrap();
    assert!((pa.z - 20.0).abs() <= TOL && (pb.z - 20.0).abs() <= TOL, "{pa:?} {pb:?}");
    // A checkpoint restored rebuilds it too.
    let cp = run(&mut s, "edit.checkpoint", json!({}))["checkpoint"].as_u64().unwrap();
    run(&mut s, "constraint.delete", json!({"constraint": "Mate:1"}));
    assert!(report(&s).rows.is_empty());
    run(&mut s, "edit.restore_checkpoint", json!({"checkpoint": cp}));
    assert_eq!(report(&s).rows.len(), 1);
}

fn pick(s: &Session, name: &str, what: PickWhat, p: Vec3) -> GeoRef {
    geo_at(&s.doc, &s.model.state(), occ(s, name), what, p).unwrap()
}

#[test]
fn preview_never_changes_the_session() {
    let mut s = blocks();
    let (before, steps) = (s.doc.clone(), s.undo.len());
    let b0 = transform(&s, "B");
    let draft = Draft {
        a: Some(pick(&s, "A", PickWhat::Face, v(50.0, 25.0, 20.0))),
        b: Some(pick(&s, "B", PickWhat::Face, v(50.0, 25.0, 40.0))),
        ..Draft::default()
    };
    assert_eq!(draft.kind, ConstraintKind::Mate);
    let pv = preview(&s, &draft);
    assert_eq!(pv.refused, None);
    let Some(Prediction { offset, solution }) = pv.predicted else { panic!("no prediction: {pv:?}") };
    assert!((offset - 20.0).abs() <= TOL, "{offset}");
    assert_eq!(solution, ConstraintSolution::MateFace);
    let b = occ(&s, "B");
    assert!(same(&pv.world[&b], &b0), "the predicted offset holds B where it is");
    assert_eq!(pv.status, Some(ConstraintStatus::Ok));
    assert_eq!(pv.dof[&b], 3);
    let pv = preview(&s, &Draft { offset: Some("0".into()), ..draft.clone() });
    assert_eq!(pv.predicted, None);
    assert!((apply_point(&pv.world[&b], v(50.0, 25.0, 40.0)).z - 20.0).abs() <= TOL);
    assert!(Arc::ptr_eq(&s.doc, &before));
    assert_eq!(s.undo.len(), steps);
    // The params run the same command.
    assert_eq!(draft.params()["type"], "mate");
    assert_eq!(draft.params()["predict"], true);

    three_mates(&mut s);
    let before = s.doc.clone();
    let over = Draft {
        a: Some(pick(&s, "A", PickWhat::Face, v(20.0, 10.0, 20.0))),
        b: Some(pick(&s, "B", PickWhat::Face, at(&s, "B", v(20.0, 10.0, 40.0)))),
        offset: Some("2".into()),
        ..Draft::default()
    };
    let pv = preview(&s, &over);
    assert_eq!(pv.refused.as_deref(), Some(OVER));
    assert!(pv.world.is_empty());
    assert!(Arc::ptr_eq(&s.doc, &before));
    // Editing: with Mate:4 made at offset 0 (redundant), Mate:1 at offset 2 leaves Mate:4, a
    // closing member, off by 2 (R110). The loop group then does not close, so its other closing
    // members (Mate:2, Mate:3) stay where they were and are named too; the tree member Mate:1 is
    // not. A draft that keeps the picks leaves them out.
    let p = mate_top_bottom(&s, 20.0, 10.0, json!(0));
    run(&mut s, "constraint.create", p);
    let edit = Draft { editing: Some(id_of(&s, "Mate:1")), offset: Some("2".into()), ..Draft::default() };
    assert_eq!(edit.params()["constraint"], id_of(&s, "Mate:1"));
    let before = s.doc.clone();
    let pv = preview(&s, &edit);
    assert_eq!(pv.refused.as_deref(), Some("over-constrains B: Mate:2, Mate:3, Mate:4 off by 2.000 mm"), "{pv:?}");
    assert!(Arc::ptr_eq(&s.doc, &before));
    let pv = preview(&s, &Draft { offset: Some("0".into()), ..edit });
    assert_eq!(pv.refused, None, "{pv:?}");
    assert_eq!(pv.status, Some(ConstraintStatus::Ok));
    // An incomplete draft says what it is missing.
    let pv = preview(&s, &Draft { b: None, ..draft });
    assert!(pv.refused.as_deref().is_some_and(|r| r.contains("`b` is missing")), "{pv:?}");
}

#[test]
fn accepts_follows_the_types() {
    use PrimKind::*;
    let all = [Plane, Line, Point, Circle, Cylinder, Sphere];
    assert!(accepts(ConstraintKind::Insert, 0, None, Circle));
    assert!(!accepts(ConstraintKind::Insert, 0, None, Plane));
    assert!(!accepts(ConstraintKind::Insert, 1, Some(Circle), Cylinder));
    assert!(accepts(ConstraintKind::Tangent, 1, Some(Plane), Cylinder));
    assert!(accepts(ConstraintKind::Tangent, 1, Some(Plane), Sphere));
    assert!(!accepts(ConstraintKind::Tangent, 1, Some(Plane), Plane));
    assert!(accepts(ConstraintKind::Tangent, 1, Some(Cylinder), Plane));
    assert!(!accepts(ConstraintKind::Tangent, 1, Some(Sphere), Sphere));
    for k in all {
        assert!(accepts(ConstraintKind::Mate, 0, None, k), "{k:?}");
        for first in [Point, Line, Plane] {
            assert!(accepts(ConstraintKind::Mate, 1, Some(first), k), "{first:?} then {k:?}");
        }
    }
    assert!(!accepts(ConstraintKind::Mate, 2, Some(Plane), Plane));
    assert!(accepts(ConstraintKind::Angle, 2, Some(Plane), Line));
    assert!(!accepts(ConstraintKind::Angle, 0, None, Point));
    let s = blocks();
    let rim = pick(&s, "Plate", PickWhat::Edge, v(200.0, 0.0, 10.0));
    assert_eq!(classify(&s, &rim), Some(Circle));
    let gone = GeoRef { occurrence: occ(&s, "B"), geo: Geo::Face { name: "Nowhere:1".into(), pick: Vec3::ZERO } };
    assert_eq!(classify(&s, &gone), None);
}

#[test]
fn every_occurrence_has_a_status_row() {
    // R141: before any constraint exists, every occurrence has its row.
    use crate::constraints::OccurrenceStatus;
    let mut s = blocks();
    let row = |s: &Session, name: &str| report(s).occurrences.iter().find(|o| o.id == occ(s, name)).cloned().unwrap();
    assert_eq!(report(&s).occurrences.len(), s.doc.occurrences.len());
    let a = row(&s, "A");
    assert!(a.grounded);
    assert_eq!((a.missing, a.status), (Some(0), OccurrenceStatus::Empty));
    let b = row(&s, "B");
    assert!(!b.grounded);
    assert_eq!((b.missing, b.status), (Some(6), OccurrenceStatus::Empty));
    let p = mate_top_bottom(&s, 50.0, 25.0, json!(0));
    run(&mut s, "constraint.create", p);
    assert_eq!(report(&s).occurrences.len(), s.doc.occurrences.len());
    let b = row(&s, "B");
    assert_eq!((b.missing, b.status), (Some(3), OccurrenceStatus::Ok));
    assert_eq!(row(&s, "A").status, OccurrenceStatus::Ok);
    let pin = row(&s, "Pin");
    assert_eq!((pin.missing, pin.status), (Some(6), OccurrenceStatus::Empty));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(row(&s, "B").status, OccurrenceStatus::Empty);
}

#[test]
fn an_opened_assembly_has_a_warm_memo_and_its_report() {
    // R131 (carried from Task 3): `doc.open` warms the DOF memo; the open's refresh builds the
    // report, a sick constraint included.
    let mut made = blocks();
    let p = mate_top_bottom(&made, 50.0, 25.0, json!(0));
    run(&mut made, "constraint.create", p);
    run(&mut made, "constraint.create", insert_plate_pin());
    // A second Mate on the pair closes a loop: the memo keeps loop counts only.
    let p = mate_top_bottom(&made, 20.0, 10.0, json!(0));
    run(&mut made, "constraint.create", p);
    let dir = std::env::temp_dir().join(format!("tcicad-con-{}-an_opened_assembly_has_a_warm_memo_and_its_report", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("blocks.tciasm");
    run(&mut made, "file.save", json!({"path": path.to_string_lossy(), "format": "json"}));
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path.to_string_lossy()}));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!s.dof_memo.lock().unwrap().is_empty(), "the open warms the memo");
    let names: Vec<&str> = report(&s).rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Mate:1", "Insert:1", "Mate:2"]);
    assert!(report(&s).rows.iter().all(|r| r.status == ConstraintStatus::Ok), "{:?}", report(&s).rows);
    assert_eq!(report(&s).occurrences.len(), s.doc.occurrences.len());
}
