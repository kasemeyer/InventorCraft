//! On-model constraint glyphs and the picked rows' highlight marks (spec §6.8, §6.4).
//!
//! A shown constraint (`Session::shown_constraints`, filled by `constraint.show`, `show_sick` and
//! `hide_all`, none an undo step) draws one glyph: our own type symbol at the midpoint between its
//! two anchors, with a leader to each, in the relationship colour, or the error colour while it is
//! Sick or Unsolved. A Lost constraint draws nothing, even while its picks still resolve (R135,
//! R162): its browser rows carry the problem. A click on a glyph picks its rows; the pointer on
//! one lights them. A picked row marks both of its anchors on the model.

use std::cell::RefCell;

use egui::{Color32, Pos2, Stroke};
use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::constraints::ConstraintStatus;
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::theme::Tokens;
use crate::viewport::Proj;

/// A glyph's size, in logical points.
pub(crate) const GLYPH_SIZE: f32 = 16.0;

/// Sick and Unsolved draw in the error colour (Lost draws nothing).
pub(crate) fn is_problem(status: &ConstraintStatus) -> bool {
    matches!(status, ConstraintStatus::Sick { .. } | ConstraintStatus::Unsolved)
}

fn report_rows(app: &SolveApp) -> impl Iterator<Item = &solidcraft_engine::constraints::Row> {
    app.session.constraint_report.rows.iter()
}

/// The glyphs to draw: (constraint id, midpoint between the two anchors (world), sick). Rows with
/// `status == Lost` give none, whether or not their anchors resolve; rows with `anchors == None`
/// give none (R135, R162).
pub(crate) fn glyphs(app: &SolveApp) -> Vec<(u64, Vec3, bool)> {
    let shown = &app.session.shown_constraints;
    report_rows(app)
        .filter(|r| shown.contains(&r.id) && !matches!(r.status, ConstraintStatus::Lost { .. }))
        .filter_map(|r| r.anchors.map(|[a, b]| (r.id, a.lerp(b, 0.5), is_problem(&r.status))))
        .collect()
}

/// The highlight marks for the picked rows: each picked constraint's two anchors (none for a Lost
/// one, by status).
pub(crate) fn highlights(app: &SolveApp) -> Vec<Vec3> {
    app.tree
        .picked_constraints
        .iter()
        .filter_map(|id| report_rows(app).find(|r| r.id == *id))
        .filter(|r| !matches!(r.status, ConstraintStatus::Lost { .. }))
        .filter_map(|r| r.anchors)
        .flatten()
        .collect()
}

/// A glyph drawn in the last frame (for tests and automation).
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug)]
pub(crate) struct DrawnGlyph {
    pub(crate) id: u64,
    pub(crate) at: Pos2,
    pub(crate) colour: Color32,
    /// Leaders drawn to its anchors.
    pub(crate) leaders: usize,
    /// The pointer is on it.
    pub(crate) hot: bool,
    /// Anchors drawn faded: behind a face from where the view looks (R176).
    pub(crate) faded: usize,
    /// Lit: the pointer is on it or its browser row is picked (the selection ring).
    pub(crate) lit: bool,
}

thread_local! {
    static DRAWN: RefCell<Vec<DrawnGlyph>> = const { RefCell::new(Vec::new()) };
}

/// The glyphs drawn in the last frame, in report order.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn drawn_glyphs() -> Vec<DrawnGlyph> {
    DRAWN.with(|d| d.borrow().clone())
}

/// Each glyph's screen point and the screen points of its two anchors.
fn placed(app: &SolveApp, proj: &Proj) -> Vec<(u64, Pos2, [Option<Pos2>; 2], bool)> {
    placed_world(app, proj).into_iter().map(|(id, at, anchors, sick)| (id, at, anchors.map(|a| a.map(|a| a.1)), sick)).collect()
}

/// One glyph as placed on screen: (id, its point, its anchors (world, screen), sick).
type Placed = (u64, Pos2, [Option<(Vec3, Pos2)>; 2], bool);

thread_local! {
    /// The glyphs placed for a view and report (`placed_key`): the click, the hover and the
    /// drawing of one frame place them once.
    static PLACED: RefCell<Option<(u64, Vec<Placed>)>> = const { RefCell::new(None) };
}

thread_local! {
    /// Each drawn anchor's depth check (in draw order), for the view, rows and model it was
    /// made on: a still view checks none again.
    static DEPTH: RefCell<Option<(u64, Vec<bool>)>> = const { RefCell::new(None) };
}

