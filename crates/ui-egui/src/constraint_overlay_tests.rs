#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Plan E-2 Task 7: on-model constraint glyphs, the picked rows' highlight marks, and Show /
//! Show Problems / Hide All (spec §6.8, §6.4; R135, R162: a Lost constraint draws no glyph, by
//! status). Inventor tests run on `inventor(Assembly)` with E-1's blocks; SolidCraft
//! (`Harness::new()`) gets the same glyphs through its JOINTS panel.

use serde_json::{Value, json};
use solidcraft_engine::doc::DocumentKind::Assembly;
use solidcraft_engine::doc::constraints::ConstraintStatus;
use solidcraft_engine::geom::Vec3;

use super::{drawn_glyphs, glyphs, highlights, hit};
use crate::scenario::Harness;
use crate::test_support::{click, click_world, clicks_expire, constraint_blocks, constraint_id, inventor};

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
    let w = solidcraft_engine::doc::apply_point(&m, Vec3::new(p[0], p[1], p[2]));
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
fn mate_ab(h: &Harness, x: f64, y: f64) -> Value {
    json!({"type": "mate", "a": face(h, "A", [x, y, 20.0]), "b": face(h, "B", [x, y + 70.0, 7.0])})
}

/// The plate's hole rim into the pin's bottom rim.
fn insert_plate_pin() -> Value {
    json!({"type": "insert", "a": {"occurrence": "Plate", "edge": [200.0, 0.0, 10.0]}, "b": {"occurrence": "Pin", "edge": [310.0, 0.0, 50.0]}})
}

fn row(h: &Harness, name: &str) -> solidcraft_engine::constraints::Row {
    h.app.session.constraint_report.rows.iter().find(|r| r.name == name).cloned().unwrap_or_else(|| panic!("no row {name}"))
}

/// The midpoint of a constraint's two anchors.
fn midpoint(h: &Harness, name: &str) -> Vec3 {
    let [a, b] = row(h, name).anchors.unwrap_or_else(|| panic!("{name} has no anchors"));
    a.lerp(b, 0.5)
}

fn shown(h: &Harness) -> Vec<u64> {
    h.app.session.shown_constraints.iter().copied().collect()
}

fn proj(h: &Harness) -> crate::viewport::Proj {
    crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap())
}

/// Test 1 (§8.3 7, R135, R162): Show Problems shows the sick Mate's glyph in the error colour and
/// lists the Lost Insert without a glyph; a Lost Mate whose picks still resolve draws none either;
/// Hide All clears; none is an undo step.
#[test]
fn show_problems_draws_sick_glyphs_and_lists_lost_ones() {
    let mut h = blocks();
    let t = crate::theme::Tokens::get();
    run(&mut h, "constraint.create", insert_plate_pin());
    let p = mate_ab(&h, 50.0, 25.0);
    run(&mut h, "constraint.create", p);
    let p = mate_ab(&h, 30.0, 15.0);
    run(&mut h, "constraint.create", p);
    run(&mut h, "parameters.add", json!({"name": "t", "expression": "0 mm"}));
    run(&mut h, "constraint.edit", json!({"constraint": "Mate:1", "offset": "t"}));
    run(&mut h, "parameters.change", json!({"name": "t", "expression": "2 mm"}));
    run(&mut h, "timeline.edit", json!({"feature": "Plate", "set": {"length": "25 mm"}}));
    h.frames(2);
    assert!(matches!(row(&h, "Mate:2").status, ConstraintStatus::Sick { .. }), "{:?}", row(&h, "Mate:2").status);
    assert!(matches!(row(&h, "Insert:1").status, ConstraintStatus::Lost { .. }), "{:?}", row(&h, "Insert:1").status);
    let (mate2, insert) = (constraint_id(&h, "Mate:2"), constraint_id(&h, "Insert:1"));
    let steps = h.app.session.undo.len();
    assert!(glyphs(&h.app).is_empty(), "nothing is shown yet");

    click(&mut h, "button:constraint.show_sick");
    h.frames(2);
    let mut want = vec![mate2, insert];
    want.sort_unstable();
    assert_eq!(shown(&h), want, "Show Problems shows the sick and the lost ones");
    let g = glyphs(&h.app);
    assert_eq!(g.len(), 1, "no glyph for the Lost Insert: {g:?}");
    let (id, m, sick) = g[0];
    assert_eq!((id, sick), (mate2, true));
    assert!(m.dist(midpoint(&h, "Mate:2")) <= 1e-9, "{m:?}");
    // Drawn once, with a leader to each anchor, in the error colour.
    let drawn = drawn_glyphs();
    assert_eq!(drawn.len(), 1, "{drawn:?}");
    assert_eq!((drawn[0].id, drawn[0].leaders, drawn[0].colour), (mate2, 2, t.error));
    assert_eq!(h.app.session.undo.len(), steps, "Show Problems is no undo step");

    click(&mut h, "button:constraint.hide_all");
    h.frames(2);
    assert!(shown(&h).is_empty());
    assert!(glyphs(&h.app).is_empty());
    assert!(drawn_glyphs().is_empty());
    assert_eq!(h.app.session.undo.len(), steps, "Hide All is no undo step");

    // A Lost Mate whose picks still resolve (its offset no longer works out): no glyph, by status.
    let mut h = blocks();
    run(&mut h, "parameters.add", json!({"name": "u", "expression": "0 mm"}));
    let mut p = mate_ab(&h, 50.0, 25.0);
    p["offset"] = json!("u");
    run(&mut h, "constraint.create", p);
    run(&mut h, "parameters.change", json!({"name": "u", "expression": "2e9 mm"}));
    h.frames(2);
    let lost = row(&h, "Mate:1");
    assert!(matches!(lost.status, ConstraintStatus::Lost { .. }), "{:?}", lost.status);
    assert!(lost.anchors.is_some(), "its picks still resolve");
    click(&mut h, "button:constraint.show_sick");
    h.frames(2);
    assert_eq!(shown(&h), [lost.id]);
    assert!(glyphs(&h.app).is_empty(), "a Lost constraint draws no glyph (R162)");
    assert!(drawn_glyphs().is_empty());
}

