#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Plan E-2 Task 8: the assembly marking menus (spec §6.7, R72, R136). Both assembly rings take
//! the captured NE (Constraint) and the component ring its E (Show Constraints); the component
//! list's Grounded is the context menu's own item (its label, check and params, R136); an open
//! Constrain or Drive panel's right-click is a list by the dialog's kind; nothing else in the
//! profile's marking-menu data moves (`workflow_warnings` and the rest of `unmapped`, R142).

use serde_json::{Value, json};
use solidcraft_engine::doc::DocumentKind::Assembly;

use super::*;
use crate::context_menu::{Item, Target, describe};
use crate::dialogs::Kind;
use crate::dialogs_assembly::Asm;
use crate::profile_workflow::{validate_workflow, workflow_warnings};
use crate::scenario::Harness;
use crate::test_support::{click_world, constraint_blocks, constraint_id, inventor, menu_pick, right_click};

fn blocks() -> Harness {
    let mut h = inventor(Assembly);
    h.app.home.open = false;
    constraint_blocks(&mut h);
    h.frames(2);
    h
}

fn occ(h: &Harness, name: &str) -> u64 {
    h.app.session.doc.occurrences.iter().find(|o| o.name == format!("{name}:1")).map(|o| o.id).unwrap_or_else(|| panic!("no occurrence {name}"))
}

/// A point on screen in front of the blocks, where the view shows nothing.
fn empty_space(h: &mut Harness) -> Value {
    let [x, y] = h.at(&json!({"world": [50, -40, 0]})).unwrap();
    json!([x, y + 120.0])
}

fn ids(menu: &Value, key: &str) -> Vec<String> {
    menu[key].as_array().unwrap_or_else(|| panic!("no {key} in {menu}")).iter().map(|i| i["id"].as_str().unwrap().to_string()).collect()
}

fn entry<'a>(menu: &'a Value, key: &str, id: &str) -> &'a Value {
    menu[key].as_array().unwrap().iter().find(|i| i["id"] == id).unwrap_or_else(|| panic!("no {id} in {key}: {menu}"))
}

/// Mate:1, A's top on B's top.
fn mate_tops(h: &mut Harness) -> u64 {
    h.app
        .run(
            "constraint.create",
            json!({"type": "mate", "a": {"occurrence": "A", "face": [50, 25, 20]}, "b": {"occurrence": "B", "face": [50, 95, 27]}}),
        )
        .unwrap();
    h.frames(2);
    constraint_id(h, "Mate:1")
}