/// What [`placed_world`] depends on: the camera and viewport, the shown set and the rows.
fn placed_key(app: &SolveApp, proj: &Proj) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{:?}{:?}", app.cam, proj.rect).hash(&mut h);
    app.session.shown_constraints.hash(&mut h);
    for r in report_rows(app) {
        r.id.hash(&mut h);
        std::mem::discriminant(&r.status).hash(&mut h);
        for p in r.anchors.iter().flatten() {
            [p.x, p.y, p.z].map(f64::to_bits).hash(&mut h);
        }
    }
    h.finish()
}

/// [`placed`] with each anchor's world point too (once per view and report, [`PLACED`]).
fn placed_world(app: &SolveApp, proj: &Proj) -> Vec<Placed> {
    let key = placed_key(app, proj);
    if let Some(hit) = PLACED.with(|p| p.borrow().as_ref().filter(|(k, _)| *k == key).map(|(_, v)| v.clone())) {
        return hit;
    }
    let placed = place_glyphs(app, proj);
    PLACED.with(|p| *p.borrow_mut() = Some((key, placed.clone())));
    placed
}

fn place_glyphs(app: &SolveApp, proj: &Proj) -> Vec<Placed> {
    glyphs(app)
        .into_iter()
        .filter_map(|(id, m, sick)| {
            let at = proj.to_screen(m)?;
            let on = |w: Vec3| proj.to_screen(w).map(|s| (w, s));
            let anchors = report_rows(app).find(|r| r.id == id).and_then(|r| r.anchors).map_or([None, None], |[a, b]| [on(a), on(b)]);
            Some((id, at, anchors, sick))
        })
        .collect()
}

/// How much of its colour a hidden anchor keeps.
const HIDDEN_ALPHA: f32 = 0.3;

/// Most anchors checked for depth each frame; past them all are drawn as in view.
const MAX_DEPTH_CHECKS: usize = 64;

/// Is world point `w` (drawn at `s`) behind a face from where the view looks (R176)? The first
/// face under `s` is nearer the eye than `w` by more than a hair.
fn behind(app: &SolveApp, proj: &Proj, w: Vec3, s: Pos2) -> bool {
    let (eye, dir) = proj.ray(s);
    let depth = (w - eye).dot(dir);
    let first = crate::viewport::pick(app, proj, s).into_iter().find_map(|h| match h {
        crate::viewport::Hit::Face { point, .. } => Some((point - eye).dot(dir)),
        _ => None,
    });
    first.is_some_and(|f| f < depth - 1e-3 * depth.abs().max(1.0))
}