/// Test 2: Show adds the selected components' constraints; with nothing selected it prompts and
/// the next component click shows that component's.
#[test]
fn show_adds_the_selected_components() {
    let mut h = blocks();
    let t = crate::theme::Tokens::get();
    let p = mate_ab(&h, 50.0, 25.0);
    run(&mut h, "constraint.create", p);
    run(&mut h, "constraint.create", insert_plate_pin());
    let (mate, insert, b) = (constraint_id(&h, "Mate:1"), constraint_id(&h, "Insert:1"), occ(&h, "B"));
    let steps = h.app.session.undo.len();
    run(&mut h, "select.set", json!({"items": [{"type": "occurrence", "path": [b]}]}));
    click(&mut h, "button:constraint.show");
    h.frames(2);
    assert_eq!(shown(&h), [mate]);
    let g = glyphs(&h.app);
    assert_eq!(g.len(), 1, "{g:?}");
    let (id, m, sick) = g[0];
    assert_eq!((id, sick), (mate, false));
    assert!(m.dist(midpoint(&h, "Mate:1")) <= 1e-9, "{m:?} vs {:?}", midpoint(&h, "Mate:1"));
    let drawn = drawn_glyphs();
    assert_eq!(drawn.len(), 1);
    assert_eq!((drawn[0].leaders, drawn[0].colour), (2, t.accent), "an Ok glyph is in the relationship colour");

    // Nothing selected: the prompt, then a click on the pin adds the Insert.
    run(&mut h, "select.clear", json!({}));
    clicks_expire(&mut h);
    click(&mut h, "button:constraint.show");
    h.frames(2);
    assert_eq!(shown(&h), [mate], "the prompt sends nothing yet");
    assert_eq!(crate::status_bar::texts(&h.app).left, "Pick components to show their constraints");
    let pin_top = at(&h, "Pin", [300.0, 5.0, 110.0]);
    click_world(&mut h, pin_top);
    h.frames(2);
    let mut want = vec![mate, insert];
    want.sort_unstable();
    assert_eq!(shown(&h), want, "the pin's constraint is added");
    assert_eq!(glyphs(&h.app).len(), 2);
    assert_ne!(crate::status_bar::texts(&h.app).left, "Pick components to show their constraints", "the prompt ends");
    assert_eq!(h.app.session.undo.len(), steps, "Show is no undo step");
}

/// Test 3: the pointer on a glyph lights its browser rows; a click on it picks its rows.
#[test]
fn clicking_a_glyph_selects_its_row() {
    let mut h = blocks();
    let p = mate_ab(&h, 50.0, 25.0);
    run(&mut h, "constraint.create", p);
    let mate = constraint_id(&h, "Mate:1");
    run(&mut h, "constraint.show", json!({"occurrences": ["B"]}));
    let (_, m, _) = glyphs(&h.app)[0];
    let proj = proj(&h);
    let s = proj.to_screen(m).unwrap();
    assert_eq!(hit(&h.app, &proj, s), Some(mate));
    assert_eq!(hit(&h.app, &proj, s + egui::vec2(40.0, 40.0)), None, "away from the glyph");
    // Hover: the glyph and its three rows light up.
    h.step(&json!({"move": [s.x, s.y]})).unwrap();
    h.frames(3);
    let rows = crate::browser::drawn_constraints();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r.hot), "{rows:?}");
    assert!(drawn_glyphs()[0].hot);
    assert!((drawn_glyphs()[0].at - s).length() <= 0.5, "drawn where it is hit");
    assert!(crate::scenario::handle_at(&format!("glyph:constraint:{mate}")).is_some(), "published for automation");
    h.step(&json!({"move": [s.x + 60.0, s.y + 60.0]})).unwrap();
    h.frames(3);
    assert!(crate::browser::drawn_constraints().iter().all(|r| !r.hot));
    // Click: its rows are picked.
    h.step(&json!({"click": [s.x, s.y]})).unwrap();
    h.frames(2);
    assert_eq!(h.app.tree.picked_constraints, [mate]);
    assert!(crate::browser::drawn_constraints().iter().all(|r| r.selected));
}

