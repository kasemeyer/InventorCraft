#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Plan E-2 Task 3: picking, the live preview and the commit (spec §6.3). Inventor tests run on
//! `inventor(Assembly)` with E-1's blocks and click world points; SolidCraft gets the same flow.

use egui::accesskit::{Node, Role, Toggled};
use serde_json::{Value, json};
use solidcraft_engine::constraints::report;
use solidcraft_engine::doc::DocumentKind::Assembly;
use solidcraft_engine::doc::Mat;
use solidcraft_engine::doc::constraints::{ConstraintKind as K, ConstraintSolution as S, Geo, GeoRef, OriginItem, PrimKind};

use super::ConstraintForm;
use crate::dialogs::{Kind, PanelAction, can_act};
use crate::dialogs_assembly::Asm;
use crate::scenario::{Harness, count_of};
use crate::test_support::{alias, click_world, constraint_blocks, constraint_id, inventor};

const A_TOP: [f64; 3] = [50.0, 25.0, 20.0];
const B_TOP: [f64; 3] = [50.0, 95.0, 27.0];
const PLATE_TOP: [f64; 3] = [160.0, 30.0, 10.0];
const PIN_TOP_FACE: [f64; 3] = [300.0, 5.0, 110.0];

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

/// The blocks, with the panel opened by `C`.
fn opened() -> Harness {
    let mut h = inventor(Assembly);
    constraint_blocks(&mut h);
    alias(&mut h, "C");
    h
}

fn occ(h: &Harness, name: &str) -> u64 {
    h.app.session.doc.occurrences.iter().find(|o| o.name == format!("{name}:1")).map(|o| o.id).unwrap_or_else(|| panic!("no occurrence {name}"))
}

fn transform(h: &Harness, name: &str) -> Mat {
    solidcraft_engine::doc::joints::occurrence_world(&h.app.session.doc, occ(h, name))
}

fn near(a: &Mat, b: &Mat, tol: f64) -> bool {
    a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() <= tol)
}

/// Hover a world point for `frames` frames.
fn hover(h: &mut Harness, p: [f64; 3], frames: usize) {
    h.step(&json!({"move": {"world": p}})).unwrap();
    h.frames(frames);
}

fn list(h: &mut Harness) -> Vec<Value> {
    h.app.run("constraint.list", json!({})).unwrap()["constraints"].as_array().cloned().unwrap_or_default()
}

/// The accessible nodes drawn inside the panel labelled `label`.
fn nodes(h: &Harness, label: &str) -> Vec<Node> {
    let Some(panel) = crate::scenario::rect_of("dialog:outer") else { return Vec::new() };
    let Some(tree) = h.output.accesskit_update.as_ref() else { return Vec::new() };
    tree.nodes
        .iter()
        .filter(|(_, n)| n.label() == Some(label) || n.value() == Some(label))
        .filter(|(_, n)| n.bounds().is_some_and(|b| panel.contains(egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32))))
        .map(|(_, n)| n.clone())
        .collect()
}

fn drawn(h: &Harness, label: &str) -> bool {
    !nodes(h, label).is_empty()
}

fn value_box(h: &Harness, label: &str) -> Option<String> {
    nodes(h, label).into_iter().find(|n| n.role() == Role::TextInput).and_then(|n| n.value().map(str::to_string))
}

fn can(h: &Harness, action: PanelAction) -> bool {
    can_act(&h.app, h.app.dialog.as_ref().unwrap(), action)
}

/// Click the panel's control labelled `label` of `role`; `last`: the lowest such (a solution
/// below the type of the same word).
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

fn check(h: &mut Harness, label: &str) {
    press_role(h, label, Role::CheckBox, false);
}

fn pick_solution(h: &mut Harness, word: &str) {
    press_role(h, word, Role::Button, true);
}