/// The glyph under screen point `at`, the nearest when several overlap.
pub(crate) fn hit(app: &SolveApp, proj: &Proj, at: Pos2) -> Option<u64> {
    let reach = GLYPH_SIZE / 2.0 + 2.0;
    placed(app, proj)
        .into_iter()
        .map(|(id, p, _, _)| (id, (p - at).abs().max_elem()))
        .filter(|(_, d)| *d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// Draw the picked rows' anchor marks, then the shown glyphs on top of the model.
pub(crate) fn draw(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let t = Tokens::get();
    for p in highlights(app).into_iter().filter_map(|p| proj.to_screen(p)) {
        crate::icons::markers::constraint_anchor(painter, p, t.sel_edge);
    }
    let hot = app.viewport.constraint_hover;
    let mut drawn = Vec::new();
    let mut checks = 0;
    // The depth checks hold while the view, the rows and the model are as they were.
    let depth_key =
        placed_key(app, proj) ^ app.session.revision.rotate_left(17) ^ (app.session.visibility.hidden_bodies.len() as u64).rotate_left(41);
    let cached: Option<Vec<bool>> = DEPTH.with(|d| d.borrow().as_ref().filter(|(k, _)| *k == depth_key).map(|(_, v)| v.clone()));
    let mut hidden_all: Vec<bool> = Vec::new();
    for (id, at, anchors, sick) in placed_world(app, proj) {
        let Some((kind, suppressed)) = report_rows(app).find(|r| r.id == id).map(|r| (r.kind, r.status == ConstraintStatus::Suppressed)) else {
            continue;
        };
        // A suppressed one is dim, as its browser row (R177).
        let colour = if sick {
            t.error
        } else if suppressed {
            t.text_dim
        } else {
            t.accent
        };
        let picked = app.tree.picked_constraints.contains(&id);
        let leaders: Vec<(Vec3, Pos2)> = anchors.into_iter().flatten().collect();
        let mut faded = 0;
        for (w, a) in &leaders {
            // An anchor behind a face fades, with its leader (R176).
            checks += 1;
            let hidden = match cached.as_ref().and_then(|c| c.get(checks - 1)) {
                Some(h) => *h,
                None => checks <= MAX_DEPTH_CHECKS && behind(app, proj, *w, *a),
            };
            hidden_all.push(hidden);
            let c = if hidden { colour.gamma_multiply(HIDDEN_ALPHA) } else { colour };
            faded += usize::from(hidden);
            painter.line_segment([at, *a], Stroke::new(1.0, c));
            painter.circle_filled(*a, 2.0, c);
        }
        let lit = hot == Some(id) || picked;
        crate::icons::markers::constraint_glyph(painter, at, kind, colour, GLYPH_SIZE);
        // Lit by the pointer or the row's pick: a selection ring round the plate.
        if lit {
            let ring = egui::Rect::from_center_size(at, egui::vec2(GLYPH_SIZE + 5.0, GLYPH_SIZE + 5.0));
            painter.rect_stroke(ring, 4.5, Stroke::new(2.0, t.sel_edge), egui::StrokeKind::Middle);
        }
        crate::scenario::publish_handle(&format!("glyph:constraint:{id}"), at);
        drawn.push(DrawnGlyph { id, at, colour, leaders: leaders.len(), hot: hot == Some(id), faded, lit });
    }
    DRAWN.with(|d| *d.borrow_mut() = drawn);
    DEPTH.with(|d| *d.borrow_mut() = Some((depth_key, hidden_all)));
}

/// The occurrence a picked body, face, edge, vertex or placement belongs to (not the root's).
fn occurrence_of(app: &SolveApp, sel: &Sel) -> Option<u64> {
    match sel {
        Sel::Occurrence { path } => path.last().copied(),
        Sel::Body { name: body } | Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } => {
            crate::dialogs_assembly::occurrence_of_body(&app.session, body)
        }
        _ => None,
    }
    .filter(|o| *o != 0)
}

/// The occurrences the selection names: placements picked in the browser or the view, the
/// components of picked bodies, faces, edges and vertices, and picked component rows.
fn selected_occurrences(app: &SolveApp) -> Vec<u64> {
    let s = &app.session;
    let mut out: Vec<u64> = s
        .selection
        .iter()
        .filter_map(|x| occurrence_of(app, x))
        .chain(app.tree.picked_components.iter().filter_map(|c| s.doc.occurrence_of(*c).map(|o| o.id)))
        .filter(|o| *o != 0)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Show (`constraint.show`): the selected components' constraints join the shown set; with no
/// component selected, it prompts and the next component click shows that one's ([`pick`]).
/// An empty list would clear the set, so it is never sent.
pub(crate) fn start_show(app: &mut SolveApp) {
    let occurrences = selected_occurrences(app);
    if occurrences.is_empty() {
        app.viewport.show_pick = true;
        app.set_status(crate::i18n::tr("ui.constraint.show_prompt").to_string(), false);
        return;
    }
    app.viewport.show_pick = false;
    let _ = app.run("constraint.show", json!({ "occurrences": occurrences }));
}

/// The status prompt while Show waits for a component click.
pub(crate) fn prompt(app: &SolveApp) -> Option<&'static str> {
    app.viewport.show_pick.then_some("ui.constraint.show_prompt")
}

/// End Show's wait for a component click, and its prompt with it (a click, Esc, another
/// command). True when it was waiting.
pub(crate) fn end_wait(app: &mut SolveApp) -> bool {
    if !app.viewport.show_pick {
        return false;
    }
    app.viewport.show_pick = false;
    let asked = crate::i18n::tr("ui.constraint.show_prompt");
    if app.status.as_ref().is_some_and(|(text, _, _)| text == asked) {
        app.status = None;
    }
    true
}

/// A click in the view while Show waits: a component shows its constraints; anything else ends
/// the wait. True when the click was taken.
pub(crate) fn pick(app: &mut SolveApp, sel: Option<&Sel>) -> bool {
    if !end_wait(app) {
        return false;
    }
    if let Some(o) = sel.and_then(|s| occurrence_of(app, s)) {
        let _ = app.run("constraint.show", json!({ "occurrences": [o] }));
    }
    true
}

#[cfg(test)]
#[path = "constraint_overlay_tests.rs"]
mod constraint_overlay_tests;
