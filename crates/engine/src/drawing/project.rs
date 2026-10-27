//! Drawing views as exact curves (spec §4.5 items 2, 3 and 5). Occlusion stays mesh based (the
//! HLR of `drawing::edges::sourced_lines`); the drawn curve is the exact one where we know it:
//!
//! - **Straight** sources (every chord point on one line within 1e-9 x the view's extent) give
//!   one `Line` per visible or hidden run along the line.
//! - **Circular** B-rep edges (`kernel::edge_circle`) whose plane faces the eye (|axis . dir| at
//!   least 1 - 1e-9) map each chord end to its angle about the projected centre, measured along the
//!   edge from its start; a run of chords with one visibility is an `Arc`, a run of 2π a `Circle`.
//!   Arcs of one 3D circle of one body that close 360° together (a hole rim the kernel halves)
//!   join into one `Circle` naming every edge (R51).
//! - **Silhouettes** of a cylindrical face snap to its exact generators (R52); seen end on, a
//!   cylinder has none, so its pieces (open-boundary chords lying on the rims, which the B-rep
//!   edges draw) are dropped, and so are pieces farther than 4 x the chord tolerance from both
//!   generators. Other faces keep their tessellated silhouettes.
//! - Anything else is a `Poly` per run, through its chord points.
//!
//! A run's `segment` is its index along its edge (0, 1, …). Runs shorter than 1e-6 mm (or rad)
//! are dropped. Within an edge, and between edges, a visible curve wins over a hidden one at the
//! same place: a hidden curve that coincides with a visible one (a line on a visible line, an arc
//! on a visible arc or circle of the same centre and radius, a polyline on visible chords, all
//! within 1e-6 mm) is dropped.
//!
//! The styles: Hidden Line keeps the hidden curves, Hidden Line Removed drops them. A shaded view
//! draws the same curves over its raster (the edge overlay; the raster is `ViewRasterizer`'s),
//! so shading changes nothing here. Tangent edges (`SourcedLine::smooth`) are dropped unless the
//! view asks for them, before the visibility rules run; one that is also a silhouette
//! (`SourcedLine::silhouette`) is an outline and always stays (R54).
// Drawing views (Task 7) are the first caller outside the tests.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::BTreeMap;
use std::f64::consts::{PI, TAU};

use solidcraft_doc::{LineStyle, ModelState, Orientation, naming};
use solidcraft_geom::{Aabb3, Vec2, Vec3};
use solidcraft_kernel::{FaceSurface, edge_circle, face_surface};
use solidcraft_render::Camera;

use crate::drawing::edges::{EdgeSource, SourcedLine, renumber_silhouettes, sourced_lines};
use crate::{Result, Session};

/// One projected curve on the view plane, in mm.
#[derive(Clone, Debug, PartialEq)]
pub enum PaperCurve {
    Line {
        a: Vec2,
        b: Vec2,
    },
    /// Counter-clockwise from `start` to `end` (radians; `start` in [0, 2π), `end` after it, by
    /// less than 2π).
    Arc {
        c: Vec2,
        r: f64,
        start: f64,
        end: f64,
    },
    Circle {
        c: Vec2,
        r: f64,
    },
    Poly {
        points: Vec<Vec2>,
    },
}

/// A curve of a view and the model edge it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewCurve {
    /// The `model_edge_id` of its source; of the first of `sources` when it joins several.
    pub edge: String,
    /// Every source's `model_edge_id`, sorted: one, or the edges a joined circle closes (R51).
    pub sources: Vec<String>,
    /// The run's index along its edge.
    pub segment: u32,
    pub hidden: bool,
    pub curve: PaperCurve,
}

/// A view's curves, in mm, centred on the box of its visible curves (the outline): `min` and
/// `max` are that box's corners.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewGeometry {
    pub curves: Vec<ViewCurve>,
    pub min: Vec2,
    pub max: Vec2,
}

#[derive(Clone, Copy, Debug)]
pub struct ProjectOptions {
    pub lines: LineStyle,
    pub tangent_edges: bool,
    /// The chord tolerance of the HLR meshes, mm (0.01 for drawings).
    pub tolerance_mm: f64,
}

