#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use serde_json::json;
use solidcraft_engine::doc::DocumentKind::Part;

use crate::scenario::{Harness, count_of, rect_of, rects};
use crate::test_support::{click, inventor};

/// The face items as text: `id` for a large button, `[a, b, c]` for a column of small ones.
fn shape(items: &[FaceItem]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            FaceItem::Large(c) => c.id.to_string(),
            FaceItem::Column(cs) => format!("[{}]", cs.iter().map(|c| c.id).collect::<Vec<_>>().join(", ")),
        })
        .collect()
}

/// The face items of a panel of the Inventor Part ribbon, sized by its `small` list.
fn inventor_face(tab: &str, panel: &str) -> Vec<String> {
    let panel = crate::profile::get(crate::profile::INVENTOR).unwrap().ribbon(Part).tab(tab).unwrap().panel(panel).unwrap();
    let face: Vec<&CommandSpec> = panel.commands.iter().take(panel.promoted).map(|id| solidcraft_engine::find_command(id).unwrap()).collect();
    shape(&face_items(&face, |id| panel.is_small(id)))
}

/// A harness `width` × 1200 on the MODEL tab, drawn.
fn on_model(mut h: Harness, width: f64) -> Harness {
    h.app.home.open = false;
    h.app.ui.tab = "MODEL".into();
    h.call("ui.resize", json!({"width": width, "height": 1200}));
    h.frames(2);
    h
}

#[test]
fn face_items_stack_small_runs_three_high() {
    assert_eq!(
        inventor_face("SKETCH", "CREATE"),
        [
            "sketch.line",
            "sketch.circle.center",
            "sketch.arc.three_point",
            "sketch.rectangle.two_point",
            "[sketch.fillet, sketch.text, sketch.point]",
            "sketch.project",
        ]
    );
    assert_eq!(
        inventor_face("MODEL", "MODIFY"),
        [
            "solid.hole",
            "solid.fillet",
            "[solid.chamfer, solid.shell, solid.draft]",
            "[solid.thread, solid.combine, surface.thicken]",
            "[solid.split_body, solid.press_pull, face.delete]",
        ]
    );
    let plain: Vec<&CommandSpec> =
        ["solid.extrude", "solid.revolve", "solid.hole"].iter().map(|id| solidcraft_engine::find_command(id).unwrap()).collect();
    assert_eq!(shape(&face_items(&plain, |_| false)), ["solid.extrude", "solid.revolve", "solid.hole"]);
    // A run longer than a column fills the next one; a short last column stays short.
    assert_eq!(shape(&face_items(&plain, |_| true)), ["[solid.extrude, solid.revolve, solid.hole]"]);
    let four: Vec<&CommandSpec> =
        ["solid.extrude", "solid.revolve", "solid.hole", "solid.fillet"].iter().map(|id| solidcraft_engine::find_command(id).unwrap()).collect();
    assert_eq!(shape(&face_items(&four, |id| id != "solid.extrude")), ["solid.extrude", "[solid.revolve, solid.hole, solid.fillet]"]);
    assert_eq!(shape(&face_items(&four, |_| true)), ["[solid.extrude, solid.revolve, solid.hole]", "[solid.fillet]"]);
}

#[test]
fn small_buttons_stack_three_high_beside_large_ones() {
    let mut h = on_model(inventor(Part), 3200.0);
    let e = rect_of("button:solid.extrude").unwrap();
    let r = rect_of("button:solid.revolve").unwrap();
    let [s, l, c] = ["solid.sweep", "solid.loft", "solid.coil"].map(|id| rect_of(&format!("button:{id}")).unwrap());
    for small in [s, l, c] {
        assert!((small.left() - s.left()).abs() <= 0.5, "{small:?} is not in {s:?}'s column");
        assert!((small.height() - e.height() / 3.0).abs() <= 1.5, "{small:?} is not a third of {e:?}");
    }
    assert!(s.top() < l.top() && l.top() < c.top());
    assert!(s.left() > r.right());
    assert!(rect_of("button:solid.emboss").unwrap().left() > s.right());
    assert!(h.app.dialog.is_none());
    click(&mut h, "toolbar:solid.sweep");
    assert!(h.app.dialog.is_some());
}

#[test]
fn the_solidcraft_ribbon_draws_no_small_buttons() {
    let _h = on_model(Harness::new(), 3200.0);
    let height = rect_of("button:solid.extrude").unwrap().height();
    let buttons = rects("button:");
    assert!(buttons.len() > 10, "{buttons:?}");
    for (name, rect) in buttons {
        assert_eq!(rect.height(), height, "{name}");
    }
}

