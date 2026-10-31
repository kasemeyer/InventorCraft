#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Plan E-2 Task 6: the Drive panel (spec §6.6, CAP §C). "The contact setup": E-1's blocks, Flush
//! A top / B top offset 0 (`Mate:1`, B's top at z = 20), a grounded component "C"
//! (`solid.box {100, 50, 20, corner}`, 13 mm above B's top at the default corner) and a contact
//! set [B, C]. The panel plays `constraint.drive`'s frames as preview poses; the document never
//! changes and closing restores the drawing (owner Q17).

use egui::accesskit::{Node, Role};
use serde_json::{Value, json};
use solidcraft_engine::doc::DocumentKind::Assembly;
use solidcraft_engine::doc::Mat;
use solidcraft_engine::doc::joints::occurrence_world;

use super::DriveForm;
use crate::context_menu::Target;
use crate::dialogs::Kind;
use crate::dialogs_assembly::Asm;
use crate::i18n::tr;
use crate::scenario::{Harness, count_of};
use crate::test_support::{clicks_expire, constraint_blocks, constraint_id, frames_for, inventor, menu_pick, right_click};

/// C's corner 13 mm above B's top once the Flush holds B's top at z = 20.
const ABOVE: [f64; 3] = [0.0, 70.0, 33.0];

fn run(h: &mut Harness, id: &str, p: Value) -> Value {
    let r = h.app.run(id, p.clone()).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    h.frames(2);
    r
}

fn occ(h: &Harness, name: &str) -> u64 {
    h.app.session.doc.occurrences.iter().find(|o| o.name == format!("{name}:1")).map(|o| o.id).unwrap_or_else(|| panic!("no occurrence {name}"))
}

fn world(h: &Harness, name: &str) -> Mat {
    occurrence_world(&h.app.session.doc, occ(h, name))
}

fn near(a: &Mat, b: &Mat, tol: f64) -> bool {
    a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() <= tol)
}

/// The contact setup on `h`, with C's box at `c`; Mate:1's id.
fn contact_setup_on(mut h: Harness, c: [f64; 3]) -> (Harness, u64) {
    constraint_blocks(&mut h);
    h.frames(2);
    run(
        &mut h,
        "constraint.create",
        json!({"type": "mate", "solution": "flush", "a": {"occurrence": "A", "face": [50, 25, 20]}, "b": {"occurrence": "B", "face": [50, 95, 27]}}),
    );
    let top = solidcraft_engine::doc::apply_point(&world(&h, "B"), solidcraft_engine::geom::Vec3::new(50.0, 95.0, 27.0));
    assert!((top.z - 20.0).abs() < 1e-9, "the Flush holds B's top at z = 20: {top:?}");
    run(&mut h, "component.create", json!({"name": "C"}));
    run(&mut h, "solid.box", json!({"length": 100, "width": 50, "height": 20, "corner": c}));
    run(&mut h, "component.activate", json!({"component": "root"}));
    let (b, cc) = (occ(&h, "B"), occ(&h, "C"));
    // Only the first placement is grounded on its own (R103/R123/R142).
    run(&mut h, "occurrence.ground", json!({"occurrence": cc, "grounded": true}));
    run(&mut h, "contact.create", json!({"occurrences": [b, cc]}));
    let mate = constraint_id(&h, "Mate:1");
    (h, mate)
}

fn contact_setup() -> (Harness, u64) {
    contact_setup_on(inventor(Assembly), ABOVE)
}

/// Drive from Mate:1's row menu under Relationships.
fn open_drive(h: &mut Harness, id: u64) {
    crate::context_menu::close(&mut h.app);
    clicks_expire(h);
    right_click(h, json!({"handle": format!("row:constraint:{id}:0")}));
    menu_pick(h, "menu", "ui.constraintDrive");
    h.frames(2);
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Assembly(Asm::DriveConstraint(_)))), "the Drive panel opened");
}

fn form(h: &Harness) -> &DriveForm {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Assembly(Asm::DriveConstraint(f))) => f,
        other => panic!("the Drive panel is not open: {other:?}"),
    }
}

