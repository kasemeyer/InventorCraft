//! Constraint picks resolved to primitives (spec §3.3): a face, edge, vertex or origin item named
//! by a [`GeoRef`] becomes the plane, line, point, circle, cylinder or sphere the compiler needs,
//! in its occurrence's component coordinates ([`resolve`]) or in the world ([`resolve_world`]).
//! A pick that no longer resolves is an error, which makes its constraint Lost. [`geo_at`] makes
//! a pick from a world point.

use serde::Serialize;
use solidcraft_geom::Vec3;
use solidcraft_kernel::FaceSurface;

use super::{EdgeEnd, Geo, GeoRef, OriginItem};
use crate::joints::edge::{edge_index, planar_face_beside};
use crate::joints::face::named_face_in;
use crate::joints::{component_bodies, component_of, occurrence_world};
use crate::{DocError, Document, IDENTITY, Mat, ModelBody, ModelState, Result, apply_point, apply_vector, mat_inverse};

/// An edge's polyline is straight when every point lies within this share of its length of
/// the line from its start to its end.
const STRAIGHT_TOL: f64 = 1e-9;
/// An edge's polyline is a circle when every point lies within this share of the radius of the
/// circle through three of them.
const CIRCLE_TOL: f64 = 1e-6;

/// What a pick resolves to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Prim {
    /// A point on the plane and its (outward) unit normal.
    Plane {
        p: Vec3,
        n: Vec3,
    },
    /// A point on the line and its unit direction.
    Line {
        p: Vec3,
        d: Vec3,
    },
    Point {
        p: Vec3,
    },
    /// Centre, unit normal of the circle's plane, radius.
    Circle {
        c: Vec3,
        axis: Vec3,
        r: f64,
    },
    /// A point on the axis, the axis's unit direction (the B-rep surface's own), radius.
    Cylinder {
        p: Vec3,
        d: Vec3,
        r: f64,
    },
    Sphere {
        c: Vec3,
        r: f64,
    },
}

/// A [`Prim`]'s kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimKind {
    Plane,
    Line,
    Point,
    Circle,
    Cylinder,
    Sphere,
}

/// What [`geo_at`] picks at a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickWhat {
    Face,
    Edge,
    Vertex,
}

impl Prim {
    pub fn kind(&self) -> PrimKind {
        match self {
            Self::Plane { .. } => PrimKind::Plane,
            Self::Line { .. } => PrimKind::Line,
            Self::Point { .. } => PrimKind::Point,
            Self::Circle { .. } => PrimKind::Circle,
            Self::Cylinder { .. } => PrimKind::Cylinder,
            Self::Sphere { .. } => PrimKind::Sphere,
        }
    }

    /// The primitive moved by the rigid transform `m`.
    pub fn moved(&self, m: &Mat) -> Prim {
        let p = |q: Vec3| apply_point(m, q);
        let d = |v: Vec3| {
            let w = apply_vector(m, v);
            w.normalized().unwrap_or(w)
        };
        match *self {
            Self::Plane { p: q, n } => Self::Plane { p: p(q), n: d(n) },
            Self::Line { p: q, d: v } => Self::Line { p: p(q), d: d(v) },
            Self::Point { p: q } => Self::Point { p: p(q) },
            Self::Circle { c, axis, r } => Self::Circle { c: p(c), axis: d(axis), r },
            Self::Cylinder { p: q, d: v, r } => Self::Cylinder { p: p(q), d: d(v), r },
            Self::Sphere { c, r } => Self::Sphere { c: p(c), r },
        }
    }

    /// The line it gives: a line; a cylinder's axis; a circle's axis through its centre.
    pub fn line(&self) -> Option<(Vec3, Vec3)> {
        match *self {
            Self::Line { p, d } | Self::Cylinder { p, d, .. } => Some((p, d)),
            Self::Circle { c, axis, .. } => Some((c, axis)),
            _ => None,
        }
    }

    /// The plane it gives: a plane; a circle's plane (its centre and axis).
    pub fn plane(&self) -> Option<(Vec3, Vec3)> {
        match *self {
            Self::Plane { p, n } => Some((p, n)),
            Self::Circle { c, axis, .. } => Some((c, axis)),
            _ => None,
        }
    }

