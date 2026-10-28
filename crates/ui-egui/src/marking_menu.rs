//! The profile's marking menu (Plan B-2 Task 4): a right-click in the graphics window opens a
//! ring of eight pills around the cursor and a list under it, laid out by the profile's
//! `workflow.marking_menu` for the context (a panel, a tool, the sketch, the selection). It opens
//! inside a tool and with a panel open too, where it offers OK, Apply (+), Cancel and Done.
//! Without a layout (SolidCraft) the upstream list menu and its opt-in ring stay as they were.

use std::collections::BTreeMap;

use egui::{Vec2, vec2};
use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::DocumentKind;

use crate::SolveApp;
use crate::context_menu::{Item, act, cmd};
use crate::dialogs::{Kind, PanelAction};
use crate::profile_workflow::{MenuContext, MenuLayout};

/// Capture pixels at 175 % scaling (Ruling 9) to logical points.
const fn px(x: f32, y: f32) -> Vec2 {
    vec2(x / 1.75, y / 1.75)
}

/// Pill offsets from the menu centre, N clockwise, in logical points, measured on the capture
/// (`marking-menus/*-crop.png`): N and S are pill centres; the other slots are the pills' inner
/// edges, which sit on a diamond around the cursor (a west pill ends there, an east one starts).
pub(crate) const PILL_OFFSETS: [Vec2; 8] =
    [px(0.0, -102.0), px(42.0, -48.0), px(78.0, 0.0), px(42.0, 51.0), px(0.0, 105.0), px(-42.0, 51.0), px(-78.0, 0.0), px(-42.0, -48.0)];

/// Top of the overflow list below the centre (or its bottom above it), logical points.
pub(crate) const LIST_DROP: f32 = 137.0 / 1.75;

/// The active profile's marking menus, if it has any.
pub(crate) fn layout(app: &SolveApp) -> Option<&'static BTreeMap<MenuContext, MenuLayout>> {
    crate::profile::active(app).workflow.marking_menu.as_ref()
}

/// Panel when a dialog with OK is open, Tool when a tool is out, the sketch's contexts while
/// sketching, the assembly's in an assembly, else Face / Edge / Body by the selection, else Empty
/// (graphics-window contexts only).
pub(crate) fn context(app: &SolveApp) -> MenuContext {
    if app.dialog.as_ref().is_some_and(|d| !matches!(d.kind, Kind::Sketch | Kind::Measure { .. })) {
        return MenuContext::Panel;
    }
    if app.tool.is_some() {
        return MenuContext::Tool;
    }
    let sel = &app.session.selection;
    let has = |f: fn(&Sel) -> bool| sel.iter().any(f);
    if app.session.active_sketch.is_some() {
        return if has(|s| matches!(s, Sel::SketchCurve { .. })) {
            MenuContext::SketchLine
        } else if has(|s| matches!(s, Sel::SketchConstraint { .. })) {
            MenuContext::SketchDimension
        } else {
            MenuContext::Sketch
        };
    }
    if app.session.doc.kind == DocumentKind::Assembly {
        return if sel.is_empty() { MenuContext::AssemblyEmpty } else { MenuContext::AssemblyComponent };
    }
    if has(|s| matches!(s, Sel::Face { .. })) {
        MenuContext::Face
    } else if has(|s| matches!(s, Sel::Edge { .. })) {
        MenuContext::Edge
    } else if has(|s| matches!(s, Sel::Body { .. })) {
        MenuContext::Body
    } else {
        MenuContext::Empty
    }
}

/// A menu is open under a profile with marking menus (which open inside tools and panels): Esc
/// and Enter are the menu's then, not the tool's or the panel's. Without marking menus
/// (SolidCraft) the keys reach the tool and the panel as they always have.
pub(crate) fn holds_keys(app: &SolveApp) -> bool {
    app.viewport.context_menu.is_some() && layout(app).is_some()
}

fn current(app: &SolveApp) -> Option<(MenuContext, &'static MenuLayout)> {
    let ctx = context(app);
    // A drawing's sheet has no part or sketch ring: only a panel's (R15; Task 10 adds its own).
    if app.session.doc.kind == DocumentKind::Drawing && ctx != MenuContext::Panel {
        return None;
    }
    layout(app)?.get(&ctx).map(|l| (ctx, l))
}

/// The 8 ring items ("" → `Item::sep()`), or None without a layout for the context.
pub(crate) fn ring(app: &SolveApp) -> Option<Vec<Item>> {
    let (ctx, l) = current(app)?;
    Some(l.ring.iter().map(|id| item(app, id, ctx)).collect())
}