fn form_mut(h: &mut Harness) -> &mut DriveForm {
    match h.app.dialog.as_mut().map(|d| &mut d.kind) {
        Some(Kind::Assembly(Asm::DriveConstraint(f))) => f,
        other => panic!("the Drive panel is not open: {other:?}"),
    }
}

/// The accessible nodes drawn inside the panel labelled `label`.
fn nodes(h: &Harness, label: &str) -> Vec<Node> {
    let Some(panel) = crate::scenario::rect_of("dialog:outer") else { return Vec::new() };
    let Some(tree) = h.output.accesskit_update.as_ref() else { return Vec::new() };
    let inside = |n: &Node| n.bounds().is_some_and(|b| panel.contains(egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)));
    tree.nodes.iter().filter(|(_, n)| (n.label() == Some(label) || n.value() == Some(label)) && inside(n)).map(|(_, n)| n.clone()).collect()
}

fn drawn(h: &Harness, label: &str) -> bool {
    !nodes(h, label).is_empty()
}

fn button(h: &Harness, label: &str) -> Node {
    nodes(h, label).into_iter().find(|n| n.role() == Role::Button).unwrap_or_else(|| panic!("no button `{label}` in the panel"))
}

fn field(h: &Harness, label: &str) -> String {
    nodes(h, label)
        .into_iter()
        .find(|n| n.role() == Role::TextInput)
        .and_then(|n| n.value().map(str::to_string))
        .unwrap_or_else(|| panic!("no `{label}` box in the panel"))
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

/// Click the panel's button `label` (one step: nothing else runs).
fn click(h: &mut Harness, label: &str) {
    let b = button(h, label).bounds().unwrap();
    h.step(&json!({"click": [(b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0]})).unwrap();
}

fn press(h: &mut Harness, label: &str) {
    click(h, label);
    h.frames(2);
}

/// Play and let the sweep run out (at most 10 s of frames).
fn play_out(h: &mut Harness) {
    press(h, tr("ui.constraint.drive_play"));
    for _ in 0..600 {
        if form(h).playing.is_none() {
            break;
        }
        h.frames(1);
    }
    assert!(form(h).playing.is_none(), "the sweep ended");
    h.frames(2);
}

/// The Position read-out: the label drawn in its published rect.
fn position(h: &Harness) -> String {
    let at = crate::scenario::rect_of("dialog:drive_position").expect("the Position read-out is drawn");
    let tree = h.output.accesskit_update.as_ref().unwrap();
    tree.nodes
        .iter()
        .filter(|(_, n)| n.role() == Role::Label)
        .find(|(_, n)| n.bounds().is_some_and(|b| at.contains(egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32))))
        .and_then(|(_, n)| n.value().map(str::to_string))
        .unwrap_or_else(|| panic!("no Position read-out"))
}

fn drives() -> f64 {
    count_of("constraint_drives").unwrap_or(0.0)
}

#[test]
fn drive_opens_with_the_captured_defaults() {
    let (mut h, mate) = contact_setup();
    open_drive(&mut h, mate);
    assert_eq!(heading(&h), "Drive", "a static title (R138)");
    assert!(drawn(&h, "Mate:1"), "the body's header row names the constraint");
    let f = form(&h);
    assert_eq!(f.constraint, mate);
    assert_eq!((f.start.as_str(), f.end.as_str(), f.pause.as_str()), ("0.000 mm", "10.000 mm", "0.000 s"));
    assert!(!f.by_steps);
    assert_eq!(f.increment, "1.000 mm");
    assert!(!f.start_end_start);
    assert_eq!(f.repeat, "1");
    assert!(!f.stop_on_contact && !f.expanded);
    assert!(f.frames.is_empty() && f.playing.is_none() && f.stopped.is_none());
    assert_eq!(f.problem(), None);
    assert_eq!(f.params(), json!({"constraint": mate, "start": 0.0, "end": 10.0, "increment": 1.0, "repeat": 1, "cycle": "start_end"}));
    // The boxes and the read-out as drawn; the disabled one-parameter combo.
    assert_eq!(field(&h, "Start"), "0.000 mm");
    assert_eq!(field(&h, "End"), "10.000 mm");
    assert_eq!(field(&h, "Pause"), "0.000 s");
    assert_eq!(position(&h), "0.000 mm");
    for b in ["ui.constraint.drive_reverse", "ui.constraint.drive_play", "ui.constraint.drive_stop", "ui.constraint.drive_to_start"] {
        assert!(!button(&h, tr(b)).is_disabled(), "{b}");
    }
    for b in ["ui.constraint.drive_step_back", "ui.constraint.drive_step_on", "ui.constraint.drive_to_end"] {
        assert!(drawn(&h, tr(b)), "{b}");
    }
    assert!(drawn(&h, "OK") && drawn(&h, "Cancel"));
    // Collapsed: More opens Stop on contact, Increment and Repetitions.
    assert!(!drawn(&h, tr("ui.constraint.drive_stop_on_contact")));
    press(&mut h, tr("ui.constraint.more"));
    assert!(form(&h).expanded);
    assert!(drawn(&h, tr("ui.constraint.drive_stop_on_contact")));
    assert_eq!(field(&h, tr("ui.constraint.drive_by_amount")), "1.000 mm");
    assert_eq!(field(&h, tr("ui.constraint.drive_times")), "1");
    // Opening runs nothing and takes no step.
    assert_eq!(drives(), 0.0);
}

/// §8.3 6: a drive that runs into C stops at contact; closing puts everything back.
#[test]
fn drive_stops_on_contact_and_restores() {
    let (mut h, mate) = contact_setup();
    let (undo, pose) = (h.app.session.undo.len(), world(&h, "B"));
    open_drive(&mut h, mate);
    let revision = h.app.session.revision;
    {
        let f = form_mut(&mut h);
        f.end = "20".into();
        f.by_steps = true;
        f.steps = "10".into();
        f.stop_on_contact = true;
        f.expanded = true;
    }
    h.frames(2);
    assert_eq!(
        form(&h).params(),
        json!({"constraint": mate, "start": 0.0, "end": 20.0, "steps": 10, "repeat": 1, "cycle": "start_end", "stop_on_contact": true})
    );
    press(&mut h, tr("ui.constraint.drive_play"));
    // Mid-sweep: B is drawn at the frame's pose, the document stays.
    for _ in 0..3 {
        h.frames(1);
    }
    assert!(h.app.preview.active, "B drawn at a frame's pose");
    assert!(count_of("constraint_drive_moved").unwrap_or(0.0) >= 1.0);
    assert!(near(&world(&h, "B"), &pose, 0.0), "the document never changes");
    play_out(&mut h);
    let f = form(&h);
    assert_eq!(f.at, f.frames.len() - 1, "the last frame is shown");
    // R119: contact is an overlap deeper than the collision depth threshold, 1e-4 × the largest
    // body size (≈ 0.011 mm for these 100 × 50 × 20 blocks): the stop is 13.000 within ±0.02.
    let last = f.frames[f.at].0;
    assert!((last - 13.0).abs() <= 0.02, "last value {last}");
    assert_eq!(position(&h), crate::dialogs_constraint::value_text(false, last, "mm"));
    let why = f.stopped.clone().expect("a stop reason");
    assert!(why.contains("B:1") && why.contains("C:1"), "{why}");
    assert!(drawn(&h, &why), "the stop reason is drawn under the buttons");
    assert_eq!(drives(), 1.0, "Play called constraint.drive once");
    // Cancel: B drawn at its document transform, no step, the offset 0.
    press(&mut h, "Cancel");
    h.frames(3);
    assert!(h.app.dialog.is_none());
    assert!(!h.app.preview.active, "nothing is left drawn at a frame's pose");
    assert_eq!(count_of("constraint_drive_moved"), Some(0.0));
    assert!(near(&world(&h, "B"), &pose, 1e-9), "B at its document transform");
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.app.session.revision, revision);
    assert_eq!(h.app.session.doc.assembly.constraint("Mate:1").unwrap().offset, 0.0);
}

