#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Plan E-2 Task 5: a constraint row's list menu (spec §6.4, R72; the right-click was not
//! captured, so DOC 7's order), the inline Modify, Edit through the Constrain panel (only the
//! touched fields are sent, R134/R147/R162), Suppress, Delete, Drive's disabled reason, Lock
//! Turn, Other Half, Find in Window, Rename and the Delete key. Inventor tests run on
//! `inventor(Assembly)` with E-1's blocks; SolidCraft (`Harness::new()`) gets the same list.

use egui::accesskit::{Node, Role};
use serde_json::{Value, json};
use solidcraft_engine::constraints::Draft;
use solidcraft_engine::doc::DocumentKind::Assembly;
use solidcraft_engine::doc::constraints::ConstraintSolution;

use super::{Item, Target};
use crate::browser::{DrawnConstraint, constraint_rows, drawn_constraints};
use crate::dialogs::Kind;
use crate::dialogs_assembly::Asm;
use crate::dialogs_constraint::ConstraintForm;
use crate::i18n::tr;
use crate::scenario::Harness;
use crate::test_support::{clicks_expire, constraint_blocks, constraint_id, frames_for, inventor, menu_pick, right_click};

/// The documented order (spec §6.4).
const ORDER: [&str; 8] = [
    "constraint.edit",
    "ui.constraintModify",
    "constraint.suppress",
    "constraint.delete",
    "ui.constraintDrive",
    "ui.constraintLock",
    "ui.constraintOtherHalf",
    "ui.constraintFind",
];

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

fn run(h: &mut Harness, id: &str, p: Value) -> Value {
    let r = h.app.run(id, p.clone()).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    h.frames(2);
    r
}

/// Mate:1: A's top on B's top (A's [50, 25, 20], B's [50, 95, 27]).
fn mate_tops(h: &mut Harness) -> u64 {
    run(
        h,
        "constraint.create",
        json!({"type": "mate", "a": {"occurrence": "A", "face": [50, 25, 20]}, "b": {"occurrence": "B", "face": [50, 95, 27]}}),
    );
    constraint_id(h, "Mate:1")
}

/// Insert:1: the plate's hole rim into the pin's bottom rim.
fn insert_plate_pin(h: &mut Harness) -> u64 {
    run(
        h,
        "constraint.create",
        json!({"type": "insert", "a": {"occurrence": "Plate", "edge": [200.0, 0.0, 10.0]}, "b": {"occurrence": "Pin", "edge": [310.0, 0.0, 50.0]}}),
    );
    constraint_id(h, "Insert:1")
}

/// The plate cut short of its hole: Insert:1 is Lost.
fn lose_the_hole(h: &mut Harness) {
    run(h, "timeline.edit", json!({"feature": "Plate", "set": {"length": "25 mm"}}));
    let row = h.app.session.constraint_report.rows.iter().find(|r| r.name == "Insert:1").unwrap();
    assert!(matches!(row.status, solidcraft_engine::doc::constraints::ConstraintStatus::Lost { .. }), "{:?}", row.status);
}

/// Right-click the row of `id` under `under` (0 = Relationships): the list opens on it.
fn row_menu(h: &mut Harness, id: u64, under: u64) -> Vec<Item> {
    super::close(&mut h.app);
    clicks_expire(h);
    right_click(h, json!({"handle": format!("row:constraint:{id}:{under}")}));
    assert_eq!(h.app.menu.target, Target::Constraint { id, under }, "the right-click opened the constraint's list");
    let open = h.app.viewport.context_menu.is_some();
    assert!(open, "the list is open");
    super::items(&h.app, &h.app.menu.target.clone())
}

fn ids(items: &[Item]) -> Vec<&str> {
    items.iter().map(|i| i.id.as_str()).collect()
}

fn find<'a>(items: &'a [Item], id: &str) -> &'a Item {
    items.iter().find(|i| i.id == id).unwrap_or_else(|| panic!("no {id} in {:?}", ids(items)))
}

fn drawn(name: &str) -> Vec<DrawnConstraint> {
    drawn_constraints().into_iter().filter(|d| d.label == name).collect()
}

fn record(h: &Harness, id: u64) -> solidcraft_engine::doc::constraints::Constraint {
    h.app.session.doc.assembly.constraint(&id.to_string()).cloned().unwrap()
}

