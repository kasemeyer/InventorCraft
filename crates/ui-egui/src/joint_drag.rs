//! Dragging a part that joints place moves it as far as its joints allow: each frame runs
//! `joint.reach`, taking the point grabbed toward the pointer in the plane through it that faces
//! the view. The whole drag is one undo step. Constraints count as joints here (R163): a Mated
//! part slides in its plane, an Inserted one turns on its axis.

use egui::Pos2;
use serde_json::json;
use solidcraft_engine::doc::{apply_point, joints, mat_inverse};
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::viewport::{Hit, Proj};

/// A drag under way.
#[derive(Clone, Debug)]
pub struct Drag {
    occurrence: u64,
    /// The point grabbed, in the occurrence's own coordinates.
    local: Vec3,
    /// The plane the pointer moves the point in: through where it was grabbed, facing the view.
    origin: Vec3,
    normal: Vec3,
    /// The drag's undo step is on top of the history (it went in with the first frame that moved).
    pushed: bool,
}

/// Begin a drag on the part under `p` when joints place it and leave it free to move (it is
/// neither grounded nor fixed to what is). True when one began.
pub fn start(app: &mut SolveApp, proj: &Proj, p: Pos2) -> bool {
    // The face under the pointer, past the part's own edges and vertices (not past sketches or
    // construction geometry in front of it).
    let hits = crate::viewport::pick(app, proj, p);
    let Some((body, at)) =
        hits.into_iter().take_while(|h| matches!(h, Hit::Face { .. } | Hit::Edge { .. } | Hit::Vertex { .. })).find_map(|h| match h {
            Hit::Face { body, point, .. } => Some((body, point)),
            _ => None,
        })
    else {
        return false;
    };
    let s = &app.session;
    let Some(occurrence) = crate::dialogs_assembly::occurrence_of_body(s, &body).filter(|o| *o != 0) else { return false };
    // Saved joints and constraints alike (a constraint compiles to joints in every solve, R163).
    let joined = s.doc.assembly.joints.iter().any(|j| !j.suppressed && (j.a.occurrence == occurrence || j.b.occurrence == occurrence));
    let constrained = s.doc.assembly.constraints.iter().any(|c| !c.suppressed && c.picks.iter().any(|p| p.occurrence == occurrence));
    if !joined && !constrained {
        return false;
    }
    let sol = joints::solve(&s.doc, &s.model.state());
    if !sol.world.contains_key(&occurrence) || sol.dof.get(&occurrence).is_none_or(|d| *d == 0) {
        return false;
    }
    let Some(local) = mat_inverse(&joints::occurrence_world(&s.doc, occurrence)).map(|inv| apply_point(&inv, at)) else { return false };
    let normal = proj.ray(p).1;
    app.viewport.joint_drag = Some(Drag { occurrence, local, origin: at, normal, pushed: false });
    true
}

/// A press on a part that neither drag takes (grounded, or held fully by its joints or
/// constraints): the status bar says what holds it (R163). Nothing for a press elsewhere.
pub fn say_refused(app: &mut SolveApp, proj: &Proj, p: Pos2) {
    let hits = crate::viewport::pick(app, proj, p);
    let Some(body) = hits.into_iter().take_while(|h| matches!(h, Hit::Face { .. } | Hit::Edge { .. } | Hit::Vertex { .. })).find_map(|h| match h {
        Hit::Face { body, .. } => Some(body),
        _ => None,
    }) else {
        return;
    };
    let Some(path) = crate::occurrence_drag::placement_of(app, &body) else { return };
    if let Err(e) = app.session.drag_group(&path) {
        app.set_status(e.to_string(), true);
    }
}

pub fn active(app: &SolveApp) -> bool {
    app.viewport.joint_drag.is_some()
}

/// Reach for the point under `p`.
pub fn to(app: &mut SolveApp, proj: &Proj, p: Pos2) {
    let Some(d) = app.viewport.joint_drag.clone() else { return };
    let (o, dir) = proj.ray(p);
    let facing = dir.dot(d.normal);
    if facing.abs() < 1e-9 {
        return;
    }
    let target = o + dir * ((d.origin - o).dot(d.normal) / facing);
    if !target.is_finite() {
        return;
    }
    let at = apply_point(&joints::occurrence_world(&app.session.doc, d.occurrence), d.local);
    let params =
        json!({"occurrence": d.occurrence, "at": [at.x, at.y, at.z], "point": [target.x, target.y, target.z], "quiet": true, "continuous": true});
    // One undo step for the whole drag: the first frame's, kept on top while the others run (not
    // truncated by depth, which a full history would make lose its oldest steps frame by frame).
    let own = if d.pushed { app.session.undo.pop() } else { None };
    // A frame that changes the design adds an undo step (a full history drops its oldest for it,
    // so its length says nothing).
    let before = app.session.doc.clone();
    let pushed = app.run("joint.reach", params).is_ok() && !std::sync::Arc::ptr_eq(&before, &app.session.doc);
    match own {
        Some(step) => {
            if pushed {
                app.session.undo.pop();
            }
            app.session.undo.push(step);
        }
        None if pushed => {
            if let Some(d) = app.viewport.joint_drag.as_mut() {
                d.pushed = true;
            }
        }
        None => {}
    }
}