/// The overflow list items, or None without a layout for the context.
pub(crate) fn overflow(app: &SolveApp) -> Option<Vec<Item>> {
    let (ctx, l) = current(app)?;
    Some(l.overflow.iter().map(|id| item(app, id, ctx)).collect())
}

/// The key the profile maps to a UI action (F5 for Previous View under Inventor).
fn function_key(app: &SolveApp, action: &str) -> String {
    crate::profile::active(app).function_keys.iter().find(|(_, a)| a.as_str() == action).map(|(k, _)| k.clone()).unwrap_or_default()
}

/// The selected curves of the sketch being edited, all or only its lines.
fn selected_curves(app: &SolveApp, lines_only: bool) -> Vec<String> {
    let st = app.session.world_state();
    let sketch = app.session.active_sketch.and_then(|sid| st.sketch(sid)).map(|ss| &ss.sketch);
    let is_line = |id: &String| {
        sketch
            .and_then(|sk| sk.curve_index(id).and_then(|i| sk.curves.get(i)))
            .is_some_and(|c| matches!(c.kind, solidcraft_engine::sketch::CurveKind::Line { .. }))
    };
    app.session
        .selection
        .iter()
        .filter_map(|s| if let Sel::SketchCurve { id } = s { Some(id.clone()) } else { None })
        .filter(|id| !lines_only || is_line(id))
        .collect()
}

/// Whether the open panel would act on `action` now; OK and Apply also need what is picked to
/// make the feature (the capture greys them with nothing picked) and, as the footer does, a
/// preview without an error, so the menu never offers what the footer greys.
fn panel_can(app: &SolveApp, action: PanelAction) -> bool {
    app.dialog.as_ref().is_some_and(|d| {
        crate::dialogs::can_act(app, d, action)
            && (action == PanelAction::Cancel || (crate::dialogs::makes_feature(app, d) && (!d.previews() || app.preview.error.is_none())))
    })
}

/// The one selected sketch dimension, if exactly one is selected.
fn selected_dimension(app: &SolveApp) -> Option<String> {
    let st = app.session.world_state();
    let ss = app.session.active_sketch.and_then(|sid| st.sketch(sid))?;
    let dims: Vec<&String> = app
        .session
        .selection
        .iter()
        .filter_map(|s| if let Sel::SketchConstraint { id } = s { Some(id) } else { None })
        .filter(|c| ss.sketch.constraints.iter().any(|k| &k.id == *c && k.kind.value().is_some()))
        .collect();
    match dims.as_slice() {
        [one] => Some((*one).clone()),
        _ => None,
    }
}