fn form(h: &Harness) -> &ConstraintForm {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Assembly(Asm::Constraint(f))) => f,
        other => panic!("the Constrain panel is not open: {other:?}"),
    }
}

fn form_mut(h: &mut Harness) -> &mut ConstraintForm {
    match h.app.dialog.as_mut().map(|d| &mut d.kind) {
        Some(Kind::Assembly(Asm::Constraint(f))) => f,
        other => panic!("the Constrain panel is not open: {other:?}"),
    }
}

/// The accessible nodes labelled `label` inside the panel.
fn nodes(h: &Harness, label: &str) -> Vec<Node> {
    let Some(panel) = crate::scenario::rect_of("dialog:outer") else { return Vec::new() };
    let Some(tree) = h.output.accesskit_update.as_ref() else { return Vec::new() };
    tree.nodes
        .iter()
        .filter(|(_, n)| n.label() == Some(label))
        .filter(|(_, n)| n.bounds().is_some_and(|b| panel.contains(egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32))))
        .map(|(_, n)| n.clone())
        .collect()
}

/// Click the panel's lowest button labelled `label` (a solution sits below the type row).
fn press_last(h: &mut Harness, label: &str) {
    let mut buttons: Vec<Node> = nodes(h, label).into_iter().filter(|n| n.role() == Role::Button).collect();
    buttons.sort_by(|a, b| a.bounds().unwrap().y0.total_cmp(&b.bounds().unwrap().y0));
    let b = buttons.last().and_then(Node::bounds).unwrap_or_else(|| panic!("no button `{label}` in the panel"));
    h.step(&json!({"click": [(b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0]})).unwrap();
    h.frames(3);
}

/// The panel's heading: the label in its header band.
fn heading(h: &Harness) -> String {
    let band = crate::scenario::rect_of("dialog:header").expect("the panel's header is drawn");
    let tree = h.output.accesskit_update.as_ref().unwrap();
    tree.nodes
        .iter()
        .filter(|(_, n)| n.role() == Role::Label)
        .find(|(_, n)| n.bounds().is_some_and(|b| band.contains(egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32))))
        .and_then(|(_, n)| n.value().map(str::to_string))
        .unwrap_or_default()
}

fn key(h: &mut Harness, k: &str) {
    h.step(&json!({"key": k})).unwrap();
    h.frames(3);
}

/// Modify on the row of `id` under `under`: the inline box opens with the record's value.
fn modify(h: &mut Harness, id: u64, under: u64) {
    row_menu(h, id, under);
    menu_pick(h, "menu", "ui.constraintModify");
    h.frames(2);
    let m = h.app.menu.modify.as_ref().expect("Modify opens the inline box");
    assert_eq!((m.constraint, m.under), (id, under));
}