#[test]
fn a_narrow_window_trims_small_columns_whole() {
    let _h = on_model(inventor(Part), 1100.0);
    let trimmed = count_of("toolbar_trimmed");
    assert!(trimmed.is_some_and(|n| n > 0.0), "{trimmed:?}");
    let column = ["solid.chamfer", "solid.shell", "solid.draft"].map(|id| rect_of(&format!("button:{id}")).is_some());
    assert!(column.iter().all(|&drawn| drawn) || column.iter().all(|&drawn| !drawn), "{column:?}");
}

#[test]
fn a_hidden_panel_can_be_shown_and_hidden_again() {
    let mut h = on_model(inventor(Part), 3200.0);
    assert!(rect_of("panel:MODEL:PRIMITIVES").is_none());
    crate::toolbar_custom::set_panel_shown(&mut h.app, "MODEL", "PRIMITIVES", true);
    h.frames(2);
    assert!(rect_of("panel:MODEL:PRIMITIVES").is_some());
    assert!(h.app.toolbar_custom.panel_shown["inventor.part/MODEL/PRIMITIVES"]);
    let mut loaded = crate::toolbar_custom::ToolbarCustom::default();
    loaded.load(&h.app.toolbar_custom.prefs());
    assert_eq!(loaded.panel_shown, h.app.toolbar_custom.panel_shown);
    crate::toolbar_custom::set_panel_shown(&mut h.app, "MODEL", "PRIMITIVES", false);
    h.frames(2);
    assert!(rect_of("panel:MODEL:PRIMITIVES").is_none());
    assert!(rect_of("panel:MODEL:PATTERN").is_some());
    crate::toolbar_custom::set_panel_shown(&mut h.app, "MODEL", "PATTERN", false);
    h.frames(2);
    assert!(rect_of("panel:MODEL:PATTERN").is_none());
    // The list is the Inventor Part ribbon's: SolidCraft's MODEL tab is unchanged.
    let ids = |app: &crate::SolveApp| crate::toolbar_custom::panel_lists(app, "MODEL").iter().map(|(p, ..)| *p).collect::<Vec<_>>();
    let fresh = ids(&Harness::new().app);
    crate::profile::choose(&mut h.app, crate::profile::SOLIDCRAFT).unwrap();
    assert!(!fresh.is_empty());
    assert_eq!(ids(&h.app), fresh);
}

#[test]
fn the_constraint_gallery_follows_the_profile_panel_order() {
    let mut h = inventor(Part);
    h.app.home.open = false;
    h.call("ui.resize", json!({"width": 3200, "height": 1200}));
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.ui.tab = "SKETCH".into();
    h.frames(3);
    let panel = crate::profile::ribbon(&h.app).tab("SKETCH").unwrap().panel("CONSTRAIN").unwrap();
    assert_eq!(panel.small.len(), 13);
    let dimension = rect_of("button:sketch.dimension").unwrap();
    let mut cells: Vec<(&str, egui::Rect)> = panel
        .small
        .iter()
        .map(|id| {
            let cell = rect_of(&format!("ribbon:constraint:{id}")).unwrap_or_else(|| panic!("{id} is not a gallery cell"));
            assert_eq!(cell.size(), egui::vec2(24.0, 20.0), "{id}");
            assert!(cell.left() > dimension.right(), "{id}");
            (id.as_str(), cell)
        })
        .collect();
    // Reading order (rows top to bottom, each left to right) is the panel's order.
    cells.sort_by(|a, b| a.1.top().total_cmp(&b.1.top()).then(a.1.left().total_cmp(&b.1.left())));
    assert_eq!(cells.iter().map(|(id, _)| *id).collect::<Vec<_>>(), panel.small);
    assert!(cells.iter().all(|(_, cell)| cell.bottom() <= dimension.bottom()), "the gallery is taller than the buttons");
    // SolidCraft keeps its own gallery and order.
    assert_eq!(crate::profile::gallery_position("DESIGN", crate::workspace::CONSTRAINT_GALLERY[0]), Some(0));
    assert_eq!(crate::profile::gallery_position("DESIGN", "sketch.constraint.fix"), None);
    assert_eq!(crate::profile::gallery_position("inventor.part", "sketch.constraint.coincident"), Some(0));
}
