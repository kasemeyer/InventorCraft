//! The drawing sheet canvas (Plan D-2 Task 3): a Drawing document shows its active sheet here
//! instead of the 3D viewport. The sheet is CADCraft's display list of the sheet's layout
//! (`cadcraft_render::build`, which leaves out invisible entities: suppressed curves and
//! annotations, R61, R94), turned into a [`SheetScene`] and drawn by the GPU crate's sheet pass.
//! Fresh shaded pictures lie under their views' lines (R76); the drawing tools' preview is drawn
//! last.
//!
//! The canvas camera is `app.cam` looking straight down (`fov` 0, `pitch` π/2, `yaw` 0, `roll`
//! 0, `up` Z), so world X, Y are paper inches. It moves only through [`navigation::handle`] with
//! orbit off: pan and zoom, the 3D mouse's translation and zoom, never a turn (spec §2.12, §7.6).
//! A camera in any other pose (a new tab's, or one an up-axis change turned) is fitted again.

use std::sync::{Arc, Weak};

use cadcraft_render::Kind;
use egui::{Pos2, Rect, Sense};
use solidcraft_engine::doc::{DocumentKind, DrawingModel};
use solidcraft_engine::drawing::{PaperCurve, PaperPick};
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::render::UpAxis;
use solidcraft_gpu::sheet::{SheetFill, SheetImage, SheetScene, SheetStroke, SheetView};

use crate::SolveApp;
use crate::navigation::NavCaps;
use crate::theme::Tokens;

/// The empty space left around a fitted sheet, as a fraction of the sheet (1 %).
const FIT_MARGIN: f64 = 1.01;
/// A click picks within this many points of the cursor.
const PICK_PX: f64 = 4.0;
/// Chord tolerance of curves and text on the sheet, inches (a quarter pixel at 8× a fitted D sheet).
const TOLERANCE_IN: f64 = 0.001;

/// What the cached scene was built from: the drawing (by identity: an edit installs a new model),
/// the sheet, the theme's colours, the preview and the selection.
pub(crate) struct SceneCache {
    model: Option<Weak<DrawingModel>>,
    sheet: usize,
    colors: [egui::Color32; 6],
    preview: Vec<(PaperCurve, bool)>,
    selection: Vec<PaperPick>,
    scene: Arc<SheetScene>,
}

/// The theme colours a scene uses.
fn colors(t: &Tokens) -> [egui::Color32; 6] {
    [t.sheet_paper, t.sheet_border, t.warning, t.accent, t.sel_edge, t.sheet_surround]
}

fn rgba(c: egui::Color32) -> [u8; 4] {
    c.to_srgba_unmultiplied()
}

/// The active drawing and its active sheet's index, when the document is a drawing with a sheet.
fn active(app: &SolveApp) -> Option<(&Arc<DrawingModel>, usize)> {
    if app.session.doc.kind != DocumentKind::Drawing {
        return None;
    }
    let m = app.session.doc.drawing.as_ref()?;
    let index = app.session.drawing_sheet.min(m.sheets.len().checked_sub(1)?);
    Some((m, index))
}

/// A drawing document shows the sheet canvas (SolidCraft's and every profile's: one code path).
pub(crate) fn shows(app: &SolveApp) -> bool {
    app.session.doc.kind == DocumentKind::Drawing
}

/// The active sheet's size in inches (D landscape when there is none: the bare drawing harness).
pub(crate) fn paper_size(app: &SolveApp) -> (f64, f64) {
    active(app).and_then(|(m, i)| m.sheets.get(i)).map_or((34.0, 22.0), |s| s.size.inches(s.landscape))
}

/// A curve tessellated to points (inches).
pub(crate) fn curve_points(c: &PaperCurve) -> Vec<[f32; 2]> {
    let at = |x: f64, y: f64| [x as f32, y as f32];
    let arc = |cx: f64, cy: f64, r: f64, a0: f64, a1: f64| {
        let sweep = a1 - a0;
        if !(r > 0.0) || !sweep.is_finite() {
            return Vec::new();
        }
        // About TOLERANCE_IN of sagitta per chord, at least 8 and at most 720 chords.
        let step = 2.0 * (1.0 - (TOLERANCE_IN / r).min(1.0)).acos().max(1e-3);
        let n = ((sweep.abs() / step).ceil() as usize).clamp(8, 720);
        (0..=n).map(|i| a0 + sweep * i as f64 / n as f64).map(|a| at(cx + r * a.cos(), cy + r * a.sin())).collect()
    };
    match c {
        PaperCurve::Line { a, b } => vec![at(a.x, a.y), at(b.x, b.y)],
        PaperCurve::Arc { c, r, start, end } => arc(c.x, c.y, *r, *start, *end),
        PaperCurve::Circle { c, r } => arc(c.x, c.y, *r, 0.0, std::f64::consts::TAU),
        PaperCurve::Poly { points } => points.iter().map(|p| at(p.x, p.y)).collect(),
    }
}