    /// The point it gives: a point; a sphere's or a circle's centre.
    pub fn point(&self) -> Option<Vec3> {
        match *self {
            Self::Point { p } => Some(p),
            Self::Sphere { c, .. } | Self::Circle { c, .. } => Some(c),
            _ => None,
        }
    }
}

fn lost(what: &str, name: &str) -> DocError {
    DocError::Invalid(format!("the {what} `{name}` no longer exists"))
}

/// The pick as a primitive in its occurrence's component coordinates. Err (the constraint is
/// Lost) when the name no longer resolves, or the face is a kind v1 does not take (cone, torus,
/// free-form).
pub fn resolve(doc: &Document, st: &ModelState, g: &GeoRef) -> Result<Prim> {
    let comp = component_of(doc, g.occurrence)?;
    match &g.geo {
        Geo::Origin { item } => Ok(origin(*item)),
        Geo::Face { name, pick } => face(doc, st, comp, name, *pick),
        Geo::Edge { name, pick } => {
            let (b, i, offset) = named_edge(doc, st, comp, name, *pick)?;
            edge(b, i, &offset)
        }
        Geo::Vertex { edge, end, pick } => {
            let (b, i, offset) = named_edge(doc, st, comp, edge, *pick)?;
            let m = b.mesh();
            let e = m.edges.get(i).ok_or_else(|| lost("edge", edge))?;
            let (Some(first), Some(last)) = (e.first(), e.last()) else { return Err(lost("edge", edge)) };
            let (first, last) = (apply_point(&offset, *first), apply_point(&offset, *last));
            let (kept, other) = match end {
                EdgeEnd::Start => (first, last),
                EdgeEnd::End => (last, first),
            };
            // The stored pick is the end as it was picked: an edge whose polyline runs the other
            // way after a rebuild has that end at its other end.
            Ok(Prim::Point { p: if other.dist(*pick) < 0.5 * kept.dist(*pick) { other } else { kept } })
        }
    }
}

/// The same in the world (occurrence 0 and origin items included).
pub fn resolve_world(doc: &Document, st: &ModelState, g: &GeoRef) -> Result<Prim> {
    let prim = resolve(doc, st, g)?;
    let m = occurrence_world(doc, g.occurrence);
    Ok(if m == IDENTITY { prim } else { prim.moved(&m) })
}

/// A pick by a world point on an occurrence: the nearest face / edge / vertex there, by
/// persistent name.
pub fn geo_at(doc: &Document, st: &ModelState, occurrence: u64, what: PickWhat, world: Vec3) -> Result<GeoRef> {
    let comp = component_of(doc, occurrence)?;
    let to_local =
        mat_inverse(&occurrence_world(doc, occurrence)).ok_or_else(|| DocError::Invalid("the occurrence's placement is degenerate".into()))?;
    let local = apply_point(&to_local, world);
    let bodies = component_bodies(doc, st, comp);
    let geo = match what {
        PickWhat::Face => bodies
            .iter()
            .find_map(|b| crate::naming::face_name_at_body(b, own(doc, b, local)))
            .map(|name| Geo::Face { name, pick: local })
            .ok_or_else(|| DocError::Invalid("no named face of that component there".into()))?,
        PickWhat::Edge => {
            let (b, _, _) = nearest_edge(doc, &bodies, local).ok_or_else(|| DocError::Invalid("no edge of that component there".into()))?;
            let name = crate::naming::edge_names_at(b, &[own(doc, b, local)]).into_iter().next().filter(|n| !n.is_empty());
            Geo::Edge { name: name.ok_or_else(|| DocError::Invalid("that edge has no name".into()))?, pick: local }
        }
        PickWhat::Vertex => {
            let (b, i, end, at) = nearest_end(doc, &bodies, local).ok_or_else(|| DocError::Invalid("no vertex of that component there".into()))?;
            let name = crate::naming::edge_names(b).get(i).cloned().filter(|n| !n.is_empty());
            Geo::Vertex { edge: name.ok_or_else(|| DocError::Invalid("that vertex's edge has no name".into()))?, end, pick: at }
        }
    };
    Ok(GeoRef { occurrence, geo })
}

