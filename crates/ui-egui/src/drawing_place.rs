//! Placing drawing views (Plan D-2 Task 6, spec §2.3-§2.4): Inventor's way, in every profile
//! (SolidCraft's drawings run this same code).
//!
//! - **Base view.** With the Base View panel open on a new view, a live preview of the view
//!   follows the cursor on the sheet. A small cube pinned at the canvas's top-right corner (where
//!   the part's ViewCube sits, wave 3 R98) picks the orientation: a click on a face, edge or
//!   corner sets the panel's orientation, the cube turns in place and the preview turns. The cube
//!   never follows the cursor or the preview. It is a picker drawn with the navigation cube's
//!   facets (`navigation_cube::target_at`); it never moves the sheet camera. A click on the sheet (or Enter, OK) places the base view where its
//!   preview is: one `drawing.view.base`, one undo step.
//! - **Projected views.** The command goes on: moving away from the base view previews the view
//!   its direction gives (top, bottom, left, right; iso on a diagonal), aligned with it, and each
//!   click places one `drawing.view.projected`. Enter, Done or Esc ends; Esc keeps what was
//!   placed. Projected (PV) first asks for the view to project from.
//! - **Placed views.** Dragging a placed view moves it (a projected view along its axis, R84):
//!   one `drawing.view.set_position` on release. A double-click opens Edit View (R14).
//!
//! The preview is not a command: `preview_view` runs only when the cursor's 0.05 in cell, the
//! orientation or the panel's fields change (R89), and nothing is placed until a click.

use egui::{Pos2, Rect, Response, vec2};
use serde_json::{Value, json};
use solidcraft_engine::doc::DocumentKind;
use solidcraft_engine::doc::drawing::{CubeTarget, DrawingView, LineStyle};
use solidcraft_engine::drawing::{PaperCurve, PaperPick, aligned_position, camera_for, preview_view, projected_orientation, projection_at};
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::render::Camera;

use crate::SolveApp;
use crate::drawing_ui::{DrawingPanel, DrawingTool};
use crate::drawing_view_panel::{Origin, ViewPanel};
use crate::navigation::CameraInput;
use crate::scenario;
use crate::sheet_canvas::paper_at;

/// The on-sheet cube's half-width, points (the 3D view's cube is 40 design points of 164 in 112).
pub(crate) const CUBE_HALF: f32 = 27.0;
/// How far the cube's centre sits in from the top-right corner of the canvas's content area on
/// each axis, points: the centre of the part ViewCube's box (`navigation_cube::SIZE` square at
/// the top-right of the viewport's content), so the cube is where the 3D view's cube is (R98).
pub(crate) const CUBE_INSET: f32 = crate::viewport::navigation_cube::SIZE / 2.0;
/// The navigation cube's design half-width the cube's labels are scaled from.
const CUBE_DESIGN_HALF: f32 = 40.0;
/// The preview is computed again only when the cursor leaves its cell (R89), inches.
const CELL_IN: f64 = 0.05;
/// A click picks a view within this many points.
const PICK_PX: f64 = 4.0;

/// What the next click does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    /// Place the base view the panel describes.
    Base,
    /// Place views projected from `parent`.
    Project { parent: u64 },
    /// Pick the view to project from (Projected, PV).
    PickParent,
}

/// The placing tool's state (`DrawingTool::Place`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Place {
    pub stage: Stage,
    /// Base View started this run (its base placed, it projects from it): the ribbon keeps Base
    /// View pressed, not Projected (fix wave 2).
    pub from_base: bool,
    /// The views this run placed, in order.
    pub placed: Vec<u64>,
    /// The cell (in 0.05 in steps) and orientation the preview was computed for.
    pub preview_key: Option<([i64; 2], CubeTarget)>,
    /// Where the base view's preview sits (inches); `None` until the cursor reaches the sheet, when
    /// it sits at the canvas centre.
    pub anchor: Option<[f64; 2]>,
    /// The last cursor point on the sheet (inches), for projected views.
    pub cursor: Option<[f64; 2]>,
    /// The panel's fields (position aside) the preview was computed for.
    params_seen: Option<Value>,
    /// Enter ends projection only from the frame after it began (the Enter that placed the base
    /// view must not also end it).
    armed: bool,
}

