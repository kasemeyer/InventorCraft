#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The Base View panel (Plan D-2 Task 5, spec §2.3, §2.5, §2.6): its two tabs, the captured
//! style order, the scale list, the defaults of each path, Esc, OK and Edit View.

use serde_json::{Value, json};
use solidcraft_engine::doc::DocumentKind::{Drawing, Part};
use solidcraft_engine::doc::drawing::{CubeTarget, LineStyle};

use super::*;
use crate::context_menu::run_item;
use crate::drawing_ui::{DrawingPanel, panel_can_ok};
use crate::profile_workflow::MenuContext;
use crate::scenario::{Harness, count_of, handle_at, handles};
use crate::test_support::{block_part, click, inventor};

/// An Inventor drawing of the block (no views yet). The harness and the block's absolute path.
fn block_drawing() -> (Harness, String) {
    let mut h = inventor(Drawing);
    h.app.home.open = false;
    let block = block_part(&h.dir);
    h.app.run("drawing.new", json!({"model": block})).unwrap();
    h.frames(2);
    (h, absolute(&block))
}

fn absolute(path: &str) -> String {
    std::path::absolute(path).unwrap().to_string_lossy().into_owned()
}

fn panel(h: &Harness) -> &ViewPanel {
    match &h.app.drawing.panel {
        Some(DrawingPanel::View(p)) => p,
        _ => panic!("the Base View panel is not open"),
    }
}

fn panel_mut(h: &mut Harness) -> &mut ViewPanel {
    match &mut h.app.drawing.panel {
        Some(DrawingPanel::View(p)) => p,
        _ => panic!("the Base View panel is not open"),
    }
}

fn views(h: &Harness) -> usize {
    h.app.session.doc.drawing.as_deref().map_or(0, |m| m.views.len())
}

fn view(h: &Harness, id: u64) -> solidcraft_engine::doc::drawing::DrawingView {
    h.app.session.doc.drawing.as_deref().unwrap().views[&id].clone()
}

fn last_undo(h: &Harness) -> (usize, Option<String>) {
    (h.app.session.undo.len(), h.app.session.undo.last().map(|u| u.label.clone()))
}

/// VIEW1 (front, hidden lines) at [10, 8] by command; VIEW2 projected above it at [10, 16] and
/// VIEW3 to its right at [20, 8].
fn three_views(h: &mut Harness, block: &str) -> [u64; 3] {
    let id = |r: Value| r["view"].as_u64().unwrap();
    let v1 = id(h
        .app
        .run("drawing.view.base", json!({"model": block, "orientation": "front", "lines": "hidden", "scale": 1, "position": [10, 8]}))
        .unwrap());
    let v2 = id(h.app.run("drawing.view.projected", json!({"parent": v1, "position": [10, 16]})).unwrap());
    let v3 = id(h.app.run("drawing.view.projected", json!({"parent": v1, "position": [20, 8]})).unwrap());
    h.frames(2);
    [v1, v2, v3]
}

/// The view list's Edit View, run for `view`.
fn edit_view(h: &mut Harness, view: u64) {
    let menu = crate::drawing_menu::view_items(&h.app, view);
    assert_eq!(menu[0].id, "ui.drawing.editView");
    assert!(menu[0].enabled);
    run_item(&mut h.app, &menu[0], egui::Pos2::ZERO);
    h.frames(3);
    assert_eq!(panel(h).origin, Origin::Edit(view));
}

#[test]
fn the_ribbon_path_starts_with_hidden_lines_only() {
    let (mut h, block) = block_drawing();
    // The ribbon's Base (and BV) goes through the drawing table to the panel (R2, R11).
    h.app.start("drawing.view.base");
    h.frames(3);
    let p = panel(&h);
    assert_eq!(p.origin, Origin::Ribbon);
    assert!(p.hidden_lines && !p.removed && !p.shaded, "hidden {} removed {} shaded {}", p.hidden_lines, p.removed, p.shaded);
    assert_eq!(p.orientation, CubeTarget::from_name("front").unwrap());
    // The ribbon path offers the open model; none is open here but the drawing's own tab.
    assert_eq!(p.model, None);
    assert!(!panel_can_ok(&h.app), "no model: no OK");
    panel_mut(&mut h).model = Some(block);
    assert!(panel_can_ok(&h.app));
}

