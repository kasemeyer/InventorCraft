#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Plan E-2 Task 2: the Constrain panel as captured (spec §6.2). Inventor tests run on
//! `inventor(Assembly)` with E-1's blocks; SolidCraft gets the same panel (`Harness::new()`).

use egui::accesskit::{Node, Role, Toggled};
use serde_json::json;
use solidcraft_engine::constraints::Draft;
use solidcraft_engine::doc::DocumentKind::Assembly;
use solidcraft_engine::doc::constraints::{ANGLE_DEFAULT, ConstraintKind as K, ConstraintSolution as S, GeoRef, PickWhat, geo_at};
use solidcraft_engine::geom::Vec3;

use super::ConstraintForm;
use crate::dialogs::{Dialog, Kind, PanelAction, can_act};
use crate::dialogs_assembly::Asm;
use crate::i18n::tr;
use crate::scenario::Harness;
use crate::test_support::{alias, constraint_blocks, inventor};

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

/// The accessible nodes drawn inside the panel labelled `label`.
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

fn of_role(h: &Harness, label: &str, role: Role) -> Option<Node> {
    nodes(h, label).into_iter().find(|n| n.role() == role)
}

/// Pressed (a toggle or a checkbox) — the first node so labelled that carries a state.
fn on(h: &Harness, label: &str) -> bool {
    nodes(h, label).iter().find_map(|n| n.toggled()).unwrap_or_else(|| panic!("no `{label}` toggle in the panel")) == Toggled::True
}

fn drawn(h: &Harness, label: &str) -> bool {
    !nodes(h, label).is_empty()
}