impl Place {
    pub(crate) fn new(stage: Stage) -> Place {
        Place {
            stage,
            from_base: stage == Stage::Base,
            placed: Vec::new(),
            preview_key: None,
            anchor: None,
            cursor: None,
            params_seen: None,
            armed: false,
        }
    }
}

/// A placed view being dragged.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Drag {
    view: u64,
    /// The pressed point less the view's position (inches).
    grab: [f64; 2],
    /// Where the view would go now.
    target: [f64; 2],
    /// The cell its preview was computed for.
    key: Option<[i64; 2]>,
}

fn place(app: &SolveApp) -> Option<&Place> {
    match &app.drawing.tool {
        Some(DrawingTool::Place(p)) => Some(p),
        _ => None,
    }
}

fn place_mut(app: &mut SolveApp) -> Option<&mut Place> {
    match &mut app.drawing.tool {
        Some(DrawingTool::Place(p)) => Some(p),
        _ => None,
    }
}

/// The open Base View panel of a new view (not Edit View).
fn new_view_panel(app: &SolveApp) -> Option<&ViewPanel> {
    match &app.drawing.panel {
        Some(DrawingPanel::View(p)) if !matches!(p.origin, Origin::Edit(_)) => Some(p),
        _ => None,
    }
}

fn cell(p: [f64; 2]) -> [i64; 2] {
    [(p[0] / CELL_IN).round() as i64, (p[1] / CELL_IN).round() as i64]
}

/// The cube target a facet direction names (faces, edges and corners have components -1, 0, 1).
pub(crate) fn target_of(d: Vec3) -> Option<CubeTarget> {
    let s = |v: f64| {
        if v > 0.5 {
            1
        } else if v < -0.5 {
            -1
        } else {
            0
        }
    };
    let t = CubeTarget { x: s(d.x), y: s(d.y), z: s(d.z) };
    CubeTarget::ALL.contains(&t).then_some(t)
}

/// The camera the cube is drawn around: the orientation's own view (D-1's `camera_for`). A local
/// value: the sheet's `app.cam` is never read or written for it.
fn cube_camera(t: CubeTarget) -> Camera {
    camera_for(&t.orientation(), Vec3::ZERO, 1.0)
}

/// The canvas centre's paper point, when the canvas has been laid out.
fn canvas_centre(app: &SolveApp) -> Option<[f64; 2]> {
    app.viewport.rect.map(|r| paper_at(app, r, r.center()))
}

/// Where a base view goes now: its preview's place, else the canvas centre (the cursor never
/// reached the sheet), else the sheet's centre.
pub(crate) fn base_position(app: &SolveApp) -> [f64; 2] {
    place(app).and_then(|p| p.anchor).or_else(|| canvas_centre(app)).unwrap_or_else(|| {
        let (w, h) = crate::sheet_canvas::paper_size(app);
        [w / 2.0, h / 2.0]
    })
}

/// The on-sheet cube's centre on canvas `rect`: pinned `CUBE_INSET` in from the top-right corner
/// of the canvas's content area (below a floating ribbon, as the part's ViewCube box is), whatever
/// the cursor, the preview or the orientation do (R98).
pub(crate) fn cube_centre(app: &SolveApp, rect: Rect) -> Pos2 {
    let content = crate::shell::content_rect(app, rect);
    Pos2::new(content.right() - CUBE_INSET, content.top() + CUBE_INSET)
}

/// The cube's orientation while it is up: a base view is being placed and its orientation may
/// change.
fn cube_shown(app: &SolveApp) -> Option<CubeTarget> {
    let placing = place(app).is_some_and(|p| p.stage == Stage::Base);
    new_view_panel(app).filter(|p| placing && p.cube_enabled()).map(|p| p.orientation)
}

/// The cube target under screen point `p`, if the cube is up and `p` is not under the shell.
fn cube_target_at(app: &SolveApp, rect: Rect, p: Pos2) -> Option<CubeTarget> {
    // Under a card or the browser the cube is not there to be clicked.
    if crate::shell::covered(app, p) {
        return None;
    }
    let t = cube_shown(app)?;
    crate::viewport::navigation_cube::target_at(&cube_camera(t), cube_centre(app, rect), f64::from(CUBE_HALF), p).and_then(target_of)
}

/// The ribbon's Base and BV (`DRAWING_STARTS`): the Base View panel on the ribbon path, its
/// preview following the cursor.
pub(crate) fn start_base(app: &mut SolveApp) {
    finish(app);
    crate::drawing_view_panel::open(app, None, Origin::Ribbon);
}