#[test]
fn the_model_path_starts_removed_and_shaded() {
    let mut h = inventor(Part);
    let block = block_part(&h.dir);
    h.app.open_path_async(&block);
    h.settle();
    h.frames(2);
    let look = -h.app.cam.back();
    crate::documents::create_drawing_view(&mut h.app);
    h.frames(3);
    let p = panel(&h);
    assert_eq!(p.origin, Origin::FromModel);
    assert!(p.removed && p.shaded && !p.hidden_lines, "hidden {} removed {} shaded {}", p.hidden_lines, p.removed, p.shaded);
    // The model tab's camera direction, snapped to the cube (the default iso view).
    assert_eq!(p.orientation, CubeTarget::nearest(look));
    assert_eq!(p.orientation.name(), "iso_top_right");
}

#[test]
fn create_drawing_view_opens_the_panel_with_the_model() {
    let mut h = inventor(Part);
    let block = block_part(&h.dir);
    h.app.open_path_async(&block);
    h.settle();
    crate::documents::create_drawing_view(&mut h.app);
    h.frames(3);
    assert_eq!((h.app.docs.count(), h.app.session.doc.kind), (2, Drawing));
    let p = panel(&h);
    assert_eq!(p.origin, Origin::FromModel);
    assert_eq!(p.model.as_deref(), Some(absolute(&block).as_str()));
    // Read-only in neither: a new view's model can still be chosen.
    assert!(p.model_editable());
    assert_eq!(count_of("drawing_view:model:editable"), Some(1.0));

    // An edited model is not saved (final wave item 30): the open tab serves the drawing the
    // model as it is in memory.
    let mut h = inventor(Part);
    let block = block_part(&h.dir);
    h.app.open_path_async(&block);
    h.settle();
    let front = |h: &Harness| {
        let params = base_params(panel(h), [17.0, 11.0]);
        let params = json!({"model": params["model"], "orientation": "front", "lines": "hidden", "position": [17, 11]});
        solidcraft_engine::drawing::preview_view(&h.app.session, "drawing.view.base", &params).unwrap().len()
    };
    let on_disk = std::fs::read(&block).unwrap();
    h.app.run("solid.box", json!({"length": "1 in", "width": "1 in", "height": "1 in", "corner": [100, 0, 0], "name": "Extra"})).unwrap();
    assert!(h.app.session.is_dirty());
    crate::documents::create_drawing_view(&mut h.app);
    h.frames(3);
    assert_eq!(h.app.session.doc.kind, Drawing);
    assert_eq!(std::fs::read(&block).unwrap(), on_disk, "the model's file is untouched");
    assert!(crate::documents::any_dirty(&h.app), "the model tab keeps its unsaved edit");
    let edited = front(&h);
    assert_eq!(crate::documents::tab_paths(&h.app)[0].as_deref(), Some(block.as_str()));
    // The untouched block's front view, for comparison.
    let (h2, _) = block_drawing();
    let mut h2 = h2;
    crate::drawing_view_panel::open(&mut h2.app, Some(absolute(&block_part(&h2.dir))), Origin::Ribbon);
    let plain = front(&h2);
    assert!(edited > plain, "the extra box is drawn: {edited} curves vs {plain}");
}

#[test]
fn style_toggles_keep_the_captured_order() {
    let (mut h, block) = block_drawing();
    open(&mut h.app, Some(block), Origin::Ribbon);
    h.frames(3);
    let at = |k: &str| handle_at(&format!("drawing_view:style:{k}")).unwrap_or_else(|| panic!("no {k} toggle: {:?}", handles("drawing_view")));
    let (hidden, removed, shaded) = (at("hidden"), at("removed"), at("shaded"));
    assert!(hidden.x < removed.x && removed.x < shaded.x, "{hidden:?} {removed:?} {shaded:?}");
    assert!((hidden.y - removed.y).abs() < 0.5 && (removed.y - shaded.y).abs() < 0.5, "one row");
    // Hidden Line and Hidden Line Removed are exclusive; Shaded is independent.
    click(&mut h, "drawing_view:style:removed");
    assert!(!panel(&h).hidden_lines && panel(&h).removed);
    click(&mut h, "drawing_view:style:shaded");
    assert!(panel(&h).removed && panel(&h).shaded);
    click(&mut h, "drawing_view:style:hidden");
    assert!(panel(&h).hidden_lines && !panel(&h).removed && panel(&h).shaded);
    click(&mut h, "drawing_view:style:shaded");
    assert!(!panel(&h).shaded);
}