fn disabled(h: &Harness, label: &str) -> bool {
    let n = nodes(h, label);
    assert!(!n.is_empty(), "no `{label}` in the panel");
    n.iter().all(Node::is_disabled)
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

fn occurrence(h: &Harness, name: &str) -> u64 {
    h.app.session.doc.occurrences.iter().find(|o| o.name == format!("{name}:1")).map(|o| o.id).unwrap_or_else(|| panic!("no occurrence {name}"))
}

/// A face pick on `name` at world point `p` (E-1's `geo_at`).
fn face(h: &Harness, name: &str, p: [f64; 3]) -> GeoRef {
    let s = &h.app.session;
    geo_at(&s.doc, &s.model.state(), occurrence(h, name), PickWhat::Face, Vec3::new(p[0], p[1], p[2])).unwrap()
}

fn can(h: &Harness, action: PanelAction) -> bool {
    can_act(&h.app, h.app.dialog.as_ref().unwrap(), action)
}

/// Click the panel's button labelled `label`; `nth` counts such buttons from the top (the type
/// Mate is above the solution Mate).
fn press(h: &mut Harness, label: &str, nth: usize) {
    let mut buttons: Vec<Node> = nodes(h, label).into_iter().filter(|n| n.role() == Role::Button).collect();
    buttons.sort_by(|a, b| a.bounds().unwrap().y0.total_cmp(&b.bounds().unwrap().y0));
    let b = buttons.get(nth).and_then(Node::bounds).unwrap_or_else(|| panic!("no button `{label}` #{nth} in the panel"));
    h.step(&json!({"click": [(b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0]})).unwrap();
    h.frames(3);
}

/// A type button (the top row of its word).
fn pick_type(h: &mut Harness, word: &str) {
    press(h, word, 0);
}

/// A solution button: the lowest button of its word.
fn pick_solution(h: &mut Harness, word: &str) {
    let n = nodes(h, word).into_iter().filter(|n| n.role() == Role::Button).count();
    press(h, word, n.saturating_sub(1));
}

#[test]
fn the_panel_opens_with_the_captured_defaults() {
    let mut h = opened();
    let f = form(&h);
    assert_eq!(f.kind, K::Mate);
    assert_eq!(f.solution, None, "the type's default");
    assert_eq!(f.armed, 0);
    assert!(!f.pick_part_first && !f.predict && !f.suppressed && !f.lock && !f.expanded);
    assert!(f.show_preview);
    assert_eq!(f.value, "0 mm");
    assert_eq!(heading(&h), "Constrain");
    // The Type and Solution rows are icon toggles (R166), their words the names and tooltips.
    crate::icon_svg::audit::start();
    h.frames(2);
    let drawn_icons = crate::icon_svg::audit::take();
    crate::icon_svg::audit::stop();
    for id in
        ["constraint_mate", "constraint_angle", "constraint_tangent", "constraint_insert", "constraint_symmetry", "solution_mate", "solution_flush"]
    {
        let r = drawn_icons.iter().find(|r| r.name == id).unwrap_or_else(|| panic!("{id} is not drawn"));
        assert!(!r.placeholder, "{id} has its own master");
    }
    // Type Mate and solution Mate are both pressed.
    let mates: Vec<bool> = nodes(&h, "Mate").iter().filter_map(|n| n.toggled()).map(|t| t == Toggled::True).collect();
    assert_eq!(mates, [true, true], "type Mate and solution Mate");
    assert!(!on(&h, "Flush"));
    assert!(on(&h, "1") && !on(&h, "2"));
    assert!(!drawn(&h, "3"), "Mate takes two picks");
    assert_eq!(of_role(&h, "Offset", Role::TextInput).and_then(|n| n.value().map(str::to_string)).as_deref(), Some("0 mm"));
    assert!(!on(&h, "Use current offset and orientation"));
    assert!(on(&h, "Preview"));
    assert!(!on(&h, "Suppressed"));
    assert!(!on(&h, "Pick part first"));
    assert!(!drawn(&h, "Lock turn"), "Lock turn is Insert's");
    assert!(!can(&h, PanelAction::Ok) && !can(&h, PanelAction::ApplyAndContinue));
    assert!(can(&h, PanelAction::Cancel));
    assert!(drawn(&h, "Apply") && drawn(&h, "OK") && drawn(&h, "Cancel"));
    assert_eq!(crate::scenario::count_of("dialog:ok:enabled"), Some(0.0));
    assert!(on(&h, "Assembly") && !disabled(&h, "Assembly"));
    for tab in ["Motion", "Transition", "Sets"] {
        assert!(disabled(&h, tab), "{tab} is not in this version");
    }
    assert!(drawn(&h, "More") && !drawn(&h, "Name"));
    // Enter with nothing picked does nothing; Esc cancels without a step.
    let undo = h.app.session.undo.len();
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(2);
    assert!(h.app.dialog.is_some(), "Enter before the picks keeps the panel");
    assert_eq!(h.app.session.undo.len(), undo);
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert!(h.app.dialog.is_none(), "Esc cancels");
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(h.app.session.doc.assembly.constraints.is_empty());
}

#[test]
fn each_type_shows_its_picks_value_and_solutions() {
    let mut h = opened();
    pick_type(&mut h, "Angle");
    assert_eq!(form(&h).kind, K::Angle);
    assert_eq!(of_role(&h, "Angle", Role::TextInput).and_then(|n| n.value().map(str::to_string)).as_deref(), Some("0.00 deg"));
    for word in ["Directed", "Undirected", "Reference axis"] {
        assert!(drawn(&h, word), "Angle offers {word}");
    }
    assert_eq!(ANGLE_DEFAULT, S::ExplicitVector);
    assert!(on(&h, "Reference axis") && !on(&h, "Directed") && !on(&h, "Undirected"));
    assert!(drawn(&h, "3"), "a reference axis takes a third pick");
    assert!(drawn(&h, "Use current offset and orientation"));
    pick_solution(&mut h, "Directed");
    assert_eq!(form(&h).solution, Some(S::Directed));
    assert!(!drawn(&h, "3"), "pick 3 only with Reference axis");

    pick_type(&mut h, "Tangent");
    assert!(drawn(&h, "Inside") && on(&h, "Outside") && !on(&h, "Inside"));
    assert!(!drawn(&h, "Use current offset and orientation"), "Tangent has no Predict");
    assert_eq!(form(&h).value, "0 mm");

    pick_type(&mut h, "Insert");
    crate::icon_svg::audit::start();
    h.frames(2);
    let insert_icons: Vec<String> = crate::icon_svg::audit::take().into_iter().filter(|r| !r.placeholder).map(|r| r.name).collect();
    crate::icon_svg::audit::stop();
    assert!(insert_icons.iter().any(|n| n == "solution_opposed") && insert_icons.iter().any(|n| n == "solution_aligned"), "{insert_icons:?}");
    assert!(on(&h, "Opposed") && drawn(&h, "Aligned") && !on(&h, "Aligned"));
    assert!(drawn(&h, "Lock turn") && !on(&h, "Lock turn"));
    assert!(!drawn(&h, "Use current offset and orientation"));

    assert!(disabled(&h, "Symmetry"), "Symmetry comes in a later version");
    assert_eq!(super::TYPES, [K::Mate, K::Angle, K::Tangent, K::Insert, K::Symmetry]);
}

#[test]
fn changing_the_type_clears_the_picks() {
    let mut h = opened();
    let top = face(&h, "A", [50.0, 25.0, 20.0]);
    {
        let f = form_mut(&mut h);
        f.picks[0] = Some(top);
        f.armed = 1;
    }
    h.frames(2);
    pick_type(&mut h, "Insert");
    let f = form(&h);
    assert_eq!(f.kind, K::Insert);
    assert_eq!(f.picks, [None, None, None]);
    assert_eq!(f.armed, 0);
    assert_eq!(f.solution, None);
}

#[test]
fn more_shows_name_and_limits() {
    let mut h = opened();
    let at = h.widget("More").expect("the More button");
    h.step(&json!({"click": at})).unwrap();
    h.frames(3);
    assert!(form(&h).expanded && drawn(&h, "Less"));
    let name = of_role(&h, "Name", Role::TextInput).expect("the Name box");
    assert_eq!(name.value(), Some(""));
    assert_eq!(super::name_hint(&h.app, form(&h)), "Mate:1");
    assert!(disabled(&h, "Rest at offset"));
    for limit in ["Max", "Min"] {
        assert_eq!(of_role(&h, limit, Role::CheckBox).unwrap().toggled(), Some(Toggled::False));
        assert!(of_role(&h, limit, Role::TextInput).unwrap().is_disabled(), "{limit}'s box waits for its checkbox");
    }
    let check = |h: &Harness| {
        let b = of_role(h, "Max", Role::CheckBox).unwrap().bounds().unwrap();
        json!({"click": [(b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0]})
    };
    let c = check(&h);
    h.step(&c).unwrap();
    h.frames(2);
    assert!(form(&h).max.0);
    assert!(!of_role(&h, "Max", Role::TextInput).unwrap().is_disabled(), "checking Max enables its box");
    form_mut(&mut h).max.1 = "5".into();
    h.frames(2);
    let c = check(&h);
    h.step(&c).unwrap();
    h.frames(2);
    assert_eq!(form(&h).max, (false, "5".to_string()), "unchecking keeps the value");
    assert!(of_role(&h, "Max", Role::TextInput).unwrap().is_disabled());
}

#[test]
fn switching_tabs_closes_the_panel_without_a_step() {
    let mut h = opened();
    let (a, b) = (face(&h, "A", [50.0, 25.0, 20.0]), face(&h, "B", [50.0, 95.0, 27.0]));
    {
        let f = form_mut(&mut h);
        f.picks[0] = Some(a);
        f.picks[1] = Some(b);
    }
    h.frames(2);
    let undo = h.app.session.undo.len();
    crate::documents::new_tab(&mut h.app);
    h.frames(3);
    assert!(h.app.dialog.is_none(), "the panel closes with its document");
    let other = h.app.session.undo.len();
    crate::documents::switch(&mut h.app, 0);
    h.frames(3);
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.session.undo.len(), undo, "nothing was committed");
    assert!(h.app.session.doc.assembly.constraints.is_empty());
    crate::documents::switch(&mut h.app, 1);
    h.frames(2);
    assert_eq!(h.app.session.undo.len(), other);
}

/// Mate:1 between A's top and B's top, made by command.
fn mate_by_command(h: &mut Harness) -> u64 {
    h.app
        .run(
            "constraint.create",
            json!({"type": "mate", "a": {"occurrence": "A:1", "face": [50, 25, 20]}, "b": {"occurrence": "B:1", "face": [50, 95, 27]}}),
        )
        .unwrap();
    crate::test_support::constraint_id(h, "Mate:1")
}

fn open_edit(h: &mut Harness, id: u64) {
    let f = ConstraintForm::edit(&h.app, id).expect("an edit form");
    h.app.dialog = Some(Dialog::new(Kind::Assembly(Asm::Constraint(Box::new(f))), vec![]));
    h.frames(3);
}

#[test]
fn edit_mode_heading_uses_the_edit_prefix() {
    let mut h = inventor(Assembly);
    constraint_blocks(&mut h);
    let id = mate_by_command(&mut h);
    let record = h.app.session.doc.assembly.constraint("Mate:1").cloned().unwrap();
    open_edit(&mut h, id);
    assert_eq!(heading(&h), crate::i18n::format("dialog.edit_title", &[("title", tr("Constrain"))]));
    assert_eq!(heading(&h), "EDIT Constrain");
    let f = form(&h);
    assert_eq!(f.editing, Some(id));
    assert_eq!(f.picks[0].as_ref(), record.picks.first());
    assert_eq!(f.picks[1].as_ref(), record.picks.get(1));
    assert_eq!(f.name, "Mate:1");
    assert_eq!(f.draft(), Draft::edit(&record));
    assert_eq!(f.draft().params(), json!({"constraint": id}));
    // Edit has no Apply; OK is live at once (the picks are the record's).
    assert!(!drawn(&h, "Apply"));
    assert!(can(&h, PanelAction::Ok));
    // The value box shows the record's own text; typing the same number changes nothing.
    let shown = form(&h).value.clone();
    assert_eq!(Some(&shown), Draft::edit(&record).offset.as_ref());
    {
        let f = form_mut(&mut h);
        f.touched.insert("offset");
        f.value = shown.clone();
    }
    assert_eq!(form(&h).draft().offset, Draft::edit(&record).offset);
    assert_eq!(form(&h).draft().params(), json!({"constraint": id}));
    let at = h.widget("More").unwrap();
    h.step(&json!({"click": at})).unwrap();
    h.frames(3);
    assert_eq!(of_role(&h, "Name", Role::TextInput).and_then(|n| n.value().map(str::to_string)).as_deref(), Some("Mate:1"));
    // OK with nothing changed takes no undo step.
    let undo = h.app.session.undo.len();
    press(&mut h, "OK", 0);
    assert!(h.app.dialog.is_none(), "OK closes the edit");
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn directed_angle_disables_limits() {
    let mut h = opened();
    pick_type(&mut h, "Angle");
    let at = h.widget("More").unwrap();
    h.step(&json!({"click": at})).unwrap();
    h.frames(3);
    pick_solution(&mut h, "Directed");
    for limit in ["Max", "Min"] {
        let c = of_role(&h, limit, Role::CheckBox).unwrap();
        assert!(c.is_disabled() && c.toggled() == Some(Toggled::False), "{limit} is off and disabled for a Directed angle");
        assert!(of_role(&h, limit, Role::TextInput).unwrap().is_disabled());
    }
    pick_solution(&mut h, "Undirected");
    for limit in ["Max", "Min"] {
        assert!(!of_role(&h, limit, Role::CheckBox).unwrap().is_disabled(), "{limit} is live for an Undirected angle");
    }
    // An edited limited Undirected Angle switched to Directed clears its limits.
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    h.app
        .run(
            "constraint.create",
            json!({"type": "angle", "solution": "undirected", "a": {"occurrence": "A:1", "face": [50, 25, 20]}, "b": {"occurrence": "B:1", "face": [50, 70, 17]}, "offset": 90, "min": 80, "max": 100}),
        )
        .unwrap();
    let id = crate::test_support::constraint_id(&h, "Angle:1");
    open_edit(&mut h, id);
    let at = h.widget("More").unwrap();
    h.step(&json!({"click": at})).unwrap();
    h.frames(3);
    assert_eq!(of_role(&h, "Max", Role::CheckBox).unwrap().toggled(), Some(Toggled::True));
    pick_solution(&mut h, "Directed");
    let p = form(&h).draft().params();
    assert_eq!(p["solution"], json!("directed"));
    assert_eq!(p.get("min"), Some(&json!(null)), "{p}");
    assert_eq!(p.get("max"), Some(&json!(null)), "{p}");
}

#[test]
fn ok_with_both_picks_is_one_undo_step() {
    let mut h = opened();
    let (a, b) = (face(&h, "A", [50.0, 25.0, 20.0]), face(&h, "B", [50.0, 95.0, 27.0]));
    {
        let f = form_mut(&mut h);
        f.picks[0] = Some(a);
        f.picks[1] = Some(b);
    }
    h.frames(2);
    assert!(form(&h).ready());
    assert!(can(&h, PanelAction::Ok) && can(&h, PanelAction::ApplyAndContinue));
    let undo = h.app.session.undo.len();
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(3);
    assert!(h.app.dialog.is_none(), "Enter is OK: it commits and closes");
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(h.app.session.doc.assembly.constraint("Mate:1").is_some());
}

/// SolidCraft opens the same panel (spec §6.9: the differences are profile data only).
#[test]
fn solidcraft_gets_the_same_panel() {
    let mut h = Harness::new();
    constraint_blocks(&mut h);
    h.app.start("constraint.create");
    h.frames(3);
    let f = form(&h);
    assert_eq!((f.kind, f.solution, f.armed, f.predict, f.show_preview), (K::Mate, None, 0, false, true));
    assert_eq!(heading(&h), "Constrain");
    assert!(disabled(&h, "Motion"));
}
