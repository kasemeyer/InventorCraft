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

/// The MODEL tab's panels on the toolbar, left to right.
fn model_panels(app: &crate::SolveApp) -> Vec<&'static str> {
    crate::toolbar_custom::panel_lists(app, "MODEL").into_iter().map(|(p, ..)| p).collect()
}

#[test]
fn a_narrow_inventor_ribbon_collapses_whole_panels_from_the_right() {
    let h = on_model(inventor(Part), 1100.0);
    let panels = model_panels(&h.app);
    let last = *panels.last().unwrap();
    assert!(count_of("toolbar_collapsed").is_some_and(|n| n > 0.0), "the tab does not fit at 1100 px");
    let button = crate::scenario::handle_at(&format!("collapsed:MODEL:{last}")).unwrap_or_else(|| panic!("{last} is not one collapsed button"));
    let label = crate::profile::panel_label(&h.app, "MODEL", last);
    let [x, y] = h.widget(&label).unwrap_or_else(|| panic!("no widget labelled {label}"));
    assert!((x as f32 - button.x).abs() < 30.0 && (y as f32 - button.y).abs() < 30.0, "the collapsed button is labelled {label}");
    assert_eq!(count_of("toolbar_trimmed"), Some(0.0), "panels collapse whole, never trimmed");
    assert_eq!(count_of("toolbar_truncated_labels"), Some(0.0), "no small-button label is truncated");
    // A panel left of a drawn panel is never collapsed: collapse runs from the right.
    let collapsed: Vec<bool> = panels.iter().map(|p| crate::scenario::handle_at(&format!("collapsed:MODEL:{p}")).is_some()).collect();
    assert!(collapsed.windows(2).all(|w| !w[0] || w[1]), "{panels:?} {collapsed:?}");
    let column = ["solid.chamfer", "solid.shell", "solid.draft"].map(|id| rect_of(&format!("button:{id}")).is_some());
    assert!(column.iter().all(|&drawn| drawn) || column.iter().all(|&drawn| !drawn), "{column:?}");
}

#[test]
fn a_wide_inventor_ribbon_collapses_nothing() {
    let _h = on_model(inventor(Part), 3200.0);
    assert_eq!(count_of("toolbar_collapsed"), Some(0.0));
    assert_eq!(count_of("toolbar_more"), Some(0.0));
    assert_eq!(count_of("toolbar_trimmed"), Some(0.0));
    assert_eq!(count_of("toolbar_truncated_labels"), Some(0.0));
}

#[test]
fn small_columns_are_as_wide_as_their_widest_label() {
    let _h = on_model(inventor(Part), 3200.0);
    let wide = rect_of("button:solid.chamfer").unwrap().width();
    let painter = egui::Painter::new(_h.ctx.clone(), egui::LayerId::background(), egui::Rect::EVERYTHING);
    let labels: Vec<&str> = ["solid.chamfer", "solid.shell", "solid.draft"]
        .iter()
        .map(|id| _h.app.tr_label(crate::profile::button_label(&_h.app, solidcraft_engine::find_command(id).unwrap())))
        .collect();
    assert_eq!(wide, column_width(&painter, &labels), "the column is as wide as its widest label");
    // Narrower, a panel collapses whole; a column still drawn keeps that width.
    for width in [1600.0, 1300.0] {
        let _h = on_model(inventor(Part), width);
        if let Some(r) = rect_of("button:solid.chamfer") {
            assert_eq!(r.width(), wide, "at {width} px");
        }
    }
}