#[test]
fn the_scale_combo_lists_the_presets() {
    let (mut h, block) = block_drawing();
    open(&mut h.app, Some(block), Origin::Ribbon);
    h.frames(3);
    click(&mut h, "drawing_view:scale:open");
    let mut rows = handles("drawing_view:scale_row:");
    rows.sort_by(|a, b| a.1.y.total_cmp(&b.1.y));
    let names: Vec<&str> = rows.iter().map(|(n, _)| n.trim_start_matches("drawing_view:scale_row:")).collect();
    assert_eq!(names, ["1:4", "1:2", "1:1", "2:1", "3:1", "4:1", "5:1", "10:1"]);
    click(&mut h, "drawing_view:scale_row:2:1");
    assert_eq!(panel(&h).scale_text, "2:1");
    assert_eq!(base_params(panel(&h), [0.0, 0.0])["scale"], json!(2.0));

    panel_mut(&mut h).scale_text = "3:8".into();
    h.frames(2);
    assert_eq!(base_params(panel(&h), [0.0, 0.0])["scale"], json!(0.375));
    assert_eq!(panel(&h).error, None);
    assert!(panel_can_ok(&h.app));

    panel_mut(&mut h).scale_text = "0".into();
    h.frames(2);
    assert!(panel(&h).error.is_some(), "a zero scale is an error");
    assert!(!panel_can_ok(&h.app));
    assert_eq!(count_of("dialog:ok:enabled"), Some(0.0));
}

#[test]
fn defaults_follow_the_capture() {
    let (mut h, block) = block_drawing();
    open(&mut h.app, Some(block.clone()), Origin::Ribbon);
    h.frames(3);
    let p = panel(&h);
    assert_eq!(p.identifier, "VIEW1");
    assert_eq!(p.scale_text, "1:1");
    assert!(!p.label_visible && !p.tangent_edges);
    assert!(p.preview);
    assert_eq!(p.tab, 0);
    assert!(!p.style_from_base && !p.scale_from_base);
    assert!(!p.follows_base_enabled(), "Style from Base and Scale from Base are disabled for a base view");
    assert_eq!(count_of("drawing_view:style_from_base:enabled"), Some(0.0));
    assert_eq!(count_of("drawing_view:scale_from_base:enabled"), Some(0.0));
    assert!(handle_at("drawing_view:style_from_base").is_some() && handle_at("drawing_view:scale_from_base").is_some(), "shown");
    // The model state row is read-only, with our single-state label.
    assert!(handle_at("dialog:report:drawing_view:model_state").is_some());
    let params = base_params(panel(&h), [12.0, 9.0]);
    assert_eq!(
        params,
        json!({"model": block, "orientation": "front", "lines": "hidden", "shaded": false, "scale": 1.0, "position": [12.0, 9.0],
               "identifier": "VIEW1", "tangent_edges": false, "label_visible": false})
    );
    // Display: Tangent Edges, off.
    click(&mut h, "drawing_view:tab:display");
    assert_eq!(panel(&h).tab, 1);
    assert!(handle_at("drawing_view:tangent_edges").is_some());
    assert!(handle_at("drawing_view:style:hidden").is_none(), "the Component tab's fields are hidden");
}

#[test]
fn esc_cancels_and_places_nothing() {
    let (mut h, block) = block_drawing();
    open(&mut h.app, Some(block), Origin::Ribbon);
    h.frames(3);
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert!(h.app.drawing.panel.is_none());
    assert_eq!(views(&h), 0);
    assert_eq!(h.app.session.undo.len(), 0);
}

#[test]
fn enter_places_one_view_through_the_base_command() {
    let (mut h, block) = block_drawing();
    open(&mut h.app, Some(block), Origin::Ribbon);
    h.frames(3);
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(2);
    assert!(h.app.drawing.panel.is_none());
    assert_eq!(views(&h), 1);
    assert_eq!(last_undo(&h), (1, Some("Base View".into())));
    let v = h.app.session.doc.drawing.as_deref().unwrap().views.values().next().unwrap().clone();
    // The cursor never reached the sheet: the view goes to the canvas centre's paper point (Task 6,
    // R14; was the sheet's centre until placement at the cursor).
    let rect = crate::scenario::rect_of("sheet_canvas").unwrap();
    let [x, y] = crate::sheet_canvas::paper_at(&h.app, rect, rect.center());
    assert_eq!((v.identifier.as_str(), v.position.x, v.position.y), ("VIEW1", x, y));
    assert_eq!((v.lines, v.shaded, v.scale), (LineStyle::Hidden, false, 1.0));
}

