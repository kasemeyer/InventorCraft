#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! R185: a single click on a constraint row shows its value box at the foot of the browser
//! (Enter edits the offset in one step, Escape reverts), on both profiles in an inch assembly.

use serde_json::{Value, json};
use solidcraft_engine::doc::DocumentKind::Assembly;

use super::value_box::shown;
use crate::scenario::{Harness, rect_of};
use crate::test_support::{constraint_blocks, constraint_id, inventor};

fn solidcraft() -> Harness {
    let mut h = Harness::new();
    h.app.session.doc_mut().kind = Assembly;
    h
}

fn inventor_assembly() -> Harness {
    inventor(Assembly)
}

/// The profiles, each harness made when its turn comes (the published handles are the last
/// frame's, so a second harness must not draw in between).
pub(super) const PROFILES: [fn() -> Harness; 2] = [inventor_assembly, solidcraft];

/// The blocks in an inch assembly, with Mate:1 (A's top on B's bottom, no offset typed).
pub(super) fn blocks_in_inches(make: fn() -> Harness) -> (Harness, u64) {
    let mut h = make();
    constraint_blocks(&mut h);
    run(
        &mut h,
        "constraint.create",
        json!({"type": "mate", "a": {"occurrence": "A", "face": [50, 25, 20]}, "b": {"occurrence": "B", "face": [50, 95, 7]}}),
    );
    run(&mut h, "document.units", json!({"units": "in"}));
    let id = constraint_id(&h, "Mate:1");
    (h, id)
}

pub(super) fn run(h: &mut Harness, id: &str, p: Value) -> Value {
    let r = h.app.run(id, p.clone()).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    h.frames(2);
    r
}

pub(super) fn occ(h: &Harness, name: &str) -> u64 {
    h.app.session.doc.occurrences.iter().find(|o| o.name == format!("{name}:1")).map(|o| o.id).unwrap()
}

pub(super) fn click_row(h: &mut Harness, id: u64, under: u64) {
    h.step(&json!({"click": {"handle": format!("row:constraint:{id}:{under}")}})).unwrap();
    h.frames(3);
    crate::test_support::clicks_expire(h);
}

fn type_in_box(h: &mut Harness, text: &str) {
    h.step(&json!({"click": {"handle": "browser:value_box"}})).unwrap();
    h.frames(1);
    h.step(&json!({"key": "A", "ctrl": true})).unwrap();
    h.step(&json!({"text": text})).unwrap();
    h.frames(1);
}

fn record(h: &Harness, id: u64) -> solidcraft_engine::doc::constraints::Constraint {
    h.app.session.doc.assembly.constraint(&id.to_string()).unwrap().clone()
}

#[test]
fn a_click_shows_the_value_box_at_the_foot_and_enter_edits_the_offset() {
    for make in PROFILES {
        let (mut h, mate) = blocks_in_inches(make);
        assert_eq!(shown(&h.app), None, "nothing picked: no box");
        let b = occ(&h, "B");
        click_row(&mut h, mate, b);
        assert_eq!(h.app.tree.picked_constraints, [mate]);
        assert_eq!(shown(&h.app), Some(("0.000 in".to_string(), true)), "the offset in the document's units");
        // At the foot of the browser pane, below every row.
        let (card, field) = (rect_of("browser:card").unwrap(), rect_of("browser:value_box").unwrap());
        assert!(card.contains_rect(field), "{field:?} in {card:?}");
        let t = crate::theme::Tokens::get();
        assert!(card.bottom() - field.bottom() < 2.0 * t.control_height, "the box is at the pane's foot: {field:?} in {card:?}");
        // Type 0.5, Enter: one step, the offset 0.5 in.
        let undo = h.app.session.undo.len();
        type_in_box(&mut h, "0.5");
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(3);
        assert_eq!(h.app.session.undo.len(), undo + 1, "one step");
        assert!((record(&h, mate).offset - 12.7).abs() < 1e-9, "0.5 in: {}", record(&h, mate).offset);
        assert_eq!(shown(&h.app).map(|s| s.0).as_deref(), Some("0.500 in"));
        // Typed then Escape: unchanged, no step.
        type_in_box(&mut h, "2");
        h.step(&json!({"key": "Escape"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.undo.len(), undo + 1, "Escape takes no step");
        assert!((record(&h, mate).offset - 12.7).abs() < 1e-9);
        assert_eq!(shown(&h.app), Some(("0.500 in".to_string(), true)), "the box shows the record's value again");
    }
}

#[test]
fn the_box_follows_the_pick_and_hides_for_none_or_several() {
    for make in PROFILES {
        let (mut h, mate) = blocks_in_inches(make);
        run(&mut h, "parameters.add", json!({"name": "gap", "expression": "0.25 in"}));
        run(
            &mut h,
            "constraint.create",
            json!({"type": "insert", "a": {"occurrence": "Plate", "edge": [200.0, 0.0, 10.0]}, "b": {"occurrence": "Pin", "edge": [310.0, 0.0, 50.0]}, "offset": "gap"}),
        );
        let insert = constraint_id(&h, "Insert:1");
        h.app.tree.picked_constraints = vec![insert];
        h.frames(2);
        assert_eq!(shown(&h.app).map(|s| s.0).as_deref(), Some("gap"), "the record's authored text");
        h.app.tree.picked_constraints = vec![mate];
        h.frames(2);
        assert_eq!(shown(&h.app).map(|s| s.0).as_deref(), Some("0.000 in"), "the box follows the pick");
        h.app.tree.picked_constraints = vec![insert, mate];
        h.frames(2);
        assert_eq!(shown(&h.app), None, "several picked: hidden");
        assert!(h.app.tree.value_box.is_none());
        h.app.tree.picked_constraints.clear();
        h.frames(2);
        assert!(h.app.tree.value_box.is_none(), "none picked: hidden");
        // Suppressed: shown, disabled.
        run(&mut h, "constraint.suppress", json!({"constraint": mate}));
        h.app.tree.picked_constraints = vec![mate];
        h.frames(2);
        assert_eq!(shown(&h.app).map(|s| s.1), Some(false), "suppressed: disabled");
    }
}

#[test]
fn an_angle_shows_degrees() {
    let (mut h, _) = blocks_in_inches(inventor_assembly);
    run(
        &mut h,
        "constraint.create",
        json!({"type": "angle", "solution": "undirected", "a": {"occurrence": "Plate", "face": [160.0, 30.0, 10.0]}, "b": {"occurrence": "Pin", "face": [300.0, 5.0, 110.0]}, "offset": 0}),
    );
    let angle = h.app.session.doc.assembly.constraints.iter().find(|c| c.kind.is_angle()).unwrap().id;
    h.app.tree.picked_constraints = vec![angle];
    h.frames(2);
    assert_eq!(shown(&h.app).map(|s| s.0).as_deref(), Some("0.000 deg"));
}
