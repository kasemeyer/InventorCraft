#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Plan E-2 Task 9: constraints under the SolidCraft profile (spec §6.9, §8.3 10). The same flow
//! as Inventor's §8.3 1, 5 and 7, reached through SolidCraft's own paths: the ASSEMBLE tab's
//! JOINTS panel (Constrain and the show commands in its list, R165), the joints folder in the
//! browser, and list menus (no ring); SolidCraft binds no key to Constrain.

use serde_json::{Value, json};
use solidcraft_engine::constraints::report;
use solidcraft_engine::doc::constraints::ConstraintStatus;

use crate::browser::{DrawnConstraint, constraint_rows, drawn_constraints};
use crate::constraint_overlay::{drawn_glyphs, glyphs};
use crate::context_menu::{Item, Target};
use crate::dialogs::Kind;
use crate::dialogs_assembly::Asm;
use crate::scenario::{Harness, handle_at};
use crate::test_support::{click, click_world, clicks_expire, constraint_blocks, constraint_id, frames_for, menu_pick, occ, right_click, world_at};

const A_TOP: [f64; 3] = [50.0, 25.0, 20.0];
const B_TOP: [f64; 3] = [50.0, 95.0, 27.0];

/// The row menu's documented order without Lock Turn (a Mate), then Rename (Task 5).
const MATE_MENU: [&str; 9] = [
    "constraint.edit",
    "ui.constraintModify",
    "constraint.suppress",
    "constraint.delete",
    "ui.constraintDrive",
    "ui.constraintOtherHalf",
    "ui.constraintFind",
    "-",
    "ui.rename",
];

/// SolidCraft (`Harness::new()`) with E-1's blocks.
fn blocks() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    constraint_blocks(&mut h);
    h.frames(2);
    h
}

fn run(h: &mut Harness, id: &str, p: Value) -> Value {
    let r = h.app.run(id, p.clone()).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    h.frames(2);
    r
}

fn drawn(name: &str) -> Vec<DrawnConstraint> {
    drawn_constraints().into_iter().filter(|d| d.label == name).collect()
}

fn ids(items: &[Item]) -> Vec<&str> {
    items.iter().map(|i| i.id.as_str()).collect()
}

fn constrain_open(h: &Harness) -> bool {
    matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Assembly(Asm::Constraint(_))))
}

/// The ASSEMBLE tab's JOINTS panel's list, then its row `id`.
fn joints_list(h: &mut Harness, id: &str) {
    if handle_at("panel:ASSEMBLE:JOINTS").is_none() {
        click(h, "tab:ASSEMBLE");
    }
    click(h, "panel:ASSEMBLE:JOINTS");
    click(h, &format!("dropdown:{id}"));
    h.frames(2);
}

/// Right-click the row of `id` under `under` (0 = the joints folder): a list opens on it.
fn row_menu(h: &mut Harness, id: u64, under: u64) -> Vec<Item> {
    crate::context_menu::close(&mut h.app);
    clicks_expire(h);
    right_click(h, json!({"handle": format!("row:constraint:{id}:{under}")}));
    assert_eq!(h.app.menu.target, Target::Constraint { id, under }, "the right-click opened the constraint's list");
    let open = h.app.viewport.context_menu.is_some();
    assert!(open, "the list is open");
    crate::context_menu::items(&h.app, &h.app.menu.target.clone())
}

/// Mate:1: A's top on B's top.
fn mate_tops(h: &mut Harness) -> u64 {
    run(h, "constraint.create", json!({"type": "mate", "a": {"occurrence": "A", "face": A_TOP}, "b": {"occurrence": "B", "face": B_TOP}}));
    constraint_id(h, "Mate:1")
}

/// Test 1 (§8.3 10 → 1): JOINTS › Constrain, two faces, Enter → Mate:1 in three places.
#[test]
fn solidcraft_makes_a_mate_from_its_joints_panel() {
    let mut h = blocks();
    let undo = h.app.session.undo.len();
    joints_list(&mut h, "constraint.create");
    assert!(constrain_open(&h), "JOINTS › Constrain opens the Constrain panel");
    click_world(&mut h, A_TOP);
    clicks_expire(&mut h);
    click_world(&mut h, B_TOP);
    clicks_expire(&mut h);
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(3);
    assert!(h.app.dialog.is_none(), "Enter is OK");
    let mate = constraint_id(&h, "Mate:1");
    assert_eq!(h.app.session.undo.len(), undo + 1, "one step");
    let (a, b) = (occ(&h, "A"), occ(&h, "B"));
    let mut under: Vec<u64> = drawn("Mate:1").iter().map(|r| r.under).collect();
    under.sort_unstable();
    let mut want = vec![0, a, b];
    want.sort_unstable();
    assert_eq!(under, want, "in the joints folder and under A and B");
    assert!(drawn("Mate:1").iter().all(|r| r.id == mate));
    // B's top now lies on A's top (z = 20).
    assert!((world_at(&h, "B", B_TOP)[2] - 20.0).abs() < 1e-6, "{:?}", world_at(&h, "B", B_TOP));
}