#[test]
fn editing_a_view_runs_set_commands() {
    let (mut h, block) = block_drawing();
    let [v1, v2, _] = three_views(&mut h, &block);
    let undo = h.app.session.undo.len();

    // Shaded on: one "View Style" step (R14, R87).
    edit_view(&mut h, v1);
    let p = panel(&h);
    assert!(p.hidden_lines && !p.removed && !p.shaded && !p.tangent_edges);
    assert_eq!((p.identifier.as_str(), p.scale_text.as_str()), ("VIEW1", "1:1"));
    assert_eq!(p.model.as_deref(), Some(block.as_str()));
    assert!(!p.model_editable() && p.cube_enabled() && !p.follows_base_enabled());
    assert_eq!(count_of("drawing_view:model:editable"), Some(0.0));
    click(&mut h, "drawing_view:style:shaded");
    crate::drawing_ui::panel_ok(&mut h.app);
    h.frames(2);
    assert!(h.app.drawing.panel.is_none());
    assert_eq!(last_undo(&h), (undo + 1, Some("View Style".into())));
    assert!(view(&h, v1).shaded);

    // Display › Tangent Edges on: one more "View Style" step (Task 0).
    edit_view(&mut h, v1);
    assert!(panel(&h).shaded);
    click(&mut h, "drawing_view:tab:display");
    click(&mut h, "drawing_view:tangent_edges");
    assert!(panel(&h).tangent_edges);
    crate::drawing_ui::panel_ok(&mut h.app);
    h.frames(2);
    assert_eq!(last_undo(&h), (undo + 2, Some("View Style".into())));
    assert!(view(&h, v1).tangent_edges);

    // Scale and the identifier together: one "Edit View" step (final wave item 11).
    edit_view(&mut h, v1);
    panel_mut(&mut h).scale_text = "1:2".into();
    panel_mut(&mut h).identifier = "FRONT".into();
    panel_mut(&mut h).label_visible = true;
    crate::drawing_ui::panel_ok(&mut h.app);
    h.frames(2);
    assert_eq!(last_undo(&h), (undo + 3, Some("Edit View".into())));
    let v = view(&h, v1);
    assert_eq!((v.scale, v.identifier.as_str(), v.label_visible), (0.5, "FRONT", true));
    // One undo takes the whole edit back.
    h.app.run("edit.undo", json!({})).unwrap();
    let v = view(&h, v1);
    assert_eq!((v.scale, v.identifier.as_str(), v.label_visible), (1.0, "VIEW1", false));
    h.app.run("edit.redo", json!({})).unwrap();
    assert_eq!(view(&h, v1).scale, 0.5);

    // Nothing changed: OK runs nothing.
    edit_view(&mut h, v1);
    crate::drawing_ui::panel_ok(&mut h.app);
    assert_eq!(h.app.session.undo.len(), undo + 3);

    // A projected view: the cube is off, the model read-only, the base flags on and shown.
    edit_view(&mut h, v2);
    let p = panel(&h);
    assert!(!p.cube_enabled() && !p.model_editable());
    assert!(p.follows_base_enabled() && p.style_from_base && p.scale_from_base);
    assert_eq!(count_of("drawing_view:style_from_base:enabled"), Some(1.0));
    assert_eq!(count_of("drawing_view:model:editable"), Some(0.0));
}

#[test]
fn tangent_edges_on_a_following_projected_view_take_effect() {
    let (mut h, block) = block_drawing();
    let [_, v2, _] = three_views(&mut h, &block);
    let undo = h.app.session.undo.len();
    edit_view(&mut h, v2);
    assert!(panel(&h).style_from_base);
    click(&mut h, "drawing_view:tab:display");
    click(&mut h, "drawing_view:tangent_edges");
    assert!(panel(&h).tangent_edges && !panel(&h).style_from_base, "a tangent-edge choice of its own stops following the base");
    crate::drawing_ui::panel_ok(&mut h.app);
    h.frames(2);
    assert!(h.app.drawing.panel.is_none());
    assert_eq!(last_undo(&h), (undo + 1, Some("View Style".into())));
    assert!(view(&h, v2).tangent_edges);
}