#[test]
fn steps_move_one_frame() {
    let (mut h, mate) = contact_setup();
    open_drive(&mut h, mate);
    play_out(&mut h);
    assert_eq!(form(&h).frames.len(), 11, "0 to 10 by 1");
    assert_eq!(form(&h).at, 10);
    assert_eq!(position(&h), "10.000 mm");
    press(&mut h, tr("ui.constraint.drive_step_back"));
    press(&mut h, tr("ui.constraint.drive_step_back"));
    assert_eq!(form(&h).at, 8);
    assert_eq!(position(&h), "8.000 mm", "two frames earlier");
    press(&mut h, tr("ui.constraint.drive_step_on"));
    assert_eq!(position(&h), "9.000 mm");
    press(&mut h, tr("ui.constraint.drive_to_start"));
    assert_eq!(form(&h).at, 0);
    assert_eq!(position(&h), "0.000 mm", "Start's value");
    press(&mut h, tr("ui.constraint.drive_to_end"));
    assert_eq!(position(&h), "10.000 mm");
    // Backward from the end plays down to Start.
    press(&mut h, tr("ui.constraint.drive_reverse"));
    for _ in 0..30 {
        h.frames(1);
    }
    assert_eq!(form(&h).at, 0);
    assert!(form(&h).playing.is_none());
    assert_eq!(drives(), 1.0, "the step buttons and a second play reuse the frames");
}