/// Shorter runs are dropped; nearer points are the same (mm, or radians for angles).
const SAME: f64 = 1e-6;

/// The orthographic camera of an orientation (right = dir × up) looking at `target` from
/// `2 × extent_mm` away (R10: public, D-2's on-sheet cube uses it).
pub fn camera_for(o: &Orientation, target: Vec3, extent_mm: f64) -> Camera {
    let back = -o.dir;
    let pitch = back.z.clamp(-1.0, 1.0).asin();
    // Straight up or down the yaw is free: the roll below turns `right` into place.
    let yaw = if back.x.hypot(back.y) < 1e-12 { 0.0 } else { (-back.x).atan2(-back.y) };
    let distance = if extent_mm.is_finite() && extent_mm > 0.5 { extent_mm * 2.0 } else { 1.0 };
    let mut cam = Camera { target, yaw, pitch, roll: 0.0, distance, fov: 0.0, up: Default::default() };
    let (r0, u0, _) = cam.basis();
    let right = o.dir.cross(o.up);
    cam.roll = right.dot(u0).atan2(right.dot(r0));
    cam
}

/// The view plane: model points to paper mm.
struct Frame {
    target: Vec3,
    right: Vec3,
    up: Vec3,
    back: Vec3,
}
impl Frame {
    fn paper(&self, p: Vec3) -> Vec2 {
        let d = p - self.target;
        Vec2::new(d.dot(self.right), d.dot(self.up))
    }
    fn paper_dir(&self, v: Vec3) -> Vec2 {
        Vec2::new(v.dot(self.right), v.dot(self.up))
    }
}

/// The view of `model`'s visible bodies along `o` as exact curves (module comment).
pub(crate) fn project_view(model: &Session, o: &Orientation, opts: &ProjectOptions) -> Result<ViewGeometry> {
    let world = model.world_state();
    let bounds = world.bodies.iter().filter(|b| model.body_visible(&b.name)).fold(Aabb3::EMPTY, |acc, b| acc.union(&b.mesh().bounds()));
    let (target, extent) = if bounds.is_empty() { (Vec3::ZERO, 1.0) } else { (bounds.center(), bounds.diagonal().max(1.0)) };
    let cam = camera_for(o, target, extent);
    let (right, up, back) = cam.basis();
    let frame = Frame { target, right, up, back };

    let mut lines = sourced_lines(model, &cam, opts.tolerance_mm)?;
    if !opts.tangent_edges {
        // R54: a tangent edge that is also a silhouette is an outline, and stays.
        lines.retain(|l| !l.smooth || l.silhouette);
    }
    lines.retain(|l| l.a.dist(l.b) >= SAME);
    snap_silhouettes(&world, &frame, o.dir, opts.tolerance_mm, &mut lines);
    renumber_silhouettes(&mut lines);

    let mut sources: BTreeMap<String, Vec<SourcedLine>> = BTreeMap::new();
    for l in lines {
        sources.entry(l.source.model_edge_id()).or_default().push(l);
    }
    let mut built = Vec::new();
    for (id, pieces) in &sources {
        built.extend(curves_of(&world, &frame, o.dir, extent, id, pieces));
    }
    let built = join_circles(built);
    let mut curves = visible_wins(built.into_iter().map(|b| b.curve).collect());
    if opts.lines == LineStyle::Removed {
        curves.retain(|c| !c.hidden);
    }
    curves.sort_by(|a, b| a.edge.cmp(&b.edge).then(a.segment.cmp(&b.segment)).then(a.hidden.cmp(&b.hidden)));
    Ok(centred(curves))
}

