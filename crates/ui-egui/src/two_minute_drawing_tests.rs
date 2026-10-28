#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The two-view drawing in two minutes (Plan D-2 Task 12, spec §7.2): the Inventor drawing
//! workflow driven end to end through the UI paths an Inventor user takes (aliases typed key by
//! key, clicks on the sheet and the on-sheet cube, the marking menu's list, the Quick Access
//! Save and File › Export), one named assertion per step; and the same drawing under SolidCraft
//! from its DRAWING tab.

use egui::Rect;
use serde_json::{Value, json};
use solidcraft_engine::doc::DocumentKind::{Drawing, Part};
use solidcraft_engine::doc::drawing::{CubeTarget, DrawingView, LineStyle, Projection};
use solidcraft_engine::drawing::{dimension_text, note_at};
use solidcraft_engine::geom::Vec3;

use crate::drawing_ui::{DrawingPanel, DrawingTool};
use crate::scenario::{Harness, handle_at};
use crate::sheet_canvas::screen_of;
use crate::test_support::{alias, click, clicks_expire, inventor, menu_pick, right_click};

const IN: f64 = 25.4;

/// The D-1 fixture's block, built through commands in the open inch part (box sizes as unit
/// strings, R1): 4 x 3 x 1 in, a drilled ⌀0.75 through-hole and four R0.5 rounded corners.
fn build_block(h: &mut Harness) {
    for (id, p) in [
        ("solid.box", json!({"length": "4 in", "width": "1 in", "height": "3 in", "corner": [-50.8, 0, 0], "name": "Block", "body_name": "Block"})),
        ("solid.hole", json!({"position": [0, 0, 38.1], "direction": [0, 1, 0], "diameter": "0.75 in", "name": "Drill"})),
        ("solid.fillet", json!({"edges": [[-50.8, 12.7, 0], [50.8, 12.7, 0], [-50.8, 12.7, 76.2], [50.8, 12.7, 76.2]], "radius": "0.5 in"})),
    ] {
        h.app.run(id, p.clone()).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    }
    h.frames(2);
}

/// The bodies of the open part and the size of the first one's extent, in inches.
fn bodies_and_size(h: &Harness) -> (usize, [f64; 3]) {
    let st = h.app.session.world_state();
    let body = &st.bodies.first().expect("a body").body;
    let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
    let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
    for e in body.edges(0.001).unwrap() {
        for p in e.points {
            lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
            hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        }
    }
    let r = |v: f64| (v / IN * 1e6).round() / 1e6;
    (st.bodies.len(), [r(hi.x - lo.x), r(hi.y - lo.y), r(hi.z - lo.z)])
}

/// A screen point below the part where the view shows nothing.
fn empty_space(h: &mut Harness) -> Value {
    let [x, y] = h.at(&json!({"world": [0, 0, 0]})).unwrap();
    json!([x, y + 160.0])
}

fn canvas() -> Rect {
    crate::scenario::rect_of("sheet_canvas").expect("the sheet canvas is drawn")
}

/// A paper point (inches) as a screen point for a step.
fn paper(h: &Harness, p: [f64; 2]) -> Value {
    let s = screen_of(&h.app, canvas(), p);
    json!([s.x, s.y])
}

fn hover(h: &mut Harness, p: [f64; 2]) {
    let at = paper(h, p);
    h.step(&json!({"move": at})).unwrap();
    h.frames(2);
}

/// Move to the paper point and click there, as a hand does.
fn click_paper(h: &mut Harness, p: [f64; 2]) {
    hover(h, p);
    let at = paper(h, p);
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
}

fn view_panel(h: &Harness) -> &crate::drawing_view_panel::ViewPanel {
    match &h.app.drawing.panel {
        Some(DrawingPanel::View(p)) => p,
        _ => panic!("the Base View panel is not open"),
    }
}

fn views(h: &Harness) -> Vec<DrawingView> {
    h.app.session.doc.drawing.as_deref().map_or_else(Vec::new, |m| m.views.values().cloned().collect())
}