/// The panel opened on a new view (ribbon or Create Drawing View): placing begins.
pub(crate) fn begin_base(app: &mut SolveApp) {
    app.drawing.drag = None;
    app.drawing.tool = Some(DrawingTool::Place(Place::new(Stage::Base)));
}

/// The ribbon's Projected and PV: project from the selected view, else pick one first.
pub(crate) fn start_projected(app: &mut SolveApp) {
    finish(app);
    if app.session.doc.kind != DocumentKind::Drawing {
        return;
    }
    // Two edges of one view are that view.
    let mut selected: Vec<u64> = app.drawing.selection.iter().filter_map(view_of).collect();
    selected.sort_unstable();
    selected.dedup();
    let stage = match selected.as_slice() {
        [parent] => Stage::Project { parent: *parent },
        _ => {
            app.set_status(crate::i18n::tr("ui.drawing.pick_parent").to_string(), false);
            Stage::PickParent
        }
    };
    app.drawing.drag = None;
    app.drawing.tool = Some(DrawingTool::Place(Place::new(stage)));
}

/// End placing (Enter, Done, Cancel, Esc): the placed views stay; the preview and a new view's
/// panel go.
pub(crate) fn finish(app: &mut SolveApp) {
    if matches!(app.drawing.tool, Some(DrawingTool::Place(_))) {
        app.drawing.tool = None;
    }
    app.drawing.preview.clear();
    if new_view_panel(app).is_some() {
        app.drawing.panel = None;
    }
}

/// The base view `view` was placed (a click, Enter or OK): projection from it begins.
pub(crate) fn base_placed(app: &mut SolveApp, view: u64) {
    let Some(p) = place_mut(app) else { return };
    p.stage = Stage::Project { parent: view };
    p.placed.push(view);
    (p.preview_key, p.params_seen, p.armed) = (None, None, false);
    app.drawing.preview.clear();
}

fn view_of(pick: &PaperPick) -> Option<u64> {
    match pick {
        PaperPick::View { view } | PaperPick::ViewEdge { view, .. } => Some(*view),
        _ => None,
    }
}

/// The view under screen point `p` (its curves, else its box).
fn view_at(app: &SolveApp, rect: Rect, p: Pos2) -> Option<u64> {
    // Inches per point (the camera's paper height over the canvas height) times the pick radius.
    let tol = PICK_PX * 2.0 * app.cam.half_height() / f64::from(rect.height().max(1.0));
    solidcraft_engine::drawing::pick(&app.session, paper_at(app, rect, p), tol).as_ref().and_then(view_of)
}

fn drawing_view(app: &SolveApp, id: u64) -> Option<DrawingView> {
    app.session.doc.drawing.as_deref()?.views.get(&id).cloned()
}

/// The sheet's pointer input while a view is placed or dragged, and a double-click on a view.
/// True when it used the frame's click (the canvas then selects nothing).
pub(crate) fn input(app: &mut SolveApp, ui: &egui::Ui, rect: Rect, resp: &Response, input: &CameraInput, navigating: bool) -> bool {
    let hover = input.hover.filter(|_| input.inside);
    let clicked = resp.clicked() && !navigating;
    let Some(stage) = place(app).map(|p| p.stage) else {
        // Placed views drag and open for edit only with no tool running: another tool (a
        // dimension's picks) owns the sheet's clicks and drags.
        if app.drawing.tool.is_some() {
            return false;
        }
        return placed_views(app, ui, rect, resp, hover, navigating);
    };
    match stage {
        Stage::Base => {
            // The panel closed (Cancel, Esc): placing ends with it.
            if new_view_panel(app).is_none() {
                finish(app);
                return clicked;
            }
            let cube = hover.and_then(|p| cube_target_at(app, rect, p));
            if let Some(p) = hover.filter(|_| cube.is_none()) {
                let at = paper_at(app, rect, p);
                if let Some(pl) = place_mut(app) {
                    pl.anchor = Some(at);
                }
            }
            if clicked {
                match cube {
                    Some(t) => {
                        if let Some(DrawingPanel::View(p)) = app.drawing.panel.as_mut() {
                            p.orientation = t;
                        }
                    }
                    None if hover.is_some() => place_base(app),
                    None => {}
                }
            }
        }
        Stage::Project { parent } => {
            if let Some(p) = hover {
                let at = paper_at(app, rect, p);
                if let Some(pl) = place_mut(app) {
                    pl.cursor = Some(at);
                }
                // A fast double-click is one click's view, not two views on one spot.
                if clicked && !resp.double_clicked() && !resp.triple_clicked() {
                    match app.run("drawing.view.projected", json!({ "parent": parent, "position": at })) {
                        Ok(r) => {
                            if let (Some(id), Some(pl)) = (r["view"].as_u64(), place_mut(app)) {
                                pl.placed.push(id);
                                pl.preview_key = None;
                            }
                        }
                        Err(e) => app.set_status(e, true),
                    }
                }
            }
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let armed = place(app).is_some_and(|p| p.armed);
            if enter && armed && app.drawing.panel.is_none() && !crate::marking_menu::holds_keys(app) {
                finish(app);
                return clicked;
            }
            if let Some(pl) = place_mut(app) {
                pl.armed = true;
            }
        }
        Stage::PickParent => {
            if clicked
                && let Some(view) = hover.and_then(|p| view_at(app, rect, p))
                && let Some(pl) = place_mut(app)
            {
                pl.stage = Stage::Project { parent: view };
                (pl.preview_key, pl.armed) = (None, false);
            }
        }
    }
    refresh_preview(app);
    true
}