#[test]
fn hiding_lines_reports_suppressed_annotations() {
    let (mut h, block) = block_drawing();
    let [_, _, v3] = three_views(&mut h, &block);
    let dim = h.app.run("drawing.dim.general", json!({"view": v3, "picks": [{"at": [20, 7.625]}], "placement": [20, 6]})).unwrap();
    let dim = dim["handle"].as_u64().unwrap();
    edit_view(&mut h, v3);
    click(&mut h, "drawing_view:style:removed");
    assert!(panel(&h).removed && !panel(&h).style_from_base, "a style of its own stops following the base");
    crate::drawing_ui::panel_ok(&mut h.app);
    h.frames(2);
    let status = h.app.status.as_ref().map(|s| s.0.clone());
    assert_eq!(status, Some(crate::i18n::format("ui.drawing.suppressed", &[("n", "1")])));
    assert_eq!(status.as_deref(), Some("1 annotations hidden with their lines"));
    let m = h.app.session.doc.drawing.as_deref().unwrap();
    assert!(solidcraft_engine::drawing::suppressed_annotations(m).contains(&dim));
}

#[test]
fn the_panel_ring_is_ok_and_cancel() {
    let (mut h, block) = block_drawing();
    open(&mut h.app, Some(block), Origin::Ribbon);
    h.frames(3);
    assert_eq!(crate::marking_menu::context(&h.app), MenuContext::Panel);
    let rect = crate::scenario::rect_of("sheet_canvas").unwrap();
    let at = crate::sheet_canvas::screen_of(&h.app, rect, [30.0, 18.0]);
    h.step(&json!({"click": [at.x, at.y], "button": "right"})).unwrap();
    h.frames(2);
    let menu = crate::context_menu::describe(&h.app);
    assert_eq!(menu["open"], json!(true), "{menu}");
    let ring: Vec<String> = menu["radial"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_string()).collect();
    let layout = &crate::marking_menu::layout(&h.app).unwrap()[&MenuContext::Panel];
    let want: Vec<String> = layout.ring.iter().map(|s| if s.is_empty() { "-".to_string() } else { s.clone() }).collect();
    assert_eq!(ring, want);
    // The captured panel layout: Cancel in the ring, OK first in the list.
    let list: Vec<String> = menu["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(list, layout.overflow);
    assert!(ring.iter().any(|i| i == "ui.cancel") && list[0] == "ui.ok", "{ring:?} {list:?}");
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert!(h.app.drawing.panel.is_some(), "Esc closed the menu, not the panel");

    // ui.ok answers to panel_can_ok, and acts as OK: exactly one view.
    let ok = |h: &Harness| crate::marking_menu::item(&h.app, "ui.ok", MenuContext::Panel);
    panel_mut(&mut h).scale_text = "0".into();
    h.frames(2);
    assert_eq!((ok(&h).enabled, panel_can_ok(&h.app)), (false, false));
    panel_mut(&mut h).scale_text = "1:1".into();
    h.frames(2);
    assert_eq!((ok(&h).enabled, panel_can_ok(&h.app)), (true, true));
    let item = ok(&h);
    run_item(&mut h.app, &item, egui::Pos2::ZERO);
    h.frames(2);
    assert!(h.app.drawing.panel.is_none());
    assert_eq!(views(&h), 1);
    assert_eq!(h.app.session.undo.len(), 1);

    // ui.cancel closes it and sends nothing.
    let placed = *h.app.session.doc.drawing.as_deref().unwrap().views.keys().next().unwrap();
    open(&mut h.app, None, Origin::Edit(placed));
    let cancel = crate::marking_menu::item(&h.app, "ui.cancel", MenuContext::Panel);
    assert!(cancel.enabled);
    run_item(&mut h.app, &cancel, egui::Pos2::ZERO);
    assert!(h.app.drawing.panel.is_none());
    assert_eq!(h.app.session.undo.len(), 1);
}

#[test]
fn a_failed_edit_view_leaves_the_view_as_it_was() {
    // Final wave item 11: the scale applies, the identifier is refused (VIEW2 is taken): the
    // scale is rolled back, no step is left, the panel stays open with the error.
    let (mut h, block) = block_drawing();
    let [v1, ..] = three_views(&mut h, &block);
    let (undo, redo) = (h.app.session.undo.len(), h.app.session.redo.len());
    edit_view(&mut h, v1);
    panel_mut(&mut h).scale_text = "1:2".into();
    panel_mut(&mut h).identifier = "VIEW2".into();
    crate::drawing_ui::panel_ok(&mut h.app);
    h.frames(2);
    assert!(h.app.drawing.panel.is_some(), "the panel stays open");
    assert!(panel(&h).error.is_some(), "{:?}", panel(&h).error);
    let v = view(&h, v1);
    assert_eq!((v.scale, v.identifier.as_str()), (1.0, "VIEW1"));
    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), (undo, redo));
}