/// Two picks in the viewport, then a frame for the preview.
fn picks(h: &mut Harness, a: [f64; 3], b: [f64; 3]) {
    click_world(h, a);
    crate::test_support::clicks_expire(h);
    click_world(h, b);
    crate::test_support::clicks_expire(h);
    h.frames(2);
}

fn enter(h: &mut Harness) {
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(3);
}

#[test]
fn c_two_faces_enter_makes_mate_1() {
    let mut h = opened();
    let undo = h.app.session.undo.len();
    picks(&mut h, A_TOP, B_TOP);
    assert!(form(&h).ready(), "{:?}", form(&h).picks);
    enter(&mut h);
    assert!(h.app.dialog.is_none(), "Enter is OK");
    let rows = list(&mut h);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], json!("Mate:1"));
    assert_eq!(rows[0]["occurrences"], json!([occ(&h, "A"), occ(&h, "B")]));
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    assert_eq!(report(&h.app.session).rows.len(), 1);
}

#[test]
fn predict_fills_seven_and_presses_flush() {
    let mut h = opened();
    check(&mut h, "Use current offset and orientation");
    assert!(form(&h).predict);
    assert_eq!(form(&h).value, "", "Predict measures: the value box is cleared (offset None)");
    let (before, doc_before) = (transform(&h, "B"), h.app.session.doc.clone());
    picks(&mut h, A_TOP, B_TOP);
    assert!(std::sync::Arc::ptr_eq(&doc_before, &h.app.session.doc), "the preview leaves the document untouched");
    assert_eq!(value_box(&h, "Offset").as_deref(), Some("7.000 mm"));
    assert!(on_solution(&h, "Flush"), "the predicted solution is pressed");
    assert_eq!(form(&h).draft().offset, None, "still measuring");
    assert!(near(&transform(&h, "B"), &before, 0.0), "the preview moves nothing in the document");
    let (_, p) = form(&h).preview.clone().expect("a preview");
    assert_eq!(p.predicted.map(|x| x.solution), Some(S::Flush));
    let b = occ(&h, "B");
    assert!(p.world.get(&b).is_none_or(|m| near(m, &before, 1e-9)), "B stays where it is");
    assert_eq!(count_of("constraint_preview_moved"), Some(0.0));
}

/// The lowest toggle of `word` (the Solution row) is pressed.
fn on_solution(h: &Harness, word: &str) -> bool {
    let mut found: Vec<Node> = nodes(h, word).into_iter().filter(|n| n.toggled().is_some()).collect();
    found.sort_by(|a, b| a.bounds().unwrap().y0.total_cmp(&b.bounds().unwrap().y0));
    found.last().and_then(Node::toggled) == Some(Toggled::True)
}

#[test]
fn apply_keeps_the_type_and_numbers_the_next() {
    let mut h = opened();
    picks(&mut h, A_TOP, B_TOP);
    press(&mut h, "Apply");
    assert!(h.app.dialog.is_some(), "Apply keeps the panel open");
    let f = form(&h);
    assert_eq!(f.kind, K::Mate);
    assert_eq!(f.picks, [None, None, None], "the picks clear");
    assert_eq!(f.armed, 0, "pick 1 is armed again");
    assert!(on_solution(&h, "Mate"));
    assert!(h.app.session.doc.assembly.constraint("Mate:1").is_some());
    picks(&mut h, PLATE_TOP, PIN_TOP_FACE);
    assert!(form(&h).ready(), "{:?}", form(&h).picks);
    press(&mut h, "Apply");
    assert!(h.app.session.doc.assembly.constraint("Mate:2").is_some());
    assert_eq!(h.app.session.doc.assembly.constraints.len(), 2, "a second Apply makes one more, no duplicate");
}