/// An origin item: fixed in its occurrence's coordinates.
fn origin(item: OriginItem) -> Prim {
    let p = Vec3::ZERO;
    match item {
        OriginItem::Xy => Prim::Plane { p, n: Vec3::Z },
        OriginItem::Xz => Prim::Plane { p, n: Vec3::Y },
        OriginItem::Yz => Prim::Plane { p, n: Vec3::X },
        OriginItem::X => Prim::Line { p, d: Vec3::X },
        OriginItem::Y => Prim::Line { p, d: Vec3::Y },
        OriginItem::Z => Prim::Line { p, d: Vec3::Z },
        OriginItem::Center => Prim::Point { p },
    }
}

/// The offset that places body `b` in its component (identity for most bodies).
fn offset_of(doc: &Document, b: &ModelBody) -> Mat {
    doc.model_body_offset(b).copied().unwrap_or(IDENTITY)
}

/// A point of the component's coordinates in body `b`'s own.
fn own(doc: &Document, b: &ModelBody, p: Vec3) -> Vec3 {
    match doc.model_body_offset(b).and_then(mat_inverse) {
        Some(inv) => apply_point(&inv, p),
        None => p,
    }
}

/// A named face: planar → its centroid and outward normal; cylindrical → its axis (the surface's
/// own direction) and radius; spherical → its centre and radius.
fn face(doc: &Document, st: &ModelState, comp: u64, name: &str, pick: Vec3) -> Result<Prim> {
    // Only the picked occurrence's own bodies: another part's like-named face is not this pick.
    let nf = named_face_in(doc, st, comp, pick, name).map_err(|_| lost("face", name))?;
    if let Some((p, n)) = nf.plane()? {
        return Ok(Prim::Plane { p, n });
    }
    let not_v1 = |kind: &str| DocError::Invalid(format!("a {kind} face is not a v1 pick"));
    let Some(surface) = solidcraft_kernel::face_surface(&nf.body.body, nf.face) else { return Err(not_v1("free-form")) };
    let prim = match surface {
        FaceSurface::Cylinder { origin, axis, radius } => Prim::Cylinder { p: origin, d: *axis, r: radius },
        FaceSurface::Sphere { center, radius } => Prim::Sphere { c: center, r: radius },
        FaceSurface::Plane { normal, point } => Prim::Plane { p: point, n: *normal },
        FaceSurface::Cone { .. } => return Err(not_v1("cone")),
        FaceSurface::Torus { .. } => return Err(not_v1("torus")),
    };
    Ok(placed(prim, &nf.offset))
}

/// `p` moved by a body's offset (as is, for the identity: no rounding of a primitive already in
/// place).
fn placed(p: Prim, offset: &Mat) -> Prim {
    if *offset == IDENTITY { p } else { p.moved(offset) }
}

/// The edge called `name` on component `comp` (the pick tells pieces apart): its body, index
/// and the body's offset.
fn named_edge<'a>(doc: &Document, st: &'a ModelState, comp: u64, name: &str, pick: Vec3) -> Result<(&'a ModelBody, usize, Mat)> {
    component_bodies(doc, st, comp)
        .into_iter()
        .find_map(|b| edge_index(b, Some(name), own(doc, b, pick)).map(|i| (b, i, offset_of(doc, b))))
        .ok_or_else(|| lost("edge", name))
}

/// Edge `i` of `b` as a line (straight) or a circle (round), placed by `offset`.
fn edge(b: &ModelBody, i: usize, offset: &Mat) -> Result<Prim> {
    let m = b.mesh();
    let e = m.edges.get(i).map(Vec::as_slice).unwrap_or_default();
    let place = |p: Prim| placed(p, offset);
    if let Some((p, d)) = straight(e) {
        return Ok(place(Prim::Line { p, d }));
    }
    let Some((c, fitted, r)) = circle(e) else { return Err(DocError::Invalid("a curved edge that is not a circle".into())) };
    // The axis is the outward normal of the planar face beside the edge (the larger one). An
    // edge with no planar face beside it (a cylinder meeting a cone) keeps the normal of the
    // circle's own plane, whose sign follows the polyline's direction: arbitrary in v1, so an
    // Opposed/Aligned Insert on such an edge may take either sense. A body whose faces can't be
    // read is an error, not a fallback.
    let axis = planar_face_beside(b, i)?.map_or(fitted, |(_, n, _)| n);
    Ok(place(Prim::Circle { c, axis, r }))
}