/// Test 2 (§8.3 10 → 5): the row's menu is a list with Task 5's rows; Suppress dims all three
/// rows and frees B, Delete removes all three; one step each.
#[test]
fn solidcraft_row_menus_are_lists() {
    let mut h = blocks();
    let mate = mate_tops(&mut h);
    let b = occ(&h, "B");
    let items = row_menu(&mut h, mate, 0);
    assert_eq!(ids(&items), MATE_MENU);
    assert!(items.iter().filter(|i| i.id != "-").all(|i| i.params == Some(json!({"constraint": mate}))));
    assert!(!h.app.menu.radial, "a list, no ring");
    assert!(handle_at("menu:constraint.suppress").is_some(), "the list is drawn");

    let undo = h.app.session.undo.len();
    row_menu(&mut h, mate, b);
    menu_pick(&mut h, "menu", "constraint.suppress");
    h.frames(2);
    assert_eq!(h.app.session.undo.len(), undo + 1, "Suppress: one step");
    assert!(h.app.session.doc.assembly.constraint("Mate:1").unwrap().suppressed);
    let rows = drawn("Mate:1");
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r.dim), "dimmed in three places: {rows:?}");
    assert_eq!(report(&h.app.session).dof.get(&b), Some(&6), "B is free again");

    let undo = h.app.session.undo.len();
    row_menu(&mut h, mate, 0);
    menu_pick(&mut h, "menu", "constraint.delete");
    h.frames(2);
    assert_eq!(h.app.session.undo.len(), undo + 1, "Delete: one step");
    assert!(drawn("Mate:1").is_empty(), "no Mate:1 row anywhere");
    assert!(constraint_rows(&h.app).is_empty());
    assert!(h.app.session.doc.assembly.constraints.is_empty());
}

/// Test 3 (§8.3 10 → 7): as Task 7 test 1, from the JOINTS panel's list. Show Problems shows the
/// sick Mate (a glyph in the error colour) and lists the Lost Insert without a glyph (R135);
/// Hide All clears; neither is a step.
#[test]
fn solidcraft_show_problems_and_hide_all() {
    let mut h = blocks();
    let t = crate::theme::Tokens::get();
    run(
        &mut h,
        "constraint.create",
        json!({"type": "insert", "a": {"occurrence": "Plate", "edge": [200.0, 0.0, 10.0]}, "b": {"occurrence": "Pin", "edge": [310.0, 0.0, 50.0]}}),
    );
    let mate = |h: &Harness, x: f64, y: f64| json!({"type": "mate", "a": {"occurrence": "A", "face": world_at(h, "A", [x, y, 20.0])}, "b": {"occurrence": "B", "face": world_at(h, "B", [x, y + 70.0, 7.0])}});
    let p = mate(&h, 50.0, 25.0);
    run(&mut h, "constraint.create", p);
    let p = mate(&h, 30.0, 15.0);
    run(&mut h, "constraint.create", p);
    run(&mut h, "parameters.add", json!({"name": "t", "expression": "0 mm"}));
    run(&mut h, "constraint.edit", json!({"constraint": "Mate:1", "offset": "t"}));
    run(&mut h, "parameters.change", json!({"name": "t", "expression": "2 mm"}));
    run(&mut h, "timeline.edit", json!({"feature": "Plate", "set": {"length": "25 mm"}}));
    h.frames(2);
    let status = |h: &Harness, name: &str| h.app.session.constraint_report.rows.iter().find(|r| r.name == name).map(|r| r.status.clone()).unwrap();
    assert!(matches!(status(&h, "Mate:2"), ConstraintStatus::Sick { .. }), "{:?}", status(&h, "Mate:2"));
    assert!(matches!(status(&h, "Insert:1"), ConstraintStatus::Lost { .. }), "{:?}", status(&h, "Insert:1"));
    let (mate2, insert) = (constraint_id(&h, "Mate:2"), constraint_id(&h, "Insert:1"));
    let steps = h.app.session.undo.len();

    joints_list(&mut h, "constraint.show_sick");
    let mut want = vec![mate2, insert];
    want.sort_unstable();
    assert_eq!(h.app.session.shown_constraints.iter().copied().collect::<Vec<_>>(), want, "the sick and the lost ones");
    let g: Vec<(u64, bool)> = glyphs(&h.app).into_iter().map(|(id, _, sick)| (id, sick)).collect();
    assert_eq!(g, [(mate2, true)], "no glyph for the Lost Insert");
    let d = drawn_glyphs();
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!((d[0].id, d[0].leaders, d[0].colour), (mate2, 2, t.error));
    assert_eq!(h.app.session.undo.len(), steps, "Show Problems is no step");

    joints_list(&mut h, "constraint.hide_all");
    assert!(h.app.session.shown_constraints.is_empty());
    assert!(glyphs(&h.app).is_empty() && drawn_glyphs().is_empty());
    assert_eq!(h.app.session.undo.len(), steps, "Hide All is no step");
}

/// Test 4 (§6.9 [A]): SolidCraft binds no key to Constrain: `C` in the assembly, then the alias
/// wait, runs no `constraint.create` and opens no Constrain panel. `C` stays SolidCraft's own key
/// (Center Diameter Circle, which outside a sketch first asks for the sketch's plane): the
/// profile is unchanged.
#[test]
fn solidcraft_has_no_constrain_key() {
    let mut h = blocks();
    let undo = h.app.session.undo.len();
    h.app.alias.fired = None;
    h.step(&json!({"key": "C"})).unwrap();
    h.frames(frames_for(0.75));
    assert_eq!(h.app.alias.fired, None, "no alias fired");
    assert!(!constrain_open(&h), "no Constrain panel");
    assert!(
        matches!(h.app.dialog.as_ref().map(|d| &d.kind), None | Some(Kind::Sketch)),
        "only SolidCraft's own C (Circle's sketch plane): {:?}",
        h.app.dialog.as_ref().map(|d| &d.kind)
    );
    assert!(h.app.session.doc.assembly.constraints.is_empty());
    assert_eq!(h.app.session.undo.len(), undo);
}
