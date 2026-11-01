#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The profile's marking menu: rings and lists read from the Inventor capture, Done / OK /
//! Cancel / Repeat inside commands, and SolidCraft's menus as they were.

use super::*;
use egui::pos2;
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{DocumentKind, FeatureKind};

use crate::context_menu::{Target, describe, open, open_for, pick};
use crate::scenario::Harness;
use crate::test_support::inventor;

/// The Inventor profile, a document of `kind`, view changes at once, and a 20 mm block.
fn boxed(kind: DocumentKind) -> Harness {
    let mut h = inventor(kind);
    h.app.preferences.look_at_anim = false;
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length":20,"width":20,"height":20,"body_name":"Block"})).unwrap();
    h.frames(2);
    h
}

fn layout_of(h: &Harness, ctx: MenuContext) -> &'static MenuLayout {
    &layout(&h.app).expect("the Inventor profile has marking menus")[&ctx]
}

fn face_ring(h: &Harness) -> Vec<String> {
    layout_of(h, MenuContext::Face).ring.clone()
}

/// A layout's ids as the menu shows them: an empty slot is a separator.
fn shown(ids: &[String]) -> Vec<String> {
    ids.iter().map(|id| if id.is_empty() { "-".to_string() } else { id.clone() }).collect()
}

fn ids(menu: &Value, key: &str) -> Vec<String> {
    menu[key].as_array().unwrap_or_else(|| panic!("no {key} in {menu}")).iter().map(|i| i["id"].as_str().unwrap().to_string()).collect()
}

fn entry<'a>(menu: &'a Value, key: &str, id: &str) -> &'a Value {
    menu[key].as_array().unwrap().iter().find(|i| i["id"] == id).unwrap_or_else(|| panic!("no {id} in {key}: {menu}"))
}

fn select_top_face(h: &mut Harness) {
    let state = h.app.session.world_state();
    let face = state
        .body("Block")
        .unwrap()
        .body
        .faces(0.001)
        .unwrap()
        .into_iter()
        .find(|face| face.plane_normal.is_some_and(|normal| normal.z > 0.99))
        .expect("the block has a +Z face");
    let sel = Sel::Face { body: "Block".into(), index: face.index, point: face.centroid };
    h.app.run("select.set", json!({"items": [sel]})).unwrap();
}

fn open_on_top_face(h: &mut Harness) -> Value {
    select_top_face(h);
    open(&mut h.app, pos2(400.0, 300.0));
    describe(&h.app)
}

fn lines_in_sketch(h: &Harness) -> usize {
    let sid = h.app.session.active_sketch.expect("a sketch is open");
    let state = h.app.session.world_state();
    state.sketch(sid).unwrap().sketch.curves.iter().filter(|c| matches!(c.kind, solidcraft_engine::sketch::CurveKind::Line { .. })).count()
}

/// A point on screen below the model, where the view shows nothing.
fn empty_space(h: &mut Harness) -> Value {
    let [x, y] = h.at(&json!({"world":[0,0,0]})).unwrap();
    json!([x, y + 160.0])
}

fn extrudes(h: &Harness) -> usize {
    h.app.session.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::Extrude { .. })).count()
}

#[test]
fn inventor_face_menu_follows_the_data() {
    let mut h = boxed(DocumentKind::Part);
    let menu = open_on_top_face(&mut h);
    assert_eq!(context(&h.app), MenuContext::Face);
    assert_eq!(ids(&menu, "radial"), shown(&face_ring(&h)));
    assert_eq!(ids(&menu, "radial")[0], "ui.lookAt", "Ruling 1: the ring item keeps the id ui.lookAt");
    assert_eq!(ids(&menu, "items"), layout_of(&h, MenuContext::Face).overflow);
    assert_eq!(entry(&menu, "items", "ui.repeat")["enabled"], json!(false), "no command has run yet");
    assert_eq!(entry(&menu, "items", "ui.home")["shortcut"], json!("F6"));
    assert_eq!(entry(&menu, "items", "ui.previousView")["shortcut"], json!("F5"));
}

