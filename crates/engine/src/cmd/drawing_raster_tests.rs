#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Shaded views (Task 9): the raster drawn by the session's `ViewRasterizer` (a recording stub
//! here, never a GPU), flat grey, 200 px per paper inch, clamped to 8192 px, kept when nothing
//! can draw it, dropped when the view is no longer shaded, and never a paper entity. VIEW1 is the
//! block's front view (4 x 3 in) at [10, 8].

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cadcraft_doc::{EntityKind, Handle};
use serde_json::{Value, json};
use solidcraft_doc::{DrawingModel, DrawingView};
use solidcraft_render::{Camera, Rgb, Scene};

use crate::Session;
use crate::capture::Picture;
use crate::cmd::drawing::drawing_fixture::{block_drawing, edit_model};
use crate::drawing::mapper::xkey_of;
use crate::drawing::{FLAT_GREY, PaperPick, ViewRasterizer, pick, raster_rect};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn model(s: &Session) -> &DrawingModel {
    s.doc.drawing.as_deref().unwrap()
}

fn view(s: &Session, id: u64) -> &DrawingView {
    model(s).views.get(&id).unwrap()
}

/// One call to the stub: the size asked for and what the scene held.
#[derive(Clone, Debug)]
struct Call {
    width: u32,
    height: u32,
    colors: Vec<Rgb>,
    face_colors: usize,
    lines: usize,
    grid: usize,
    fov: f64,
}

/// Records each call and draws a flat grey picture whose first column is the background it was
/// given (so keying it out shows).
#[derive(Default)]
struct Recording(Mutex<Vec<Call>>);

impl Recording {
    fn calls(&self) -> Vec<Call> {
        self.0.lock().unwrap().clone()
    }
}

