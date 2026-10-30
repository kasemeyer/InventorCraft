#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Plan E-2 Task 1: the constraint commands on the ribbons (spec §6.1, §6.9), the `C` alias and
//! the commands' own icons (R159).

use super::*;
use serde_json::json;
use solidcraft_engine::doc::DocumentKind::{self, *};

use crate::scenario::Harness;
use crate::test_support::{alias, click_world, constraint_blocks, constraint_id, frames_for, inventor};

const L: bool = false;
const S: bool = true;

/// The panel's face as the ribbon draws it: (command, small?), in order.
fn face(p: &Profile, kind: DocumentKind, tab: &str, panel: &str) -> Vec<(String, bool)> {
    let p = p.ribbon(kind).tab(tab).unwrap().panel(panel).unwrap();
    p.commands.iter().take(p.promoted).map(|c| (c.clone(), p.is_small(c))).collect()
}

fn owned(face: &[(&str, bool)]) -> Vec<(String, bool)> {
    face.iter().map(|(c, s)| ((*c).to_owned(), *s)).collect()
}

fn inv() -> &'static Profile {
    get(INVENTOR).unwrap()
}

/// The ribbon's view of a command: what greys the button and why.
fn info(h: &Harness, id: &str) -> solidcraft_engine::CommandInfo {
    crate::render_workspace::interactive_command_info(&h.app, solidcraft_engine::find_command(id).unwrap())
}

/// CAP §E: Joint and Constrain large, then Show, Show Problems and Hide All stacked small; the
/// other joint commands in the slide-out; Position as it was (Q18).
#[test]
fn relationships_panel_follows_the_capture() {
    assert_eq!(
        face(inv(), Assembly, "ASSEMBLE", "RELATIONSHIPS"),
        owned(&[("joint.create", L), ("constraint.create", L), ("constraint.show", S), ("constraint.show_sick", S), ("constraint.hide_all", S),])
    );
    let p = inv().ribbon(Assembly).tab("ASSEMBLE").unwrap().panel("RELATIONSHIPS").unwrap();
    assert_eq!(p.promoted, 5);
    assert_eq!(
        p.commands[p.promoted..],
        ["joint.as_built", "joint.rigid_group", "joint.gear", "joint.rack_pinion", "joint.screw", "joint.cam", "joint.limits", "joint.reach"]
    );
    let position = inv().ribbon(Assembly).tab("ASSEMBLE").unwrap().panel("POSITION").unwrap();
    assert_eq!(position.promoted, 1);
    assert_eq!(position.commands, ["solid.move", "occurrence.ground", "component.capture_position", "component.revert_position"]);
    assert_eq!(validate(inv()), Vec::<String>::new());
}

/// Show Problems is drawn greyed with E-1's reason until a constraint is sick or lost; Constrain
/// is greyed in an assembly with no components.
#[test]
fn show_problems_greys_until_something_is_sick() {
    let mut h = inventor(Assembly);
    constraint_blocks(&mut h);
    h.frames(3);
    assert_eq!(h.app.ui.tab, "ASSEMBLE");
    assert!(crate::scenario::handle_at("button:constraint.show_sick").is_some(), "{:?}", crate::scenario::handles("button:"));
    let sick = info(&h, "constraint.show_sick");
    assert!(!sick.enabled);
    assert_eq!(sick.disabled_reason.as_deref(), Some("nothing is sick"));
    assert!(info(&h, "constraint.create").enabled);

    // The plate's hole rim in the pin's top rim, then the plate cut short of its hole.
    let insert =
        json!({"type": "insert", "a": {"occurrence": "Plate", "edge": [200.0, 0.0, 10.0]}, "b": {"occurrence": "Pin", "edge": [310.0, 0.0, 110.0]}});
    h.app.run("constraint.create", insert).unwrap();
    assert!(constraint_id(&h, "Insert:1") > 0);
    assert_eq!(info(&h, "constraint.show_sick").disabled_reason.as_deref(), Some("nothing is sick"), "the Insert is met");
    h.app.run("timeline.edit", json!({"feature": "Plate", "set": {"length": "25 mm"}})).unwrap();
    h.frames(3);
    let sick = info(&h, "constraint.show_sick");
    assert!(sick.enabled, "{:?}", sick.disabled_reason);
    assert!(crate::scenario::handle_at("button:constraint.show_sick").is_some());

    let mut h = inventor(Assembly);
    h.app.run("document.new_assembly", json!({})).unwrap();
    h.frames(3);
    let create = info(&h, "constraint.create");
    assert!(!create.enabled);
    assert_eq!(create.disabled_reason.as_deref(), Some("needs an assembly with components"));
}