/// §8.3 8: NE = Constraint in both rings, E = Show Constraints on a component; the component
/// list's Grounded is the context menu's item (R136); the empty list holds Show and Hide All.
#[test]
fn assembly_rings_take_the_captured_slots() {
    let mut h = blocks();
    let mate = mate_tops(&mut h);
    let b = occ(&h, "B");
    // B picked in the view, then a right-click on empty space (which keeps the pick).
    click_world(&mut h, [50.0, 95.0, 27.0]);
    assert!(!h.app.session.selection.is_empty(), "B is picked");
    let empty = empty_space(&mut h);
    right_click(&mut h, empty.clone());
    assert!(!h.app.session.selection.is_empty(), "the right-click on empty space keeps B");
    assert_eq!(context(&h.app), MenuContext::AssemblyComponent);
    let menu = describe(&h.app);
    let ring = ids(&menu, "radial");
    assert_eq!(ring, ["-", "constraint.create", "constraint.show", "-", "-", "-", "edit.undo", "inspect.measure"]);
    assert_eq!(ring[1], "constraint.create", "NE");
    assert_eq!(ring[2], "constraint.show", "E");
    assert_eq!(ids(&menu, "items"), ["ui.repeat", "-", "ui.delete", "-", "solid.move", "-", "occurrence.ground"]);
    // Grounded is the context menu's own item for B: its words, its check, its params (R136).
    let ground = entry(&menu, "items", "occurrence.ground");
    assert_eq!(ground["label"], json!("Grounded"));
    assert_eq!(ground["checked"], json!(false));
    assert_eq!(ground["params"], json!({"occurrence": b, "grounded": true}));
    let listed: Item = crate::context_menu::items(&h.app, &Target::Occurrence { path: vec![b] })
        .into_iter()
        .find(|i| i.id == "occurrence.ground")
        .expect("B's context menu has Grounded");
    assert_eq!(
        (ground["label"].as_str(), ground["checked"].as_bool(), &ground["params"]),
        (Some(listed.label.as_str()), listed.checked, &json!(listed.params))
    );
    // E fires Show: B's constraints join the shown set.
    assert!(h.app.session.shown_constraints.is_empty());
    menu_pick(&mut h, "marking", "constraint.show");
    assert!(h.app.session.shown_constraints.contains(&mate), "E showed B's Mate:1: {:?}", h.app.session.shown_constraints);
    // Grounded grounds B; reopened, the item is checked and would unground it.
    right_click(&mut h, empty.clone());
    menu_pick(&mut h, "menu", "occurrence.ground");
    assert!(h.app.session.doc.occurrences.iter().any(|o| o.id == b && o.grounded), "B is grounded");
    right_click(&mut h, empty.clone());
    let menu = describe(&h.app);
    let ground = entry(&menu, "items", "occurrence.ground");
    assert_eq!((ground["checked"].clone(), ground["params"].clone()), (json!(true), json!({"occurrence": b, "grounded": false})));
    crate::context_menu::close(&mut h.app);

    // Nothing picked: NE = Constraint, the list holds Show and Hide All; NE opens the panel.
    h.app.session.selection.clear();
    h.app.tree.picked_components.clear();
    right_click(&mut h, empty);
    assert_eq!(context(&h.app), MenuContext::AssemblyEmpty);
    let menu = describe(&h.app);
    assert_eq!(ids(&menu, "radial")[1], "constraint.create", "NE");
    let list = ids(&menu, "items");
    assert!(list.iter().any(|i| i == "constraint.show") && list.iter().any(|i| i == "constraint.hide_all"), "{list:?}");
    menu_pick(&mut h, "marking", "constraint.create");
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Assembly(Asm::Constraint(_)))), "NE opened the Constrain panel");
}

/// An open Constrain panel's right-click is the list OK, Apply, Cancel (OK waits for two picks);
/// an open Drive panel's is OK, Cancel. Lists, no ring (R72).
#[test]
fn the_panel_and_drive_lists() {
    let mut h = blocks();
    let empty = empty_space(&mut h);
    h.app.start("constraint.create");
    h.frames(2);
    right_click(&mut h, empty.clone());
    let menu = describe(&h.app);
    assert_eq!(ids(&menu, "radial"), Vec::<String>::new(), "a list, no ring");
    assert_eq!(ids(&menu, "items"), ["ui.ok", "ui.applyContinue", "ui.cancel"]);
    assert_eq!(entry(&menu, "items", "ui.ok")["enabled"], json!(false), "OK waits for two picks");
    crate::context_menu::close(&mut h.app);
    click_world(&mut h, [50.0, 25.0, 20.0]);
    crate::test_support::clicks_expire(&mut h);
    click_world(&mut h, [50.0, 95.0, 27.0]);
    h.frames(2);
    right_click(&mut h, empty.clone());
    let menu = describe(&h.app);
    assert_eq!(entry(&menu, "items", "ui.ok")["enabled"], json!(true), "two picks: OK can place it");
    menu_pick(&mut h, "menu", "ui.ok");
    let mate = constraint_id(&h, "Mate:1");
    assert!(h.app.dialog.is_none(), "OK placed Mate:1 and closed the panel");

    crate::dialogs_constraint_drive::open(&mut h.app, mate);
    h.frames(2);
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Assembly(Asm::DriveConstraint(_)))));
    right_click(&mut h, empty);
    let menu = describe(&h.app);
    assert_eq!(ids(&menu, "radial"), Vec::<String>::new(), "a list, no ring");
    assert_eq!(ids(&menu, "items"), ["ui.ok", "ui.cancel"]);
    menu_pick(&mut h, "menu", "ui.cancel");
    assert!(h.app.dialog.is_none(), "Cancel closed the Drive panel");

    // SolidCraft has no marking menus: its panel menu is as it was (no kind list).
    let mut s = Harness::new();
    s.app.home.open = false;
    assert!(layout(&s.app).is_none());
    assert!(overflow(&s.app).is_none() && ring(&s.app).is_none());
}