/// One marking-menu entry for a layout id in a context. In a drawing, Done, Cancel, OK and
/// Delete answer to the drawing's own tool, panel and selection (R72; a document-kind test).
pub(crate) fn item(app: &SolveApp, id: &str, ctx: MenuContext) -> Item {
    if app.session.doc.kind == DocumentKind::Drawing
        && let Some(item) = drawing_item(app, id)
    {
        return item;
    }
    let sketching = app.session.active_sketch.is_some();
    let bodies = || crate::context_menu::touched_bodies(app);
    match id {
        "" | "-" => Item::sep(),
        "ui.repeat" => match &app.last_command {
            Some((_, label)) => act("ui.repeat", &format!("Repeat {label}"), "redo"),
            None => act("ui.repeat", "Repeat", "redo").on(false),
        },
        // Inventor's tool ring says OK for ending the running command.
        "ui.done" => act("ui.done", "OK", "finish").on(app.tool.is_some()),
        "ui.ok" => act("ui.ok", "OK", "finish").key("Enter").on(panel_can(app, PanelAction::Ok)),
        "ui.applyContinue" => act("ui.applyContinue", "Apply", "plus").on(panel_can(app, PanelAction::ApplyAndContinue)),
        "ui.cancel" => {
            let can = if app.dialog.is_some() { panel_can(app, PanelAction::Cancel) } else { app.tool.is_some() };
            act("ui.cancel", "Cancel", "sketch_cancel").key("Esc").on(can)
        }
        "ui.finishSketch" => act("ui.finishSketch", "Finish Sketch", "finish").on(sketching),
        "ui.cancelSketch" => act("ui.cancelSketch", "Cancel Sketch", "sketch_cancel").on(sketching),
        "ui.selectOther" => act("ui.selectOther", "Select Other…", "select").on(app.menu.over_geometry),
        // Edge chains only: a face or body has no tangent chain to add.
        "ui.tangentChain" => {
            act("ui.tangentChain", "Select Tangent Chain", "chain").on(app.session.selection.iter().any(|s| matches!(s, Sel::Edge { .. })))
        }
        "ui.lookAt" => {
            let look = act("ui.lookAt", "Look At", "lookat");
            match (ctx, app.session.active_sketch) {
                (MenuContext::Sketch | MenuContext::SketchLine | MenuContext::SketchDimension | MenuContext::Tool, Some(sketch)) => {
                    look.with(json!({ "sketch": sketch }))
                }
                (MenuContext::Face | MenuContext::Edge | MenuContext::Body, _) => {
                    let can = solidcraft_engine::find_command("view.normal_to").is_some_and(|c| c.info(&app.session).disabled_reason.is_none());
                    look.with(json!({ "camera": app.cam })).on(can)
                }
                _ => look.on(false),
            }
        }
        "ui.previousView" => {
            act("ui.previousView", "Previous View", "rollback").key(&function_key(app, "ui.view_previous")).on(crate::view_history::can_go_back(app))
        }
        "ui.home" => act("ui.home", "Home View", "home").key(&function_key(app, "ui.view_home")),
        "ui.properties" => {
            let bodies = bodies();
            act("ui.properties", "Properties", "measure").on(!bodies.is_empty()).with(json!({ "bodies": bodies }))
        }
        "ui.delete" => act("ui.delete", "Delete", "delete").key("Del").on(!app.session.selection.is_empty()),
        "ui.copy" => act("ui.copy", "Copy", "copy").key("Ctrl+C").on(!app.session.selection.is_empty()),
        "ui.hide" => {
            let bodies = bodies();
            act("ui.hide", "Visibility", "eye").on(!bodies.is_empty()).with(json!({ "bodies": bodies }))
        }
        "ui.showAll" => {
            let v = &app.session.visibility;
            let hidden = !v.hidden_bodies.is_empty() || !v.hidden_sketches.is_empty() || !v.hidden_folders.is_empty();
            act("ui.showAll", "Show All", "eye").on(hidden)
        }
        // A drawing of the open part or assembly (spec §2.1 path 2c).
        "ui.createDrawingView" => {
            act("ui.createDrawingView", "Create Drawing View", "camera").on(app.session.doc.kind != DocumentKind::Drawing && !app.file_picker_busy())
        }
        "ui.findBrowser" => {
            let body = bodies().into_iter().next();
            act("ui.findBrowser", "Find in Browser", "list").on(body.is_some()).with(json!({ "body": body }))
        }
        // Registry commands that act on the selection take it as their parameters.
        "sketch.construction" => {
            let curves = selected_curves(app, false);
            cmd(app, id, "Construction").on(!curves.is_empty()).with(json!({ "curves": curves }))
        }
        "sketch.centerline" => {
            let lines = selected_curves(app, true);
            cmd(app, id, "Centerline").on(!lines.is_empty()).with(json!({ "curves": lines }))
        }
        "sketch.toggle_driven" => match selected_dimension(app) {
            Some(dim) => cmd(app, id, "Toggle Driven Dimension").with(json!({ "constraint": dim })),
            None => cmd(app, id, "Toggle Driven Dimension").on(false),
        },
        "component.from_bodies" => {
            let bodies = bodies();
            cmd(app, id, "Create Components from Bodies").on(!bodies.is_empty()).with(json!({ "bodies": bodies }))
        }
        id => cmd(app, id, solidcraft_engine::find_command(id).map_or(id, |c| c.label)),
    }
}

/// The drawing arm of [`item`]: `ui.done`/`ui.cancel` against the drawing tool, `ui.ok`/`ui.cancel`
/// against the drawing panel, `ui.delete` against the sheet's selection. `ui.home` keeps its row:
/// on a sheet it frames the sheet (`sheet_canvas::fit`, through `animate_view`).
fn drawing_item(app: &SolveApp, id: &str) -> Option<Item> {
    let d = &app.drawing;
    Some(match id {
        "ui.done" => act("ui.done", "OK", "finish").on(d.tool.is_some()),
        "ui.ok" => act("ui.ok", "OK", "finish").key("Enter").on(d.panel.is_some() && crate::drawing_ui::panel_can_ok(app)),
        "ui.cancel" => act("ui.cancel", "Cancel", "sketch_cancel").key("Esc").on(d.tool.is_some() || d.panel.is_some()),
        "ui.delete" => act("ui.delete", "Delete", "delete").key("Del").on(!d.selection.is_empty()),
        _ => return None,
    })
}

#[cfg(test)]
#[path = "marking_menu_tests.rs"]
mod tests;