/// Test 1: the list in the documented order, each row with `{constraint}`; Lock Turn only on an
/// Insert; Drive disabled with the engine's reason on a suppressed or Lost constraint.
#[test]
fn the_row_menu_lists_in_documented_order() {
    let mut h = blocks();
    let mate = mate_tops(&mut h);
    let insert = insert_plate_pin(&mut h);
    let items = row_menu(&mut h, insert, 0);
    let mut want: Vec<&str> = ORDER.to_vec();
    want.extend(["-", "ui.rename"]);
    assert_eq!(ids(&items), want);
    for i in items.iter().filter(|i| i.id != "-") {
        assert_eq!(i.params, Some(json!({"constraint": insert})), "{}", i.id);
        assert!(i.enabled, "{} is enabled on an Ok Insert", i.id);
    }
    let labels: Vec<&str> = items.iter().filter(|i| i.id != "-").map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["Edit", "Change Value", "Suppress", "Delete", "Drive", "Lock Turn", "Other Half", "Find in Window", "Rename"]);
    assert_eq!(find(&items, "ui.constraintLock").checked, Some(false), "Lock Turn unchecked");
    assert_eq!(find(&items, "constraint.suppress").checked, Some(false));
    // The list is drawn (a list, R72: no ring on a browser row).
    assert!(crate::scenario::handle_at("menu:constraint.edit").is_some());
    assert!(!h.app.menu.radial);

    // A Mate: the same without Lock Turn.
    let a = occ(&h, "A");
    let items = row_menu(&mut h, mate, a);
    let mut want: Vec<&str> = ORDER.iter().copied().filter(|i| *i != "ui.constraintLock").collect();
    want.extend(["-", "ui.rename"]);
    assert_eq!(ids(&items), want);
    assert!(items.iter().filter(|i| i.id != "-").all(|i| i.params == Some(json!({"constraint": mate}))));

    // Suppressed: Drive disabled with the engine's reason; Suppress checked.
    run(&mut h, "constraint.suppress", json!({"constraint": mate}));
    let items = row_menu(&mut h, mate, 0);
    let drive = find(&items, "ui.constraintDrive");
    assert!(!drive.enabled);
    assert_eq!(drive.tip.as_deref(), Some("Mate:1 is suppressed: unsuppress it to drive it"));
    let engine = h.app.session.execute("constraint.drive", &json!({"constraint": mate})).unwrap_err().to_string();
    assert!(engine.ends_with(drive.tip.as_deref().unwrap()), "the engine's words: {engine}");
    assert_eq!(find(&items, "constraint.suppress").checked, Some(true));
    assert!(find(&items, "constraint.edit").enabled && find(&items, "ui.constraintModify").enabled);

    // Lost: Drive disabled with the engine's reason; Edit and Modify stay live (R134).
    lose_the_hole(&mut h);
    let items = row_menu(&mut h, insert, 0);
    let drive = find(&items, "ui.constraintDrive");
    assert!(!drive.enabled);
    let tip = drive.tip.clone().unwrap();
    assert!(tip.starts_with("Insert:1 is lost (") && tip.ends_with("): edit it to drive it"), "{tip}");
    let engine = h.app.session.execute("constraint.drive", &json!({"constraint": insert})).unwrap_err().to_string();
    assert!(engine.ends_with(&tip), "the engine's words: {engine} / {tip}");
    assert!(find(&items, "constraint.edit").enabled && find(&items, "ui.constraintModify").enabled);

    // SolidCraft: the same list on the same rows (under its Joints folder).
    let mut h = blocks_on(Harness::new());
    let mate = mate_tops(&mut h);
    let items = row_menu(&mut h, mate, 0);
    let mut want: Vec<&str> = ORDER.iter().copied().filter(|i| *i != "ui.constraintLock").collect();
    want.extend(["-", "ui.rename"]);
    assert_eq!(ids(&items), want);
}

/// Test 2 (§8.3 5): Suppress greys all three rows and frees B, one step; Delete removes all
/// three, one step.
#[test]
fn suppress_greys_and_frees_delete_removes_all_three() {
    let mut h = blocks();
    let mate = mate_tops(&mut h);
    let b = occ(&h, "B");
    let undo = h.app.session.undo.len();
    row_menu(&mut h, mate, b);
    menu_pick(&mut h, "menu", "constraint.suppress");
    h.frames(2);
    assert_eq!(h.app.session.undo.len(), undo + 1, "one step");
    assert!(record(&h, mate).suppressed);
    let rows = drawn("Mate:1");
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r.dim), "dimmed in three places: {rows:?}");
    assert_eq!(solidcraft_engine::constraints::report(&h.app.session).dof.get(&b), Some(&6), "B is free again");

    let undo = h.app.session.undo.len();
    row_menu(&mut h, mate, 0);
    menu_pick(&mut h, "menu", "constraint.delete");
    h.frames(2);
    assert_eq!(h.app.session.undo.len(), undo + 1, "one step");
    assert!(drawn("Mate:1").is_empty(), "no Mate:1 row anywhere");
    assert!(constraint_rows(&h.app).is_empty());
    assert!(h.app.session.doc.assembly.constraints.is_empty());
}

