#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Copy and Paste of a placed component (R186), on both profiles: a selection that resolves to
//! an occurrence (its body, face or edge in the view, or its Browser row) pastes a new
//! occurrence of the same component, never a body at the root.

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::DocumentKind::Assembly;
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::{Session, doc};

use crate::scenario::Harness;
use crate::test_support::inventor;

/// An assembly on each profile with the saved 10 x 20 x 30 part `P` placed once (grounded at
/// the origin), seen from the iso view.
fn both() -> Vec<(Harness, u64)> {
    let mut sc = Harness::new();
    sc.app.session.doc_mut().kind = Assembly;
    [sc, inventor(Assembly)]
        .into_iter()
        .map(|mut h| {
            h.app.home.open = false;
            let file = h.dir.join("P.solidcraft");
            let mut s = Session::default();
            s.execute("solid.box", &json!({"length": 10, "width": 20, "height": 30})).unwrap();
            s.execute("file.save", &json!({"path": file})).unwrap();
            let out = h.app.run("component.insert_design", json!({"path": file})).unwrap();
            let occ = out["occurrence"].as_u64().unwrap();
            h.call("ui.view", json!({"view": "iso"}));
            h.call("ui.camera", json!({"target": [20, 20, 0], "distance": 260}));
            h.frames(3);
            (h, occ)
        })
        .collect()
}

fn copies(h: &Harness, component: u64) -> Vec<doc::Occurrence> {
    h.app.session.doc.occurrences.iter().filter(|o| o.component == component).cloned().collect()
}

fn component_of(h: &Harness, occ: u64) -> u64 {
    h.app.session.doc.occurrences.iter().find(|o| o.id == occ).unwrap().component
}

/// Bodies of the assembly's own (root) design.
fn root_bodies(h: &Harness) -> usize {
    let st = h.app.session.world_state();
    st.bodies.iter().filter(|b| h.app.session.doc.model_body_component(b) == 0).count()
}

fn key(h: &mut Harness, event: egui::Event) {
    h.app.synthetic.push(event);
    h.settle();
}

#[test]
fn ctrl_c_ctrl_v_of_a_placed_parts_body_pastes_a_new_occurrence_at_the_pointer() {
    for (mut h, occ) in both() {
        let component = component_of(&h, occ);
        // Select the placed part's body in the view (the owner's pick).
        h.step(&json!({"click": {"world": [5.0, 10.0, 30.0]}})).unwrap();
        h.frames(2);
        assert!(!h.app.session.selection.is_empty(), "the click selects the part");
        assert_eq!(super::selected_occurrences(&h.app), Some(vec![occ]), "the pick resolves to P:1: {:?}", h.app.session.selection);
        key(&mut h, egui::Event::Copy);
        let undo = h.app.session.undo.len();
        h.step(&json!({"move": {"world": [60.0, 40.0, 0.0]}})).unwrap();
        h.frames(1);
        key(&mut h, egui::Event::Paste(String::new()));
        let all = copies(&h, component);
        assert_eq!(all.len(), 2, "P:2 exists: {:?}", h.app.status);
        assert_eq!(root_bodies(&h), 0, "no root body");
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
        let pasted = all.iter().find(|o| o.id != occ).unwrap();
        assert!(!pasted.grounded, "the copy is free");
        let at = Vec3::new(pasted.transform[3][0], pasted.transform[3][1], pasted.transform[3][2]);
        assert!(at.dist(Vec3::new(60.0, 40.0, 0.0)) < 0.5, "placed at the pointer: {at:?}");
        assert_eq!(h.app.session.selection, vec![Sel::Occurrence { path: vec![pasted.id] }], "the paste is selected");
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(copies(&h, component).len(), 1, "undo takes it back");
    }
}

#[test]
fn a_browser_row_copy_pastes_an_occurrence_offset_from_the_source_off_the_view() {
    for (mut h, occ) in both() {
        let component = component_of(&h, occ);
        // The Browser row's pick.
        h.app.session.selection = vec![Sel::Occurrence { path: vec![occ] }];
        assert!(super::copy(&mut h.app));
        let undo = h.app.session.undo.len();
        assert!(super::can_paste(&h.app));
        assert!(super::paste(&mut h.app, None), "{:?}", h.app.status);
        let all = copies(&h, component);
        assert_eq!(all.len(), 2);
        assert_eq!(root_bodies(&h), 0);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        let pasted = all.iter().find(|o| o.id != occ).unwrap();
        assert_eq!(pasted.transform[3][..3], super::PASTE_OFFSET, "Inventor's paste offset from the source");
        // A second paste of the same copy goes on.
        assert!(super::paste(&mut h.app, None));
        assert_eq!(copies(&h, component).len(), 3);
    }
}

#[test]
fn a_root_body_pick_is_not_a_placement() {
    let mut h = inventor(Assembly);
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    let body = h.app.session.world_state().bodies[0].name.clone();
    h.app.session.selection = vec![Sel::Body { name: body }];
    assert_eq!(super::picked_placements(&h.app), super::Picked::Other, "a root body is not a placement");
    assert_eq!(super::selected_occurrences(&h.app), None);
}