/// Place the base view the panel describes where its preview is (the panel's own OK path).
fn place_base(app: &mut SolveApp) {
    let Some(DrawingPanel::View(mut p)) = app.drawing.panel.take() else { return };
    if !crate::drawing_view_panel::apply(app, &mut p) {
        app.drawing.panel = Some(DrawingPanel::View(p));
    }
}

/// The preview for what the next click places, computed again only when its key changes (R89).
fn refresh_preview(app: &mut SolveApp) {
    let Some(pl) = place(app) else { return };
    match pl.stage {
        Stage::Base => {
            let Some(p) = new_view_panel(app) else { return clear_preview(app) };
            if !p.preview || !p.can_ok() {
                return clear_preview(app);
            }
            let at = base_position(app);
            let key = (cell(at), p.orientation);
            let mut seen = crate::drawing_view_panel::base_params(p, [0.0, 0.0]);
            seen["position"] = Value::Null;
            if pl.preview_key == Some(key) && pl.params_seen.as_ref() == Some(&seen) {
                return;
            }
            let curves = preview_view(&app.session, "drawing.view.base", &crate::drawing_view_panel::base_params(p, at)).unwrap_or_default();
            app.drawing.preview = curves;
            if let Some(pl) = place_mut(app) {
                (pl.preview_key, pl.params_seen) = (Some(key), Some(seen));
            }
        }
        Stage::Project { parent } => {
            let (Some(at), Some(pv)) = (pl.cursor, drawing_view(app, parent)) else { return clear_preview(app) };
            let from = [pv.position.x, pv.position.y];
            let o = projected_orientation(&pv.orientation, projection_at(from, at));
            let key = (cell(at), CubeTarget::matching(&o).unwrap_or_else(|| CubeTarget::nearest(o.dir)));
            if pl.preview_key == Some(key) {
                return;
            }
            let curves = preview_view(&app.session, "drawing.view.projected", &json!({ "parent": parent, "position": at })).unwrap_or_default();
            app.drawing.preview = curves;
            if let Some(pl) = place_mut(app) {
                pl.preview_key = Some(key);
            }
        }
        Stage::PickParent => clear_preview(app),
    }
}

fn clear_preview(app: &mut SolveApp) {
    if !app.drawing.preview.is_empty() {
        app.drawing.preview.clear();
    }
    if let Some(pl) = place_mut(app) {
        (pl.preview_key, pl.params_seen) = (None, None);
    }
}