/// The paper handles a pick names.
fn pick_handle(p: &PaperPick) -> Option<u64> {
    match p {
        PaperPick::ViewEdge { handle, .. } | PaperPick::Dimension { handle } | PaperPick::Annotation { handle } => Some(*handle),
        PaperPick::View { .. } => None,
    }
}

/// A stroke's width in pixels from its lineweight (mm): 0.50 mm visible lines draw 2 px, the
/// 0.25 mm thin lines 1 px.
fn width_px(lw_mm: f32) -> f32 {
    if lw_mm.is_finite() { (lw_mm / 0.25).clamp(1.0, 3.0) } else { 1.0 }
}

/// A shaded view's picture as RGBA rows, or None when its PNG does not decode to its size.
fn picture(raster: &solidcraft_engine::doc::RasterRef) -> Option<(Arc<Vec<u8>>, [u32; 2])> {
    let img = image::load_from_memory_with_format(&raster.png, image::ImageFormat::Png).ok()?.to_rgba8();
    let size = [img.width(), img.height()];
    (size[0] > 0 && size[1] > 0).then(|| (Arc::new(img.into_raw()), size))
}

/// The active sheet as the GPU pass draws it (pure: tests read it headless). R88: `paper` is
/// `[0, 0, w, h]` from the sheet's size; border and title block in `sheet_border`; sick
/// dimensions in `warning`; other entities in their layer colours on the paper; fresh pictures at
/// their `raster_rect` (R76); the preview last, in `accent`. A Drawing with no model shows the
/// surround only (an empty scene).
pub(crate) fn scene(app: &SolveApp) -> SheetScene {
    let t = Tokens::get();
    let mut out = SheetScene { paper: [0.0; 4], paper_color: rgba(t.sheet_paper), strokes: Vec::new(), fills: Vec::new(), images: Vec::new() };
    let Some((m, index)) = active(app) else { return out };
    let Some(sheet) = m.sheets.get(index) else { return out };
    let (w, h) = sheet.size.inches(sheet.landscape);
    out.paper = [0.0, 0.0, w as f32, h as f32];
    let opts = cadcraft_render::Options { tolerance: TOLERANCE_IN, lineweights: true, view_height: h, ..Default::default() };
    let list = cadcraft_render::build(&m.paper, &cadcraft_doc::Space::Paper(sheet.layout.clone()), &opts);
    let sick = solidcraft_engine::drawing::sick_dimensions(m);
    let selected: Vec<u64> = app.drawing.selection.iter().filter_map(pick_handle).collect();
    let [pr, pg, pb, _] = out.paper_color;
    let paper_rgb = cadcraft_doc::color::Rgb(pr, pg, pb);
    let border = [sheet.border, sheet.title_block];
    for p in &list.prims {
        let color = if border.contains(&p.handle) {
            rgba(t.sheet_border)
        } else if sick.contains(&p.handle.0) {
            rgba(t.warning)
        } else if selected.contains(&p.handle.0) {
            rgba(t.sel_edge)
        } else {
            let c = p.display_rgb(paper_rgb);
            [c.0, c.1, c.2, p.alpha()]
        };
        let pts = list.points(p);
        match p.kind {
            Kind::Polyline if pts.len() >= 2 => {
                out.strokes.push(SheetStroke { points: pts.iter().map(|v| [v.x as f32, v.y as f32]).collect(), color, width_px: width_px(p.lw) });
            }
            Kind::Tris | Kind::Mask => {
                let color = if p.kind == Kind::Mask { out.paper_color } else { color };
                let triangles =
                    pts.as_chunks::<3>().0.iter().map(|[a, b, c]| [[a.x as f32, a.y as f32], [b.x as f32, b.y as f32], [c.x as f32, c.y as f32]]);
                out.fills.push(SheetFill { triangles: triangles.collect(), color });
            }
            // Point markers and construction lines have no place on a drawing sheet.
            _ => {}
        }
    }
    for v in m.views.values().filter(|v| v.sheet == index && v.shaded) {
        if !solidcraft_engine::drawing::raster_fresh(m, v) {
            continue;
        }
        let (Some(rect), Some(raster)) = (solidcraft_engine::drawing::raster_rect(m, v), v.raster.as_ref()) else { continue };
        if let Some((rgba, size)) = picture(raster) {
            out.images.push(SheetImage { rect: rect.map(|x| x as f32), rgba, size });
        }
    }
    let accent = rgba(t.accent);
    for (curve, hidden) in &app.drawing.preview {
        let color = if *hidden { [accent[0], accent[1], accent[2], accent[3] / 2] } else { accent };
        let points = curve_points(curve);
        if points.len() >= 2 {
            out.strokes.push(SheetStroke { points, color, width_px: 1.0 });
        }
    }
    out
}