#[test]
fn fit_whole_collapses_rightmost_panels_first() {
    let b = |w: f32| crate::toolbar::ButtonSize { width: w, min_width: w };
    let panels: Vec<crate::toolbar::PanelSize> =
        [3, 2, 2].iter().map(|n| crate::toolbar::PanelSize { buttons: (0..*n).map(|_| b(60.0)).collect() }).collect();
    let full = crate::toolbar::fit_whole(&panels, 10_000.0);
    assert!(full.iter().all(|p| p.fit == crate::toolbar::Fit::Buttons && p.shown == p.widths.len()));
    let room: f32 = full.iter().map(|p| p.width() + 1.0).sum();
    let one = crate::toolbar::fit_whole(&panels, room - 1.0);
    let fits: Vec<_> = one.iter().map(|p| p.fit).collect();
    assert_eq!(fits, [crate::toolbar::Fit::Buttons, crate::toolbar::Fit::Buttons, crate::toolbar::Fit::Collapsed]);
    assert!(one.iter().all(|p| p.fit != crate::toolbar::Fit::Buttons || p.widths.iter().all(|w| *w == 60.0)), "no button narrows");
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
    assert_eq!(panel.grid.len(), 13);
    let dimension = rect_of("button:sketch.dimension").unwrap();
    let mut cells: Vec<(&str, egui::Rect)> = panel
        .grid
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
    assert_eq!(cells.iter().map(|(id, _)| *id).collect::<Vec<_>>(), panel.grid);
    assert!(cells.iter().all(|(_, cell)| cell.bottom() <= dimension.bottom()), "the gallery is taller than the buttons");
    // SolidCraft keeps its own gallery and order.
    assert_eq!(crate::profile::gallery_position("DESIGN", crate::workspace::CONSTRAINT_GALLERY[0]), Some(0));
    assert_eq!(crate::profile::gallery_position("DESIGN", "sketch.constraint.fix"), None);
    assert_eq!(crate::profile::gallery_position("inventor.part", "sketch.constraint.coincident"), Some(0));
}

#[test]
fn the_constrain_panel_is_dimension_large_then_a_small_column_then_the_grid() {
    let mut h = inventor(Part);
    h.app.home.open = false;
    h.call("ui.resize", json!({"width": 3200, "height": 1200}));
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.ui.tab = "SKETCH".into();
    h.frames(3);
    // The face, before the grid: Dimension alone at the large tier, then one column of three.
    assert_eq!(
        inventor_face("SKETCH", "CONSTRAIN").get(..2).unwrap(),
        ["sketch.dimension", "[sketch.auto_constrain, sketch.auto_constrain_from_datum, sketch.constrainer]"]
    );
    let dimension = rect_of("button:sketch.dimension").unwrap();
    assert!(rect_of("ribbon:constraint:sketch.dimension").is_none(), "Dimension is drawn in the grid");
    let pattern = rect_of("button:sketch.pattern.rectangular").unwrap();
    let ids = ["sketch.auto_constrain", "sketch.auto_constrain_from_datum", "sketch.constrainer"];
    let helpers = ids.map(|id| rect_of(&format!("button:{id}")).unwrap());
    // Dimension is the panel's first control: right of the PATTERN panel, left of everything else.
    assert!(dimension.left() > pattern.right());
    for helper in helpers {
        assert!((helper.height() - dimension.height() / 3.0).abs() <= 1.5, "{helper:?} is not small beside {dimension:?}");
        assert!((helper.left() - helpers[0].left()).abs() <= 0.5, "{helper:?} is not in one column");
        assert!(helper.left() > dimension.right());
    }
    for id in ids {
        assert!(rect_of(&format!("ribbon:constraint:{id}")).is_none(), "{id} is drawn in the grid");
    }
    assert!(helpers[0].top() < helpers[1].top() && helpers[1].top() < helpers[2].top());
    // The grid follows the column and still holds every constraint, and no helper.
    let panel = crate::profile::ribbon(&h.app).tab("SKETCH").unwrap().panel("CONSTRAIN").unwrap();
    assert_eq!(panel.grid.len(), 13);
    for id in &panel.grid {
        let cell = rect_of(&format!("ribbon:constraint:{id}")).unwrap_or_else(|| panic!("{id} is not a gallery cell"));
        assert!(cell.left() > helpers[0].right(), "{id} is not after the small column");
    }
    // The alias still starts Dimension.
    h.app.alias.fired = None;
    h.step(&json!({"key": "D"})).unwrap();
    h.frames(crate::profile::active(&h.app).alias_wait_ms as usize * 60 / 1000 + 2);
    assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.dimension"));
}