/// R52: moves each silhouette piece of a cylindrical face onto the exact generator it runs
/// along, or drops it (module comment).
fn snap_silhouettes(world: &ModelState, frame: &Frame, dir: Vec3, tolerance_mm: f64, lines: &mut Vec<SourcedLine>) {
    let band = 4.0 * tolerance_mm + SAME;
    let mut generators: BTreeMap<(String, String), Option<Vec<(Vec2, Vec2)>>> = BTreeMap::new();
    lines.retain_mut(|l| {
        let EdgeSource::Silhouette { body_name, face_name, .. } = &l.source else { return true };
        let lines_of =
            generators.entry((body_name.clone(), face_name.clone())).or_insert_with(|| cylinder_generators(world, frame, dir, body_name, face_name));
        let Some(lines_of) = lines_of else { return true };
        for (p, d) in lines_of.iter() {
            let off = |q: Vec2| (q - *p).cross(*d).abs();
            if off(l.a) <= band && off(l.b) <= band {
                l.a = *p + *d * (l.a - *p).dot(*d);
                l.b = *p + *d * (l.b - *p).dot(*d);
                return l.a.dist(l.b) >= SAME;
            }
        }
        false
    });
}

/// The two projected generators (a point and a unit direction) a cylindrical face shows as its
/// silhouettes; none when it is seen end on. `None`: not a cylinder (keep the tessellation).
fn cylinder_generators(world: &ModelState, frame: &Frame, dir: Vec3, body: &str, face: &str) -> Option<Vec<(Vec2, Vec2)>> {
    let body = world.body(body)?;
    let index = naming::face_names(body).iter().position(|n| n == face)?;
    let FaceSurface::Cylinder { origin, axis, radius } = face_surface(&body.body, index)? else { return None };
    let axis = Vec3::from(axis);
    if axis.dot(dir).abs() >= 1.0 - 1e-9 {
        return Some(Vec::new());
    }
    let n = axis.cross(dir).normalized()?;
    let along = frame.paper_dir(axis).normalized()?;
    Some([1.0, -1.0].iter().map(|s| (frame.paper(origin + n * (radius * s)), along)).collect())
}

/// A curve before the joins, with the 3D circle (centre, radius) of an arc from a circular edge.
struct Built {
    curve: ViewCurve,
    circle: Option<(Vec3, f64)>,
}