/// Test 3: Modify types a value in the row; Enter edits the offset only (one step); Esc leaves it.
#[test]
fn modify_edits_the_value_inline() {
    let mut h = blocks();
    let mate = mate_tops(&mut h);
    let a = occ(&h, "A");
    modify(&mut h, mate, a);
    assert_eq!(h.app.menu.modify.as_ref().unwrap().text, "0", "the record's own text");
    let row = drawn("Mate:1").into_iter().find(|r| r.under == a).unwrap();
    assert_eq!(row.modify.as_deref(), Some("0"), "the box is in the row");
    let undo = h.app.session.undo.len();
    h.step(&json!({"text": "12"})).unwrap();
    h.frames(2);
    key(&mut h, "Enter");
    h.frames(2);
    assert!(h.app.menu.modify.is_none(), "Enter closes the box");
    assert_eq!(h.app.session.undo.len(), undo + 1, "one step");
    let r = record(&h, mate);
    assert_eq!(r.offset, 12.0);
    assert_eq!(r.solution, ConstraintSolution::MateFace, "only the offset changed");
    // B's top faces A's top, 12 mm above it.
    let top = at(&h, "B", [50.0, 95.0, 27.0]);
    assert!((top[2] - 32.0).abs() < 1e-6, "B's top is 12.000 mm from A's: {top:?}");

    // Esc in a second Modify leaves it.
    modify(&mut h, mate, 0);
    assert_eq!(h.app.menu.modify.as_ref().unwrap().text, "12");
    h.step(&json!({"text": "30"})).unwrap();
    h.frames(2);
    key(&mut h, "Escape");
    assert!(h.app.menu.modify.is_none(), "Esc closes the box");
    assert_eq!(h.app.session.undo.len(), undo + 1, "no step");
    assert_eq!(record(&h, mate).offset, 12.0);
}

/// Test 4 (Review Focus): a bad value keeps the row's value, names the problem in `t.error` and
/// takes no step.
#[test]
fn modify_rejects_bad_values_without_a_step() {
    let mut h = blocks();
    let mate = mate_tops(&mut h);
    let t = crate::theme::Tokens::get();
    let undo = h.app.session.undo.len();
    for (typed, problem) in [("abc", "not a number"), ("1e300", "too large"), ("", "type a value")] {
        modify(&mut h, mate, 0);
        if typed.is_empty() {
            key(&mut h, "Backspace");
        } else {
            h.step(&json!({"text": typed})).unwrap();
            h.frames(2);
        }
        key(&mut h, "Enter");
        let m = h.app.menu.modify.as_ref().unwrap_or_else(|| panic!("`{typed}`: the box stays open"));
        assert_eq!(m.text, typed, "the typed text stays to fix");
        assert_eq!(m.error.as_deref(), Some(problem), "`{typed}`");
        let row = drawn("Mate:1").into_iter().find(|r| r.under == 0).unwrap();
        assert_eq!(row.error.as_deref(), Some(problem), "the message is drawn in the row");
        assert_eq!(row.error_color, Some(t.error));
        assert_eq!(h.app.session.undo.len(), undo, "`{typed}`: no step");
        assert_eq!(record(&h, mate).offset, 0.0, "the value is kept");
        key(&mut h, "Escape");
        assert!(h.app.menu.modify.is_none());
    }
    assert_eq!(tr("ui.constraint.not_a_number"), "not a number");
    assert_eq!(tr("ui.constraint.too_large"), "too large");
    assert_eq!(tr("ui.constraint.type_a_value"), "type a value");
}