#[test]
fn insert_highlights_only_circles() {
    let mut h = opened();
    press(&mut h, "Insert");
    assert_eq!(form(&h).kind, K::Insert);
    hover(&mut h, A_TOP, 3);
    assert_eq!(form(&h).hover, None, "a plane is no Insert pick");
    hover(&mut h, [200.0, 0.0, 10.0], 3);
    assert_eq!(form(&h).hover.as_ref().map(|x| x.1), Some(PrimKind::Circle), "the hole's top rim");
    hover(&mut h, [310.0, 0.0, 80.0], 3);
    assert_eq!(form(&h).hover, None, "the pin's wall is a cylinder");
}

#[test]
fn the_same_part_twice_is_refused_at_hover() {
    let mut h = opened();
    click_world(&mut h, A_TOP);
    crate::test_support::clicks_expire(&mut h);
    assert_eq!(form(&h).picks[0].as_ref().map(|g| g.occurrence), Some(occ(&h, "A")));
    assert_eq!(form(&h).armed, 1);
    hover(&mut h, [100.0, 25.0, 10.0], 3);
    assert_eq!(form(&h).hover, None, "A's side is on the part pick 1 is on");
    hover(&mut h, B_TOP, 3);
    assert_eq!(form(&h).hover.as_ref().map(|x| x.0.occurrence), Some(occ(&h, "B")));
}

#[test]
fn a_refused_draft_shows_why_and_moves_nothing() {
    let mut h = inventor(Assembly);
    constraint_blocks(&mut h);
    h.app
        .run(
            "constraint.create",
            json!({"type": "mate", "solution": "flush", "a": {"occurrence": "A:1", "face": A_TOP}, "b": {"occurrence": "B:1", "face": B_TOP}, "offset": 7}),
        )
        .unwrap();
    let pose = transform(&h, "B");
    alias(&mut h, "C");
    picks(&mut h, A_TOP, B_TOP);
    pick_solution(&mut h, "Flush");
    form_mut(&mut h).value = "2".into();
    h.frames(3);
    let why = "over-constrains B: Mate:2 off by 5.000 mm";
    assert_eq!(form(&h).preview.as_ref().and_then(|(_, p)| p.refused.clone()).as_deref(), Some(why));
    assert!(drawn(&h, why), "the reason is shown under Solution");
    assert!(!can(&h, PanelAction::Ok) && !can(&h, PanelAction::ApplyAndContinue));
    assert_eq!(count_of("constraint_preview_moved"), Some(0.0));
    assert!(near(&transform(&h, "B"), &pose, 0.0));
}

#[test]
fn editing_the_value_re_previews_once() {
    let mut h = opened();
    picks(&mut h, A_TOP, B_TOP);
    let start = count_of("constraint_previews").expect("the running count is published");
    form_mut(&mut h).value = "5".into();
    h.frames(1);
    assert_eq!(count_of("constraint_previews"), Some(start + 1.0), "one new preview for the new value");
    h.frames(1);
    assert_eq!(count_of("constraint_previews"), Some(start + 1.0), "an unchanged frame previews nothing");
}

#[test]
fn cancel_after_two_picks_runs_no_command() {
    let mut h = opened();
    let (undo, pose) = (h.app.session.undo.len(), transform(&h, "B"));
    picks(&mut h, A_TOP, B_TOP);
    assert!(form(&h).ready(), "{:?}", form(&h).picks);
    press(&mut h, "Cancel");
    assert!(h.app.dialog.is_none(), "Cancel closes the panel");
    assert!(h.app.session.doc.assembly.constraints.is_empty(), "no constraint was made");
    assert_eq!(h.app.session.undo.len(), undo, "no undo step");
    assert!(near(&transform(&h, "B"), &pose, 0.0));
    assert_eq!(count_of("constraint_preview_moved"), Some(0.0));
}