/// The curves of one source's pieces.
fn curves_of(world: &ModelState, frame: &Frame, dir: Vec3, extent: f64, id: &str, pieces: &[SourcedLine]) -> Vec<Built> {
    let make = |segment: usize, hidden: bool, curve: PaperCurve, circle: Option<(Vec3, f64)>| Built {
        curve: ViewCurve { edge: id.to_string(), sources: vec![id.to_string()], segment: u32::try_from(segment).unwrap_or(u32::MAX), hidden, curve },
        circle,
    };
    let Some(first) = pieces.first() else { return Vec::new() };
    let (body, mesh_edge) = match &first.source {
        EdgeSource::Brep { body_name, .. } => (world.body(body_name), first.mesh_edge),
        EdgeSource::Silhouette { .. } => (None, None),
    };
    // The edge's projected polyline (a B-rep edge's mesh edge), which orders its runs.
    let polyline: Vec<Vec2> = match (body, mesh_edge) {
        (Some(b), Some(i)) => b.mesh().edges.get(i as usize).map(|e| e.iter().map(|p| frame.paper(*p)).collect()).unwrap_or_default(),
        _ => Vec::new(),
    };
    let segs: Vec<(Vec2, Vec2, bool)> = pieces.iter().map(|l| (l.a, l.b, l.hidden)).collect();

    if let Some((origin, d)) = straight(&segs, extent, polyline.first().copied()) {
        let runs = interval_runs(segs.iter().map(|(a, b, h)| {
            let (s, t) = ((*a - origin).dot(d), (*b - origin).dot(d));
            (s.min(t), s.max(t), *h)
        }));
        return runs
            .into_iter()
            .enumerate()
            .map(|(n, (s, t, h))| make(n, h, PaperCurve::Line { a: origin + d * s, b: origin + d * t }, None))
            .collect();
    }
    if let (Some(b), Some(i)) = (body, mesh_edge)
        && let Some(c) = edge_circle(&b.body, i as usize)
        && Vec3::from(c.axis).dot(dir).abs() >= 1.0 - 1e-9
    {
        let centre = frame.paper(c.center);
        let angle = |p: Vec2| (p.y - centre.y).atan2(p.x - centre.x);
        let theta0 = angle(frame.paper(c.start));
        // The edge runs counter-clockwise on paper when its axis faces the eye.
        let sign = if Vec3::from(c.axis).dot(frame.back) > 0.0 { 1.0 } else { -1.0 };
        let closed = c.angle >= TAU - SAME;
        let along = |p: Vec2| (sign * (angle(p) - theta0)).rem_euclid(TAU);
        let mut runs = interval_runs(segs.iter().map(|(a, b, h)| {
            let ua = along(*a);
            let delta = wrap_pi(sign * (angle(*b) - angle(*a)));
            let (mut lo, mut hi) = (ua.min(ua + delta), ua.max(ua + delta));
            if lo < -SAME {
                (lo, hi) = (lo + TAU, hi + TAU);
            } else if lo > TAU - SAME {
                (lo, hi) = (lo - TAU, hi - TAU);
            }
            (lo, hi, *h)
        }));
        // A closed edge's runs through its start are one run.
        if closed
            && runs.len() > 1
            && let (Some(&(f0, f1, fh)), Some(&(l0, l1, lh))) = (runs.first(), runs.last())
            && fh == lh
            && f0 <= SAME
            && l1 >= TAU - SAME
        {
            runs.pop();
            if let Some(first) = runs.first_mut() {
                *first = (l0 - TAU, f1, fh);
            }
        }
        return runs
            .into_iter()
            .enumerate()
            .map(|(n, (u0, u1, h))| {
                let curve = if u1 - u0 >= TAU - 1e-9 {
                    PaperCurve::Circle { c: centre, r: c.radius }
                } else {
                    let start = if sign > 0.0 { theta0 + u0 } else { theta0 - u1 };
                    let mut start = start.rem_euclid(TAU);
                    if start >= TAU - 1e-9 {
                        start = 0.0;
                    }
                    PaperCurve::Arc { c: centre, r: c.radius, start, end: start + (u1 - u0) }
                };
                make(n, h, curve, Some((c.center, c.radius)))
            })
            .collect();
    }
    poly_runs(&segs, &polyline).into_iter().enumerate().map(|(n, (points, h))| make(n, h, PaperCurve::Poly { points }, None)).collect()
}

/// `a` in (-π, π].
fn wrap_pi(a: f64) -> f64 {
    let a = a.rem_euclid(TAU);
    if a > PI { a - TAU } else { a }
}

/// The line every piece lies on (within 1e-9 x `extent`), as a point and a unit direction: from
/// the end nearer `start` (the edge's start), else pointing right (or up).
fn straight(segs: &[(Vec2, Vec2, bool)], extent: f64, start: Option<Vec2>) -> Option<(Vec2, Vec2)> {
    let points: Vec<Vec2> = segs.iter().flat_map(|(a, b, _)| [*a, *b]).collect();
    let first = *points.first()?;
    let far = |from: Vec2| points.iter().copied().max_by(|p, q| p.dist(from).total_cmp(&q.dist(from))).unwrap_or(from);
    let p = far(first);
    let q = far(p);
    let mut d = (q - p).normalized()?;
    let tol = 1e-9 * extent.max(1.0);
    if points.iter().any(|x| (*x - p).cross(d).abs() > tol) {
        return None;
    }
    let toward = match start {
        Some(s) => s.dist(p) <= s.dist(q),
        None => d.x > 1e-12 || (d.x.abs() <= 1e-12 && d.y > 0.0),
    };
    if !toward {
        d = -d;
    }
    Some((if toward { p } else { q }, d))
}