/// Q12: `C` is Constrain in an assembly; `J` is still Joint.
#[test]
fn c_fires_constrain_in_an_assembly() {
    let mut h = inventor(Assembly);
    constraint_blocks(&mut h);
    alias(&mut h, "C");
    assert_eq!(h.app.alias.fired.as_deref(), Some("constraint.create"));

    let mut h = inventor(Assembly);
    constraint_blocks(&mut h);
    alias(&mut h, "J");
    assert_eq!(h.app.alias.fired.as_deref(), Some("joint.create"));
}

/// Review Focus: `CH` (model-scope Chamfer) still wins inside the wait; `C` alone waits the
/// profile's 750 ms and then constrains; a Part has no `C`.
#[test]
fn c_waits_for_ch_then_constrains() {
    let mut h = inventor(Assembly);
    constraint_blocks(&mut h);
    alias(&mut h, "CH");
    assert_eq!(h.app.alias.fired.as_deref(), Some("solid.chamfer"));

    let mut h = inventor(Assembly);
    constraint_blocks(&mut h);
    h.app.alias.fired = None;
    h.step(&json!({"key": "C"})).unwrap();
    assert_eq!(h.app.alias.fired, None, "C waits for CH");
    h.frames(frames_for(0.75));
    assert_eq!(h.app.alias.fired.as_deref(), Some("constraint.create"));

    let mut h = inventor(Part);
    h.app.alias.fired = None;
    h.step(&json!({"key": "C"})).unwrap();
    h.frames(frames_for(0.75));
    assert_eq!(h.app.alias.fired, None, "a part has no C alias");
}

/// Spec §6.9: SolidCraft's JOINTS panel gains Constrain beside Joint (a SolidCraft panel promotes
/// at most four, so Rigid Group moves to the panel's list) and the three show commands in its list.
#[test]
fn solidcraft_joints_panel_gains_the_four() {
    let sc = solidcraft();
    let joints = sc.ribbon(Assembly).tab("ASSEMBLE").unwrap().panel("JOINTS").unwrap();
    for id in ["constraint.create", "constraint.show", "constraint.show_sick", "constraint.hide_all"] {
        assert!(joints.commands.iter().any(|c| c == id), "{id}: {:?}", joints.commands);
    }
    assert_eq!(
        face(&sc, Assembly, "ASSEMBLE", "JOINTS"),
        owned(&[("joint.create", L), ("constraint.create", L), ("joint.as_built", L), ("joint.origin", L)])
    );
    let after: Vec<&str> = joints.commands[joints.promoted..].iter().map(String::as_str).collect();
    assert_eq!(after.first(), Some(&"joint.rigid_group"));
    let shows: Vec<&str> = after.into_iter().filter(|c| c.starts_with("constraint.")).collect();
    assert_eq!(shows, ["constraint.show", "constraint.show_sick", "constraint.hide_all"]);
    assert!(sc.aliases.iter().all(|a| a.command != "constraint.create"), "SolidCraft binds no key to Constrain");
}

/// R159: the constraint commands draw their own masters (with a small-slot variant), not the
/// borrowed `joint`, `eye` and `inspect`.
#[test]
fn the_commands_use_their_own_icons() {
    for (id, icon) in [
        ("constraint.create", "constrain"),
        ("constraint.show", "show_relationships"),
        ("constraint.show_sick", "show_sick"),
        ("constraint.hide_all", "hide_relationships"),
        ("constraint.suppress", "hide"),
    ] {
        assert_eq!(solidcraft_engine::find_command(id).unwrap().icon, icon, "{id}");
        let r = crate::icon_svg::resolve(icon);
        assert!(!r.placeholder && r.master == icon, "{icon} has no master");
        let (big, small) = (crate::icon_svg::doc_for(icon, 64).unwrap(), crate::icon_svg::doc_for(icon, 16).unwrap());
        assert!(!std::ptr::eq(big, small), "{icon} has no small-slot variant");
    }
}

/// R162: a suppressed constraint row carries its own badge; an unknown name draws nothing.
#[test]
fn the_suppressed_badge_draws() {
    let shapes = |name: &'static str| {
        let ctx = egui::Context::default();
        let out = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
            let r = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(12.0, 12.0));
            crate::icons::glyphs::badge(ui.painter(), r, name, ui.visuals().text_color());
        });
        out.shapes.len()
    };
    assert!(shapes("suppressed") > shapes("no such badge"));
}

/// The fixture is laid out for picking from the home view: B's top face is under its pick.
#[test]
fn constraint_blocks_pick_from_the_home_view() {
    let mut h = inventor(Assembly);
    constraint_blocks(&mut h);
    assert_eq!(h.app.session.doc.occurrences.len(), 4);
    click_world(&mut h, [50.0, 95.0, 27.0]);
    assert!(!h.app.session.selection.is_empty(), "nothing picked at B's top");
}