#[test]
fn pause_delays_each_frame() {
    let (mut h, mate) = contact_setup();
    open_drive(&mut h, mate);
    {
        let f = form_mut(&mut h);
        f.pause = "0.1".into();
        f.end = "2".into();
        f.increment = "1".into();
    }
    h.frames(2);
    // A quick tap: Play is pressed and released in one harness frame, so the frames below count
    // from the frame it started on.
    let b = button(&h, tr("ui.constraint.drive_play")).bounds().unwrap();
    let at = egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
    h.step(&json!({"move": [at.x, at.y]})).unwrap();
    let button_event = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    h.frame_with(vec![button_event(true), button_event(false)]);
    assert_eq!(form(&h).playing, Some(true));
    assert_eq!(form(&h).frames.len(), 3);
    for i in 1..=5 {
        h.frames(1);
        assert_eq!(form(&h).at, 0, "frame {i} of the 0.1 s pause");
    }
    h.frames(frames_for(0.1) - 5);
    assert_eq!(form(&h).at, 1, "the next frame after 0.1 s");
}

#[test]
fn hostile_drive_entries_are_refused_inline() {
    let (mut h, mate) = contact_setup();
    open_drive(&mut h, mate);
    press(&mut h, tr("ui.constraint.more"));
    let fresh = form(&h).clone();
    type Edit = fn(&mut DriveForm);
    let cases: [(&str, Edit, &str); 7] = [
        ("End abc", |f| f.end = "abc".into(), "End"),
        ("Total steps 0", |f| (f.by_steps, f.steps) = (true, "0".into()), "Total steps"),
        ("Total steps 10000", |f| (f.by_steps, f.steps) = (true, "10000".into()), "Total steps"),
        ("Repetitions -1", |f| f.repeat = "-1".into(), "Repetitions"),
        ("Repetitions 1001", |f| f.repeat = "1001".into(), "Repetitions"),
        ("By amount 0", |f| f.increment = "0".into(), "By amount"),
        ("9999 steps x 2", |f| (f.by_steps, f.steps, f.repeat) = (true, "9999".into(), "2".into()), "at most 10,000 frames"),
    ];
    for (case, edit, names) in cases {
        *form_mut(&mut h) = fresh.clone();
        edit(form_mut(&mut h));
        h.frames(2);
        let why = form(&h).problem().unwrap_or_else(|| panic!("{case}: no problem"));
        assert!(why.contains(names), "{case}: {why}");
        assert!(drawn(&h, &why), "{case}: the problem is drawn inline");
        assert!(button(&h, tr("ui.constraint.drive_play")).is_disabled(), "{case}: Play disabled");
        assert!(button(&h, tr("ui.constraint.drive_reverse")).is_disabled(), "{case}: Play backward disabled");
        let p = form(&h).params();
        assert!(p.as_object().unwrap().values().all(|v| !v.is_null()), "{case}: never a null {p}");
        assert!(!(p.get("increment").is_some() && p.get("steps").is_some()), "{case}: increment XOR steps {p}");
        press(&mut h, tr("ui.constraint.drive_play"));
        assert!(form(&h).frames.is_empty() && form(&h).playing.is_none(), "{case}");
        assert_eq!(drives(), 0.0, "{case}: no command ran");
    }
    // The 10,000-frame cap counts a pass times its repetitions.
    *form_mut(&mut h) = fresh.clone();
    (form_mut(&mut h).by_steps, form_mut(&mut h).steps, form_mut(&mut h).repeat) = (true, "4999".into(), "2".into());
    assert_eq!(form(&h).problem(), None, "5,000 x 2 frames");
}

