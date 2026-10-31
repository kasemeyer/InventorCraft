#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Plan E-2 Task 4: constraint rows under Relationships and under both components, the
//! components' status markers, the problem / limit / suppressed badges and tooltips, and the
//! profile's Grounded entry (spec §6.4, §6.5; R135, R139, R141, R146, R161). Inventor tests run on
//! `inventor(Assembly)` with E-1's blocks; SolidCraft (`Harness::new()`) gets the same rows under
//! its Joints folder and keeps Ground / Unground.

use serde_json::{Value, json};
use solidcraft_engine::doc::DocumentKind::Assembly;
use solidcraft_engine::doc::constraints::ConstraintStatus;

use super::{DrawnConstraint, constraint_rows, drawn_constraints, status_marker, status_tip};
use crate::context_menu::Target;
use crate::scenario::{Harness, handle_at};
use crate::test_support::{constraint_blocks, constraint_id, inventor, menu_pick, right_click};

fn blocks_on(mut h: Harness) -> Harness {
    constraint_blocks(&mut h);
    h.frames(2);
    h
}

fn blocks() -> Harness {
    blocks_on(inventor(Assembly))
}

fn occ(h: &Harness, name: &str) -> u64 {
    h.app.session.doc.occurrences.iter().find(|o| o.name == format!("{name}:1")).map(|o| o.id).unwrap_or_else(|| panic!("no occurrence {name}"))
}

/// The world point of `name`'s component point `p` (where the part is now).
fn at(h: &Harness, name: &str, p: [f64; 3]) -> [f64; 3] {
    let m = solidcraft_engine::doc::joints::occurrence_world(&h.app.session.doc, occ(h, name));
    let w = solidcraft_engine::doc::apply_point(&m, solidcraft_engine::geom::Vec3::new(p[0], p[1], p[2]));
    [w.x, w.y, w.z]
}

fn face(h: &Harness, name: &str, p: [f64; 3]) -> Value {
    json!({"occurrence": name, "face": at(h, name, p)})
}

fn run(h: &mut Harness, id: &str, p: Value) -> Value {
    let r = h.app.run(id, p.clone()).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    h.frames(2);
    r
}

/// A's top on B's bottom, picked at A's (x, y) and B's (x, y + 70).
fn mate_ab(h: &Harness, x: f64, y: f64, extra: Value) -> Value {
    let mut p = json!({"type": "mate", "a": face(h, "A", [x, y, 20.0]), "b": face(h, "B", [x, y + 70.0, 7.0])});
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    p
}

/// The plate's hole rim into the pin's bottom rim.
fn insert_plate_pin() -> Value {
    json!({"type": "insert", "a": {"occurrence": "Plate", "edge": [200.0, 0.0, 10.0]}, "b": {"occurrence": "Pin", "edge": [310.0, 0.0, 50.0]}})
}

fn drawn(name: &str) -> Vec<DrawnConstraint> {
    drawn_constraints().into_iter().filter(|d| d.label == name).collect()
}

fn missing(h: &Harness, name: &str) -> Option<usize> {
    let id = occ(h, name);
    h.app.session.constraint_report.occurrences.iter().find(|o| o.id == id).unwrap_or_else(|| panic!("no report row for {name}")).missing
}

fn has_badge(badge: &str, label: &str) -> bool {
    handle_at(&format!("badge:{badge}:{label}")).is_some()
}

fn has_marker(marker: &str, label: &str) -> bool {
    handle_at(&format!("marker:{marker}:{label}")).is_some()
}