#[test]
fn look_at_on_a_face_turns_the_view_to_it() {
    let mut h = boxed(DocumentKind::Part);
    open_on_top_face(&mut h);
    assert!(h.app.cam.back().z < 0.999, "the view starts off the face");
    pick(&mut h.app, "ui.lookAt").unwrap();
    h.frames(2);
    assert!(h.app.cam.back().z > 0.999, "the +Z face faces the viewer: {:?}", h.app.cam.back());
}

#[test]
fn right_click_with_a_line_tool_opens_the_tool_menu() {
    let mut h = inventor(DocumentKind::Part);
    h.app.preferences.look_at_anim = false;
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.start("sketch.line");
    h.frames(2);
    h.step(&json!({"click":{"world":[0,0,0]}})).unwrap();
    h.step(&json!({"click":{"world":[30,0,0]}})).unwrap();
    h.step(&json!({"click":{"world":[30,20,0]},"button":"right"})).unwrap();
    assert!(h.app.tool.is_some(), "a right-click does not end the tool");
    assert!(h.app.viewport.context_menu.is_some(), "it opens the marking menu");
    let menu = describe(&h.app);
    let ring = ids(&menu, "radial");
    // The capture: OK at E, Cancel [ESC] at W, Finish 2D Sketch at S.
    assert_eq!(ring, shown(&layout_of(&h, MenuContext::Tool).ring));
    assert_eq!((ring[2].as_str(), ring[4].as_str(), ring[6].as_str()), ("ui.done", "ui.finishSketch", "ui.cancel"));
    assert_eq!(entry(&menu, "radial", "ui.done")["label"], json!("OK"));
    pick(&mut h.app, "ui.done").unwrap();
    assert!(h.app.tool.is_none(), "OK ends the tool");
    assert!(h.app.session.active_sketch.is_some(), "and keeps the sketch");
    assert_eq!(lines_in_sketch(&h), 1);
}

#[test]
fn right_click_with_a_panel_open_offers_ok_and_continue() {
    let mut h = inventor(DocumentKind::Part);
    h.app.preferences.look_at_anim = false;
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[60,40]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.start("solid.extrude");
    h.frames(2);
    let empty = empty_space(&mut h);
    h.step(&json!({"click":empty,"button":"right"})).unwrap();
    assert!(h.app.dialog.is_some(), "the right-click keeps the panel");
    let menu = describe(&h.app);
    // The capture: Apply (+) at N, Continue (unmapped) at E, Home View at S, Cancel at W; OK in the list.
    assert_eq!(ids(&menu, "radial"), ["ui.applyContinue", "-", "-", "-", "ui.home", "-", "ui.cancel", "-"]);
    assert_eq!(entry(&menu, "radial", "ui.applyContinue")["enabled"], json!(true), "the profile is picked: + can apply");
    assert_eq!(ids(&menu, "items")[0], "ui.ok");
    pick(&mut h.app, "ui.ok").unwrap();
    h.frames(3);
    assert_eq!(extrudes(&h), 1);
    assert!(h.app.dialog.is_none(), "OK closed the panel");
}

#[test]
fn the_panel_ring_applies_and_continues_and_cancels() {
    let mut h = inventor(DocumentKind::Part);
    h.app.preferences.look_at_anim = false;
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[60,40]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.start("solid.extrude");
    h.frames(2);
    open(&mut h.app, pos2(400.0, 300.0));
    pick(&mut h.app, "ui.applyContinue").unwrap();
    h.frames(3);
    assert_eq!(extrudes(&h), 1);
    assert!(h.app.dialog.as_ref().is_some_and(|d| d.inputs[0].items.is_empty()), "+ keeps the panel for the next one");
    open(&mut h.app, pos2(400.0, 300.0));
    assert_eq!(entry(&describe(&h.app), "radial", "ui.applyContinue")["enabled"], json!(false), "nothing picked: + cannot apply");
    pick(&mut h.app, "ui.cancel").unwrap();
    h.frames(2);
    assert!(h.app.dialog.is_none(), "Cancel closed the panel");
    assert_eq!(extrudes(&h), 1, "and kept what was applied");
}