/// Test 5: a double-click opens Edit; only the touched fields are sent (R162); a Lost constraint
/// is renamed through Edit (R134).
#[test]
fn double_click_edits_sending_changed_fields_only() {
    let mut h = blocks();
    let mate = mate_tops(&mut h);
    let rec = record(&h, mate);
    clicks_expire(&mut h);
    h.step(&json!({"click": {"handle": format!("row:constraint:{mate}:0")}, "double": true})).unwrap();
    h.frames(3);
    assert_eq!(heading(&h), "EDIT Constrain");
    let f = form(&h);
    assert_eq!(f.editing, Some(mate));
    assert_eq!(f.kind, solidcraft_engine::doc::constraints::ConstraintKind::Mate);
    assert_eq!(f.picks[0].as_ref(), rec.picks.first());
    assert_eq!(f.picks[1].as_ref(), rec.picks.get(1));
    assert_eq!(Some(&f.value), Draft::edit(&rec).offset.as_ref(), "the record's own value text (R162)");
    assert_eq!(f.value, "0");
    assert_eq!(crate::dialogs_constraint::commit(&h.app, f).unwrap(), ("constraint.edit".to_string(), json!({"constraint": mate})));
    press_last(&mut h, "Flush");
    assert_eq!(
        crate::dialogs_constraint::commit(&h.app, form(&h)).unwrap(),
        ("constraint.edit".to_string(), json!({"constraint": mate, "solution": "flush"})),
        "the untouched value box is not re-sent"
    );
    let undo = h.app.session.undo.len();
    let ok = h.widget("OK").expect("the panel's OK");
    h.step(&json!({"click": ok})).unwrap();
    h.frames(3);
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.session.undo.len(), undo + 1, "one step");
    let list = run(&mut h, "constraint.list", json!({}));
    assert_eq!(list["constraints"][0]["solution"], "flush");

    // OK with nothing touched takes no step.
    clicks_expire(&mut h);
    h.step(&json!({"click": {"handle": format!("row:constraint:{mate}:0")}, "double": true})).unwrap();
    h.frames(3);
    assert_eq!(form(&h).editing, Some(mate));
    let undo = h.app.session.undo.len();
    let ok = h.widget("OK").unwrap();
    h.step(&json!({"click": ok})).unwrap();
    h.frames(3);
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.session.undo.len(), undo, "no step");

    // A Lost Insert: Edit → Name → OK sends {constraint, name} only, and the rename succeeds.
    let insert = insert_plate_pin(&mut h);
    lose_the_hole(&mut h);
    row_menu(&mut h, insert, 0);
    menu_pick(&mut h, "menu", "constraint.edit");
    assert_eq!(form(&h).editing, Some(insert));
    assert_eq!(heading(&h), "EDIT Constrain");
    {
        let f = form_mut(&mut h);
        f.name = "Pin-hole".into();
        f.touched.insert("name");
    }
    h.frames(2);
    assert_eq!(
        crate::dialogs_constraint::commit(&h.app, form(&h)).unwrap(),
        ("constraint.edit".to_string(), json!({"constraint": insert, "name": "Pin-hole"}))
    );
    let undo = h.app.session.undo.len();
    let ok = h.widget("OK").unwrap();
    h.step(&json!({"click": ok})).unwrap();
    h.frames(3);
    assert!(h.app.dialog.is_none(), "OK closes the edit: {:?}", h.app.status);
    assert_eq!(record(&h, insert).name, "Pin-hole");
    assert_eq!(h.app.session.undo.len(), undo + 1);
}

/// Test 6: Other Half picks the row under the other component; Find in Window frames both
/// parts through the view-fit path.
#[test]
fn other_half_and_find() {
    let mut h = blocks();
    let mate = mate_tops(&mut h);
    let (a, b) = (occ(&h, "A"), occ(&h, "B"));
    row_menu(&mut h, mate, a);
    menu_pick(&mut h, "menu", "ui.constraintOtherHalf");
    assert_eq!(h.app.tree.picked_constraints, [mate]);
    assert_eq!(h.app.tree.constraint_row, Some((mate, b)), "the row under B leads");
    h.frames(2);
    assert_eq!(h.app.tree.constraint_reveal, None, "scrolled to it");
    let row = drawn("Mate:1").into_iter().find(|r| r.under == b).unwrap();
    assert!(row.selected && row.lead, "{row:?}");
    // From B, back to A.
    row_menu(&mut h, mate, b);
    menu_pick(&mut h, "menu", "ui.constraintOtherHalf");
    assert_eq!(h.app.tree.constraint_row, Some((mate, a)));

    let home = h.app.cam.target;
    row_menu(&mut h, mate, 0);
    menu_pick(&mut h, "menu", "ui.constraintFind");
    h.frames(frames_for(3.0));
    let found = h.app.cam;
    assert!((found.target - home).len() > 1.0, "the view moved: {home:?} -> {:?}", found.target);
    // The same view Fit gives with both parts selected (the view-fit path, laid out around the
    // panels), from the same start.
    assert_eq!(h.call("ui.view", json!({"view": "home"}))["ok"], json!(true));
    h.frames(frames_for(3.0));
    run(&mut h, "select.set", json!({"items": [{"type": "occurrence", "path": [a]}, {"type": "occurrence", "path": [b]}]}));
    h.app.animate_view("fit");
    h.frames(frames_for(3.0));
    let fit = h.app.cam;
    assert!((found.target - fit.target).len() < 1e-6, "Find frames A and B as Fit does: {:?} vs {:?}", found.target, fit.target);
    assert!((found.distance - fit.distance).abs() < 1e-6, "{} vs {}", found.distance, fit.distance);
    let bb = crate::dialogs_assembly::occurrence_bounds(&h.app.session, &[a, b]);
    assert!(!bb.is_empty());
}

