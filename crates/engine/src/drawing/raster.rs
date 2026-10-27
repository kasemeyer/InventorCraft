//! Shaded views (spec §4.5): a view whose style is shaded carries a picture of its model drawn
//! by the host's renderer, flat grey, orthographic along the view's orientation, at view scale.
//! The picture is data on the view (`DrawingView::raster`), never a paper entity: it is drawn
//! under the view's curves (the edge overlay) by whoever shows or prints the sheet, over the
//! rectangle [`raster_rect`], so it is never picked and never written to DXF.
//!
//! The engine (L4) cannot call the GPU crate (L5): pictures go through the renderer the hosts
//! already install with `capture::install` (the app, the CLI's `run`, `snapshot` and `mcp`), by
//! way of [`CaptureRasterizer`], the session's default [`ViewRasterizer`] (R18).

use solidcraft_doc::{DrawingModel, DrawingView, RasterRef};
use solidcraft_geom::{Aabb3, Vec2};
use solidcraft_render::{Camera, Rgb, Scene, SceneMesh};

use super::mapper::{self, Seg};
use super::model_ref::LoadedModel;
use super::project::camera_for;
use crate::{EngineError, Session};

/// Who draws shaded views. The app, the CLI (`run`, `snapshot`, `mcp`) already install a picture renderer
/// with `capture::install`; `CaptureRasterizer` draws through it, so no host installs anything new.
pub trait ViewRasterizer: Send + Sync {
    fn rasterize(&self, scene: &Scene, cam: &Camera, px_w: u32, px_h: u32) -> Result<crate::capture::Picture, String>;
}

/// The default: `capture::installed()`; with none installed it returns Err and the view goes stale.
pub struct CaptureRasterizer;

impl ViewRasterizer for CaptureRasterizer {
    fn rasterize(&self, scene: &Scene, cam: &Camera, px_w: u32, px_h: u32) -> Result<crate::capture::Picture, String> {
        crate::capture::rgba(scene, cam, px_w, px_h)
    }
}

/// The colour of every body of a shaded view (owner default; §8.5).
pub const FLAT_GREY: Rgb = Rgb(190, 190, 190);
/// A shaded view's resolution on screen and in a PDF (spec §4.5).
pub const SCREEN_PX_PER_IN: f64 = 200.0;
pub const PDF_PX_PER_IN: f64 = 300.0;
/// = gpu::capture::MAX_SIDE (gpu/src/capture.rs:27); pinned by D-2 Task 3 (R41)
pub const MAX_RASTER_SIDE: u32 = 8192;

/// The background a shaded picture is drawn on, keyed out to transparency afterwards (the
/// renderers paint a background whatever the scene says).
const KEY: Rgb = Rgb(255, 0, 255);

/// What a view's raster is after a command: drawn again (`fresh`), kept from before because
/// nothing could draw it (`stale`), or none (the view is not shaded).
pub(crate) const FRESH: &str = "fresh";
pub(crate) const STALE: &str = "stale";
pub(crate) const NONE: &str = "none";

/// A picture to ask for: what it shows, from where, and its size in pixels.
struct Shot {
    scene: Scene,
    cam: Camera,
    width: u32,
    height: u32,
}

/// The pixel size of a picture `w` × `h` pixels wide and high (fractions round up; a few
/// millionths over a whole pixel are rounding, not a pixel), its long side at most
/// `MAX_RASTER_SIDE` with the aspect kept.
pub(crate) fn pixel_size(w: f64, h: f64) -> (u32, u32) {
    let up = |v: f64| (v - 1e-6).ceil().max(1.0);
    let (w, h) = (up(w), up(h));
    let max = f64::from(MAX_RASTER_SIDE);
    let long = w.max(h);
    let (w, h) = if long > max { ((w * max / long).round().max(1.0), (h * max / long).round().max(1.0)) } else { (w, h) };
    (w.min(max) as u32, h.min(max) as u32)
}

/// The shaded picture of view `v` of `model` at `px_per_in`: the model's visible bodies in
/// `FLAT_GREY`, no edges, orthographic through `camera_for(&v.orientation, …)`, framing the box
/// of the model seen along the view (the view's paper extents). None: no body to draw.
fn shot(model: &LoadedModel, v: &DrawingView, px_per_in: f64) -> Option<Shot> {
    let session = &model.session;
    let world = session.world_state();
    let meshes: Vec<_> = world.bodies.iter().filter(|b| session.body_visible(&b.name)).map(|b| b.mesh()).collect();
    let bounds = meshes.iter().fold(Aabb3::EMPTY, |acc, m| acc.union(&m.bounds()));
    if bounds.is_empty() {
        return None;
    }
    let (target, extent) = (bounds.center(), bounds.diagonal().max(1.0));
    let (right, up, _) = camera_for(&v.orientation, target, extent).basis();
    let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in meshes.iter().flat_map(|m| m.positions.iter()) {
        let d = *p - target;
        let q = Vec2::new(d.dot(right), d.dot(up));
        lo = Vec2::new(lo.x.min(q.x), lo.y.min(q.y));
        hi = Vec2::new(hi.x.max(q.x), hi.y.max(q.y));
    }
    let size = hi - lo;
    if !(size.x > 0.0 && size.y > 0.0 && size.x.is_finite() && size.y.is_finite()) {
        return None;
    }
    let mid = (lo + hi) * 0.5;
    let centre = target + right * mid.x + up * mid.y;
    // Pixels per model mm.
    let k = v.scale / 25.4 * px_per_in;
    let (width, height) = pixel_size(size.x * k, size.y * k);
    let mut cam = camera_for(&v.orientation, centre, extent);
    // Orthographic: half the picture's height is `distance * 0.4` (`Camera::half_height`), so
    // the picture is exactly the box's height, and its width the box's within a pixel.
    cam.distance = size.y * 0.5 / 0.4;
    cam.fov = 0.0;
    let scene = Scene {
        meshes: meshes.into_iter().map(|m| SceneMesh::solid(m, FLAT_GREY)).collect(),
        background: Some((KEY, KEY)),
        radius: bounds.diagonal() * 0.5 + bounds.center().dist(centre) + 1.0,
        ..Scene::default()
    };
    Some(Shot { scene, cam, width, height })
}