#[test]
fn repeat_restarts_the_last_command() {
    let mut h = inventor(DocumentKind::Part);
    h.app.home.open = false;
    h.app.start("solid.extrude");
    h.frames(2);
    crate::dialogs::cancel(&mut h.app);
    h.frames(2);
    let empty = empty_space(&mut h);
    h.step(&json!({"click":empty,"button":"right"})).unwrap();
    assert_eq!(context(&h.app), MenuContext::Empty);
    let menu = describe(&h.app);
    let first = &menu["items"][0];
    assert_eq!((first["id"].as_str(), first["label"].as_str()), (Some("ui.repeat"), Some("Repeat Extrude")));
    assert_eq!(first["shortcut"], json!(""), "the marking menu's Repeat has no key");
    pick(&mut h.app, "ui.repeat").unwrap();
    h.frames(2);
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Extrude { .. })));
}

#[test]
fn the_sketch_menu_finishes_the_sketch() {
    let mut h = inventor(DocumentKind::Part);
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.frames(2);
    open(&mut h.app, pos2(400.0, 300.0));
    assert_eq!(context(&h.app), MenuContext::Sketch);
    let ring = ids(&describe(&h.app), "radial");
    // The capture: Create Line at N, Finish 2D Sketch at S.
    assert_eq!((ring[0].as_str(), ring[4].as_str()), ("sketch.line", "ui.finishSketch"));
    pick(&mut h.app, "ui.finishSketch").unwrap();
    assert!(h.app.session.active_sketch.is_none());
}

#[test]
fn a_selected_sketch_line_gets_its_own_ring() {
    let mut h = inventor(DocumentKind::Part);
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let line =
        h.app.run("sketch.line", json!({"points":[[0,0],[20,0]],"connect":false,"infer":false})).unwrap()["curves"][0].as_str().unwrap().to_owned();
    h.app.session.selection = vec![Sel::SketchCurve { id: line.clone() }];
    open_for(&mut h.app, pos2(400.0, 300.0), Target::Viewport);
    assert_eq!(context(&h.app), MenuContext::SketchLine);
    let menu = describe(&h.app);
    assert_eq!(ids(&menu, "radial"), ["ui.delete", "-", "sketch.construction", "-", "sketch.centerline", "-", "edit.undo", "-"]);
    pick(&mut h.app, "sketch.construction").unwrap();
    let sid = h.app.session.active_sketch.unwrap();
    let state = h.app.session.world_state();
    let sketch = &state.sketch(sid).unwrap().sketch;
    assert!(sketch.curves[sketch.curve_index(&line).unwrap()].construction, "Construction acts on the selected line");
}

#[test]
fn solidcraft_right_click_with_a_tool_still_ends_it() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.start("sketch.line");
    h.frames(2);
    h.step(&json!({"click":{"world":[0,0,0]}})).unwrap();
    assert!(h.app.tool.as_ref().is_some_and(|t| !t.pts.is_empty()), "the line is under way");
    h.step(&json!({"click":{"world":[30,20,0]},"button":"right"})).unwrap();
    assert!(h.app.viewport.context_menu.is_none(), "no menu inside a tool under SolidCraft");
    assert!(h.app.tool.as_ref().is_none_or(|t| t.pts.is_empty()), "the chain ended");
    // A second right-click ends the tool; the next one opens the list menu, with no ring.
    for _ in 0..2 {
        if h.app.viewport.context_menu.is_none() {
            h.step(&json!({"click":{"world":[30,20,0]},"button":"right"})).unwrap();
        }
    }
    assert!(h.app.tool.is_none());
    assert!(h.app.viewport.context_menu.is_some());
    assert_eq!(describe(&h.app)["radial"], json!([]));
    assert!(layout(&h.app).is_none() && ring(&h.app).is_none() && overflow(&h.app).is_none());
}

