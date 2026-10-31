#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! R185: F2 or a slow second click on a picked constraint's row renames it in the row (Enter:
//! `constraint.edit {name}`, one step; a taken name is refused with `check_name`'s words); a
//! fast double-click still opens Edit. Both profiles.

use serde_json::json;

use super::constraint_value_tests::{PROFILES, blocks_in_inches, click_row, occ, run};
use crate::context_menu::RenameWhat;
use crate::dialogs::Kind;
use crate::dialogs_assembly::Asm;
use crate::scenario::Harness;
use crate::test_support::{clicks_expire, constraint_id};

/// The constraint renamed in a row, and the row it is typed in.
fn renaming(h: &Harness) -> Option<(u64, bool)> {
    let r = h.app.menu.rename.as_ref()?;
    match r.what {
        RenameWhat::Constraint(c) => Some((c, r.inline)),
        _ => None,
    }
}

fn type_name(h: &mut Harness, name: &str) {
    h.step(&json!({"key": "A", "ctrl": true})).unwrap();
    h.step(&json!({"text": name})).unwrap();
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(3);
}

fn name(h: &Harness, id: u64) -> String {
    h.app.session.doc.assembly.constraint(&id.to_string()).unwrap().name.clone()
}

#[test]
fn f2_renames_the_picked_constraint_in_its_row() {
    for make in PROFILES {
        let (mut h, mate) = blocks_in_inches(make);
        let b = occ(&h, "B");
        click_row(&mut h, mate, b);
        h.step(&json!({"key": "F2"})).unwrap();
        h.frames(2);
        assert_eq!(renaming(&h), Some((mate, true)), "F2 renames in the row");
        assert!(crate::scenario::handle_at(&format!("rename:constraint:{mate}:{b}")).is_some(), "the box is in the clicked row");
        let undo = h.app.session.undo.len();
        type_name(&mut h, "Pipe-seat");
        assert_eq!(h.app.session.undo.len(), undo + 1, "one step");
        assert_eq!(name(&h, mate), "Pipe-seat");
        assert_eq!(renaming(&h), None);
        assert!(crate::browser::drawn_constraints().iter().filter(|r| r.id == mate).all(|r| r.label == "Pipe-seat"));
    }
}

#[test]
fn a_slow_second_click_renames_and_a_fast_double_click_edits() {
    for make in PROFILES {
        let (mut h, mate) = blocks_in_inches(make);
        let b = occ(&h, "B");
        click_row(&mut h, mate, b);
        assert_eq!(h.app.tree.picked_constraints, [mate]);
        click_row(&mut h, mate, b);
        assert_eq!(renaming(&h), Some((mate, true)), "a slow second click renames");
        assert_eq!(h.app.tree.picked_constraints, [mate], "and keeps the pick");
        h.step(&json!({"key": "Escape"})).unwrap();
        h.frames(2);
        assert_eq!(renaming(&h), None, "Escape cancels");
        assert_eq!(name(&h, mate), "Mate:1");
        // A fast double-click is Edit, not a rename.
        clicks_expire(&mut h);
        h.step(&json!({"click": {"handle": format!("row:constraint:{mate}:{b}")}, "double": true})).unwrap();
        h.frames(2);
        clicks_expire(&mut h);
        assert_eq!(renaming(&h), None, "no rename on a double-click");
        assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Assembly(Asm::Constraint(_)))), "Edit opens");
    }
}

#[test]
fn a_taken_name_is_refused_in_the_row() {
    for make in PROFILES {
        let (mut h, mate) = blocks_in_inches(make);
        run(
            &mut h,
            "constraint.create",
            json!({"type": "insert", "a": {"occurrence": "Plate", "edge": [200.0, 0.0, 10.0]}, "b": {"occurrence": "Pin", "edge": [310.0, 0.0, 50.0]}}),
        );
        let _ = constraint_id(&h, "Insert:1");
        let b = occ(&h, "B");
        click_row(&mut h, mate, b);
        h.step(&json!({"key": "F2"})).unwrap();
        h.frames(2);
        let undo = h.app.session.undo.len();
        type_name(&mut h, "Insert:1");
        assert_eq!(h.app.session.undo.len(), undo, "no step");
        assert_eq!(name(&h, mate), "Mate:1");
        let why = "`Insert:1` is already a constraint's name";
        assert_eq!(h.app.menu.rename.as_ref().and_then(|r| r.error.clone()).as_deref(), Some(why), "the box stays, saying why");
        assert!(crate::browser::drawn_constraints().iter().any(|r| r.id == mate && r.error.as_deref() == Some(why)), "drawn under the row");
    }
}