pub fn end(app: &mut SolveApp) {
    app.viewport.joint_drag = None;
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use solidcraft_engine::doc::{apply_point, joints};
    use solidcraft_engine::geom::Vec3;

    use crate::scenario::Harness;
    use crate::test_support::play;

    fn run(h: &mut Harness, id: &str, p: Value) -> Value {
        h.app.run(id, p.clone()).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    fn screen(h: &mut Harness, p: Vec3) -> egui::Pos2 {
        let [x, y] = h.at(&json!({"world": [p.x, p.y, p.z]})).expect("on screen");
        egui::pos2(x as f32, y as f32)
    }

    /// Where the arm's point `local` (its own coordinates) is.
    fn arm_at(h: &Harness, local: Vec3) -> Vec3 {
        let s = &h.app.session;
        let c = s.doc.find_component("Arm").unwrap_or(0);
        let o = s.doc.occurrence_of(c).map(|o| o.id).unwrap_or(0);
        apply_point(&joints::occurrence_world(&s.doc, o), local)
    }

    /// An arm hinged about +Z on a grounded base (seen from the top): its far end, and the screen
    /// points of that end and of the end turned a quarter round.
    fn hinged_arm() -> (Harness, Vec3, egui::Pos2, egui::Pos2) {
        let mut h = Harness::new();
        h.app.home.open = false;
        for (name, corner, size) in [("Base", [-5, -5, 0], [10, 10, 5]), ("Arm", [-3, -3, 5], [63, 6, 5])] {
            run(&mut h, "component.activate", json!({"component": "root"}));
            run(&mut h, "component.create", json!({"name": name}));
            run(&mut h, "solid.box", json!({"length": size[0], "width": size[1], "height": size[2], "corner": corner}));
        }
        run(&mut h, "component.activate", json!({"component": "root"}));
        let base = h.app.session.doc.find_component("Base").and_then(|c| h.app.session.doc.occurrence_of(c)).map(|o| o.id).unwrap_or(0);
        run(&mut h, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
        let at = |o: &str| json!({"occurrence": o, "point": [0, 0, 5], "z": [0, 0, 1]});
        run(&mut h, "joint.create", json!({"type": "revolute", "a": at("Base"), "b": at("Arm"), "flip": true, "name": "Hinge"}));
        h.call("ui.view", json!({"view": "top"}));
        h.app.cam.distance = 200.0;
        h.frames(3);
        let end = Vec3::new(55.0, 0.0, 10.0);
        let (a, b) = (screen(&mut h, end), screen(&mut h, Vec3::new(0.0, 55.0, 10.0)));
        (h, end, a, b)
    }

    /// Press at `a` and move to `b` in 12 frames, holding the button there.
    fn press_and_move(h: &mut Harness, a: egui::Pos2, b: egui::Pos2) {
        h.app.synthetic.push(egui::Event::PointerMoved(a));
        h.app.synthetic.push(egui::Event::PointerButton {
            pos: a,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.app.synthetic.extend((1..=12).map(|k| egui::Event::PointerMoved(a + (b - a) * (k as f32 / 12.0))));
        play(h);
    }

    fn release(h: &mut Harness, b: egui::Pos2) {
        h.app.synthetic.push(egui::Event::PointerButton {
            pos: b,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        play(h);
    }

    /// Dragged by its far end a quarter turn round, the arm turns there in one undo step, and undo
    /// turns it back.
    #[test]
    fn dragging_a_jointed_part_turns_it_about_its_hinge() {
        let (mut h, end, a, b) = hinged_arm();
        let undo = h.app.session.undo.len();
        press_and_move(&mut h, a, b);
        assert!(h.app.viewport.joint_drag.is_some() && h.app.viewport.boxsel.is_none(), "the drag moves the arm, not a selection box");
        release(&mut h, b);
        assert!(h.app.viewport.joint_drag.is_none());
        assert!(arm_at(&h, end).dist(Vec3::new(0.0, 55.0, 10.0)) < 0.5, "{:?}", arm_at(&h, end));
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step for the drag");
        assert!(!h.app.session.log.iter().any(|l| l.contains("Reach")), "frames are not echoed: {:?}", h.app.session.log);
        run(&mut h, "edit.undo", json!({}));
        assert!(arm_at(&h, end).dist(end) < 1e-9, "{:?}", arm_at(&h, end));
    }

    /// #5137: while a part is dragged along its joint, nothing under the pointer highlights; hover
    /// comes back once the drag ends.
    #[test]
    fn dragging_a_jointed_part_highlights_nothing_under_the_pointer() {
        let (mut h, _, a, b) = hinged_arm();
        h.app.synthetic.push(egui::Event::PointerMoved(a));
        play(&mut h);
        assert!(h.app.viewport.hover.is_some(), "the arm under the pointer must highlight before the drag");
        press_and_move(&mut h, a, b);
        assert!(h.app.viewport.joint_drag.is_some(), "the drag must move the arm");
        assert!(h.app.viewport.hover.is_none(), "dragging a part must not highlight what is under the pointer");
        h.frames(2);
        assert!(h.app.viewport.hover.is_none(), "a drag held still must not highlight either");
        release(&mut h, b);
        h.frames(2);
        assert!(h.app.viewport.hover.is_some(), "hover must return after the drag");
    }

    /// With the undo history full, a drag still adds one step: it drops the oldest once, not one
    /// per frame.
    #[test]
    fn a_drag_on_a_full_undo_history_drops_one_step() {
        let (mut h, _, a, b) = hinged_arm();
        let step = h.app.session.undo.last().cloned().expect("an undo step");
        while h.app.session.undo.len() < 200 {
            let mut older = step.clone();
            older.label = format!("step {}", h.app.session.undo.len());
            h.app.session.undo.insert(0, older);
        }
        let second = h.app.session.undo.get(1).map(|s| s.label.clone());
        press_and_move(&mut h, a, b);
        release(&mut h, b);
        assert_eq!(h.app.session.undo.len(), 200);
        assert_eq!(h.app.session.undo.first().map(|s| s.label.clone()), second, "only the oldest step went");
        assert_eq!(h.app.session.undo.last().map(|s| s.label.as_str()), Some("Reach Point"), "the drag's step is on top");
    }

    // --- R163: parts held by constraints drag within their remaining freedom (wave 1 item 1) ---

    use crate::test_support::{constraint_blocks, inventor, occ, world_at};
    use solidcraft_engine::doc::DocumentKind::Assembly;

    /// E-1's blocks on each profile: SolidCraft (`Harness::new()`) and Inventor.
    fn blocks_both() -> Vec<Harness> {
        [Harness::new(), inventor(Assembly)]
            .into_iter()
            .map(|mut h| {
                h.app.home.open = false;
                constraint_blocks(&mut h);
                h.frames(2);
                h
            })
            .collect()
    }

    fn face(h: &Harness, name: &str, p: [f64; 3]) -> Value {
        json!({"occurrence": name, "face": world_at(h, name, p)})
    }

    fn world(h: &Harness, name: &str) -> solidcraft_engine::doc::Mat {
        joints::occurrence_world(&h.app.session.doc, occ(h, name))
    }

    /// The point a press at `a` grabs, and where the pointer at `b` takes it: on the plane through
    /// it facing the view (as a drag does).
    fn dropped(h: &Harness, a: egui::Pos2, b: egui::Pos2) -> (Vec3, Vec3) {
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let origin = crate::viewport::pick(&h.app, &proj, a)
            .into_iter()
            .find_map(|hit| match hit {
                crate::viewport::Hit::Face { point, .. } => Some(point),
                _ => None,
            })
            .expect("a face under the press");
        let normal = proj.ray(a).1;
        let (o, dir) = proj.ray(b);
        (origin, o + dir * ((origin - o).dot(normal) / dir.dot(normal)))
    }

    /// Drag from `a` to `b` and let go.
    fn drag(h: &mut Harness, a: egui::Pos2, b: egui::Pos2) {
        press_and_move(h, a, b);
        release(h, b);
        h.frames(2);
    }

    /// A free, unconstrained part keeps its free drag: the grabbed point lands where it is dropped.
    #[test]
    fn r163_a_free_part_moves_to_the_dropped_point() {
        for mut h in blocks_both() {
            let grab = Vec3::new(160.0, 30.0, 10.0);
            let a = screen(&mut h, grab);
            let b = a + egui::vec2(60.0, 25.0);
            let before = world(&h, "Plate");
            let (grabbed, want) = dropped(&h, a, b);
            drag(&mut h, a, b);
            let local = apply_point(&solidcraft_engine::doc::mat_inverse(&before).unwrap(), grabbed);
            let got = apply_point(&world(&h, "Plate"), local);
            assert!(got.dist(want) < 1e-6, "{got:?} != {want:?}");
        }
    }

    /// A Mated part slides in the mate's plane: its height (the plane's normal) is unchanged.
    #[test]
    fn r163_a_mated_part_slides_only_in_plane() {
        for mut h in blocks_both() {
            let p = json!({"type": "mate", "a": face(&h, "A", [50.0, 25.0, 20.0]), "b": face(&h, "B", [50.0, 95.0, 7.0])});
            run(&mut h, "constraint.create", p);
            h.frames(2);
            let before = world(&h, "B");
            let grab = apply_point(&before, Vec3::new(50.0, 95.0, 27.0));
            let a = screen(&mut h, grab);
            let b = a + egui::vec2(-80.0, 40.0);
            press_and_move(&mut h, a, b);
            assert!(h.app.viewport.joint_drag.is_some(), "the drag reaches through the Mate");
            release(&mut h, b);
            let after = world(&h, "B");
            assert!((after[3][2] - before[3][2]).abs() < 1e-9, "the mate's normal component is unchanged: {before:?} -> {after:?}");
            let moved = Vec3::new(after[3][0] - before[3][0], after[3][1] - before[3][1], 0.0);
            assert!(moved.len() > 5.0, "B slid in the plane: {moved:?}");
            let report = run(&mut h, "constraint.list", json!({}));
            assert!(report.to_string().contains("\"ok\""), "the Mate still holds: {report}");
        }
    }

    /// An Inserted pin only turns on (and, with an Insert, keeps) its axis.
    #[test]
    fn r163_an_inserted_pin_turns_only_on_its_axis() {
        for mut h in blocks_both() {
            let plate = occ(&h, "Plate");
            run(&mut h, "occurrence.ground", json!({"occurrence": plate, "grounded": true}));
            let p = json!({"type": "insert", "a": {"occurrence": "Plate", "edge": [200.0, 0.0, 10.0]}, "b": {"occurrence": "Pin", "edge": [310.0, 0.0, 50.0]}});
            run(&mut h, "constraint.create", p);
            h.frames(2);
            let axis_point = |h: &Harness| apply_point(&world(h, "Pin"), Vec3::new(300.0, 0.0, 50.0));
            let axis_dir = |h: &Harness| solidcraft_engine::doc::apply_vector(&world(h, "Pin"), Vec3::Z);
            let (p0, d0) = (axis_point(&h), axis_dir(&h));
            let before = world(&h, "Pin");
            let grab = apply_point(&before, Vec3::new(310.0, 0.0, 80.0));
            let a = screen(&mut h, grab);
            let b = a + egui::vec2(30.0, 60.0);
            press_and_move(&mut h, a, b);
            assert!(h.app.viewport.joint_drag.is_some(), "the drag reaches through the Insert");
            release(&mut h, b);
            assert!(axis_point(&h).dist(p0) < 1e-9 && axis_dir(&h).dist(d0) < 1e-9, "the axis stays: {:?} {:?}", axis_point(&h), axis_dir(&h));
            assert!(world(&h, "Pin") != before, "the pin turned");
        }
    }

    /// A grounded part does not move; the status bar names what holds it, as it does for a part
    /// its constraints hold fully.
    #[test]
    fn r163_a_grounded_or_fully_held_part_stays_and_says_why() {
        for mut h in blocks_both() {
            let before = world(&h, "A");
            let a = screen(&mut h, Vec3::new(50.0, 25.0, 20.0));
            drag(&mut h, a, a + egui::vec2(60.0, 30.0));
            assert_eq!(world(&h, "A"), before, "a grounded part stays");
            let status = h.app.status.clone().map(|s| s.0).unwrap_or_default();
            assert!(status.contains("A:1") && status.contains("grounded"), "{status}");
            // B fully held: Mate top/bottom, Flush front, Flush right (each picked where B is).
            type Pick = fn(&Harness) -> Value;
            let picks: [Pick; 3] = [
                |h| json!({"type": "mate", "a": face(h, "A", [50.0, 25.0, 20.0]), "b": face(h, "B", [50.0, 95.0, 7.0])}),
                |h| json!({"type": "mate", "solution": "flush", "a": face(h, "A", [50.0, 0.0, 10.0]), "b": face(h, "B", [50.0, 70.0, 17.0])}),
                |h| json!({"type": "mate", "solution": "flush", "a": face(h, "A", [100.0, 25.0, 10.0]), "b": face(h, "B", [100.0, 95.0, 17.0])}),
            ];
            for pick in picks {
                let p = pick(&h);
                run(&mut h, "constraint.create", p);
                h.frames(2);
            }
            let held = world(&h, "B");
            let a = screen(&mut h, apply_point(&held, Vec3::new(50.0, 95.0, 27.0)));
            drag(&mut h, a, a + egui::vec2(60.0, 30.0));
            assert_eq!(world(&h, "B"), held, "a fully held part stays");
            let status = h.app.status.clone().map(|s| s.0).unwrap_or_default();
            assert!(status.contains("B:1") && status.contains("Mate:1"), "the status names what holds it: {status}");
        }
    }
}