#[test]
fn browser_right_click_stays_a_list_under_inventor() {
    let mut h = inventor(DocumentKind::Part);
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
    let id = h.app.session.active_sketch.unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    open_for(&mut h.app, pos2(10.0, 10.0), Target::Sketch { id });
    assert!(!h.app.menu.radial, "browser rows keep their list menus");
    let want: Vec<String> = crate::context_menu::items(&h.app, &Target::Sketch { id }).into_iter().map(|i| i.id).collect();
    assert!(want.contains(&"ui.editSketch".to_string()));
    assert_eq!(ids(&describe(&h.app), "items"), want);
}

#[test]
fn the_ring_is_not_cleared_by_the_hold_tracker() {
    let mut h = inventor(DocumentKind::Part);
    h.app.home.open = false;
    assert!(!h.app.preferences.radial_context_menu, "the upstream opt-in stays off");
    open(&mut h.app, pos2(400.0, 300.0));
    h.frames(2);
    assert!(h.app.menu.radial);
    assert_eq!(describe(&h.app)["radial"].as_array().unwrap().len(), 8);
}

#[test]
fn long_assembly_lists_never_cover_the_quick_bubbles() {
    let mut h = inventor(DocumentKind::Assembly);
    h.app.home.open = false;
    let part = h.app.run("component.create", json!({"name":"Block"})).unwrap();
    h.app.run("solid.box", json!({"length":20,"width":20,"height":20})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    h.call("ui.resize", json!({"width":1000,"height":650}));
    for y in [250.0, 400.0, 580.0] {
        h.app.session.selection = vec![Sel::Occurrence { path: vec![part["occurrence"].as_u64().unwrap()] }];
        open_for(&mut h.app, pos2(600.0, y), Target::Viewport);
        h.frames(4);
        let pills = h.ctx.memory(|m| m.area_rect(egui::Id::new("sc_marking_menu"))).unwrap();
        let list = h.ctx.memory(|m| m.area_rect(egui::Id::new("sc_context_menu"))).unwrap();
        assert!(!pills.intersects(list), "the long list hides the bubbles at {y}: {pills:?}, {list:?}");
        if y == 250.0 {
            assert!(list.top() >= pills.bottom(), "keep the bubbles above a scrolling list when there is room below");
        }
        assert!(h.ctx.content_rect().contains_rect(pills));
        assert!(h.ctx.content_rect().contains_rect(list));
        assert!(crate::scenario::handle_at("marking:constraint.create").is_some(), "the quick actions remain drawn");
        let open = ring(&h.app).unwrap().into_iter().find(|item| item.id == "ui.openComponent").unwrap();
        assert_eq!(open.label, "Open", "a disabled action still has a user-facing label");
        crate::context_menu::close(&mut h.app);
        h.frames(2);
    }
}

#[test]
fn editing_a_part_inside_an_assembly_uses_the_part_quick_bubbles() {
    let mut h = inventor(DocumentKind::Assembly);
    h.app.home.open = false;
    h.app.run("component.create", json!({"name":"Block"})).unwrap();
    h.app.run("solid.box", json!({"length":20,"width":20,"height":20})).unwrap();
    h.app.session.selection.clear();
    open_for(&mut h.app, pos2(600.0, 350.0), Target::Viewport);
    h.frames(3);
    assert_eq!(context(&h.app), MenuContext::Empty);
    assert!(crate::scenario::handle_at("marking:solid.extrude").is_some());
    assert!(crate::scenario::handle_at("marking:sketch.create").is_some());
}

#[test]
fn marking_menus_have_no_hold_or_flick() {
    let mut h = inventor(DocumentKind::Part);
    h.app.home.open = false;
    h.app.preferences.radial_context_menu = true;
    h.app.menu.press = Some((pos2(400.0, 300.0), 0.0));
    h.frames(2);
    assert!(h.app.menu.press.is_none());
    assert!(!h.app.menu.flick);
    assert!(h.app.viewport.context_menu.is_none());
}

#[test]
fn empty_slots_are_separators() {
    let mut h = inventor(DocumentKind::Part);
    h.app.home.open = false;
    h.app.start("solid.extrude");
    h.frames(2);
    // The panel ring has empty slots; the part's empty-space ring has none.
    for ctx in [MenuContext::Panel, MenuContext::Empty] {
        if ctx == MenuContext::Empty {
            crate::dialogs::cancel(&mut h.app);
        }
        assert_eq!(context(&h.app), ctx);
        let slots = &layout_of(&h, ctx).ring;
        let ring = ring(&h.app).unwrap();
        assert_eq!(ring.len(), 8);
        for (slot, item) in slots.iter().zip(&ring) {
            assert_eq!(slot.is_empty(), item.id == "-", "{ctx} slot {slot:?} drew {}", item.id);
        }
    }
    assert!(layout_of(&h, MenuContext::Panel).ring.iter().any(String::is_empty));
}

#[test]
fn every_captured_layout_says_what_it_left_out() {
    let h = inventor(DocumentKind::Part);
    let layouts = layout(&h.app).unwrap();
    // The panel ring's Continue has no TCI-CAD counterpart: recorded, and its slot stays empty.
    assert!(layouts[&MenuContext::Panel].unmapped.iter().any(|u| u == "E: Continue"));
    assert_eq!(layouts[&MenuContext::Panel].ring[2], "");
    for (ctx, l) in layouts {
        assert!(!l.unmapped.is_empty(), "{ctx}: the capture always shows something TCI-CAD lacks (Help Topics, How To…)");
    }
}

#[test]
fn assembly_contexts_follow_the_selection() {
    let mut h = inventor(DocumentKind::Assembly);
    h.app.home.open = false;
    assert_eq!(context(&h.app), MenuContext::AssemblyEmpty);
    let ring: Vec<String> = ring(&h.app).unwrap().into_iter().map(|i| i.id).collect();
    assert_eq!(ring[0], "component.insert_design", "Place Component at N");
    h.app.run("solid.box", json!({"length":20,"width":20,"height":20,"body_name":"Block"})).unwrap();
    h.app.session.selection = vec![Sel::Body { name: "Block".into() }];
    assert_eq!(context(&h.app), MenuContext::AssemblyComponent);
}

#[test]
fn pills_sit_where_the_capture_puts_them() {
    // Capture pixels at 175 % scaling: N and S centred above and below, E and W 78 px out.
    assert_eq!(PILL_OFFSETS[0], egui::vec2(0.0, -102.0 / 1.75));
    assert_eq!(PILL_OFFSETS[2], egui::vec2(78.0 / 1.75, 0.0));
    assert_eq!(PILL_OFFSETS[6], egui::vec2(-78.0 / 1.75, 0.0));
    for k in 1..4 {
        assert_eq!(PILL_OFFSETS[k].x, -PILL_OFFSETS[8 - k].x, "slot {k} mirrors slot {}", 8 - k);
    }
    assert_eq!(LIST_DROP, 137.0 / 1.75);
}

#[test]
fn the_ring_is_drawn_as_pills_and_a_click_runs_one() {
    let mut h = boxed(DocumentKind::Part);
    open_on_top_face(&mut h);
    h.frames(2);
    let at = h.app.viewport.context_menu.expect("the menu is open");
    let pill = |id: &str| crate::scenario::rect_of(&format!("marking:{id}")).unwrap_or_else(|| panic!("{id} is not drawn"));
    // N is centred above the cursor; W ends at its offset, E starts at its offset.
    let (n, e, w) = (pill("ui.lookAt"), pill("ui.selectOther"), pill("edit.undo"));
    assert!((n.center().x - at.x).abs() < 0.5 && (n.center().y - (at.y + PILL_OFFSETS[0].y)).abs() < 0.5, "{n:?}");
    assert!((e.left() - (at.x + PILL_OFFSETS[2].x)).abs() < 0.5, "{e:?}");
    assert!((w.right() - (at.x + PILL_OFFSETS[6].x)).abs() < 0.5, "{w:?}");
    h.step(&json!({"click":{"handle":"marking:ui.lookAt"}})).unwrap();
    h.frames(2);
    assert!(h.app.viewport.context_menu.is_none(), "a pill click closes the menu");
    assert!(h.app.cam.back().z > 0.999, "and runs Look At: {:?}", h.app.cam.back());
}

/// A finished 60 × 40 rectangle and the Extrude panel open on it, under Inventor.
fn extrude_panel() -> Harness {
    let mut h = inventor(DocumentKind::Part);
    h.app.preferences.look_at_anim = false;
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[60,40]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.start("solid.extrude");
    h.frames(2);
    h
}

#[test]
fn escape_with_the_menu_open_over_a_tool_only_closes_the_menu() {
    let mut h = inventor(DocumentKind::Part);
    h.app.preferences.look_at_anim = false;
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.start("sketch.line");
    h.frames(2);
    open(&mut h.app, pos2(400.0, 300.0));
    h.frames(1);
    assert!(h.app.viewport.context_menu.is_some());
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.viewport.context_menu.is_none(), "Esc closed the menu");
    assert!(h.app.tool.is_some(), "and left the tool out");
    assert!(h.app.session.active_sketch.is_some());
}