/// Pieces as intervals along a parameter, each visible or hidden, to runs: the visible union,
/// then the hidden union less the visible (a visible stroke wins), joined across gaps of up to
/// 1e-6 and in order; runs shorter than 1e-6 are dropped.
fn interval_runs(pieces: impl Iterator<Item = (f64, f64, bool)>) -> Vec<(f64, f64, bool)> {
    let (mut visible, mut hidden) = (Vec::new(), Vec::new());
    for (lo, hi, h) in pieces {
        if h { hidden.push((lo, hi)) } else { visible.push((lo, hi)) }
    }
    let visible = union(visible);
    let hidden = subtract(union(hidden), &visible);
    let mut runs: Vec<(f64, f64, bool)> = visible
        .into_iter()
        .map(|(a, b)| (a, b, false))
        .chain(hidden.into_iter().map(|(a, b)| (a, b, true)))
        .filter(|(a, b, _)| b - a >= SAME)
        .collect();
    runs.sort_by(|p, q| p.0.total_cmp(&q.0));
    runs
}

fn union(mut v: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    v.sort_by(|p, q| p.0.total_cmp(&q.0));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (lo, hi) in v {
        match out.last_mut() {
            Some(last) if lo <= last.1 + SAME => last.1 = last.1.max(hi),
            _ => out.push((lo, hi)),
        }
    }
    out
}

/// `a` less `b` (both sorted and disjoint).
fn subtract(a: Vec<(f64, f64)>, b: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for (mut lo, hi) in a {
        for &(blo, bhi) in b {
            if bhi <= lo || blo >= hi {
                continue;
            }
            if blo > lo {
                out.push((lo, blo));
            }
            lo = lo.max(bhi);
        }
        if hi > lo {
            out.push((lo, hi));
        }
    }
    out
}

/// Chains the pieces end to end and splits the chains where the visibility changes. A chain runs
/// along `polyline` (the edge's projection) when there is one, else from its leftmost (then
/// lowest) end; the runs come in order along the edge.
fn poly_runs(segs: &[(Vec2, Vec2, bool)], polyline: &[Vec2]) -> Vec<(Vec<Vec2>, bool)> {
    let at = |p: Vec2| polyline_param(polyline, p);
    let mut used = vec![false; segs.len()];
    let mut chains: Vec<Vec<(Vec2, Vec2, bool)>> = Vec::new();
    for (i, seg) in segs.iter().enumerate() {
        if used.get(i).copied().unwrap_or(true) {
            continue;
        }
        if let Some(u) = used.get_mut(i) {
            *u = true;
        }
        let mut chain = vec![*seg];
        // Grow the tail, turn the chain round, grow the other end, turn it back.
        for _ in 0..2 {
            while let Some(&(_, end, _)) = chain.last() {
                let next =
                    segs.iter().enumerate().find(|(j, (a, b, _))| used.get(*j) == Some(&false) && (a.dist(end) <= SAME || b.dist(end) <= SAME));
                let Some((j, &(a, b, h))) = next else { break };
                if let Some(u) = used.get_mut(j) {
                    *u = true;
                }
                chain.push(if a.dist(end) <= SAME { (a, b, h) } else { (b, a, h) });
            }
            chain = chain.into_iter().rev().map(|(a, b, h)| (b, a, h)).collect();
        }
        let (Some(&(head, _, _)), Some(&(_, tail, _))) = (chain.first(), chain.last()) else { continue };
        let reverse = match (at(head), at(tail)) {
            (Some(s), Some(t)) => t < s,
            _ => (tail.x, tail.y) < (head.x, head.y),
        };
        if reverse {
            chain = chain.into_iter().rev().map(|(a, b, h)| (b, a, h)).collect();
        }
        chains.push(chain);
    }
    let mut runs: Vec<(Vec<Vec2>, bool)> = Vec::new();
    for chain in chains {
        let mut fresh = true;
        for (a, b, h) in chain {
            match runs.last_mut() {
                Some((points, rh)) if !fresh && *rh == h => points.push(b),
                _ => runs.push((vec![a, b], h)),
            }
            fresh = false;
        }
    }
    let length = |points: &[Vec2]| points.windows(2).map(|w| w.first().zip(w.get(1)).map_or(0.0, |(p, q)| p.dist(*q))).sum::<f64>();
    runs.retain(|(points, _)| length(points) >= SAME);
    let key = |points: &[Vec2]| points.first().map(|p| (at(*p).unwrap_or(0.0), p.x, p.y)).unwrap_or_default();
    runs.sort_by(|p, q| {
        let (a, b) = (key(&p.0), key(&q.0));
        a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.total_cmp(&b.2))
    });
    runs
}