#[test]
fn esc_mid_preview_restores_and_adds_no_step() {
    let mut h = opened();
    let (undo, pose) = (h.app.session.undo.len(), transform(&h, "B"));
    picks(&mut h, A_TOP, B_TOP);
    assert_eq!(count_of("constraint_preview_moved"), Some(1.0), "B is drawn at its preview pose");
    assert!(h.app.preview.active);
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(3);
    assert!(h.app.dialog.is_none());
    assert_eq!(count_of("constraint_preview_moved"), Some(0.0));
    assert!(!h.app.preview.active, "no preview is left drawn: B is drawn at its document transform");
    assert!(near(&transform(&h, "B"), &pose, 0.0));
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn undo_after_two_applies_removes_one_each() {
    let mut h = opened();
    let undo = h.app.session.undo.len();
    let b_before = transform(&h, "B");
    picks(&mut h, A_TOP, B_TOP);
    press(&mut h, "Apply");
    let (pin_before, plate_before) = (transform(&h, "Pin"), transform(&h, "Plate"));
    picks(&mut h, PLATE_TOP, PIN_TOP_FACE);
    press(&mut h, "Apply");
    assert!(h.app.session.doc.assembly.constraint("Mate:2").is_some());
    // R167: the free Plate/Pin pair is its own group, anchored on the Plate (pick 1): the Pin moves.
    assert!(near(&transform(&h, "Plate"), &plate_before, 1e-9), "the plate (pick 1) holds");
    assert!(!near(&transform(&h, "Pin"), &pin_before, 1e-3), "the pin moved onto the plate");
    assert_eq!(h.app.session.undo.len(), undo + 2, "two Applies, two steps");
    h.step(&json!({"key": "Z", "cmd": true})).unwrap();
    h.frames(3);
    assert!(h.app.session.doc.assembly.constraint("Mate:2").is_none());
    assert!(h.app.session.doc.assembly.constraint("Mate:1").is_some());
    assert!(near(&transform(&h, "Pin"), &pin_before, 1e-9), "the pin is back");
    assert!(near(&transform(&h, "Plate"), &plate_before, 1e-9), "the plate is back");
    h.step(&json!({"key": "Z", "cmd": true})).unwrap();
    h.frames(3);
    assert!(h.app.session.doc.assembly.constraints.is_empty());
    assert!(near(&transform(&h, "B"), &b_before, 1e-9));
    let corner = solidcraft_engine::doc::apply_point(&transform(&h, "B"), solidcraft_engine::geom::Vec3::new(0.0, 70.0, 7.0));
    assert!(corner.dist(solidcraft_engine::geom::Vec3::new(0.0, 70.0, 7.0)) < 1e-9, "B back at its corner [0, 70, 7]");
}

#[test]
fn pick_part_first_limits_hovers() {
    let mut h = opened();
    check(&mut h, "Pick part first");
    assert!(form(&h).pick_part_first);
    click_world(&mut h, B_TOP);
    crate::test_support::clicks_expire(&mut h);
    assert_eq!(form(&h).part, Some(occ(&h, "B")));
    assert_eq!(form(&h).picks, [None, None, None], "the first click picks the part");
    hover(&mut h, A_TOP, 3);
    assert_eq!(form(&h).hover, None);
    hover(&mut h, B_TOP, 3);
    assert_eq!(form(&h).hover.as_ref().map(|x| x.0.occurrence), Some(occ(&h, "B")));
}

#[test]
fn origin_picks_from_the_browser() {
    let mut h = opened();
    let b = occ(&h, "B");
    let fold = format!("fold:occurrence_origin:{b}");
    assert!(crate::scenario::handle_at(&fold).is_some(), "B's Origin folder is listed");
    h.step(&json!({"click": {"handle": fold}})).unwrap();
    h.frames(3);
    // An occurrence's own rows never shadow the document Origin's label handles (the s37
    // regression: a "fold:Origin" click opened the last occurrence's folder instead).
    h.step(&json!({"click": {"handle": "fold:Origin"}})).unwrap();
    h.frames(3);
    let opened: Vec<String> = crate::scenario::handles("row:occurrence_origin:")
        .into_iter()
        .map(|x| x.0)
        .filter(|n| n.matches(':').count() == 3 && !n.starts_with(&format!("row:occurrence_origin:{b}:")))
        .collect();
    assert!(opened.is_empty(), "fold:Origin opens the document's Origin, not {opened:?}");
    assert!(crate::browser::is_open(&h.app, "origin", false), "the document's Origin opened");
    h.step(&json!({"click": {"handle": format!("row:occurrence_origin:{b}:XY")}})).unwrap();
    h.frames(3);
    assert_eq!(form(&h).picks[0], Some(GeoRef { occurrence: b, geo: Geo::Origin { item: OriginItem::Xy } }));
    form_mut(&mut h).armed = 0;
    h.frames(2);
    h.step(&json!({"click": {"handle": format!("row:occurrence_origin:{b}:O")}})).unwrap();
    h.frames(3);
    assert_eq!(form(&h).picks[0], Some(GeoRef { occurrence: b, geo: Geo::Origin { item: OriginItem::Center } }));
}

#[test]
fn vertex_edge_and_axis_picks() {
    // B's corner vertex (a point) on A's top: one freedom taken.
    let mut h = opened();
    picks(&mut h, [100.0, 120.0, 27.0], A_TOP);
    assert!(matches!(form(&h).picks[0].as_ref().map(|g| &g.geo), Some(Geo::Vertex { .. })), "{:?}", form(&h).picks);
    enter(&mut h);
    assert_eq!(report(&h.app.session).dof.get(&occ(&h, "B")), Some(&5));
    // B's top front edge (a line) on A's top: two taken.
    let mut h = opened();
    picks(&mut h, [50.0, 70.0, 27.0], A_TOP);
    assert!(matches!(form(&h).picks[0].as_ref().map(|g| &g.geo), Some(Geo::Edge { .. })), "{:?}", form(&h).picks);
    enter(&mut h);
    assert_eq!(report(&h.app.session).dof.get(&occ(&h, "B")), Some(&4));
    // The pin's wall (its axis) and B's top front edge (a line): two axes. The hole's wall
    // [190, -10, 5] is behind the plate's top from the home view, so a click there takes the top.
    let mut h = opened();
    picks(&mut h, [310.0, 0.0, 80.0], [50.0, 70.0, 27.0]);
    assert!(form(&h).ready(), "{:?}", form(&h).picks);
    for word in ["Opposed", "Aligned", "Undirected"] {
        assert!(drawn(&h, word), "two axes offer {word}");
    }
    assert!(on_solution(&h, "Opposed"));
}

#[test]
fn a_solution_the_picks_no_longer_offer_goes_back_to_the_default() {
    let mut h = opened();
    pick_solution(&mut h, "Flush");
    assert_eq!(form(&h).solution, Some(S::Flush));
    picks(&mut h, [310.0, 0.0, 80.0], [50.0, 70.0, 27.0]);
    assert_eq!(form(&h).solution, None, "Flush is not an axis solution");
    assert!(on_solution(&h, "Opposed"));
    assert_eq!(form(&h).draft().params().get("solution"), None);
}

#[test]
fn suppressed_lock_and_limits_reach_the_command() {
    // Suppressed: listed suppressed, nothing moves.
    let mut h = opened();
    let pose = transform(&h, "B");
    check(&mut h, "Suppressed");
    picks(&mut h, A_TOP, B_TOP);
    enter(&mut h);
    let rows = list(&mut h);
    assert_eq!(rows[0]["suppressed"], json!(true));
    assert!(near(&transform(&h, "B"), &pose, 0.0));
    // Insert with Lock turn: the pin is held all round.
    let mut h = opened();
    press(&mut h, "Insert");
    check(&mut h, "Lock turn");
    picks(&mut h, [200.0, 0.0, 10.0], [310.0, 0.0, 110.0]);
    assert!(form(&h).ready(), "{:?}", form(&h).picks);
    enter(&mut h);
    let id = constraint_id(&h, "Insert:1");
    assert!(h.app.session.doc.assembly.constraint(&id.to_string()).unwrap().lock);
    assert_eq!(report(&h.app.session).dof.get(&occ(&h, "Pin")), Some(&0));
    // A limited Mate.
    let mut h = opened();
    {
        let f = form_mut(&mut h);
        f.expanded = true;
        f.max = (true, "10".into());
        f.min = (true, "0".into());
    }
    picks(&mut h, A_TOP, B_TOP);
    enter(&mut h);
    assert_eq!(list(&mut h)[0]["limits"], json!({"min": 0.0, "max": 10.0}));
    // A Directed Angle takes no limits (R152).
    let mut h = opened();
    press(&mut h, "Angle");
    pick_solution(&mut h, "Directed");
    form_mut(&mut h).expanded = true;
    h.frames(2);
    let max = nodes(&h, "Max").into_iter().find(|n| n.role() == Role::CheckBox).unwrap();
    assert!(max.is_disabled());
    picks(&mut h, [50.0, 0.0, 10.0], [50.0, 70.0, 17.0]);
    enter(&mut h);
    let a = h.app.session.doc.assembly.constraint("Angle:1").expect("the angle is made");
    assert!(a.limits.is_empty());
}

#[test]
fn an_expression_offset_is_kept() {
    let mut h = opened();
    h.app.run("parameters.add", json!({"name": "t", "expression": "3 mm"})).unwrap();
    form_mut(&mut h).value = "t * 2".into();
    picks(&mut h, A_TOP, B_TOP);
    enter(&mut h);
    let rows = list(&mut h);
    assert!((rows[0]["offset"].as_f64().unwrap() - 6.0).abs() < 1e-9, "{}", rows[0]);
    assert_eq!(rows[0]["expressions"]["offset"], json!("t * 2"));
}

#[test]
fn hover_resolves_once_per_entity() {
    let mut h = opened();
    h.frames(2);
    let start = count_of("constraint_hover_resolves").expect("the count is published");
    hover(&mut h, A_TOP, 10);
    assert_eq!(count_of("constraint_hover_resolves"), Some(start + 1.0), "10 frames on one face: one lookup");
    hover(&mut h, B_TOP, 3);
    assert_eq!(count_of("constraint_hover_resolves"), Some(start + 2.0));
}

#[test]
fn a_limited_angle_previews_where_it_commits() {
    let set = |h: &mut Harness, value: &str| {
        press(h, "Angle");
        pick_solution(h, "Undirected");
        let f = form_mut(h);
        f.expanded = true;
        f.min = (true, "10".into());
        f.max = (true, "40".into());
        f.value = value.into();
        picks(h, [50.0, 0.0, 10.0], [50.0, 70.0, 17.0]);
    };
    let mut h = opened();
    set(&mut h, "30");
    let b = occ(&h, "B");
    let (_, p) = form(&h).preview.clone().expect("a preview");
    assert_eq!(p.refused, None);
    let shown = *p.world.get(&b).expect("B is placed by the preview");
    enter(&mut h);
    assert!(near(&transform(&h, "B"), &shown, 1e-9), "the preview pose is the committed pose (R160)");

    let mut h = opened();
    set(&mut h, "50");
    let warning = form(&h).preview.as_ref().and_then(|(_, p)| p.warning.clone()).expect("the clamp warning");
    assert!(drawn(&h, &warning), "shown under Value");
    enter(&mut h);
    let status = h.app.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
    assert!(status.contains("starts at 40"), "{status}");
}

#[test]
fn the_prompt_names_the_armed_pick() {
    let mut h = opened();
    assert_eq!(crate::status_bar::texts(&h.app).left, "Pick the first face, edge, axis or point");
    click_world(&mut h, A_TOP);
    crate::test_support::clicks_expire(&mut h);
    assert_eq!(crate::status_bar::texts(&h.app).left, "Pick the second face, edge, axis or point");
}

#[test]
fn edit_mode_type_change_takes_a_fitting_solution() {
    let mut h = inventor(Assembly);
    constraint_blocks(&mut h);
    h.app
        .run(
            "constraint.create",
            json!({"type": "mate", "solution": "flush", "a": {"occurrence": "A:1", "face": A_TOP}, "b": {"occurrence": "B:1", "face": B_TOP}, "offset": 7}),
        )
        .unwrap();
    let id = constraint_id(&h, "Mate:1");
    let mut f = ConstraintForm::edit(&h.app, id).unwrap();
    f.set_kind(K::Tangent);
    assert_eq!(f.solution, Some(K::Tangent.default_solution(false)), "an edit never keeps a solution that does not fit");
}

/// SolidCraft takes the same picks (spec §6.9).
#[test]
fn solidcraft_picks_and_commits_the_same() {
    let mut h = Harness::new();
    constraint_blocks(&mut h);
    h.app.start("constraint.create");
    h.frames(3);
    picks(&mut h, A_TOP, B_TOP);
    assert!(form(&h).ready(), "{:?}", form(&h).picks);
    enter(&mut h);
    assert!(h.app.session.doc.assembly.constraint("Mate:1").is_some());
}

/// R175: OK and Apply that make a constraint each ask for the pop once, on both profiles; none
/// with the preference off, none for a refused draft, none for an edit (no sound device needed:
/// the requests are counted).
#[test]
fn a_made_constraint_pops_once() {
    use crate::audio::requests;
    for inventor_profile in [true, false] {
        let mut h = if inventor_profile { opened() } else { Harness::new() };
        if !inventor_profile {
            constraint_blocks(&mut h);
            h.app.start("constraint.create");
            h.frames(3);
        }
        let before = requests();
        picks(&mut h, A_TOP, B_TOP);
        enter(&mut h);
        assert_eq!(requests(), before + 1, "OK made Mate:1: one pop");
        h.app.start("constraint.create");
        h.frames(3);
        picks(&mut h, PLATE_TOP, PIN_TOP_FACE);
        press(&mut h, "Apply");
        assert_eq!(h.app.session.doc.assembly.constraints.len(), 2);
        assert_eq!(requests(), before + 2, "Apply made Mate:2: one pop");
        press(&mut h, "Cancel");
        // An edit is not a new constraint.
        let mate = constraint_id(&h, "Mate:1");
        crate::dialogs_constraint::open_edit(&mut h.app, mate);
        h.frames(3);
        form_mut(&mut h).value = "3".into();
        form_mut(&mut h).touched.insert("offset");
        h.frames(3);
        enter(&mut h);
        assert_eq!(h.app.session.doc.assembly.constraint("Mate:1").map(|c| c.offset), Some(3.0), "the edit ran");
        assert_eq!(requests(), before + 2, "no pop for an edit");
        // The preference off: silent.
        h.app.preferences.constraint_audio = false;
        h.app.start("constraint.create");
        h.frames(3);
        picks(&mut h, A_TOP, PLATE_TOP);
        enter(&mut h);
        assert_eq!(h.app.session.doc.assembly.constraints.len(), 3);
        assert_eq!(requests(), before + 2, "no pop with the preference off");
    }
}

#[test]
fn a_refused_create_does_not_pop() {
    let mut h = inventor(Assembly);
    constraint_blocks(&mut h);
    h.app
        .run(
            "constraint.create",
            json!({"type": "mate", "solution": "flush", "a": {"occurrence": "A:1", "face": A_TOP}, "b": {"occurrence": "B:1", "face": B_TOP}, "offset": 7}),
        )
        .unwrap();
    let before = crate::audio::requests();
    alias(&mut h, "C");
    picks(&mut h, A_TOP, B_TOP);
    pick_solution(&mut h, "Flush");
    form_mut(&mut h).value = "2".into();
    h.frames(3);
    enter(&mut h);
    assert_eq!(h.app.session.doc.assembly.constraints.len(), 1, "refused");
    assert_eq!(crate::audio::requests(), before, "no pop for a refusal");
}