#[test]
fn escape_with_the_menu_open_over_a_panel_keeps_the_panel() {
    let mut h = extrude_panel();
    open(&mut h.app, pos2(400.0, 300.0));
    h.frames(1);
    assert!(h.app.viewport.context_menu.is_some());
    h.step(&json!({"key":"Escape"})).unwrap();
    h.frames(1);
    assert!(h.app.viewport.context_menu.is_none(), "Esc closed the menu");
    assert!(h.app.dialog.is_some(), "and kept the Extrude panel");
    h.step(&json!({"key":"Escape"})).unwrap();
    h.frames(1);
    assert!(h.app.dialog.is_none(), "the next Esc cancels the panel");
}

#[test]
fn enter_with_the_menu_open_does_not_apply_the_panel() {
    let mut h = extrude_panel();
    open(&mut h.app, pos2(400.0, 300.0));
    h.frames(1);
    assert_eq!(entry(&describe(&h.app), "items", "ui.ok")["enabled"], json!(true), "the panel could apply");
    h.step(&json!({"key":"Enter"})).unwrap();
    h.frames(2);
    assert_eq!(extrudes(&h), 0, "Enter over the menu applied nothing");
    assert!(h.app.dialog.is_some(), "the panel stays open");
}

#[test]
fn the_menus_ok_and_apply_are_off_while_the_preview_fails() {
    let mut h = extrude_panel();
    open(&mut h.app, pos2(400.0, 300.0));
    let menu = describe(&h.app);
    assert_eq!(entry(&menu, "items", "ui.ok")["enabled"], json!(true));
    assert_eq!(entry(&menu, "radial", "ui.applyContinue")["enabled"], json!(true));
    // The footer greys OK and + while the preview reports an error; so does the menu.
    h.app.preview.error = Some("the extrusion fails".into());
    let menu = describe(&h.app);
    assert_eq!(entry(&menu, "items", "ui.ok")["enabled"], json!(false));
    assert_eq!(entry(&menu, "radial", "ui.applyContinue")["enabled"], json!(false));
    assert_eq!(entry(&menu, "radial", "ui.cancel")["enabled"], json!(true), "Cancel still works");
}