/// Test 1: Relationships first (sorted by id), then each component's constraints in browser
/// order; the Relationships folder has no expander until the first constraint.
#[test]
fn each_constraint_shows_three_times() {
    let mut h = blocks();
    let rel = crate::browser::drawn_rows().into_iter().find(|r| r.0 == "Relationships").expect("the Relationships folder is drawn");
    assert_eq!(rel.3, None, "no expander before the first constraint");
    let p = mate_ab(&h, 50.0, 25.0, json!({}));
    run(&mut h, "constraint.create", p);
    run(&mut h, "constraint.create", insert_plate_pin());
    let (mate, insert) = (constraint_id(&h, "Mate:1"), constraint_id(&h, "Insert:1"));
    let (a, b, plate, pin) = (occ(&h, "A"), occ(&h, "B"), occ(&h, "Plate"), occ(&h, "Pin"));
    let want = vec![(mate, 0), (insert, 0), (mate, a), (mate, b), (insert, plate), (insert, pin)];
    assert_eq!(constraint_rows(&h.app), want);
    let order_drawn: Vec<(u64, u64)> = drawn_constraints().iter().map(|d| (d.id, d.under)).collect();
    assert_eq!(order_drawn, want, "the drawn rows follow constraint_rows");
    let labels: Vec<String> = drawn_constraints().into_iter().map(|d| d.label).collect();
    assert_eq!(labels, ["Mate:1", "Insert:1", "Mate:1", "Mate:1", "Insert:1", "Insert:1"]);
    assert!(drawn_constraints().iter().all(|d| d.icon.starts_with("constraint_")), "type icons");
    assert_eq!(drawn("Mate:1")[0].icon, "constraint_mate");
    assert_eq!(drawn("Insert:1")[0].icon, "constraint_insert");
    let rel = crate::browser::drawn_rows().into_iter().find(|r| r.0 == "Relationships").unwrap();
    assert_eq!(rel.3, Some(true), "the folder folds once it has rows");
    // Relationships sits before Origin, the components after it.
    let order: Vec<String> = crate::browser::drawn_rows().into_iter().map(|r| r.0).collect();
    let pos = |l: &str| order.iter().position(|x| x == l).unwrap_or_else(|| panic!("{l} not drawn: {order:?}"));
    assert!(pos("Relationships") < pos("Origin"), "{order:?}");
    assert!(pos("Origin") < pos("A:1"), "{order:?}");
}

/// Test 2 (§8.3 9, R141, R146): markers from `Report.occurrences`; Grounded toggles the pin.
#[test]
fn status_markers_and_the_pin() {
    let mut h = blocks();
    let (a, b) = (occ(&h, "A"), occ(&h, "B"));
    assert_eq!(status_marker(&h.app, a), "status_full");
    assert!(has_marker("status_full", "A:1") && has_badge("pin", "A:1"));
    let b_row = h.app.session.constraint_report.occurrences.iter().find(|o| o.id == b).unwrap().clone();
    assert_eq!(b_row.status, solidcraft_engine::constraints::OccurrenceStatus::Empty);
    assert_eq!(b_row.missing, Some(6));
    assert_eq!(status_marker(&h.app, b), "status_empty");
    assert!(has_marker("status_empty", "B:1") && !has_badge("pin", "B:1"));

    let p = mate_ab(&h, 50.0, 25.0, json!({}));
    run(&mut h, "constraint.create", p);
    assert_eq!(missing(&h, "B"), Some(3));
    assert_eq!(status_marker(&h.app, b), "status_half");
    assert!(has_marker("status_half", "B:1"));
    // Two more fix B.
    let p = json!({"type": "mate", "solution": "flush", "a": face(&h, "A", [0.0, 25.0, 10.0]), "b": face(&h, "B", [0.0, 95.0, 17.0])});
    run(&mut h, "constraint.create", p);
    let p = json!({"type": "mate", "solution": "flush", "a": face(&h, "A", [50.0, 0.0, 10.0]), "b": face(&h, "B", [50.0, 70.0, 17.0])});
    run(&mut h, "constraint.create", p);
    assert_eq!(missing(&h, "B"), Some(0));
    assert_eq!(status_marker(&h.app, b), "status_full");
    assert!(has_marker("status_full", "B:1"));

    // The component menu's Grounded, checked on A; unchecking it drops the pin.
    let path = vec![a];
    right_click(&mut h, json!({"handle": format!("row:occurrence:{a}")}));
    let items = crate::context_menu::items(&h.app, &Target::Occurrence { path: path.clone() });
    let ground = items.iter().find(|i| i.id == "occurrence.ground").expect("the menu has Grounded");
    assert_eq!((ground.label.as_str(), ground.checked), ("Grounded", Some(true)));
    menu_pick(&mut h, "menu", "occurrence.ground");
    h.frames(2);
    assert!(!h.app.session.doc.occurrences.iter().find(|o| o.id == a).unwrap().grounded);
    assert!(!has_badge("pin", "A:1"), "ungrounded: no pin");
    let want = match missing(&h, "A") {
        Some(0) => "status_full",
        Some(6) => "status_empty",
        _ => "status_half",
    };
    assert_eq!(status_marker(&h.app, a), want, "A's marker follows its missing ({:?})", missing(&h, "A"));
    right_click(&mut h, json!({"handle": format!("row:occurrence:{a}")}));
    let items = crate::context_menu::items(&h.app, &Target::Occurrence { path });
    let ground = items.iter().find(|i| i.id == "occurrence.ground").unwrap();
    assert_eq!((ground.label.as_str(), ground.checked), ("Grounded", Some(false)));
    menu_pick(&mut h, "menu", "occurrence.ground");
    h.frames(2);
    assert!(has_badge("pin", "A:1"), "grounded again: the pin is back");
    assert_eq!(status_marker(&h.app, a), "status_full");

    // Ctrl+Z of the third Mate: B is half held once the frame's refresh has recounted.
    for _ in 0..4 {
        if h.app.session.doc.assembly.constraints.len() != 3 {
            break;
        }
        h.step(&json!({"key": "Z", "ctrl": true, "cmd": true})).unwrap();
        h.frames(2);
    }
    assert_eq!(h.app.session.doc.assembly.constraints.len(), 2);
    h.frames(1);
    // Mate:1 and the left Flush remain: B slides along y only.
    assert_eq!(missing(&h, "B"), Some(1), "recounted after the frame");
    assert_eq!(status_marker(&h.app, b), "status_half");
    assert!(has_marker("status_half", "B:1"));
}