/// Test 7: the Delete key with constraint rows picked deletes them in one step.
#[test]
fn the_delete_key_deletes_picked_constraints_in_one_step() {
    let mut h = blocks();
    let mate = mate_tops(&mut h);
    let insert = insert_plate_pin(&mut h);
    clicks_expire(&mut h);
    h.step(&json!({"click": {"handle": format!("row:constraint:{mate}:0")}})).unwrap();
    h.frames(2);
    clicks_expire(&mut h);
    h.step(&json!({"click": {"handle": format!("row:constraint:{insert}:0")}, "ctrl": true})).unwrap();
    h.frames(2);
    assert_eq!(h.app.tree.picked_constraints, [mate, insert]);
    let undo = h.app.session.undo.len();
    key(&mut h, "Delete");
    h.settle();
    assert!(h.app.session.doc.assembly.constraints.is_empty(), "both gone");
    assert!(drawn_constraints().is_empty());
    assert_eq!(h.app.session.undo.len(), undo + 1, "one step");
    assert!(h.app.tree.picked_constraints.is_empty());
}

/// Lock Turn flips the Insert's lock in one step; Rename renames through `constraint.edit {name}`.
#[test]
fn lock_turn_and_rename() {
    let mut h = blocks();
    let insert = insert_plate_pin(&mut h);
    let undo = h.app.session.undo.len();
    let pin = occ(&h, "Pin");
    row_menu(&mut h, insert, pin);
    menu_pick(&mut h, "menu", "ui.constraintLock");
    assert!(record(&h, insert).lock);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let items = row_menu(&mut h, insert, 0);
    assert_eq!(find(&items, "ui.constraintLock").checked, Some(true));
    super::close(&mut h.app);

    // Rename: the shared rename box, committed as constraint.edit {constraint, name}.
    let undo = h.app.session.undo.len();
    row_menu(&mut h, insert, 0);
    menu_pick(&mut h, "menu", "ui.rename");
    assert_eq!(h.app.menu.rename.as_ref().map(|r| r.text.as_str()), Some("Insert:1"));
    assert!(super::finish_rename(&mut h.app, Some("Pin-hole"), true));
    h.frames(2);
    assert_eq!(record(&h, insert).name, "Pin-hole");
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(drawn_constraints().iter().all(|d| d.label == "Pin-hole"));
}

/// Wave 1 item 8: a Modify box whose row is no longer drawn (its folder folded, the browser
/// hidden) is dropped, not left pending to commit later.
#[test]
fn modify_closes_when_its_row_is_not_drawn() {
    let mut h = blocks();
    let mate = mate_tops(&mut h);
    let a = occ(&h, "A");
    modify(&mut h, mate, a);
    h.frames(2);
    assert!(h.app.menu.modify.is_some(), "drawn: it stays open");
    h.app.ui.show_browser = false;
    h.frames(2);
    assert!(h.app.menu.modify.is_none(), "its row is not drawn: Modify is dropped");
}

/// Wave 1 item 8: Find in Window's framing goes through the pages that frame themselves, as Fit
/// does: on a sheet it asks the sheet to fit, and the model camera stays.
#[test]
fn framing_on_a_sheet_fits_the_sheet() {
    let mut h = inventor(solidcraft_engine::doc::DocumentKind::Drawing);
    h.app.drawing.fit_pending = false;
    let cam = h.app.cam;
    let mut b = solidcraft_engine::geom::Aabb3::EMPTY;
    b.add(solidcraft_engine::geom::Vec3::new(0.0, 0.0, 0.0));
    b.add(solidcraft_engine::geom::Vec3::new(10.0, 10.0, 10.0));
    h.app.frame_bounds(b);
    assert!(h.app.drawing.fit_pending, "the sheet fits");
    assert!(h.app.cam_anim.is_none() && h.app.cam == cam, "the model camera stays");
}