#[test]
fn the_face_ring_has_no_dead_select_tangencies_pill() {
    let mut h = boxed(DocumentKind::Part);
    let menu = open_on_top_face(&mut h);
    let ring = ids(&menu, "radial");
    assert_eq!(ring[3], "-", "SE is an empty slot on a face: {ring:?}");
    assert!(!ring.contains(&"ui.tangentChain".to_string()));
    assert!(layout_of(&h, MenuContext::Face).unmapped.iter().any(|u| u == "SE: Select Tangencies"));
    assert_eq!(layout_of(&h, MenuContext::Edge).ring[3], "ui.tangentChain", "an edge keeps Select Tangent Chain");
}

/// A part with a finished 20 x 10 rectangle sketch "Base" on XY (unconsumed), nothing else.
fn sketched(mut h: Harness) -> (Harness, u64) {
    h.app.preferences.look_at_anim = false;
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY","name":"Base"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]})).unwrap();
    let id = h.app.session.active_sketch.unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.frames(2);
    (h, id)
}

const SKETCH_SELECTED_RING: [&str; 8] =
    ["ui.lookAt", "solid.extrude", "solid.revolve", "solid.hole", "ui.editSketch", "construct.plane.offset", "edit.undo", "inspect.measure"];

/// A sketch picked in a part (its Browser row or its profile in the view) swaps Inventor's
/// compass to Look At, Extrude, Revolve, Hole, Edit Sketch, Work Plane, Undo, Measure
/// (clean-room theme doc §Marking menus, R197).
#[test]
fn a_selected_sketch_in_a_part_has_its_own_ring() {
    for pick_of in [|id: u64| Sel::Feature { id }, |id: u64| Sel::Profile { sketch: id, index: 0 }] {
        let (mut h, id) = sketched(inventor(DocumentKind::Part));
        h.app.run("select.set", json!({"items": [pick_of(id)]})).unwrap();
        assert_eq!(context(&h.app), MenuContext::SketchSelected, "{:?}", h.app.session.selection);
        open(&mut h.app, pos2(400.0, 300.0));
        let menu = describe(&h.app);
        assert_eq!(ids(&menu, "radial"), SKETCH_SELECTED_RING, "{menu}");
        for id in SKETCH_SELECTED_RING.iter().filter(|i| **i != "edit.undo") {
            assert_eq!(entry(&menu, "radial", id)["enabled"], true, "{id} is live: {menu}");
        }
        // S: Edit Sketch opens that sketch.
        pick(&mut h.app, "ui.editSketch").unwrap();
        h.frames(2);
        assert_eq!(h.app.session.active_sketch, Some(id), "Edit Sketch opens the picked sketch");
    }
}

