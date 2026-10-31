#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Place Component's ghost (R171), on both profiles: the first part of an empty assembly at the
//! origin, grounded, at once; each click one more free copy where the ghost is; Esc ends; undo
//! takes one back per step; navigating leaves the ghost live and places nothing.

use serde_json::json;
use solidcraft_engine::Session;
use solidcraft_engine::doc::DocumentKind::Assembly;
use solidcraft_engine::doc::{IDENTITY, Mat, Occurrence};
use solidcraft_engine::geom::Vec3;

use crate::scenario::Harness;
use crate::test_support::inventor;

/// SolidCraft's Place and Inventor's, each with a saved 10 x 20 x 30 box to place.
fn both() -> Vec<Harness> {
    [Harness::new(), inventor(Assembly)]
        .into_iter()
        .map(|mut h| {
            h.app.home.open = false;
            let mut s = Session::default();
            s.execute("solid.box", &json!({"length": 10, "width": 20, "height": 30})).unwrap();
            s.execute("file.save", &json!({"path": h.dir.join("part1.solidcraft")})).unwrap();
            h.call("ui.view", json!({"view": "iso"}));
            h.frames(2);
            h
        })
        .collect()
}

fn place(h: &mut Harness) {
    h.step(&json!({"choose_file": "part1.solidcraft"})).unwrap();
    h.step(&json!({"start": "component.insert_design"})).unwrap();
    // The first placement fits the view: let it settle.
    h.settle();
    // Back off so the copies' spots beside it are in view.
    h.app.cam.distance *= 4.0;
    h.frames(3);
}

fn roots(h: &Harness) -> Vec<Occurrence> {
    h.app.session.doc.occurrences.iter().filter(|o| o.parent == 0).cloned().collect()
}

fn near(a: &Mat, b: &Mat, tol: f64) -> bool {
    a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() <= tol)
}

/// Hover the world point on the ground, then click there: the ghost's pose just before the click
/// and the occurrence it made.
fn click_at(h: &mut Harness, p: [f64; 3]) -> (Mat, Occurrence) {
    h.step(&json!({"move": {"world": p}})).unwrap();
    h.frames(2);
    let ghost = super::ghost(&h.app).expect("the ghost follows the pointer");
    let at = super::ghost_world(&h.app).unwrap();
    assert!(at.dist(Vec3::new(p[0], p[1], p[2])) < 0.5, "the ghost is under the pointer: {at:?} for {p:?}");
    let before = roots(h).len();
    h.step(&json!({"click": {"world": p}})).unwrap();
    h.frames(2);
    let all = roots(h);
    assert_eq!(all.len(), before + 1, "one occurrence per click");
    (ghost, all.last().cloned().unwrap())
}

#[test]
fn the_first_part_of_an_empty_assembly_is_grounded_at_the_origin_and_the_ghost_goes_on() {
    for mut h in both() {
        assert!(roots(&h).is_empty());
        place(&mut h);
        let r = roots(&h);
        assert_eq!(r.len(), 1, "placed without a click");
        assert!(r[0].grounded, "grounded (R103)");
        assert!(near(&r[0].transform, &IDENTITY, 0.0), "at the origin: {:?}", r[0].transform);
        assert!(super::live(&h.app), "the ghost goes on for more copies");
        assert_eq!(crate::status_bar::texts(&h.app).left, crate::i18n::tr("ui.place.prompt"), "the status bar prompts");
    }
}

#[test]
fn each_click_places_a_free_copy_where_the_ghost_is_and_undo_takes_one_back() {
    for mut h in both() {
        place(&mut h);
        let undo = h.app.session.undo.len();
        let (g1, o1) = click_at(&mut h, [60.0, 40.0, 0.0]);
        let (g2, o2) = click_at(&mut h, [-50.0, 30.0, 0.0]);
        for (g, o) in [(g1, &o1), (g2, &o2)] {
            assert!(near(&o.transform, &g, 1e-9), "placed at the ghost's pose: {:?} vs {g:?}", o.transform);
            assert!(!o.grounded, "copies are free");
        }
        assert_eq!(o1.component, o2.component, "copies of one design share its component");
        assert_eq!(h.app.session.undo.len(), undo + 2, "one undo step per click");
        assert!(super::live(&h.app));
        for left in [2, 1, 0] {
            h.app.run("edit.undo", json!({})).unwrap();
            h.frames(1);
            assert_eq!(roots(&h).len(), left, "undo removes one placement per step");
        }
    }
}

#[test]
fn esc_ends_place_and_a_click_then_places_nothing() {
    for mut h in both() {
        place(&mut h);
        click_at(&mut h, [60.0, 40.0, 0.0]);
        h.step(&json!({"key": "Escape"})).unwrap();
        h.frames(2);
        assert!(!super::live(&h.app), "Esc ends Place");
        h.step(&json!({"click": {"world": [-50.0, 30.0, 0.0]}})).unwrap();
        h.frames(2);
        assert_eq!(roots(&h).len(), 2, "no further occurrence");
        assert!(!h.app.preview.active || h.app.preview.bounds.is_empty(), "the ghost is gone");
    }
}