/// The arc length along `polyline` of its point nearest `p`; None without a polyline.
fn polyline_param(polyline: &[Vec2], p: Vec2) -> Option<f64> {
    let mut best: Option<(f64, f64)> = None;
    let mut walked = 0.0;
    for w in polyline.windows(2) {
        let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
        let d = b - a;
        let len = d.len();
        let t = if len > 0.0 { ((p - a).dot(d) / (len * len)).clamp(0.0, 1.0) } else { 0.0 };
        let dist = (a + d * t).dist(p);
        if best.is_none_or(|(bd, _)| dist < bd) {
            best = Some((dist, walked + t * len));
        }
        walked += len;
    }
    best.map(|(_, s)| s)
}

/// R51: arcs of one body's 3D circle that close 360° end to end, with one visibility, become one
/// `Circle` naming all their edges (the first that sorts as its edge).
fn join_circles(built: Vec<Built>) -> Vec<Built> {
    let body = |id: &str| id.split("/e/").next().unwrap_or(id).to_string();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (i, b) in built.iter().enumerate() {
        let (Some((c, r)), PaperCurve::Arc { .. }) = (b.circle, &b.curve.curve) else { continue };
        let found = groups.iter_mut().find(|g| {
            g.first().and_then(|j| built.get(*j)).is_some_and(|o| {
                o.curve.hidden == b.curve.hidden
                    && body(&o.curve.edge) == body(&b.curve.edge)
                    && o.circle.is_some_and(|(oc, or)| oc.dist(c) <= SAME && (or - r).abs() <= SAME)
            })
        });
        match found {
            Some(g) => g.push(i),
            None => groups.push(vec![i]),
        }
    }
    let mut joined: Vec<(Vec<usize>, Built)> = Vec::new();
    for g in groups.into_iter().filter(|g| g.len() > 1) {
        let mut arcs: Vec<(f64, f64, Vec2, f64)> = g
            .iter()
            .filter_map(|i| match built.get(*i).map(|b| &b.curve.curve) {
                Some(PaperCurve::Arc { c, r, start, end }) => Some((*start, *end, *c, *r)),
                _ => None,
            })
            .collect();
        arcs.sort_by(|p, q| p.0.total_cmp(&q.0));
        let total: f64 = arcs.iter().map(|(s, e, _, _)| e - s).sum();
        let end_to_end = arcs.iter().zip(arcs.iter().cycle().skip(1)).all(|(p, q)| wrap_pi(q.0 - p.1).abs() <= SAME);
        let Some(&(_, _, c, r)) = arcs.first() else { continue };
        if (total - TAU).abs() > SAME || !end_to_end {
            continue;
        }
        let mut sources: Vec<String> = g.iter().filter_map(|i| built.get(*i)).flat_map(|b| b.curve.sources.clone()).collect();
        sources.sort();
        sources.dedup();
        let Some(edge) = sources.first().cloned() else { continue };
        let hidden = g.first().and_then(|i| built.get(*i)).is_some_and(|b| b.curve.hidden);
        let circle = g.first().and_then(|i| built.get(*i)).and_then(|b| b.circle);
        joined.push((g, Built { curve: ViewCurve { edge, sources, segment: 0, hidden, curve: PaperCurve::Circle { c, r } }, circle }));
    }
    let gone: Vec<usize> = joined.iter().flat_map(|(g, _)| g.iter().copied()).collect();
    built.into_iter().enumerate().filter(|(i, _)| !gone.contains(i)).map(|(_, b)| b).chain(joined.into_iter().map(|(_, b)| b)).collect()
}