#[test]
fn a_selected_sketchs_extrude_starts_with_its_profile_and_look_at_faces_it() {
    let (mut h, id) = sketched(inventor(DocumentKind::Part));
    h.app.run("ui.view", json!({"view": "iso"})).ok();
    h.app.run("select.set", json!({"items": [Sel::Feature { id }]})).unwrap();
    open(&mut h.app, pos2(400.0, 300.0));
    pick(&mut h.app, "ui.lookAt").unwrap();
    h.frames(30);
    assert!(h.app.cam.back().z.abs() > 0.999, "facing the XY sketch: {:?}", h.app.cam.back());
    h.app.run("select.set", json!({"items": [Sel::Feature { id }]})).unwrap();
    open(&mut h.app, pos2(400.0, 300.0));
    pick(&mut h.app, "solid.extrude").unwrap();
    h.frames(2);
    let d = h.app.dialog.as_ref().expect("Extrude opened");
    assert!(matches!(d.kind, crate::dialogs::Kind::Extrude { .. }));
    let items = &d.inputs[0].items;
    assert!(
        !items.is_empty() && items.iter().all(|s| matches!(s, Sel::Profile { sketch, .. } if *sketch == id)),
        "the sketch's profile is picked: {items:?}"
    );
}

/// SolidCraft has no marking menus: a picked sketch keeps its list menu, as before.
#[test]
fn solidcraft_keeps_its_menu_for_a_selected_sketch() {
    let (mut h, id) = sketched(Harness::new());
    h.app.run("select.set", json!({"items": [Sel::Feature { id }]})).unwrap();
    open(&mut h.app, pos2(400.0, 300.0));
    assert_eq!(describe(&h.app)["radial"], json!([]));
    assert!(layout(&h.app).is_none() && ring(&h.app).is_none());
}

/// Without a `sketch_selected` layout a profile's picked sketch falls back to its empty ring.
#[test]
fn a_profile_without_a_sketch_selected_layout_falls_back_to_empty() {
    let (mut h, id) = sketched(inventor(DocumentKind::Part));
    h.app.run("select.set", json!({"items": [Sel::Feature { id }]})).unwrap();
    let mut p = crate::profile::active(&h.app).clone();
    p.workflow.marking_menu.as_mut().unwrap().remove(&MenuContext::SketchSelected);
    assert!(crate::profile_workflow::validate_workflow(&p.workflow).is_empty(), "the layout is optional");
    assert_eq!(super::sketch_context(&h.app, p.workflow.marking_menu.as_ref()), MenuContext::Empty);
}