/// `rgba` with the key background made transparent. A pixel blended from the key and a grey
/// `c` at coverage `a` is `(a·c + (1−a)·255, a·c, a·c + (1−a)·255)`, so `1 − a` is how far red
/// and blue stand above green; the grey is recovered from it.
fn keyed(mut rgba: Vec<u8>) -> Vec<u8> {
    for px in rgba.as_chunks_mut::<4>().0 {
        let [r, g, b, alpha] = px.map(f64::from);
        let fade = (((r + b) * 0.5 - g) / 255.0).clamp(0.0, 1.0);
        let a = 1.0 - fade;
        if a * 255.0 < 0.5 {
            px.copy_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let back = fade * 255.0;
        let un = |c: f64, key: f64| ((c - key) / a).round().clamp(0.0, 255.0) as u8;
        px.copy_from_slice(&[un(r, back), un(g, 0.0), un(b, back), (a * alpha).round().clamp(0.0, 255.0) as u8]);
    }
    rgba
}

/// The view's shaded picture at `px_per_in`, or None when no rasterizer can draw.
pub(crate) fn rasterize(s: &Session, model: &LoadedModel, v: &DrawingView, px_per_in: f64) -> crate::Result<Option<RasterRef>> {
    let Some(rasterizer) = s.view_rasterizer.as_deref() else { return Ok(None) };
    let Some(shot) = shot(model, v, px_per_in) else { return Ok(None) };
    let (width, height) = (shot.width, shot.height);
    let picture = match rasterizer.rasterize(&shot.scene, &shot.cam, width, height) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("shaded view {}: {e}", v.identifier);
            return Ok(None);
        }
    };
    if picture.rgba.len() as u64 != u64::from(width) * u64::from(height) * 4 {
        log::warn!("shaded view {}: the renderer returned {} bytes for {width} × {height} pixels", v.identifier, picture.rgba.len());
        return Ok(None);
    }
    let png = solidcraft_render::png(width as usize, height as usize, keyed(picture.rgba))
        .ok_or_else(|| EngineError::Other(format!("shaded view {}: PNG encoding failed", v.identifier)))?;
    Ok(Some(RasterRef { width, height, png }))
}

/// Brings `v`'s raster up to date with its style: none unless shaded; a shaded view is drawn
/// again at `SCREEN_PX_PER_IN` from `model`, or keeps its last raster when the model is not
/// loaded or nothing can draw it. Returns `fresh`, `stale` or `none`.
pub(crate) fn refresh(s: &Session, model: Option<&LoadedModel>, v: &mut DrawingView) -> &'static str {
    if !v.shaded {
        v.raster = None;
        return NONE;
    }
    match model.map(|m| rasterize(s, m, v, SCREEN_PX_PER_IN)) {
        Some(Ok(Some(raster))) => {
            v.raster = Some(raster);
            FRESH
        }
        Some(Err(e)) => {
            log::warn!("{e}");
            STALE
        }
        _ => STALE,
    }
}

/// Whether `r`, the stored picture of a view whose raster lies over `rect`, is what the view
/// draws now: its pixel size is the size of `rect` (paper inches) at `SCREEN_PX_PER_IN`, within
/// one pixel each way (the picture is framed on the model's mesh, the rectangle on the curves).
pub(crate) fn fits(r: &RasterRef, rect: [f64; 4]) -> bool {
    let (w, h) = pixel_size((rect[2] - rect[0]) * SCREEN_PX_PER_IN, (rect[3] - rect[1]) * SCREEN_PX_PER_IN);
    r.width.abs_diff(w) <= 1 && r.height.abs_diff(h) <= 1
}

/// Whether shaded view `v` of `m` has a fresh picture ([`fits`] its [`raster_rect`]).
pub(crate) fn fresh(m: &DrawingModel, v: &DrawingView) -> bool {
    match (&v.raster, raster_rect(m, v)) {
        (Some(r), Some(rect)) => fits(r, rect),
        _ => false,
    }
}

/// Where view `v`'s raster lies on its sheet, `[x0, y0, x1, y1]` in paper inches: the box of
/// its visible curves (the picture frames the model as the curves do), drawn under them.
pub fn raster_rect(m: &DrawingModel, v: &DrawingView) -> Option<[f64; 4]> {
    let layout = m.paper.layout(&m.sheets.get(v.sheet)?.layout)?;
    let mut b = cadcraft_doc::geom::Bounds2::EMPTY;
    for e in layout.entities.iter() {
        if mapper::xkey_of(e).is_some_and(|k| k.view == v.id && k.seg != Seg::Mark) && e.common.layer == mapper::VISIBLE_LAYER {
            b = b.union(&cadcraft_doc::entity_bounds(&m.paper, e, 0));
        }
    }
    (!b.is_empty()).then_some([b.min.x, b.min.y, b.max.x, b.max.y])
}
