#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The first minute of constraints (Plan E-2 Task 10; spec §2.4, §8.3): the Inventor workflow
//! driven end to end through the UI paths an Inventor user takes (the `C` alias typed key by key,
//! clicks in the view and the panel, the browser row's menu, the ribbon, the marking menu), one
//! assertion per step. SolidCraft's pass is `solidcraft_constraint_tests` (Task 9).

use egui::accesskit::{Node, Role};
use serde_json::{Value, json};
use solidcraft_engine::doc::DocumentKind::Assembly;
use solidcraft_engine::doc::Mat;
use solidcraft_engine::doc::constraints::{ConstraintSolution, ConstraintStatus};
use solidcraft_engine::doc::joints::occurrence_world;

use crate::browser::{DrawnConstraint, drawn_constraints, status_marker};
use crate::constraint_overlay::{drawn_glyphs, glyphs};
use crate::dialogs::Kind;
use crate::dialogs_assembly::Asm;
use crate::i18n::tr;
use crate::scenario::{Harness, count_of, handle_at};
use crate::test_support::{
    alias, click, click_world, clicks_expire, constraint_blocks, constraint_id, inventor, menu_pick, occ, right_click, world_at,
};

const A_TOP: [f64; 3] = [50.0, 25.0, 20.0];
const B_TOP: [f64; 3] = [50.0, 95.0, 27.0];
const A_RIGHT: [f64; 3] = [100.0, 25.0, 10.0];
/// B's own left face, in B's component space (x = 0, half way up B's 7..27).
const B_SIDE: [f64; 3] = [0.0, 95.0, 17.0];
const HOLE_RIM: [f64; 3] = [200.0, 0.0, 10.0];
const PIN_RIM: [f64; 3] = [310.0, 0.0, 110.0];

fn world(h: &Harness, name: &str) -> Mat {
    occurrence_world(&h.app.session.doc, occ(h, name))
}

fn near(a: &Mat, b: &Mat, tol: f64) -> bool {
    a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() <= tol)
}

fn close(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
}

fn panel(h: &Harness) -> Option<&Kind> {
    h.app.dialog.as_ref().map(|d| &d.kind)
}

fn constrain_open(h: &Harness) -> bool {
    matches!(panel(h), Some(Kind::Assembly(Asm::Constraint(_))))
}

/// The accessible nodes labelled `label` inside the open panel.
fn nodes(h: &Harness, label: &str) -> Vec<Node> {
    let Some(at) = crate::scenario::rect_of("dialog:outer") else { return Vec::new() };
    let Some(tree) = h.output.accesskit_update.as_ref() else { return Vec::new() };
    tree.nodes
        .iter()
        .filter(|(_, n)| n.label() == Some(label))
        .filter(|(_, n)| n.bounds().is_some_and(|b| at.contains(egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32))))
        .map(|(_, n)| n.clone())
        .collect()
}