/// Test 4 (R142): a picked row marks both of its picks on the model; a picked Lost row marks
/// none.
#[test]
fn a_picked_row_highlights_both_picks() {
    let mut h = blocks();
    let p = mate_ab(&h, 50.0, 25.0);
    run(&mut h, "constraint.create", p);
    let (mate, b) = (constraint_id(&h, "Mate:1"), occ(&h, "B"));
    assert!(highlights(&h.app).is_empty());
    h.step(&json!({"click": {"handle": format!("row:constraint:{mate}:{b}")}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.tree.picked_constraints, [mate]);
    let [pa, pb] = row(&h, "Mate:1").anchors.unwrap();
    assert_eq!(highlights(&h.app), vec![pa, pb]);
    assert!(glyphs(&h.app).is_empty(), "a highlight is not a shown glyph");

    // Lost: picked, but nothing to mark.
    let mut h = blocks();
    run(&mut h, "constraint.create", insert_plate_pin());
    run(&mut h, "timeline.edit", json!({"feature": "Plate", "set": {"length": "25 mm"}}));
    h.frames(2);
    let (insert, plate) = (constraint_id(&h, "Insert:1"), occ(&h, "Plate"));
    h.step(&json!({"click": {"handle": format!("row:constraint:{insert}:{plate}")}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.tree.picked_constraints, [insert]);
    assert_eq!(highlights(&h.app), Vec::<Vec3>::new());
}

/// Both profiles: SolidCraft's JOINTS panel shows the same glyphs, and Show Problems colours them
/// the same.
#[test]
fn solidcraft_draws_the_same_glyphs() {
    let mut h = blocks_on(Harness::new());
    let t = crate::theme::Tokens::get();
    let p = mate_ab(&h, 50.0, 25.0);
    run(&mut h, "constraint.create", p);
    let p = mate_ab(&h, 30.0, 15.0);
    run(&mut h, "constraint.create", p);
    run(&mut h, "parameters.add", json!({"name": "t", "expression": "0 mm"}));
    run(&mut h, "constraint.edit", json!({"constraint": "Mate:1", "offset": "t"}));
    run(&mut h, "parameters.change", json!({"name": "t", "expression": "2 mm"}));
    let (mate1, mate2, b) = (constraint_id(&h, "Mate:1"), constraint_id(&h, "Mate:2"), occ(&h, "B"));
    run(&mut h, "select.set", json!({"items": [{"type": "occurrence", "path": [b]}]}));
    h.app.start("constraint.show");
    h.frames(2);
    let g: Vec<(u64, bool)> = glyphs(&h.app).into_iter().map(|(id, _, sick)| (id, sick)).collect();
    assert_eq!(g, [(mate1, false), (mate2, true)]);
    let colours: Vec<egui::Color32> = drawn_glyphs().iter().map(|d| d.colour).collect();
    assert_eq!(colours, [t.accent, t.error]);
    h.app.start("constraint.hide_all");
    h.frames(2);
    assert!(glyphs(&h.app).is_empty() && drawn_glyphs().is_empty());
    h.app.start("constraint.show_sick");
    h.frames(2);
    assert_eq!(glyphs(&h.app).into_iter().map(|g| g.0).collect::<Vec<_>>(), [mate2]);
}

/// Unsolved is a problem colour too (spec §6.8, R161); Ok, Redundant and Suppressed are not.
#[test]
fn unsolved_and_sick_take_the_error_colour() {
    use super::is_problem;
    assert!(is_problem(&ConstraintStatus::Unsolved));
    assert!(is_problem(&ConstraintStatus::Sick { gap_mm: 1.0, gap_deg: 0.0, with: vec![] }));
    assert!(!is_problem(&ConstraintStatus::Ok));
    assert!(!is_problem(&ConstraintStatus::Redundant));
    assert!(!is_problem(&ConstraintStatus::Suppressed));
}