/// Drops each hidden curve that a visible curve draws over (module comment).
fn visible_wins(curves: Vec<ViewCurve>) -> Vec<ViewCurve> {
    let visible: Vec<PaperCurve> = curves.iter().filter(|c| !c.hidden).map(|c| c.curve.clone()).collect();
    let on_segment = |p: Vec2, a: Vec2, b: Vec2| {
        let d = b - a;
        let t = if d.len2() > 0.0 { ((p - a).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
        (a + d * t).dist(p) <= SAME
    };
    let chords: Vec<(Vec2, Vec2)> = visible
        .iter()
        .flat_map(|c| match c {
            PaperCurve::Line { a, b } => vec![(*a, *b)],
            PaperCurve::Poly { points } => points.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).collect(),
            _ => Vec::new(),
        })
        .collect();
    let same_circle = |c: Vec2, r: f64, d: Vec2, s: f64| d.dist(c) <= SAME && (s - r).abs() <= SAME;
    let covered = |c: &PaperCurve| -> bool {
        match c {
            PaperCurve::Line { a, b } => {
                visible.iter().any(|v| matches!(v, PaperCurve::Line { a: p, b: q } if on_segment(*a, *p, *q) && on_segment(*b, *p, *q)))
            }
            PaperCurve::Poly { points } => points.iter().all(|p| chords.iter().any(|(a, b)| on_segment(*p, *a, *b))),
            PaperCurve::Circle { c, r } => visible.iter().any(|v| matches!(v, PaperCurve::Circle { c: d, r: s } if same_circle(*c, *r, *d, *s))),
            PaperCurve::Arc { c, r, start, end } => visible.iter().any(|v| match v {
                PaperCurve::Circle { c: d, r: s } => same_circle(*c, *r, *d, *s),
                PaperCurve::Arc { c: d, r: s, start: vs, end: ve } => {
                    let from = (start - vs).rem_euclid(TAU);
                    let from = if from >= TAU - SAME { 0.0 } else { from };
                    same_circle(*c, *r, *d, *s) && from + (end - start) <= ve - vs + SAME
                }
                _ => false,
            }),
        }
    };
    curves.into_iter().filter(|c| !c.hidden || !covered(&c.curve)).collect()
}

/// The curves moved so the box of the visible ones (all of them when none is visible) is
/// centred on the origin.
fn centred(mut curves: Vec<ViewCurve>) -> ViewGeometry {
    let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    let any_visible = curves.iter().any(|c| !c.hidden);
    for c in curves.iter().filter(|c| !any_visible || !c.hidden) {
        for p in extreme_points(&c.curve) {
            lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
    }
    if !(lo.x <= hi.x && lo.y <= hi.y) {
        return ViewGeometry { curves, min: Vec2::ZERO, max: Vec2::ZERO };
    }
    let mid = (lo + hi) * 0.5;
    for c in &mut curves {
        match &mut c.curve {
            PaperCurve::Line { a, b } => {
                *a -= mid;
                *b -= mid;
            }
            PaperCurve::Arc { c, .. } | PaperCurve::Circle { c, .. } => *c -= mid,
            PaperCurve::Poly { points } => points.iter_mut().for_each(|p| *p -= mid),
        }
    }
    ViewGeometry { curves, min: lo - mid, max: hi - mid }
}

/// Points whose box is the curve's box.
fn extreme_points(c: &PaperCurve) -> Vec<Vec2> {
    let on = |c: Vec2, r: f64, a: f64| c + Vec2::new(a.cos(), a.sin()) * r;
    match c {
        PaperCurve::Line { a, b } => vec![*a, *b],
        PaperCurve::Poly { points } => points.clone(),
        PaperCurve::Circle { c, r } => (0..4).map(|k| on(*c, *r, f64::from(k) * PI / 2.0)).collect(),
        PaperCurve::Arc { c, r, start, end } => {
            let mut out = vec![on(*c, *r, *start), on(*c, *r, *end)];
            // The quarter points the arc passes.
            let mut k = (start / (PI / 2.0)).ceil();
            while k * PI / 2.0 < *end {
                out.push(on(*c, *r, k * PI / 2.0));
                k += 1.0;
            }
            out
        }
    }
}