/// Click the panel's control `label` of `role`; `last`: the lowest one (a solution sits below the
/// type of the same word).
fn press_role(h: &mut Harness, label: &str, role: Role, last: bool) {
    let mut found: Vec<Node> = nodes(h, label).into_iter().filter(|n| n.role() == role).collect();
    found.sort_by(|a, b| a.bounds().unwrap().y0.total_cmp(&b.bounds().unwrap().y0));
    let n = if last { found.last() } else { found.first() };
    let b = n.and_then(Node::bounds).unwrap_or_else(|| panic!("no {role:?} `{label}` in the panel"));
    h.step(&json!({"click": [(b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0]})).unwrap();
    h.frames(3);
}

fn press(h: &mut Harness, label: &str) {
    press_role(h, label, Role::Button, false);
}

/// Two picks in the view, each click left to expire.
fn picks(h: &mut Harness, a: [f64; 3], b: [f64; 3]) {
    click_world(h, a);
    clicks_expire(h);
    click_world(h, b);
    clicks_expire(h);
    h.frames(2);
}

fn key(h: &mut Harness, k: &str) {
    h.step(&json!({"key": k})).unwrap();
    h.frames(3);
}

fn drawn(name: &str) -> Vec<DrawnConstraint> {
    drawn_constraints().into_iter().filter(|d| d.label == name).collect()
}

fn status(h: &Harness, name: &str) -> ConstraintStatus {
    h.app.session.constraint_report.rows.iter().find(|r| r.name == name).map(|r| r.status.clone()).unwrap_or_else(|| panic!("no row {name}"))
}

/// A point on screen in front of the blocks, where the view shows nothing.
fn empty_space(h: &mut Harness) -> Value {
    let [x, y] = h.at(&json!({"world": [50, -40, 0]})).unwrap();
    json!([x, y + 120.0])
}

#[test]
fn the_first_minute_under_inventor() {
    let mut h = inventor(Assembly);
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;

    // 1. The blocks: A, placed first, is grounded (the pin) and fully held.
    constraint_blocks(&mut h);
    h.frames(2);
    let (a, b) = (occ(&h, "A"), occ(&h, "B"));
    assert_eq!(status_marker(&h.app, a), "status_full");
    assert!(handle_at("badge:pin:A:1").is_some() && handle_at("marker:status_full:A:1").is_some(), "A shows the pin and the full marker");
    assert_eq!(status_marker(&h.app, b), "status_empty");
    let start = h.app.session.undo.len();

    // 2. C, A's top, B's top, Enter: Mate:1 puts B's top face on A's top, B flipped above A.
    alias(&mut h, "C");
    assert!(constrain_open(&h), "C opens the Constrain panel");
    picks(&mut h, A_TOP, B_TOP);
    key(&mut h, "Enter");
    assert!(h.app.dialog.is_none(), "Enter is OK");
    let mate1 = h.app.session.doc.assembly.constraint("Mate:1").expect("Mate:1");
    assert_eq!(mate1.solution, ConstraintSolution::MateFace);
    assert_eq!(h.app.session.undo.len(), start + 1, "one step");
    let top = world_at(&h, "B", B_TOP);
    assert!((top[2] - 20.0).abs() <= 1e-9, "gap 0.000: B's top on A's top: {top:?}");
    let bottom = world_at(&h, "B", [50.0, 95.0, 7.0]);
    assert!((bottom[2] - 40.0).abs() <= 1e-9, "B spans z 20..40, flipped: {bottom:?}");
    assert_eq!(status_marker(&h.app, b), "status_half");
    assert!(handle_at("marker:status_half:B:1").is_some());
    let dof = |h: &Harness| solidcraft_engine::constraints::report(&h.app.session).dof.get(&b).copied();
    let dof_after_mate1 = dof(&h).expect("B's DOF after Mate:1");

    // 3. C again: Flush A's right face with B's side face where B now is (R142; B turned over
    //    about Y, so B's own left face, component x = 0, now faces +x at x = 100); Apply; Insert
    //    the pin's top rim into the hole's top rim; Apply; Cancel.
    //    The brief's pick is B's right face (component [100, 95, 17]), which needs B turned a
    //    half-turn about Z; E-1 refuses that antiparallel Flush ("over-constrains B"), ledgered
    //    for E-1 wave 1 ("a Flush/Mate whose dry solve needs a half-turn must not be refused —
    //    solve from the flipped seed"). Restore the brief's pick and B's turn about Z then.
    alias(&mut h, "C");
    press_role(&mut h, "Flush", Role::Button, true);
    let b_side = world_at(&h, "B", B_SIDE);
    assert!((20.0..=40.0).contains(&b_side[2]) && (b_side[0] - 100.0).abs() <= 1e-9, "the pick lies on B, at z 20..40: {b_side:?}");
    let b_pose = world(&h, "B");
    picks(&mut h, A_RIGHT, b_side);
    press(&mut h, "Apply");
    assert!(constrain_open(&h), "Apply keeps the panel open");
    let mate2 = h.app.session.doc.assembly.constraint("Mate:2").expect("Mate:2");
    assert_eq!(mate2.solution, ConstraintSolution::Flush);
    assert!(near(&world(&h, "B"), &b_pose, 1e-9), "already flush (x = 100): B stays");
    assert_eq!(status(&h, "Mate:2"), ConstraintStatus::Ok, "Mate:2 holds");
    let dof_after_mate2 = dof(&h).expect("B's DOF after Mate:2");
    assert!(dof_after_mate2 < dof_after_mate1, "Mate:2 takes B's freedom: {dof_after_mate1} -> {dof_after_mate2}");
    let pin_before = world(&h, "Pin");
    press(&mut h, "Insert");
    picks(&mut h, HOLE_RIM, PIN_RIM);
    press(&mut h, "Apply");
    let insert = constraint_id(&h, "Insert:1");
    press(&mut h, "Cancel");
    assert!(h.app.dialog.is_none(), "Cancel closes the panel");
    assert_eq!(h.app.session.undo.len(), start + 3, "Mate:2 and Insert:1: a step each; Cancel none");
    // Insert: the pin's top centre [300, 0, 110] on the hole's top centre [190, 0, 10], its axis
    // opposed (the pin turned over, standing up out of the plate).
    let (centre, base) = (world_at(&h, "Pin", [300.0, 0.0, 110.0]), world_at(&h, "Pin", [300.0, 0.0, 50.0]));
    assert!(close(centre, [190.0, 0.0, 10.0], 1e-6), "the pin's top rim sits in the hole's: {centre:?}");
    assert!(close(base, [190.0, 0.0, 70.0], 1e-6), "opposed: the pin stands up out of the hole: {base:?}");
    assert!(!near(&world(&h, "Pin"), &pin_before, 1e-3), "the pin moved, the plate held (pick 1, R167)");
    let names: Vec<String> = h.app.session.doc.assembly.constraints.iter().map(|c| c.name.clone()).collect();
    assert_eq!(names, ["Mate:1", "Mate:2", "Insert:1"]);
    for name in ["Mate:1", "Mate:2", "Insert:1"] {
        assert_eq!(drawn(name).len(), 3, "{name} under Relationships and under both parts");
    }

    // 4. Insert:1's row › Drive; End 20, Play; Cancel: the pin is drawn back, no step.
    let pin_pose = world(&h, "Pin");
    let steps = h.app.session.undo.len();
    clicks_expire(&mut h);
    right_click(&mut h, json!({"handle": format!("row:constraint:{insert}:0")}));
    menu_pick(&mut h, "menu", "ui.constraintDrive");
    assert!(matches!(panel(&h), Some(Kind::Assembly(Asm::DriveConstraint(_)))), "the Drive panel opened");
    press_role(&mut h, "End", Role::TextInput, false);
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": "20"})).unwrap();
    h.frames(2);
    let Some(Kind::Assembly(Asm::DriveConstraint(f))) = panel(&h) else { panic!("the Drive panel closed") };
    assert_eq!(f.end, "20", "End typed");
    press(&mut h, tr("ui.constraint.drive_play"));
    for _ in 0..600 {
        let Some(Kind::Assembly(Asm::DriveConstraint(f))) = panel(&h) else { panic!("the Drive panel closed") };
        if f.playing.is_none() {
            break;
        }
        h.frames(1);
    }
    let Some(Kind::Assembly(Asm::DriveConstraint(f))) = panel(&h) else { panic!("the Drive panel closed") };
    assert!(f.playing.is_none(), "the sweep ended");
    let last = f.frames.last().map(|x| x.0).expect("the sweep's frames");
    assert!((last - 20.0).abs() <= 1e-9, "driven to 20: {last}");
    assert!(h.app.preview.active, "the pin is drawn at the last frame's pose");
    assert!(near(&world(&h, "Pin"), &pin_pose, 0.0), "the document never changes");
    press(&mut h, "Cancel");
    assert!(h.app.dialog.is_none());
    assert!(!h.app.preview.active, "the pin is drawn back at its document pose");
    assert_eq!(count_of("constraint_drive_moved"), Some(0.0));
    assert_eq!(h.app.session.undo.len(), steps, "Drive takes no step");

    // 5. Cut the hole off the plate (by command); the ribbon's Show Problems lists Insert:1, its
    //    rows show the problem, and it draws no glyph (Lost, R135). Ctrl+Z, Hide All.
    h.app.run("timeline.edit", json!({"feature": "Plate", "set": {"length": "25 mm"}})).unwrap();
    h.frames(2);
    assert!(matches!(status(&h, "Insert:1"), ConstraintStatus::Lost { .. }), "{:?}", status(&h, "Insert:1"));
    let steps = h.app.session.undo.len();
    click(&mut h, "button:constraint.show_sick");
    h.frames(2);
    assert_eq!(h.app.session.shown_constraints.iter().copied().collect::<Vec<_>>(), [insert], "Show Problems shows Insert:1");
    let rows = drawn("Insert:1");
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r.badges.iter().any(|x| x == "error")), "its rows show the problem: {rows:?}");
    assert!(glyphs(&h.app).iter().all(|g| g.0 != insert) && drawn_glyphs().iter().all(|g| g.id != insert), "no glyph for a Lost one");
    assert_eq!(h.app.session.undo.len(), steps, "Show Problems takes no step");
    h.step(&json!({"key": "Z", "cmd": true})).unwrap();
    h.frames(3);
    assert_eq!(h.app.session.undo.len(), steps - 1, "Ctrl+Z undid the edit");
    assert_eq!(status(&h, "Insert:1"), ConstraintStatus::Ok, "the hole is back");
    click(&mut h, "button:constraint.hide_all");
    h.frames(2);
    assert!(h.app.session.shown_constraints.is_empty(), "Hide All");
    assert!(drawn_glyphs().is_empty());

    // 6. Right-click empty space, the ring's NE: the panel opens; Esc closes it, no step.
    let steps = h.app.session.undo.len();
    let empty = empty_space(&mut h);
    right_click(&mut h, empty);
    let ring = crate::context_menu::radial_items(&h.app);
    assert_eq!(ring.get(1).map(|i| i.id.as_str()), Some("constraint.create"), "NE is Constraint");
    menu_pick(&mut h, "marking", "constraint.create");
    assert!(constrain_open(&h), "NE opened the Constrain panel");
    key(&mut h, "Escape");
    assert!(h.app.dialog.is_none(), "Esc closes it");
    assert_eq!(h.app.session.undo.len(), steps);
    assert_eq!(h.app.session.doc.assembly.constraints.len(), 3);
}