/// The scene to draw: the cached one while nothing it was built from changed.
fn cached_scene(app: &mut SolveApp) -> Arc<SheetScene> {
    let t = Tokens::get();
    let model = active(app).map(|(m, _)| Arc::downgrade(m));
    let sheet = app.session.drawing_sheet;
    let colors = colors(&t);
    if let Some(c) = &app.drawing.scene {
        let same_model = match (&c.model, &model) {
            (Some(a), Some(b)) => a.ptr_eq(b),
            (None, None) => true,
            _ => false,
        };
        if same_model && c.sheet == sheet && c.colors == colors && c.preview == app.drawing.preview && c.selection == app.drawing.selection {
            return c.scene.clone();
        }
    }
    let scene = Arc::new(scene(app));
    app.drawing.scene = Some(SceneCache {
        model,
        sheet,
        colors,
        preview: app.drawing.preview.clone(),
        selection: app.drawing.selection.clone(),
        scene: scene.clone(),
    });
    scene
}

/// Points per paper inch at the camera's zoom in `rect`.
fn points_per_inch(app: &SolveApp, rect: Rect) -> f64 {
    f64::from(rect.height().max(1.0)) / (2.0 * app.cam.half_height())
}

/// The screen point (points, f64) of paper point `paper` in the canvas `rect`.
pub(crate) fn screen_of_f64(app: &SolveApp, rect: Rect, paper: [f64; 2]) -> [f64; 2] {
    let k = points_per_inch(app, rect);
    let c = app.cam.frame_coords(Vec3::new(paper[0], paper[1], 0.0));
    [f64::from(rect.center().x) + c.x * k, f64::from(rect.center().y) - c.y * k]
}

/// The paper point under screen point `p` (points, f64) of the canvas `rect`.
pub(crate) fn paper_at_f64(app: &SolveApp, rect: Rect, p: [f64; 2]) -> [f64; 2] {
    let k = points_per_inch(app, rect);
    let (right, up, _) = app.cam.basis();
    let (dx, dy) = ((p[0] - f64::from(rect.center().x)) / k, (f64::from(rect.center().y) - p[1]) / k);
    let w = app.cam.target + right * dx + up * dy;
    [w.x, w.y]
}

/// Paper inches under a screen point of the canvas `rect`.
pub(crate) fn paper_at(app: &SolveApp, rect: Rect, p: Pos2) -> [f64; 2] {
    paper_at_f64(app, rect, [f64::from(p.x), f64::from(p.y)])
}

/// The screen point of a paper point (rounded to egui's f32 points). The drawing tools place
/// their handles with it (Tasks 6-9); until then only tests call it.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn screen_of(app: &SolveApp, rect: Rect, paper: [f64; 2]) -> Pos2 {
    let [x, y] = screen_of_f64(app, rect, paper);
    Pos2::new(x as f32, y as f32)
}

/// Whether the camera looks straight down on the paper, as the canvas needs.
fn on_sheet(cam: &solidcraft_engine::render::Camera) -> bool {
    cam.fov == 0.0
        && cam.pitch == std::f64::consts::FRAC_PI_2
        && cam.yaw == 0.0
        && cam.roll == 0.0
        && cam.up == UpAxis::Z
        && cam.is_valid()
        && cam.target.z.abs() < 1e-6
}