/// No new ring context: `workflow_warnings` is what it was, the data validates, and `unmapped`
/// lost exactly the six captured entries Task 8 maps (three per layout, R142).
#[test]
fn workflow_warnings_unchanged() {
    let inventor = crate::profile::get(crate::profile::INVENTOR).unwrap();
    // Before Task 8 (`afacd258`): every Inventor layout has its capture, so no warning.
    let before: Vec<String> = vec![];
    assert_eq!(workflow_warnings(&inventor.workflow), before);
    assert_eq!(validate_workflow(&inventor.workflow), Vec::<String>::new());
    let layouts = inventor.workflow.marking_menu.as_ref().unwrap();
    let component_before = [
        "N: Open",
        "NE: Constraint",
        "E: Show Relationships",
        "SE: Visibility",
        "S: Edit",
        "SW: Find in Browser",
        "list: Copy",
        "list: Add to New Folder",
        "list: Selection",
        "list: Isolate",
        "list: Undo Isolate",
        "list: Free Rotate",
        "list: Substitute",
        "list: Representation…",
        "list: Component",
        "list: Create Note",
        "list: BOM Structure",
        "list: Visibility",
        "list: iMate Glyph Visibility",
        "list: Grounded",
        "list: Adaptive",
        "list: Enabled",
        "list: Transparent",
        "list: Contact Set",
        "list: Suppress",
        "list: Instance Properties…",
        "list: iProperties…",
        "list: AEC Properties…",
        "list: How To…",
    ];
    let empty_before = [
        "NE: Constraint",
        "E: Pattern Component",
        "SW: Free Rotate",
        "list: Replace from Content Center…",
        "list: Make Layout",
        "list: Create Pipe Run…",
        "list: Create Harness…",
        "list: Assemble",
        "list: Show Relationships",
        "list: Hide All",
        "list: Create iMate",
        "list: Dimension Display",
        "list: Help Topics…",
    ];
    let mut removed = 0;
    for (ctx, before, mapped) in [
        (MenuContext::AssemblyComponent, &component_before[..], ["NE: Constraint", "E: Show Relationships", "list: Grounded"]),
        (MenuContext::AssemblyEmpty, &empty_before[..], ["NE: Constraint", "list: Show Relationships", "list: Hide All"]),
    ] {
        let expected: Vec<&str> = before.iter().copied().filter(|u| !mapped.contains(u)).collect();
        assert_eq!(layouts[&ctx].unmapped, expected, "{ctx}");
        removed += before.len() - layouts[&ctx].unmapped.len();
    }
    assert_eq!(removed, 6);
    assert_eq!(
        layouts[&MenuContext::AssemblyComponent].ring,
        ["", "constraint.create", "constraint.show", "", "", "", "edit.undo", "inspect.measure"]
    );
    assert_eq!(
        layouts[&MenuContext::AssemblyEmpty].ring,
        ["component.insert_design", "constraint.create", "", "solid.move", "component.create", "", "edit.undo", "inspect.measure"]
    );
    assert_eq!(
        layouts[&MenuContext::AssemblyEmpty].overflow,
        ["joint.create", "-", "constraint.show", "constraint.hide_all", "-", "ui.previousView", "ui.home", "-", "ui.createDrawingView"]
    );
}