fn near(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
    (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol
}

/// The active sheet's entities as (handle, kind, layer).
fn entities(h: &Harness) -> Vec<(u64, cadcraft_doc::EntityKind, String)> {
    let m = h.app.session.doc.drawing.as_deref().unwrap();
    let layout = m.paper.layout(&m.sheets[0].layout).unwrap();
    layout.entities.iter().map(|e| (e.handle.0, e.kind.clone(), e.common.layer.clone())).collect()
}

fn center_lines(h: &Harness) -> usize {
    entities(h).iter().filter(|(_, k, layer)| matches!(k, cadcraft_doc::EntityKind::Line(_)) && layer == "CENTER").count()
}

/// Where the centre mark's two CENTER lines cross (paper inches).
fn center_cross(h: &Harness) -> [f64; 2] {
    let lines: Vec<cadcraft_doc::Line> = entities(h)
        .into_iter()
        .filter_map(|(_, k, layer)| match k {
            cadcraft_doc::EntityKind::Line(l) if layer == "CENTER" => Some(l),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 2, "two CENTER lines");
    let (p, r) = ([lines[0].a.x, lines[0].a.y], [lines[0].b.x - lines[0].a.x, lines[0].b.y - lines[0].a.y]);
    let (q, s) = ([lines[1].a.x, lines[1].a.y], [lines[1].b.x - lines[1].a.x, lines[1].b.y - lines[1].a.y]);
    let cross = r[0] * s[1] - r[1] * s[0];
    assert!(cross.abs() > 1e-9, "the two lines are not parallel");
    let t = ((q[0] - p[0]) * s[1] - (q[1] - p[1]) * s[0]) / cross;
    [p[0] + t * r[0], p[1] + t * r[1]]
}

fn dimensions(h: &Harness) -> Vec<u64> {
    entities(h).into_iter().filter(|(_, k, _)| matches!(k, cadcraft_doc::EntityKind::Dimension(_))).map(|(handle, ..)| handle).collect()
}

fn texts(h: &Harness) -> Vec<u64> {
    entities(h).into_iter().filter(|(_, k, _)| matches!(k, cadcraft_doc::EntityKind::MText(_))).map(|(handle, ..)| handle).collect()
}

fn dim_text(h: &Harness, dim: u64) -> String {
    dimension_text(h.app.session.doc.drawing.as_deref().unwrap(), dim).unwrap()
}

/// With General Dimension running: pick `on`, place at `at`. The new dimension's text.
fn dimension(h: &mut Harness, on: [f64; 2], at: [f64; 2]) -> String {
    let before = dimensions(h);
    click_paper(h, on);
    hover(h, at);
    click_paper(h, at);
    let after = dimensions(h);
    assert_eq!(after.len(), before.len() + 1, "one dimension from {on:?} at {at:?}: {:?}", h.app.status);
    let new = after.into_iter().find(|d| !before.contains(d)).unwrap();
    // Let the placement clicks expire before the next pick.
    clicks_expire(h);
    dim_text(h, new)
}

fn tool_is_dimension(h: &Harness) -> bool {
    matches!(h.app.drawing.tool, Some(DrawingTool::Dimension(_)))
}

/// The four dimensions of spec §7.2 step 4, every pick on a visible edge of VIEW1 (front at
/// [10, 8]: the straight bottom edge, the straight right edge between its fillets, the hole and the
/// bottom-left fillet arc). Their texts in order.
fn four_dimensions(h: &mut Harness) -> Vec<String> {
    vec![
        dimension(h, [10.0, 6.5], [10.0, 5.8]),
        dimension(h, [12.0, 8.0], [12.8, 8.0]),
        dimension(h, [10.375, 8.0], [11.5, 9.5]),
        dimension(h, [8.146, 6.646], [7.2, 6.0]),
    ]
}

/// A point on the hole's circle at 225°, clear of the centre mark's legs and the ⌀ leader.
const HOLE_SW: [f64; 2] = [10.0 - 0.375 * std::f64::consts::FRAC_1_SQRT_2, 8.0 - 0.375 * std::f64::consts::FRAC_1_SQRT_2];
const HOLE_NOTE_AT: [f64; 2] = [8.0, 10.3];

/// The new note's text after placing a hole note on the hole.
fn hole_note(h: &mut Harness) -> String {
    let before = texts(h);
    click_paper(h, HOLE_SW);
    hover(h, HOLE_NOTE_AT);
    click_paper(h, HOLE_NOTE_AT);
    let after = texts(h);
    assert_eq!(after.len(), before.len() + 1, "one hole note: {:?}", h.app.status);
    let note = after.into_iter().find(|t| !before.contains(t)).unwrap();
    note_at(h.app.session.doc.drawing.as_deref().unwrap(), note).unwrap().text
}

/// The page boxes (points) a PDF's `/MediaBox` entries declare.
fn media_boxes(pdf: &[u8]) -> Vec<Vec<f64>> {
    let text = String::from_utf8_lossy(pdf);
    text.match_indices("/MediaBox")
        .map(|(i, m)| {
            let rest = &text[i + m.len()..];
            let inside = rest.split(']').next().unwrap_or_default().trim().trim_start_matches('[');
            inside.split_whitespace().map(|n| n.parse::<f64>().unwrap()).collect()
        })
        .collect()
}

#[test]
fn a_two_view_drawing_in_two_minutes_under_inventor() {
    let mut h = inventor(Part);
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;

    // 1. The block, built through commands in a new inch part, and saved.
    crate::documents::new_document(&mut h.app, Part, Some("in"));
    h.frames(3);
    build_block(&mut h);
    let part_path = h.dir.join("block.tcipart").to_string_lossy().into_owned();
    h.app.run("file.save", json!({"path": part_path})).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.doc.units, "in", "an inch part");
    assert_eq!(bodies_and_size(&h), (1, [4.0, 1.0, 3.0]), "one body, 4 x 1 x 3 in");
    assert!(!h.app.session.is_dirty(), "the part is saved");

    // 2. Right-click empty space › list › Create Drawing View: a Drawing tab and the Base View
    // panel on the block, placing with the on-sheet cube; Front on the cube; click [10, 8].
    let empty = empty_space(&mut h);
    right_click(&mut h, empty);
    menu_pick(&mut h, "menu", "ui.createDrawingView");
    h.settle();
    h.frames(2);
    assert_eq!((h.app.docs.count(), h.app.docs.active, h.app.session.doc.kind), (2, 1, Drawing), "the drawing opens in its own tab");
    let block = std::path::absolute(&part_path).unwrap().to_string_lossy().into_owned();
    assert_eq!(view_panel(&h).model.as_deref(), Some(block.as_str()), "the Base View panel shows the block");
    assert_eq!(view_panel(&h).origin, crate::drawing_view_panel::Origin::FromModel);
    assert_eq!(view_panel(&h).orientation.name(), "iso_top_right", "the part's iso camera, snapped");
    assert!(matches!(&h.app.drawing.tool, Some(DrawingTool::Place(p)) if p.stage == crate::drawing_place::Stage::Base));
    assert!(!h.app.drawing.preview.is_empty(), "the live preview");
    assert_eq!(crate::scenario::count_of("drawing_place:cube"), Some(1.0), "the on-sheet cube");
    let cam = h.app.cam;
    let front =
        handle_at("drawing_place:cube:front").unwrap_or_else(|| panic!("no front face: {:?}", crate::scenario::handles("drawing_place:cube")));
    h.step(&json!({"move": [front.x, front.y]})).unwrap();
    h.frames(1);
    h.step(&json!({"click": [front.x, front.y]})).unwrap();
    h.frames(3);
    assert_eq!(view_panel(&h).orientation.name(), "front", "Front on the cube");
    assert!(views(&h).is_empty() && h.app.cam == cam, "the cube picks; it places nothing and never moves the sheet");
    clicks_expire(&mut h);
    click_paper(&mut h, [10.0, 8.0]);
    let all = views(&h);
    assert_eq!(all.len(), 1, "VIEW1 placed");
    let v1 = all[0].clone();
    assert_eq!((v1.identifier.as_str(), v1.parent), ("VIEW1", None));
    assert_eq!(v1.orientation, CubeTarget::from_name("front").unwrap().orientation());
    assert!(near([v1.position.x, v1.position.y], [10.0, 8.0], 1e-3), "{:?}", v1.position);
    assert_eq!((v1.lines, v1.shaded), (LineStyle::Removed, true), "Hidden Line Removed + Shaded, the from-part default");
    assert_eq!(v1.scale, 1.0);
    assert!(h.app.drawing.panel.is_none(), "the panel closes with the base view");

    // 3. Up to [11, 16] and click: the top view, on VIEW1's column; right-click › list › Done.
    clicks_expire(&mut h);
    hover(&mut h, [11.0, 16.0]);
    click_paper(&mut h, [11.0, 16.0]);
    let at = paper(&h, [25.0, 15.0]);
    right_click(&mut h, at);
    let menu = crate::context_menu::describe(&h.app);
    let ids: Vec<&str> = menu["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["ui.done", "ui.cancel"], "{menu}");
    menu_pick(&mut h, "menu", "ui.done");
    let all = views(&h);
    assert_eq!(all.len(), 2);
    let v2 = all[1].clone();
    assert_eq!((v2.identifier.as_str(), v2.parent, v2.projection), ("VIEW2", Some(v1.id), Some(Projection::Top)));
    assert!(near([v2.position.x, v2.position.y], [10.0, 16.0], 1e-3), "VIEW2 aligned above VIEW1: {:?}", v2.position);
    assert!(h.app.drawing.tool.is_none(), "Done ends placing");
    let labels: Vec<&str> = h.app.session.undo.iter().map(|u| u.label.as_str()).collect();
    assert_eq!(labels, ["Base View", "Projected View"]);

    // 4. D: the bottom and right straight edges, the hole and a fillet arc.
    clicks_expire(&mut h);
    alias(&mut h, "D");
    assert!(tool_is_dimension(&h), "D starts General Dimension");
    assert_eq!(four_dimensions(&mut h), ["3.00", "2.00", "⌀.75", "R.50"]);
    assert_eq!(dimensions(&h).len(), 4);
    let m = h.app.session.doc.drawing.clone().unwrap();
    assert!(solidcraft_engine::drawing::suppressed_annotations(&m).is_empty(), "every pick is a visible edge: none suppressed");
    assert!(tool_is_dimension(&h), "the tool repeats");
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert!(h.app.drawing.tool.is_none());

    // 5. CM on the hole: two CENTER lines; HN on the hole: ⌀.75 THRU.
    clicks_expire(&mut h);
    alias(&mut h, "CM");
    assert_eq!(center_lines(&h), 0);
    click_paper(&mut h, [10.375, 8.0]);
    assert_eq!(center_lines(&h), 2, "the hole's centre mark");
    assert!(near(center_cross(&h), [10.0, 8.0], 1e-3), "centred on the hole: {:?}", center_cross(&h));
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    clicks_expire(&mut h);
    alias(&mut h, "HN");
    assert_eq!(hole_note(&mut h), "⌀.75 THRU");
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    let labels: Vec<&str> = h.app.session.undo.iter().map(|u| u.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Base View",
            "Projected View",
            "General Dimension",
            "General Dimension",
            "General Dimension",
            "General Dimension",
            "Center Mark",
            "Hole and Thread Note"
        ]
    );
    assert_eq!(h.app.cam, cam, "nothing on the sheet moved the camera");

    // 6. Save (the QAT's Save, accepting the picker's default): block.tcidrw beside the part.
    click(&mut h, "appbar:save");
    h.settle();
    h.frames(2);
    let saved = std::path::absolute(h.dir.join("block.tcidrw")).unwrap();
    assert_eq!(h.app.session.path.as_deref().map(std::path::PathBuf::from), Some(saved.clone()), "saved beside the part");
    assert!(saved.is_file(), "{} exists", saved.display());
    assert!(!h.app.session.is_dirty());
    // File › Export PDF… to the picker's default (a temp folder): one D landscape page.
    h.app.custom_titlebar = true;
    h.frames(2);
    h.step(&json!({"click": {"handle": "appbar:file"}})).unwrap();
    h.frames(3);
    h.step(&json!({"widget": "Export PDF…"})).unwrap();
    h.settle();
    h.frames(3);
    let pdf = std::path::absolute(h.dir.join("block.pdf")).unwrap();
    let bytes = std::fs::read(&pdf).unwrap_or_else(|e| panic!("{}: {e}; status {:?}", pdf.display(), h.app.status));
    assert!(bytes.starts_with(b"%PDF"), "a PDF");
    assert_eq!(media_boxes(&bytes), [[0.0, 0.0, 2448.0, 1584.0]], "one D landscape page, 34 x 22 in");
    assert!(!h.app.session.is_dirty(), "an export is not an edit");
}

#[test]
fn the_same_drawing_under_solidcraft() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;
    let part = crate::test_support::block_part(&h.dir);
    h.app.open_path(&part);
    h.frames(2);
    assert_eq!(h.app.session.doc.kind, Part);
    let block = std::path::absolute(&part).unwrap().to_string_lossy().into_owned();

    // File › New Drawing.
    h.app.custom_titlebar = true;
    h.frames(2);
    h.step(&json!({"click": {"handle": "appbar:file"}})).unwrap();
    h.frames(3);
    h.step(&json!({"widget": "New Drawing"})).unwrap();
    h.frames(3);
    assert_eq!((h.app.docs.count(), h.app.session.doc.kind), (2, Drawing));
    assert_eq!(crate::profile::ribbon(&h.app).home(), "DRAWING");

    // The DRAWING tab's Base View, the block chosen in the Model combo, placed at [10, 8].
    click(&mut h, "toolbar:drawing.view.base");
    // The only open model is offered first; choose it in the combo as a user does.
    assert_eq!(view_panel(&h).model.as_deref(), Some(block.as_str()));
    // Cleared first, so the pick below is what sets it.
    if let Some(crate::drawing_ui::DrawingPanel::View(p)) = h.app.drawing.panel.as_mut() {
        p.model = None;
    }
    h.frames(2);
    assert_eq!(view_panel(&h).model, None);
    click(&mut h, "drawing_view:model");
    click(&mut h, "drawing_view:model_row:block.tcipart");
    assert_eq!(view_panel(&h).model.as_deref(), Some(block.as_str()));
    assert_eq!(view_panel(&h).orientation.name(), "front");
    clicks_expire(&mut h);
    click_paper(&mut h, [10.0, 8.0]);
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    // Projected: pick VIEW1, then above it.
    clicks_expire(&mut h);
    click(&mut h, "toolbar:drawing.view.projected");
    clicks_expire(&mut h);
    click_paper(&mut h, [10.5, 8.5]);
    clicks_expire(&mut h);
    click_paper(&mut h, [11.0, 16.0]);
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    let all = views(&h);
    assert_eq!(all.len(), 2);
    assert_eq!((all[0].identifier.as_str(), all[0].lines, all[0].shaded), ("VIEW1", LineStyle::Hidden, false), "the ribbon path's default style");
    assert!(near([all[0].position.x, all[0].position.y], [10.0, 8.0], 1e-3));
    assert_eq!((all[1].parent, all[1].projection), (Some(all[0].id), Some(Projection::Top)));
    assert!(near([all[1].position.x, all[1].position.y], [10.0, 16.0], 1e-3));

    // The four dimensions, Center Mark and Hole Note from the DRAWING tab.
    clicks_expire(&mut h);
    click(&mut h, "toolbar:drawing.dim.general");
    assert_eq!(four_dimensions(&mut h), ["3.00", "2.00", "⌀.75", "R.50"]);
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    clicks_expire(&mut h);
    click(&mut h, "toolbar:drawing.centermark");
    click_paper(&mut h, [10.375, 8.0]);
    assert_eq!(center_lines(&h), 2);
    assert!(near(center_cross(&h), [10.0, 8.0], 1e-3), "centred on the hole: {:?}", center_cross(&h));
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    clicks_expire(&mut h);
    click(&mut h, "toolbar:drawing.note.hole");
    assert_eq!(hole_note(&mut h), "⌀.75 THRU");
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);

    // EXPORT › PDF (the picker's default name in the temp folder: the drawing is unsaved).
    clicks_expire(&mut h);
    click(&mut h, "toolbar:drawing.export.pdf");
    h.settle();
    h.frames(3);
    let pdf = h.dir.join(format!("{}.pdf", h.app.session.doc.name));
    let bytes = std::fs::read(&pdf).unwrap_or_else(|e| panic!("{}: {e}; status {:?}", pdf.display(), h.app.status));
    assert!(bytes.starts_with(b"%PDF"));
    assert_eq!(media_boxes(&bytes), [[0.0, 0.0, 2448.0, 1584.0]]);

    // SolidCraft's Part ribbon is as before (Task 2's Review Focus, R67).
    crate::documents::switch(&mut h.app, 0);
    h.frames(2);
    assert_eq!(h.app.session.doc.kind, Part);
    let ribbon = crate::profile::ribbon(&h.app);
    let mut want: Vec<&str> = crate::workspace::TABS.to_vec();
    want.extend(["SKETCH", "SHEET METAL", "RENDER"]);
    assert_eq!(ribbon.tabs.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), want);
    assert!(ribbon.commands().iter().all(|c| !c.starts_with("drawing.")));
    assert_eq!(ribbon, crate::profile::solidcraft().ribbon(Part));
}