/// Fit the active sheet in the canvas (first show of a drawing, `ui.view_fit`, `ui.home`, Home):
/// in the part of `rect` the browser and the floating cards leave (`shell::content_rect`), as
/// the flat pattern's fit does.
pub(crate) fn fit(app: &mut SolveApp, rect: Rect) {
    app.drawing.fit_pending = false;
    if !(rect.width() >= 1.0 && rect.height() >= 1.0) {
        return;
    }
    let content = crate::shell::content_rect(app, rect);
    let area = if content.width() >= 50.0 && content.height() >= 50.0 { content } else { rect };
    let (w, h) = paper_size(app);
    let scale = (f64::from(area.width()) / (w * FIT_MARGIN)).min(f64::from(area.height()) / (h * FIT_MARGIN));
    let cam = &mut app.cam;
    let (dx, dy) = (f64::from(area.center().x - rect.center().x), f64::from(area.center().y - rect.center().y));
    cam.target = Vec3::new(w / 2.0 - dx / scale, h / 2.0 + dy / scale, 0.0);
    cam.yaw = 0.0;
    cam.pitch = std::f64::consts::FRAC_PI_2;
    cam.roll = 0.0;
    cam.fov = 0.0;
    cam.up = UpAxis::Z;
    cam.distance = f64::from(rect.height()) / (2.0 * scale) / 0.4;
    app.cam_anim = None;
    app.viewport.orbit_free = None;
}

/// A primary click with no tool: what is under it becomes the selection (Shift adds it).
fn select(app: &mut SolveApp, rect: Rect, p: Pos2, add: bool) {
    let at = paper_at(app, rect, p);
    let tol = PICK_PX / points_per_inch(app, rect);
    let hit = solidcraft_engine::drawing::pick(&app.session, at, tol);
    if !add {
        app.drawing.selection.clear();
    }
    if let Some(hit) = hit
        && !app.drawing.selection.contains(&hit)
    {
        app.drawing.selection.push(hit);
    }
}

pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let rect = ui.max_rect();
    app.viewport.rect = Some(rect);
    crate::scenario::publish_rect("sheet_canvas", rect);
    let resp = ui.interact(rect, ui.id().with("sheet_canvas"), Sense::click_and_drag());
    if app.drawing.fit_pending || !on_sheet(&app.cam) {
        fit(app, rect);
    }
    let input = crate::navigation::handle(app, ui, rect, &resp, NavCaps { orbit: false, pick: true });
    let navigating = input.mode.is_some() || (input.primary_nav.is_some() && input.primary_down);
    // Placing views, dragging a placed one, a double-click's Edit View (Task 6).
    // General Dimension, and a double-click's Edit Dimension (Task 8), before placing views.
    // Text, leader, centre mark, centreline and hole note tools, and a double-click's Edit Note
    // (Task 9).
    let used = crate::drawing_notes_tool::input(app, rect, &resp, &input, navigating)
        || crate::drawing_dim_tool::input(app, rect, &resp, &input, navigating)
        || crate::drawing_place::input(app, ui, rect, &resp, &input, navigating);
    if resp.clicked()
        && !navigating
        && !used
        && app.drawing.tool.is_none()
        && let Some(p) = input.hover.filter(|_| input.inside)
    {
        select(app, rect, p, input.mods.shift);
    }
    // The viewport's right-click: the profile's ring where it lays one out for a sheet, else the
    // drawing's list (`context_menu::items` sends a sheet's viewport target to `drawing_menu`).
    if resp.secondary_clicked()
        && input.delta == egui::Vec2::ZERO
        && !crate::mouse_gesture::took_press(app)
        && let Some(p) = input.hover
    {
        // A right-click on a dimension selects it, so its list opens (Task 8).
        if app.drawing.tool.is_none()
            && let Some(dim) = crate::drawing_dim_tool::dimension_at(app, rect, p)
        {
            app.drawing.selection = vec![PaperPick::Dimension { handle: dim }];
        }
        crate::context_menu::open_for(app, p, crate::context_menu::Target::Viewport);
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, t.sheet_surround);
    let scene = cached_scene(app);
    if app.viewport.gpu.is_some() {
        let ppp = ui.ctx().pixels_per_point();
        let [cx, cy] = paper_at(app, rect, rect.center());
        let view = SheetView {
            center: [cx as f32, cy as f32],
            px_per_in: (points_per_inch(app, rect) * f64::from(ppp)) as f32,
            size_px: [rect.width() * ppp, rect.height() * ppp],
        };
        painter.add(egui_wgpu::Callback::new_paint_callback(rect, crate::gpu::SheetCallback { scene, view }));
    }
    // The on-sheet cube over the sheet, beside the base view's preview.
    crate::drawing_place::paint(app, ui, rect);
}

#[cfg(test)]
#[path = "sheet_canvas_tests.rs"]
mod tests;