/// The root box's body and P's face picked together: Copy refuses with a reason, and the
/// clipboard keeps what it had (so the next paste is still P, never stale features).
#[test]
fn a_mixed_component_and_root_body_copy_is_refused_and_keeps_the_clipboard() {
    for (mut h, occ) in both() {
        let component = component_of(&h, occ);
        h.app.session.selection = vec![Sel::Occurrence { path: vec![occ] }];
        assert!(super::copy(&mut h.app));
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-50, -50, 0]})).unwrap();
        let root =
            h.app.session.world_state().bodies.iter().find(|b| h.app.session.doc.model_body_component(b) == 0).map(|b| b.name.clone()).unwrap();
        let before = h.app.occurrence_clipboard.clone();
        h.app.session.selection = vec![Sel::Occurrence { path: vec![occ] }, Sel::Body { name: root }];
        assert_eq!(super::picked_placements(&h.app), super::Picked::Mixed);
        assert!(!super::copy(&mut h.app), "refused");
        assert_eq!(h.app.status.as_ref().map(|s| s.0.as_str()), Some("Copy components or bodies, not both"));
        assert_eq!(h.app.occurrence_clipboard, before, "the clipboard is unchanged");
        assert!(super::paste(&mut h.app, None));
        assert_eq!(copies(&h, component).len(), 2, "the paste is still P");
        assert_eq!(root_bodies(&h), 1, "no new root body");
    }
}

/// A SolidCraft Part design with a component keeps its own Copy (R186 is about assemblies).
#[test]
fn a_part_design_keeps_its_own_copy() {
    let (mut h, occ) = both().remove(1);
    h.app.session.doc_mut().kind = solidcraft_engine::doc::DocumentKind::Part;
    h.app.session.selection = vec![Sel::Occurrence { path: vec![occ] }];
    assert_eq!(super::picked_placements(&h.app), super::Picked::Other);
}

/// Two copied placements, the second of which is gone by the paste: the first is pasted and
/// selected, and the status says where it stopped.
#[test]
fn a_paste_that_stops_part_way_selects_what_it_pasted() {
    let (mut h, occ) = both().remove(0);
    let component = component_of(&h, occ);
    h.app.run("component.create", json!({"name": "Q", "grounded": false})).unwrap();
    h.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [80, 0, 0]})).unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    let q = h.app.session.doc.occurrences.iter().find(|o| o.name == "Q:1").unwrap().id;
    h.app.session.selection = vec![Sel::Occurrence { path: vec![occ] }, Sel::Occurrence { path: vec![q] }];
    assert!(super::copy(&mut h.app));
    h.app.run("component.delete", json!({"component": "Q"})).unwrap();
    assert!(super::paste(&mut h.app, None));
    let pasted = copies(&h, component).into_iter().find(|o| o.id != occ).expect("P was pasted");
    assert_eq!(h.app.session.selection, vec![Sel::Occurrence { path: vec![pasted.id] }]);
    let status = h.app.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
    assert!(status.starts_with(&format!("Pasted {}, then stopped:", pasted.name)), "{status}");
}

/// The Position panel's Move/Copy with P's body picked in the view (R186): it moves P as an
/// occurrence, and with Create Copy places a new occurrence, P:2, never a root body. P is
/// grounded (the first part), so only the copy moves.
#[test]
fn position_move_with_create_copy_places_a_new_occurrence() {
    for (mut h, occ) in both() {
        let component = component_of(&h, occ);
        h.step(&json!({"click": {"world": [5.0, 10.0, 30.0]}})).unwrap();
        h.frames(2);
        crate::test_support::clicks_expire(&mut h);
        assert!(matches!(h.app.session.selection.as_slice(), [Sel::Face { .. }]), "{:?}", h.app.session.selection);
        h.app.start("solid.move");
        h.frames(3);
        assert_eq!(h.app.tree.occurrence_move.as_ref().map(|m| m.occurrence), Some(occ), "the occurrence's Move/Copy opens, not the body's");
        assert!(h.app.dialog.is_none(), "no body Move/Copy");
        let undo = h.app.session.undo.len();
        h.step(&json!({"widget": "Create Copy"})).unwrap();
        h.frames(2);
        let copy = h.app.tree.occurrence_move.as_ref().map(|m| m.occurrence).unwrap();
        assert_ne!(copy, occ, "the panel moves the copy");
        assert_eq!(h.app.session.undo.len(), undo + 1, "the copy is one step");
        h.step(&json!({"click": {"handle": "occurrence-move:distance:0"}})).unwrap();
        h.step(&json!({"key": "A", "ctrl": true})).unwrap();
        h.step(&json!({"text": "40"})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(2);
        h.step(&json!({"widget": "Capture All Positions"})).unwrap();
        h.frames(2);
        assert!(h.app.tree.occurrence_move.is_none());
        let all = copies(&h, component);
        assert_eq!(all.len(), 2, "P:2 exists");
        assert_eq!(root_bodies(&h), 0, "no root body");
        let source = all.iter().find(|o| o.id == occ).unwrap();
        let made = all.iter().find(|o| o.id != occ).unwrap();
        assert_eq!(source.transform[3][..3], [0.0, 0.0, 0.0], "P:1 stays");
        assert!((made.transform[3][0] - 40.0).abs() < 1e-6, "the copy moved 40 mm: {:?}", made.transform[3]);
        assert!(!made.grounded);
    }
}

#[test]
fn a_picked_component_row_and_the_rings_copy_paste_occurrences_too() {
    for (mut h, occ) in both() {
        let component = component_of(&h, occ);
        // SolidCraft's component row (a pick, no selection).
        h.app.session.selection.clear();
        h.app.tree.picked_components = vec![component];
        assert_eq!(super::selected_occurrences(&h.app), Some(vec![occ]));
        // The ring's / list's Copy and Paste run the same clipboard.
        let at = egui::Pos2::ZERO;
        crate::context_menu::run_item(&mut h.app, &crate::context_menu::act("ui.copy", "Copy", "copy"), at);
        crate::context_menu::run_item(&mut h.app, &crate::context_menu::act("ui.paste", "Paste", "paste"), at);
        assert_eq!(copies(&h, component).len(), 2, "{:?}", h.app.status);
        assert_eq!(root_bodies(&h), 0);
    }
}