/// With no tool: a double-click on a view opens Edit View (R14); a drag moves a view, a projected
/// one along its axis (R84), with one `drawing.view.set_position` on release.
fn placed_views(app: &mut SolveApp, ui: &egui::Ui, rect: Rect, resp: &Response, hover: Option<Pos2>, navigating: bool) -> bool {
    if resp.double_clicked()
        && !navigating
        && let Some(view) = hover.and_then(|p| view_at(app, rect, p))
    {
        crate::drawing_view_panel::open(app, None, Origin::Edit(view));
        return true;
    }
    if resp.drag_started_by(egui::PointerButton::Primary)
        && !navigating
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        && let Some(view) = view_at(app, rect, origin)
        && let Some(v) = drawing_view(app, view)
    {
        let at = paper_at(app, rect, origin);
        let pos = [v.position.x, v.position.y];
        app.drawing.drag = Some(Drag { view, grab: [at[0] - pos[0], at[1] - pos[1]], target: pos, key: None });
    }
    let Some(mut d) = app.drawing.drag.clone() else { return false };
    let Some(v) = drawing_view(app, d.view) else {
        app.drawing.drag = None;
        app.drawing.preview.clear();
        return false;
    };
    if let Some(p) = ui.input(|i| i.pointer.latest_pos()) {
        let at = paper_at(app, rect, p);
        let at = [at[0] - d.grab[0], at[1] - d.grab[1]];
        d.target = match v.parent.and_then(|id| drawing_view(app, id)) {
            Some(parent) => {
                let from = [parent.position.x, parent.position.y];
                aligned_position(from, v.projection.unwrap_or_else(|| projection_at(from, at)), at)
            }
            None => at,
        };
    }
    if resp.drag_stopped() {
        app.drawing.drag = None;
        app.drawing.preview.clear();
        // Back where it started: no move, no empty undo step.
        let unmoved = (d.target[0] - v.position.x).abs() <= 1e-9 && (d.target[1] - v.position.y).abs() <= 1e-9;
        if !unmoved && let Err(e) = app.run("drawing.view.set_position", json!({ "view": d.view, "position": d.target })) {
            app.set_status(e, true);
        }
        return true;
    }
    if d.key != Some(cell(d.target)) {
        d.key = Some(cell(d.target));
        app.drawing.preview = drag_preview(app, &v, d.target);
    }
    app.drawing.drag = Some(d);
    false
}

/// View `v` drawn at `at`: its own orientation, style and scale, previewed as a base view.
fn drag_preview(app: &SolveApp, v: &DrawingView, at: [f64; 2]) -> Vec<(PaperCurve, bool)> {
    let Some(model) = crate::browser::drawing::model_path_of(app, v.id) else { return Vec::new() };
    let (d, u) = (v.orientation.dir, v.orientation.up);
    let params = json!({
        "model": model,
        "orientation": { "dir": [d.x, d.y, d.z], "up": [u.x, u.y, u.z] },
        "lines": if v.lines == LineStyle::Removed { "removed" } else { "hidden" },
        "scale": v.scale,
        "tangent_edges": v.tangent_edges,
        "position": at,
    });
    preview_view(&app.session, "drawing.view.base", &params).unwrap_or_default()
}

/// The on-sheet cube, drawn over the sheet beside the base view's preview (after the sheet pass).
pub(crate) fn paint(app: &SolveApp, ui: &egui::Ui, rect: Rect) {
    let Some(t) = cube_shown(app) else {
        scenario::publish_count("drawing_place:cube", 0.0);
        return;
    };
    let cam = cube_camera(t);
    let c = cube_centre(app, rect);
    let faces = crate::viewport::navigation_cube::facets(&cam, c, f64::from(CUBE_HALF), true);
    let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p) && !crate::shell::covered(app, *p));
    let hovered = hover.and_then(|p| crate::viewport::navigation_cube::hit(&faces, p));
    let tokens = crate::theme::Tokens::get().navigation_cube(hovered.is_some());
    crate::icons::navigation::paint_cube(&ui.painter_at(rect), &faces, hovered, tokens, true, CUBE_HALF / CUBE_DESIGN_HALF);
    scenario::publish_count("drawing_place:cube", 1.0);
    scenario::publish_rect("drawing_place:cube", Rect::from_center_size(c, vec2(2.0, 2.0) * CUBE_HALF));
    // One handle per target: the centre of its most facing patch (facets are sorted by depth).
    let mut seen: Vec<CubeTarget> = Vec::new();
    for f in faces.iter().rev() {
        if let Some(target) = target_of(f.direction)
            && !seen.contains(&target)
        {
            seen.push(target);
            scenario::publish_handle(&format!("drawing_place:cube:{}", target.name()), f.centre);
        }
    }
    if hovered.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

#[cfg(test)]
#[path = "drawing_place_tests.rs"]
mod tests;