/// A straight polyline's middle and unit direction (start to end).
fn straight(e: &[Vec3]) -> Option<(Vec3, Vec3)> {
    let (a, b) = (*e.first()?, *e.last()?);
    let len = a.dist(b);
    let d = (b - a).normalized()?;
    let off = |p: &Vec3| {
        let v = *p - a;
        (v - d * v.dot(d)).len()
    };
    e.iter().all(|p| off(p) <= STRAIGHT_TOL * len).then_some(((a + b) * 0.5, d))
}

/// The circle a polyline lies on (centre, unit normal, radius): the circle through three of its
/// points (a quarter, half and three quarters round a closed one; the ends and the middle of an
/// arc), when every point lies on it.
fn circle(e: &[Vec3]) -> Option<(Vec3, Vec3, f64)> {
    let n = e.len().checked_sub(1)?;
    if n < 2 {
        return None;
    }
    let (first, last) = (*e.first()?, *e.last()?);
    let span = e.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>();
    let closed = first.dist(last) <= span * 1e-9;
    let (a, b, c) = if closed { (*e.get(n / 4)?, *e.get(n / 2)?, *e.get(3 * n / 4)?) } else { (first, *e.get(n / 2)?, last) };
    let (u, v) = (b - a, c - a);
    let w = u.cross(v);
    if !(w.len2() > u.len2() * v.len2() * 1e-16) {
        return None;
    }
    let centre = a + (v * u.len2() - u * v.len2()).cross(w) * (1.0 / (2.0 * w.len2()));
    let axis = w.normalized()?;
    let r = centre.dist(a);
    let tol = CIRCLE_TOL * r;
    let on = |p: &Vec3| {
        let q = *p - centre;
        let h = q.dot(axis);
        ((q - axis * h).len() - r).abs() <= tol && h.abs() <= tol
    };
    (r.is_finite() && r > 0.0 && e.iter().all(on)).then_some((centre, axis, r))
}

/// Points this close to an edge (a share of the body's size, at least 1 µm) pick it.
fn pick_tol(b: &ModelBody) -> f64 {
    (b.body.size() * 2e-3).max(1e-3)
}

/// Of `bodies`' edges, the one nearest `p` (component coordinates), within the pick tolerance:
/// its body, index and distance.
fn nearest_edge<'a>(doc: &Document, bodies: &[&'a ModelBody], p: Vec3) -> Option<(&'a ModelBody, usize, f64)> {
    bodies
        .iter()
        .filter_map(|b| {
            let q = own(doc, b, p);
            let i = edge_index(b, None, q)?;
            let d = b.mesh().edges.get(i)?.windows(2).map(|w| q.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min);
            Some((*b, i, d))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
}

/// Of `bodies`' edge ends, the one nearest `p` (component coordinates), within the pick
/// tolerance: its body, edge, which end, and the end (component coordinates).
fn nearest_end<'a>(doc: &Document, bodies: &[&'a ModelBody], p: Vec3) -> Option<(&'a ModelBody, usize, EdgeEnd, Vec3)> {
    let mut best: Option<(f64, &'a ModelBody, usize, EdgeEnd, Vec3)> = None;
    for b in bodies {
        let (q, tol, offset) = (own(doc, b, p), pick_tol(b), offset_of(doc, b));
        for (i, e) in b.mesh().edges.iter().enumerate() {
            for (end, at) in [(EdgeEnd::Start, e.first()), (EdgeEnd::End, e.last())] {
                let Some(at) = at else { continue };
                let d = at.dist(q);
                if d <= tol && best.as_ref().is_none_or(|x| d < x.0) {
                    best = Some((d, *b, i, end, apply_point(&offset, *at)));
                }
            }
        }
    }
    best.map(|(_, b, i, end, at)| (b, i, end, at))
}