/// Test 3 (§8.3 9, R103/R123): the first placed part is grounded; hardware is not.
#[test]
fn the_first_placed_part_is_grounded_and_hardware_is_not() {
    let dir = std::env::temp_dir().join(format!("tci-e2t4-ground-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let block = crate::test_support::block_part(&dir);
    let mut h = inventor(Assembly);
    run(&mut h, "document.new_assembly", json!({"name": "asm"}));
    let first = run(&mut h, "component.insert_design", json!({"path": block, "name": "First"}));
    let second = run(&mut h, "component.insert_design", json!({"path": block, "name": "Second"}));
    let (o1, o2) = (first["occurrence"].as_u64().unwrap(), second["occurrence"].as_u64().unwrap());
    assert!(has_badge("pin", "First:1") && has_marker("status_full", "First:1"));
    assert_eq!(status_marker(&h.app, o1), "status_full");
    assert!(!has_badge("pin", "Second:1") && !has_marker("status_full", "Second:1"));
    assert_eq!(status_marker(&h.app, o2), "status_empty");
    let mut h = inventor(Assembly);
    run(&mut h, "document.new_assembly", json!({"name": "hw"}));
    let bolt = run(&mut h, "parts.fastener", json!({"family": "socket_head_cap_screw", "size": "M6", "length": 20}));
    let o = h.app.session.doc.occurrence_of(bolt["component"].as_u64().unwrap()).unwrap().clone();
    assert!(!o.grounded);
    assert!(crate::scenario::handles("badge:pin:").is_empty(), "hardware placed first has no pin");
    assert_eq!(status_marker(&h.app, o.id), "status_empty");
    let _ = std::fs::remove_dir_all(dir);
}

/// Test 4: Lost, Sick, suppressed, limited and offset rows.
#[test]
fn sick_lost_suppressed_and_limited_rows() {
    // Lost: the plate cut short of its hole.
    let mut h = blocks();
    let t = crate::theme::Tokens::get();
    run(&mut h, "constraint.create", insert_plate_pin());
    run(&mut h, "timeline.edit", json!({"feature": "Plate", "set": {"length": "25 mm"}}));
    let rows = drawn("Insert:1");
    assert_eq!(rows.len(), 3, "Lost rows still show three times");
    assert!(matches!(h.app.session.constraint_report.rows[0].status, ConstraintStatus::Lost { .. }));
    for r in &rows {
        assert!(r.badges.contains(&"error".to_string()), "{r:?}");
        assert_eq!(r.color, Some(t.error), "{r:?}");
        assert!(r.tip.as_deref().is_some_and(|s| s.contains("no longer exists")), "{r:?}");
    }
    assert!(has_badge("error", "Plate:1") && has_badge("error", "Pin:1"), "both occurrences of a lost constraint gain the problem badge (R169)");

    // Ok, offset, limited, suppressed and sick Mates.
    let mut h = blocks();
    let p = mate_ab(&h, 50.0, 25.0, json!({}));
    run(&mut h, "constraint.create", p);
    let p = json!({"type": "mate", "a": face(&h, "A", [80.0, 10.0, 20.0]), "b": face(&h, "Plate", [160.0, 30.0, 0.0]), "offset": 5});
    run(&mut h, "constraint.create", p);
    let p = json!({"type": "mate", "a": face(&h, "A", [10.0, 40.0, 20.0]), "b": face(&h, "Pin", [305.0, 0.0, 50.0]), "max": 10});
    run(&mut h, "constraint.create", p);
    let p = mate_ab(&h, 20.0, 10.0, json!({"suppressed": true}));
    run(&mut h, "constraint.create", p);
    let p = mate_ab(&h, 30.0, 15.0, json!({}));
    run(&mut h, "constraint.create", p);
    run(&mut h, "parameters.add", json!({"name": "t", "expression": "0 mm"}));
    run(&mut h, "constraint.edit", json!({"constraint": "Mate:1", "offset": "t"}));
    run(&mut h, "parameters.change", json!({"name": "t", "expression": "2 mm"}));
    h.frames(2);

    let ok = &drawn("Mate:1")[0];
    assert!(ok.badges.is_empty() && ok.color.is_none() && !ok.dim, "{ok:?}");
    let offset = &drawn("Mate:2")[0];
    assert_eq!(offset.label, "Mate:2", "the name only, no value");
    assert!(offset.tip.as_deref().is_some_and(|s| s.contains("5.000 mm")), "{offset:?}");
    let limited = &drawn("Mate:3")[0];
    assert_eq!(limited.badges, ["limit"], "{limited:?}");
    let suppressed = &drawn("Mate:4")[0];
    assert!(suppressed.dim && suppressed.badges == ["suppressed"] && suppressed.tip.is_none(), "{suppressed:?}");
    let sick = &drawn("Mate:5")[0];
    match &h.app.session.constraint_report.rows.iter().find(|r| r.name == "Mate:5").unwrap().status {
        ConstraintStatus::Sick { with, .. } => assert!(with.is_empty(), "{with:?}"),
        other => panic!("Mate:5 is {other:?}"),
    }
    assert!(sick.badges.contains(&"error".to_string()) && sick.color == Some(t.error), "{sick:?}");
    let tip = sick.tip.clone().unwrap();
    assert!(tip.starts_with("off by 2.000 mm") && !tip.contains("— with"), "{tip}");
    // B carries a sick constraint: it keeps its DOF marker and gains the problem badge; so does A,
    // its other occurrence, grounded or not (R169).
    assert!(has_badge("error", "B:1") && has_badge("error", "A:1"));
    assert!(!has_marker("status_empty", "B:1"));
}

/// Test 5: a row click picks the constraint; its three rows draw selected.
#[test]
fn selecting_a_row_selects_all_three() {
    let mut h = blocks();
    let p = mate_ab(&h, 50.0, 25.0, json!({}));
    run(&mut h, "constraint.create", p);
    let (mate, b) = (constraint_id(&h, "Mate:1"), occ(&h, "B"));
    h.step(&json!({"click": {"handle": format!("row:constraint:{mate}:{b}")}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.tree.picked_constraints, [mate]);
    let rows = drawn("Mate:1");
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r.selected), "{rows:?}");
}

/// Test 6: SolidCraft keeps Ground / Unground (no `menu_labels`) and gets the same rows under
/// its Joints folder; Inventor reads Grounded with a check.
#[test]
fn solidcraft_keeps_ground_and_unground() {
    let mut h = blocks_on(Harness::new());
    let a = occ(&h, "A");
    let items = crate::context_menu::items(&h.app, &Target::Occurrence { path: vec![a] });
    let ground = items.iter().find(|i| i.id == "occurrence.ground").unwrap();
    assert_eq!((ground.label.as_str(), ground.checked), ("Unground", None));
    let items =
        crate::context_menu::items(&h.app, &Target::Component { id: h.app.session.doc.occurrences.iter().find(|o| o.id == a).unwrap().component });
    let ground = items.iter().find(|i| i.id == "occurrence.ground").unwrap();
    assert_eq!((ground.label.as_str(), ground.checked), ("Unground", None));
    // The same rows.
    let p = mate_ab(&h, 50.0, 25.0, json!({}));
    run(&mut h, "constraint.create", p);
    let (mate, b) = (constraint_id(&h, "Mate:1"), occ(&h, "B"));
    assert_eq!(constraint_rows(&h.app), [(mate, 0), (mate, a), (mate, b)]);
    let order_drawn: Vec<(u64, u64)> = drawn_constraints().iter().map(|d| (d.id, d.under)).collect();
    assert_eq!(order_drawn, [(mate, 0), (mate, a), (mate, b)]);
    assert!(has_marker("status_half", "B:1") && has_marker("status_full", "A:1"));

    let h = blocks();
    let items = crate::context_menu::items(&h.app, &Target::Occurrence { path: vec![occ(&h, "A")] });
    let ground = items.iter().find(|i| i.id == "occurrence.ground").unwrap();
    assert_eq!((ground.label.as_str(), ground.checked), ("Grounded", Some(true)));
    assert_eq!(ground.params, Some(json!({"occurrence": occ(&h, "A"), "grounded": false})));
}

/// R170: an Unsolved row draws as a problem (badge, error colour) and marks its occurrences; a
/// picked constraint that goes away is dropped from the picks even with the browser hidden.
#[test]
fn an_unsolved_row_is_a_problem_on_its_parts() {
    use solidcraft_engine::doc::constraints::{ConstraintKind, next_name};
    let mut h = blocks();
    let t = crate::theme::Tokens::get();
    let p = mate_ab(&h, 50.0, 25.0, json!({}));
    run(&mut h, "constraint.create", p);
    // 101 copies close 101 loops through B's one tree member: over the loop cap, Unsolved.
    let first = h.app.session.doc.assembly.constraints[0].clone();
    for _ in 0..101 {
        let doc = h.app.session.doc_mut();
        let mut c = first.clone();
        c.id = doc.allocate_id().unwrap();
        c.name = next_name(&doc.assembly, ConstraintKind::Mate);
        c.state.clear();
        c.signatures = Default::default();
        doc.assembly.constraints.push(c);
    }
    h.app.session.refresh();
    h.frames(2);
    let row = h.app.session.constraint_report.rows.iter().find(|r| r.name == "Mate:2").unwrap().clone();
    assert_eq!(row.status, ConstraintStatus::Unsolved);
    let drawn_rows = drawn("Mate:2");
    assert!(!drawn_rows.is_empty());
    for r in &drawn_rows {
        assert!(r.badges.contains(&"error".to_string()) && r.color == Some(t.error), "{r:?}");
    }
    // Unsolved marks its occurrences (R170): their rows' problem badge (`occurrence_sick`; the
    // rows themselves are scrolled past 102 constraint rows here).
    for name in ["A", "B"] {
        let id = occ(&h, name);
        let o = h.app.session.constraint_report.occurrences.iter().find(|o| o.id == id).unwrap();
        assert_eq!(o.status, solidcraft_engine::constraints::OccurrenceStatus::Sick, "{name}");
        assert!(super::occurrence_sick(&h.app, id), "{name}");
    }

    // Prune: Mate:2 picked, the browser hidden, the constraint deleted.
    let mate2 = constraint_id(&h, "Mate:2");
    h.app.tree.picked_constraints = vec![mate2];
    h.app.ui.show_browser = false;
    h.frames(1);
    run(&mut h, "constraint.delete", json!({"constraint": "Mate:2"}));
    h.frames(1);
    assert!(h.app.tree.picked_constraints.is_empty(), "{:?}", h.app.tree.picked_constraints);
}

/// Test 7 (R161): Unsolved and stuck-loop texts.
#[test]
fn unsolved_and_stuck_loop_tips() {
    assert_eq!(status_tip(&ConstraintStatus::Unsolved).as_deref(), Some("too many linked loops to solve here"));
    let stuck = status_tip(&ConstraintStatus::Sick { gap_mm: f64::NAN, gap_deg: f64::NAN, with: vec![] }).unwrap();
    assert_eq!(stuck, "loop cannot close");
    assert!(!stuck.contains("NaN"));
    let sick = status_tip(&ConstraintStatus::Sick { gap_mm: 2.0, gap_deg: 0.0, with: vec!["Mate:1".into()] });
    assert_eq!(sick.as_deref(), Some("off by 2.000 mm and 0.000° — with Mate:1"));
    let two = status_tip(&ConstraintStatus::Sick { gap_mm: 2.0, gap_deg: 0.0, with: vec!["Mate:1".into(), "Mate:3".into()] });
    assert_eq!(two.as_deref(), Some("off by 2.000 mm and 0.000° — with Mate:1, Mate:3"), "the list separator is the catalog's");
    for s in [ConstraintStatus::Ok, ConstraintStatus::Redundant, ConstraintStatus::Suppressed] {
        assert_eq!(status_tip(&s), None, "{s:?}");
    }
}