impl ViewRasterizer for Recording {
    fn rasterize(&self, scene: &Scene, cam: &Camera, px_w: u32, px_h: u32) -> Result<Picture, String> {
        self.0.lock().unwrap().push(Call {
            width: px_w,
            height: px_h,
            colors: scene.meshes.iter().map(|m| m.color).collect(),
            face_colors: scene.meshes.iter().map(|m| m.face_colors.len()).sum(),
            lines: scene.lines.len(),
            grid: scene.grid.len(),
            fov: cam.fov,
        });
        let back = scene.background.map_or(Rgb(255, 255, 255), |(top, _)| top);
        let mut rgba = Vec::with_capacity((px_w * px_h * 4) as usize);
        for _ in 0..px_h {
            for x in 0..px_w {
                let Rgb(r, g, b) = if x == 0 { back } else { FLAT_GREY };
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
        }
        Ok(Picture { rgba, renderer: "gpu" })
    }
}

/// The block drawing with a recording rasterizer.
fn recorded(name: &str) -> (Session, String, Arc<Recording>) {
    let (mut s, block) = block_drawing(name);
    let rec = Arc::new(Recording::default());
    s.view_rasterizer = Some(rec.clone());
    (s, block, rec)
}

/// `drawing.view.base` of the block: front unless said, shaded, at [10, 8].
fn shaded_view(s: &mut Session, block: &str, extra: Value) -> (u64, Value) {
    let mut p = json!({"model": block, "orientation": "front", "lines": "hidden", "shaded": true, "scale": 1, "position": [10, 8]});
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    let r = run(s, "drawing.view.base", p);
    (r["view"].as_u64().unwrap(), r)
}

fn decoded(s: &Session, id: u64) -> image::RgbaImage {
    image::load_from_memory(&view(s, id).raster.as_ref().unwrap().png).unwrap().to_rgba8()
}

/// The handles of view `id`'s keyed curves and how many are on `layer`.
/// View `id`'s entities on `layer`: (visible, invisible). Hidden Line Removed keeps hidden
/// curves as invisible entities (R61).
fn on_layer(s: &Session, id: u64, layer: &str) -> (usize, usize) {
    let m = model(s);
    let layout = m.paper.layout(&m.sheets[0].layout).unwrap();
    let on: Vec<bool> =
        layout.entities.iter().filter(|e| xkey_of(e).is_some_and(|k| k.view == id) && e.common.layer == layer).map(|e| e.common.visible).collect();
    (on.iter().filter(|v| **v).count(), on.iter().filter(|v| !**v).count())
}

fn handles(s: &Session) -> BTreeSet<Handle> {
    let m = model(s);
    m.paper.layout(&m.sheets[0].layout).unwrap().entities.iter().map(|e| e.handle).collect()
}

#[test]
fn a_shaded_view_asks_for_200_px_per_inch() {
    let (mut s, block, rec) = recorded("a_shaded_view_asks_for_200_px_per_inch");
    let (v1, r) = shaded_view(&mut s, &block, json!({}));
    assert_eq!(r["raster"], "fresh");
    let calls = rec.calls();
    assert_eq!(calls.iter().map(|c| (c.width, c.height)).collect::<Vec<_>>(), vec![(800, 600)]);
    let raster = view(&s, v1).raster.clone().unwrap();
    assert_eq!((raster.width, raster.height), (800, 600));
    let img = decoded(&s, v1);
    assert_eq!(img.dimensions(), (800, 600));
    // Flat grey where the model is; the background keyed out to transparency.
    assert_eq!(img.get_pixel(400, 300).0, [190, 190, 190, 255]);
    assert_eq!(img.get_pixel(0, 300).0[3], 0);
    // It lies under the view's curves: the 4 x 3 in box about [10, 8].
    let rect = raster_rect(model(&s), view(&s, v1)).unwrap();
    for (got, want) in rect.iter().zip([8.0, 6.5, 12.0, 9.5]) {
        assert!((got - want).abs() <= 1e-6, "{rect:?}");
    }
    // Regenerating draws it again in place: same view, same entities, a new picture.
    let before = handles(&s);
    edit_model(&block, &[("solid.hole", json!({"position": [25.4, 0, 38.1], "direction": [0, 1, 0], "diameter": "0.25 in", "name": "Drill2"}))]);
    run(&mut s, "drawing.update", json!({"views": [v1]}));
    assert_eq!(rec.calls().len(), 2);
    let after = handles(&s);
    assert!(before.is_subset(&after) && after.len() > before.len(), "{before:?} {after:?}");
    assert_eq!(view(&s, v1).raster.as_ref().map(|r| (r.width, r.height)), Some((800, 600)));
}

#[test]
fn the_shading_is_flat_grey_without_edges() {
    let (mut s, block, rec) = recorded("the_shading_is_flat_grey_without_edges");
    shaded_view(&mut s, &block, json!({}));
    let call = rec.calls().pop().unwrap();
    assert_eq!(call.colors, vec![FLAT_GREY]);
    assert_eq!(FLAT_GREY, Rgb(190, 190, 190));
    assert_eq!((call.face_colors, call.lines, call.grid), (0, 0, 0));
    assert_eq!(call.fov, 0.0);
}

#[test]
fn without_a_rasterizer_the_raster_goes_stale() {
    let (mut s, block, _) = recorded("without_a_rasterizer_the_raster_goes_stale");
    let (v1, _) = shaded_view(&mut s, &block, json!({"orientation": "top"}));
    let png = view(&s, v1).raster.clone().unwrap().png;
    let shown_hidden = on_layer(&s, v1, "HIDDEN");
    assert_eq!(shown_hidden, (2, 0));
    s.view_rasterizer = None;
    let r = run(&mut s, "drawing.view.set_style", json!({"view": v1, "lines": "removed"}));
    assert_eq!(r["raster"], "stale");
    assert_eq!(view(&s, v1).raster.as_ref().unwrap().png, png);
    assert_eq!(on_layer(&s, v1, "HIDDEN"), (0, 2));
    // A rasterizer that fails is the same as none.
    struct Failing;
    impl ViewRasterizer for Failing {
        fn rasterize(&self, _: &Scene, _: &Camera, _: u32, _: u32) -> Result<Picture, String> {
            Err("no adapter".into())
        }
    }
    s.view_rasterizer = Some(Arc::new(Failing));
    let r = run(&mut s, "drawing.view.set_style", json!({"view": v1, "lines": "hidden"}));
    assert_eq!(r["raster"], "stale");
    assert_eq!(view(&s, v1).raster.as_ref().unwrap().png, png);
    assert_eq!(on_layer(&s, v1, "HIDDEN"), shown_hidden);
}

#[test]
fn a_huge_shaded_view_is_clamped() {
    let (mut s, block, rec) = recorded("a_huge_shaded_view_is_clamped");
    let (v1, r) = shaded_view(&mut s, &block, json!({"scale": 25}));
    assert_eq!(r["raster"], "fresh");
    assert_eq!(rec.calls().iter().map(|c| (c.width, c.height)).collect::<Vec<_>>(), vec![(8192, 6144)]);
    assert_eq!(view(&s, v1).raster.as_ref().map(|r| (r.width, r.height)), Some((8192, 6144)));
}

#[test]
fn unshading_drops_the_raster() {
    let (mut s, block, rec) = recorded("unshading_drops_the_raster");
    let (v1, _) = shaded_view(&mut s, &block, json!({}));
    let r = run(&mut s, "drawing.view.set_style", json!({"view": v1, "shaded": false}));
    assert_eq!(r["raster"], "none");
    assert_eq!(view(&s, v1).raster, None);
    assert_eq!(rec.calls().len(), 1);
    // Undo brings the shaded view and its picture back.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(view(&s, v1).raster.as_ref().map(|r| (r.width, r.height)), Some((800, 600)));
}

#[test]
fn lines_still_draw_over_the_shading() {
    let (mut s, block, rec) = recorded("lines_still_draw_over_the_shading");
    let (v1, r) = shaded_view(&mut s, &block, json!({"orientation": "top"}));
    assert_eq!(r["raster"], "fresh");
    assert_eq!(on_layer(&s, v1, "HIDDEN"), (2, 0));
    assert!(on_layer(&s, v1, "VISIBLE").0 >= 4);
    assert_eq!(on_layer(&s, v1, "VISIBLE").1, 0);
    // The top view is 4 x 1 in.
    assert_eq!(rec.calls().iter().map(|c| (c.width, c.height)).collect::<Vec<_>>(), vec![(800, 200)]);
}

#[test]
fn shading_an_unshaded_view_draws_its_raster() {
    let (mut s, block, rec) = recorded("shading_an_unshaded_view_draws_its_raster");
    let (v1, r) = shaded_view(&mut s, &block, json!({"shaded": false}));
    assert_eq!(r["raster"], "none");
    assert_eq!((rec.calls().len(), view(&s, v1).raster.is_none()), (0, true));
    let before = handles(&s);
    let r = run(&mut s, "drawing.view.set_style", json!({"view": v1, "shaded": true}));
    assert_eq!(r["raster"], "fresh");
    assert_eq!(rec.calls().iter().map(|c| (c.width, c.height)).collect::<Vec<_>>(), vec![(800, 600)]);
    assert_eq!(view(&s, v1).raster.as_ref().map(|r| (r.width, r.height)), Some((800, 600)));
    // The overlay is untouched and nothing joined the paper.
    assert_eq!(handles(&s), before);
}

#[test]
fn rescaling_draws_the_raster_again() {
    let (mut s, block, rec) = recorded("rescaling_draws_the_raster_again");
    let (v1, _) = shaded_view(&mut s, &block, json!({}));
    let r = run(&mut s, "drawing.view.set_scale", json!({"view": v1, "scale": 0.5}));
    assert_eq!(r["raster"], "fresh");
    assert_eq!(rec.calls().iter().map(|c| (c.width, c.height)).collect::<Vec<_>>(), vec![(800, 600), (400, 300)]);
    assert_eq!(view(&s, v1).raster.as_ref().map(|r| (r.width, r.height)), Some((400, 300)));
    let rect = raster_rect(model(&s), view(&s, v1)).unwrap();
    for (got, want) in rect.iter().zip([9.0, 7.25, 11.0, 8.75]) {
        assert!((got - want).abs() <= 1e-6, "{rect:?}");
    }
    // An unshaded view's scale draws nothing.
    run(&mut s, "drawing.view.set_style", json!({"view": v1, "shaded": false}));
    let r = run(&mut s, "drawing.view.set_scale", json!({"view": v1, "scale": 1}));
    assert_eq!(r["raster"], "none");
    assert_eq!(rec.calls().len(), 2);
}

#[test]
fn the_raster_is_never_on_the_paper_nor_picked() {
    let (mut s, block, _) = recorded("the_raster_is_never_on_the_paper_nor_picked");
    let (v1, _) = shaded_view(&mut s, &block, json!({}));
    let m = model(&s);
    let layout = m.paper.layout(&m.sheets[0].layout).unwrap();
    assert!(!layout.entities.iter().any(|e| matches!(e.kind, EntityKind::Image(_))));
    // Inside the view, away from its curves: the view, nothing else.
    assert_eq!(pick(&s, [9.0, 7.0], 0.05), Some(PaperPick::View { view: v1 }));
}

/// Draws as `capture::install_test_renderer` does (a flat picture whose first byte counts the
/// scene's meshes, so the capture tests running beside this one see what they expect), and
/// counts the 800 x 600 calls.
static CAPTURED: AtomicUsize = AtomicUsize::new(0);

#[test]
fn the_default_rasterizer_is_the_installed_renderer() {
    assert!(Session::default().view_rasterizer.is_some());
    let renderer: Arc<crate::capture::Renderer> = Arc::new(|scene: &Scene, _: &Camera, w: u32, h: u32| {
        if (w, h) == (800, 600) {
            CAPTURED.fetch_add(1, Ordering::SeqCst);
        }
        let mut rgba = vec![200; (w * h * 4) as usize];
        if let Some(first) = rgba.first_mut() {
            *first = scene.meshes.len() as u8;
        }
        Ok(Picture { rgba, renderer: "gpu" })
    });
    let (mut s, block) = block_drawing("the_default_rasterizer_is_the_installed_renderer");
    // Other tests install their own renderer at any time: try again while one replaced ours.
    for _ in 0..10 {
        crate::capture::install(renderer.clone());
        let before = CAPTURED.load(Ordering::SeqCst);
        let (v1, r) = shaded_view(&mut s, &block, json!({"position": [10, 8]}));
        let ours = crate::capture::installed().is_some_and(|r| Arc::ptr_eq(&r, &renderer));
        if ours {
            assert_eq!(r["raster"], "fresh");
            assert_eq!(CAPTURED.load(Ordering::SeqCst) - before, 1);
            assert_eq!(view(&s, v1).raster.as_ref().map(|r| (r.width, r.height)), Some((800, 600)));
            return;
        }
        run(&mut s, "drawing.view.delete", json!({"view": v1}));
    }
    panic!("the test renderer was replaced every time");
}
