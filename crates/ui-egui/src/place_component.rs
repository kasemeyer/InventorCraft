//! Place Component (R171): after the file picker a ghost of the design follows the pointer, on
//! the face under it or else the active component's XY plane (the paste's placement), and each
//! left click places one occurrence where the ghost is (`component.insert_design` with
//! `reuse`, one undo step each); the ghost goes on for further copies until Esc, the right-click
//! list's Done (a right-click where the profile has no marking menu) or another command ends it.
//! The first component of an empty assembly is placed at the origin and grounded at once
//! (R103/R123), and the ghost still follows for more copies. Both profiles place this way.

use std::sync::Arc;

use egui::Pos2;
use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::doc::{IDENTITY, Mat, mat_inverse, mat_mul};
use solidcraft_engine::geom::{Aabb3, Mesh, Vec3};

use crate::SolveApp;
use crate::gpu::GpuScene;
use crate::preview::{Built, Colors};

/// Place Component under way.
pub(crate) struct Place {
    /// The design file, as the picker gave it.
    path: String,
    /// The design's bodies as a fresh insert puts them (world, the occurrence at identity in the
    /// active component), and that component's world frame.
    meshes: Vec<Arc<Mesh>>,
    frame: Mat,
    /// The placement a copy starts from (`insert_design` with `reuse` copies the first
    /// occurrence of a design already linked; a fresh one is the identity): its rotation is the
    /// ghost's.
    base: Mat,
    /// Where the next copy goes, while the pointer is over the view.
    ghost: Option<Ghost>,
}

/// The next copy's placement in the active component, and the `translate` that puts it there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Ghost {
    pub(crate) local: Mat,
    translate: [f64; 3],
}

pub(crate) fn live(app: &SolveApp) -> bool {
    app.viewport.place.is_some()
}

/// The ghost's placement in the active component (tests and the screenshot).
pub(crate) fn ghost(app: &SolveApp) -> Option<Mat> {
    app.viewport.place.as_ref()?.ghost.map(|g| g.local)
}

/// The picker chose `path`: an empty assembly gets it at the origin, grounded, at once; then the
/// ghost follows the pointer.
pub(crate) fn begin(app: &mut SolveApp, path: String) {
    end(app);
    if !app.session.doc.occurrences.iter().any(|o| o.parent == 0) {
        if app.run("component.insert_design", json!({ "path": path })).is_err() {
            return;
        }
        app.fit_view();
    }
    let Some(meshes) = design_meshes(&app.session, &path) else {
        app.set_status(crate::i18n::tr("ui.place.unreadable"), true);
        return;
    };
    let base = base_of(&app.session, &path);
    app.viewport.place = Some(Place { path, meshes, frame: frame(app), base, ghost: None });
}

/// End Place Component; true when it was running (Esc's answer).
pub(crate) fn end(app: &mut SolveApp) -> bool {
    let was = app.viewport.place.take().is_some();
    if was {
        app.preview.reset();
    }
    was
}

/// The status bar's prompt while placing.
pub(crate) fn prompt(app: &SolveApp) -> Option<&'static str> {
    live(app).then_some("ui.place.prompt")
}

/// The active component's world frame (the paste's: `edit_clipboard::paste_placement`).
fn frame(app: &SolveApp) -> Mat {
    let s = &app.session;
    let path = s.active_occurrence_path.clone().unwrap_or_else(|| s.doc.first_path(s.active_component));
    s.occurrence_placement(&path).map(|p| p.world).unwrap_or(IDENTITY)
}

/// The design's bodies as a fresh insert into a scratch copy of the session puts them.
fn design_meshes(s: &Session, path: &str) -> Option<Vec<Arc<Mesh>>> {
    let mut scratch = s.scratch();
    let out = scratch.execute("component.insert_design", &json!({ "path": path })).ok()?;
    let st = scratch.world_state();
    let meshes: Vec<Arc<Mesh>> =
        out.get("bodies")?.as_array()?.iter().filter_map(Value::as_str).filter_map(|n| st.body(n).map(|b| b.mesh())).collect();
    (!meshes.is_empty()).then_some(meshes)
}

/// Where a copy with no `translate` would go (its placement in the active component).
fn base_of(s: &Session, path: &str) -> Mat {
    let mut scratch = s.scratch();
    let Ok(out) = scratch.execute("component.insert_design", &json!({ "path": path, "reuse": true })) else { return IDENTITY };
    let id = out.get("occurrence").and_then(Value::as_u64);
    scratch.doc.occurrences.iter().find(|o| Some(o.id) == id).map_or(IDENTITY, |o| o.transform)
}

/// The pointer at `at` over the view (None: off it): the ghost goes there. Not called while the
/// view is being navigated, so the ghost holds still meanwhile.
pub(crate) fn hover(app: &mut SolveApp, at: Option<Pos2>) {
    if !live(app) {
        return;
    }
    let ghost = at.map(|at| {
        let (_, local) = crate::edit_clipboard::paste_placement(app, Some(at));
        let base = app.viewport.place.as_ref().map_or(IDENTITY, |p| p.base);
        let mut placed = base;
        placed[3] = [local[0], local[1], local[2], 1.0];
        Ghost { local: placed, translate: [local[0] - base[3][0], local[1] - base[3][1], local[2] - base[3][2]] }
    });
    if let Some(p) = app.viewport.place.as_mut() {
        p.ghost = ghost;
    }
}

/// A left click in the view: one occurrence where the ghost is.
pub(crate) fn click(app: &mut SolveApp) {
    let Some((path, Some(g))) = app.viewport.place.as_ref().map(|p| (p.path.clone(), p.ghost)) else { return };
    if app.run("component.insert_design", json!({ "path": path, "reuse": true, "translate": g.translate })).is_err() {
        return;
    }
    // The next copy starts from the design's first occurrence now (its link exists).
    let base = base_of(&app.session, &path);
    let frame = frame(app);
    if let Some(p) = app.viewport.place.as_mut() {
        p.base = base;
        p.frame = frame;
    }
}

/// The ghost's scene for the preview, keyed by its pose and the design's revision.
pub(crate) fn scene(app: &SolveApp, colors: Colors) -> Option<(u64, Built)> {
    use std::hash::{Hash, Hasher};
    let p = app.viewport.place.as_ref()?;
    let g = p.ghost?;
    // The meshes are world points of the occurrence at identity in `frame`: moved by
    // frame · local · frame⁻¹.
    let m = mat_mul(&mat_mul(&p.frame, &g.local), &mat_inverse(&p.frame)?);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    "place".hash(&mut h);
    for v in m.iter().flatten() {
        v.to_bits().hash(&mut h);
    }
    app.session.revision.hash(&mut h);
    format!("{colors:?}").hash(&mut h);
    let mut scene = GpuScene::default();
    let mut bounds = Aabb3::EMPTY;
    for mesh in &p.meshes {
        crate::move_preview::draw(&mut scene, mesh, &m, colors, &mut bounds);
    }
    let built = Built { scene, sketches: Vec::new(), applied: None, stopped: None, tapered: Vec::new(), replaced: Vec::new(), error: None, bounds };
    Some((h.finish(), built))
}

/// The world point of the ghost's origin (tests).
#[cfg(test)]
pub(crate) fn ghost_world(app: &SolveApp) -> Option<Vec3> {
    let p = app.viewport.place.as_ref()?;
    let g = p.ghost?;
    Some(solidcraft_engine::doc::apply_point(&mat_mul(&p.frame, &g.local), Vec3::ZERO))
}

#[cfg(test)]
#[path = "place_component_tests.rs"]
mod tests;