/// A drive whose very first value already touches stops there: the reason has no value
/// (`stopped_by_contact.value` is null), no frame plays and Position reads Start.
#[test]
fn a_stop_on_the_first_value_shows_no_value() {
    let (mut h, mate) = contact_setup();
    open_drive(&mut h, mate);
    {
        let f = form_mut(&mut h);
        f.start = "15".into();
        f.end = "20".into();
        f.stop_on_contact = true;
    }
    h.frames(2);
    press(&mut h, tr("ui.constraint.drive_play"));
    h.frames(3);
    let f = form(&h);
    assert!(f.frames.is_empty(), "no frame plays");
    assert!(f.playing.is_none());
    let why = f.stopped.clone().expect("a stop reason");
    assert_eq!(why, crate::i18n::format("ui.constraint.drive_contact_first", &[("a", "B:1"), ("b", "C:1")]));
    assert!(!why.contains("mm"), "no value: {why}");
    assert!(drawn(&h, &why));
    assert_eq!(position(&h), "15.000 mm", "Position reads Start");
    press(&mut h, tr("ui.constraint.drive_step_on"));
    press(&mut h, tr("ui.constraint.drive_to_end"));
    assert_eq!(position(&h), "15.000 mm");
}

/// R162: a suppressed or Lost constraint has no Drive panel (the menu's reason); SolidCraft gets
/// the same panel from the same row menu.
#[test]
fn drive_refuses_suppressed_and_lost_and_runs_on_solidcraft() {
    let (mut h, mate) = contact_setup();
    run(&mut h, "constraint.suppress", json!({"constraint": mate}));
    assert!(DriveForm::new(&h.app, mate).is_none());
    let items = crate::context_menu::items(&h.app, &Target::Constraint { id: mate, under: 0 });
    let drive = items.iter().find(|i| i.id == "ui.constraintDrive").unwrap();
    assert!(!drive.enabled, "the row's Drive is disabled");
    crate::context_menu::run_item(&mut h.app, drive, egui::pos2(10.0, 10.0));
    h.frames(2);
    assert!(h.app.dialog.is_none(), "no panel on a suppressed constraint");
    // Reached another way (the control channel), the panel still refuses, saying why.
    super::open(&mut h.app, mate);
    assert!(h.app.dialog.is_none());
    assert!(h.app.status.as_ref().is_some_and(|s| s.0.ends_with("Mate:1 is suppressed: unsuppress it to drive it")), "{:?}", h.app.status);

    // Lost: the plate's hole cut short under an Insert.
    run(
        &mut h,
        "constraint.create",
        json!({"type": "insert", "a": {"occurrence": "Plate", "edge": [200.0, 0.0, 10.0]}, "b": {"occurrence": "Pin", "edge": [310.0, 0.0, 50.0]}}),
    );
    let insert = constraint_id(&h, "Insert:1");
    run(&mut h, "timeline.edit", json!({"feature": "Plate", "set": {"length": "25 mm"}}));
    assert!(DriveForm::new(&h.app, insert).is_none(), "a Lost constraint has no Drive panel");

    // SolidCraft: the same panel, the same frames.
    let (mut h, mate) = contact_setup_on(Harness::new(), ABOVE);
    open_drive(&mut h, mate);
    assert_eq!(heading(&h), "Drive");
    play_out(&mut h);
    assert_eq!(form(&h).frames.len(), 11);
    assert_eq!(position(&h), "10.000 mm");
    press(&mut h, "OK");
    h.frames(3);
    assert!(h.app.dialog.is_none() && !h.app.preview.active, "OK closes and restores too");
}