/// Right-click ends Place: the marking menu's list says Done (Inventor), and where the profile
/// has no marking menu (SolidCraft) the right-click itself is Done, as it ends a tool.
#[test]
fn right_click_done_ends_place() {
    for mut h in both() {
        place(&mut h);
        h.step(&json!({"move": {"world": [60.0, 40.0, 0.0]}})).unwrap();
        crate::test_support::right_click(&mut h, json!({"world": [60.0, 40.0, 0.0]}));
        if crate::marking_menu::layout(&h.app).is_some() {
            assert!(super::live(&h.app), "the menu opens over the ghost");
            let list = crate::marking_menu::overflow(&h.app).unwrap();
            assert_eq!(list.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["ui.placeDone"]);
            crate::test_support::menu_pick(&mut h, "menu", "ui.placeDone");
        }
        assert!(!super::live(&h.app), "Done ends Place");
        assert_eq!(roots(&h).len(), 1, "a right-click places nothing");
    }
}

/// Navigation guard: orbiting while the ghost follows turns the view, places nothing, and leaves
/// Place running.
#[test]
fn orbiting_while_the_ghost_is_live_places_nothing() {
    use crate::prefs::NavButton;
    for mut h in both() {
        place(&mut h);
        h.step(&json!({"move": {"world": [60.0, 40.0, 0.0]}})).unwrap();
        h.frames(2);
        let none = egui::Modifiers::NONE;
        let chords = [none, egui::Modifiers::SHIFT, egui::Modifiers::CTRL, egui::Modifiers::ALT];
        let (button, mods) = [(NavButton::Middle, "middle"), (NavButton::Right, "right"), (NavButton::Left, "primary")]
            .into_iter()
            .flat_map(|b| chords.into_iter().map(move |m| (b, m)))
            .find(|((b, _), m)| crate::prefs::nav_mode(&h.app, *b, *m) == Some(crate::viewport::NavMode::Orbit))
            .expect("the preset orbits with a mouse drag");
        let cam = h.app.cam;
        let r = h.call(
            "ui.drag",
            json!({"x0": 700, "y0": 500, "x1": 820, "y1": 430, "steps": 8, "button": button.1, "shift": mods.shift, "ctrl": mods.ctrl, "alt": mods.alt}),
        );
        assert_eq!(r["ok"], true, "{r}");
        h.frames(3);
        assert_ne!(h.app.cam, cam, "the view turned");
        assert_eq!(roots(&h).len(), 1, "orbiting places nothing");
        assert!(super::live(&h.app), "the ghost is still live");
    }
}

/// In an assembly that already has a part, Place puts nothing down until the first click.
#[test]
fn a_part_into_a_started_assembly_waits_for_the_click() {
    for mut h in both() {
        h.app.run("component.create", json!({"name": "Base"})).unwrap();
        h.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5})).unwrap();
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        place(&mut h);
        assert_eq!(roots(&h).len(), 1, "only Base: the part waits for a click");
        assert!(super::live(&h.app));
        let (g, o) = click_at(&mut h, [60.0, 40.0, 0.0]);
        assert!(near(&o.transform, &g, 1e-9) && !o.grounded, "{:?}", o.transform);
        let (g, o) = click_at(&mut h, [-50.0, 30.0, 0.0]);
        assert!(near(&o.transform, &g, 1e-9) && !o.grounded, "the second copy too: {:?}", o.transform);
    }
}

/// The control channel starts Place on a file, past the picker (agents, screenshots).
#[test]
fn the_control_channel_places_a_named_file() {
    for mut h in both() {
        let path = h.dir.join("part1.solidcraft").to_string_lossy().into_owned();
        let r = h.call("ui.start", json!({"command": "component.insert_design", "params": {"path": path}}));
        assert_eq!(r["result"]["placing"], json!(true), "{r}");
        h.settle();
        assert_eq!(roots(&h).len(), 1, "the empty assembly's first part, grounded at once");
        assert!(super::live(&h.app));
        let r = h.call("ui.start", json!({"command": "component.insert_design", "params": {}}));
        assert_eq!(r["ok"], json!(false), "{r}");
    }
}

/// R184: a copy comes in the part's own orientation at the click, though the first occurrence
/// (the one `reuse` copies) has been turned a half round (as constraints turn a part).
#[test]
fn a_copy_comes_in_unrotated_though_the_source_is_turned() {
    for mut h in both() {
        place(&mut h);
        let first = roots(&h)[0].id;
        let turned = solidcraft_engine::doc::rigid(Vec3::new(10.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, std::f64::consts::PI);
        h.app.session.doc_mut().occurrences.iter_mut().find(|o| o.id == first).unwrap().transform = turned;
        h.app.session.refresh();
        h.frames(2);
        let (g, o) = click_at(&mut h, [60.0, 40.0, 0.0]);
        assert!(near(&o.transform, &g, 1e-9), "{:?}", o.transform);
        let rotation_free = (0..3).all(|r| (0..3).all(|c| (o.transform[r][c] - IDENTITY[r][c]).abs() <= 1e-12));
        assert!(rotation_free, "the part's own orientation: {:?}", o.transform);
    }
}