#[test]
fn an_open_panel_is_not_replaced() {
    // Final wave item 12: Base View (or Edit View) asked for while a panel is open leaves it.
    let (mut h, block) = block_drawing();
    let [v1, ..] = three_views(&mut h, &block);
    edit_view(&mut h, v1);
    panel_mut(&mut h).identifier = "TYPED".into();
    open(&mut h.app, Some(block.clone()), Origin::Ribbon);
    h.frames(2);
    assert_eq!((panel(&h).origin, panel(&h).identifier.as_str()), (Origin::Edit(v1), "TYPED"));
}

/// A 40 × 20 × 10 in box (x, depth y, height z) saved in `dir`; its absolute path.
fn big_box(dir: &std::path::Path) -> String {
    let path = dir.join("big.tcipart").to_string_lossy().into_owned();
    let mut s = solidcraft_engine::Session::default();
    for (id, p) in [
        ("document.new_part", json!({})),
        ("document.units", json!({"units": "in"})),
        ("solid.box", json!({"length": "40 in", "width": "20 in", "height": "10 in", "corner": [0, 0, 0], "name": "Big"})),
        ("file.save", json!({"path": path})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    }
    absolute(&path)
}

/// Wave 3 item 3 (R100 amended by R108, owner default §8.8): a new base view's Scale is the
/// largest preset up to 1:1 at which front, top and right fit half the sheet inside its border.
/// On the D sheet (printable 33 × 21 in, half 16.5 × 10.5): the 4 × 1 × 3 block fits at 1:1
/// ((4 + 1) + 1 = 6 across); the 40 × 20 × 10 box does not at 1:2 (60·0.5 + 1 = 31 > 16.5) but
/// does at 1:4 (60·0.25 + 1 = 16 ≤ 16.5, 30·0.25 + 1 = 8.5 ≤ 10.5), as Inventor draws it.
/// Both the ribbon path and Create Drawing View's; Edit View keeps the view's own scale.
fn the_scale_defaults_to_the_fit(mut h: Harness) {
    h.app.home.open = false;
    let block = absolute(&block_part(&h.dir));
    h.app.run("drawing.new", json!({"model": block})).unwrap();
    h.frames(2);
    open(&mut h.app, Some(block.clone()), Origin::Ribbon);
    h.frames(2);
    assert_eq!(panel(&h).scale_text, "1:1");
    h.app.drawing.panel = None;
    crate::drawing_place::finish(&mut h.app);
    h.frames(2);
    let big = big_box(&h.dir);
    for origin in [Origin::Ribbon, Origin::FromModel] {
        open(&mut h.app, Some(big.clone()), origin);
        h.frames(2);
        assert_eq!(panel(&h).scale_text, "1:4", "{origin:?}");
        h.app.drawing.panel = None;
        crate::drawing_place::finish(&mut h.app);
        h.frames(2);
    }
    // A model picked after the panel opened is fitted too, unless a scale was typed.
    open(&mut h.app, Some(block.clone()), Origin::Ribbon);
    h.frames(2);
    panel_mut(&mut h).model = Some(big.clone());
    h.frames(2);
    assert_eq!(panel(&h).scale_text, "1:4");
    panel_mut(&mut h).scale_text = "3:1".into();
    panel_mut(&mut h).model = Some(block.clone());
    h.frames(2);
    assert_eq!(panel(&h).scale_text, "3:1", "a typed scale stays");
    h.app.drawing.panel = None;
    crate::drawing_place::finish(&mut h.app);
    h.frames(2);
    // Edit View keeps the view's scale: the big box placed at 1:1 stays 1:1.
    let v = h.app.run("drawing.view.base", json!({"model": big, "orientation": "front", "scale": 1, "position": [17, 11]})).unwrap()["view"]
        .as_u64()
        .unwrap();
    h.frames(2);
    open(&mut h.app, None, Origin::Edit(v));
    h.frames(2);
    assert_eq!(panel(&h).scale_text, "1:1");
}

#[test]
fn the_scale_defaults_to_the_fit_under_inventor() {
    the_scale_defaults_to_the_fit(inventor(Drawing));
}

#[test]
fn the_scale_defaults_to_the_fit_under_solidcraft() {
    the_scale_defaults_to_the_fit(Harness::new());
}
