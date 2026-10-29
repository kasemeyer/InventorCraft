//! Assembly constraints for the UI (Plan E-1): what `constraint.create` measures when asked to
//! predict (spec §3.4 Predict Offset and Orientation), a pick written back as the command takes
//! it, and how far a constraint is from met. The commands are `cmd::constraints`.

use serde::Serialize;
use serde_json::{Value, json};
use solidcraft_doc::constraints::{ConstraintKind, ConstraintSolution, EdgeEnd, Geo, GeoRef, Prim, resolve_world};
use solidcraft_geom::Vec3;

use crate::{EngineError, Result, Session};

/// What `constraint.create` measures when asked to predict (spec §3.4 Predict Offset and
/// Orientation).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Prediction {
    /// mm, or degrees for an Angle.
    pub offset: f64,
    pub solution: ConstraintSolution,
}

/// The offset and solution that keep the picked parts where they are: Mate planes → the
/// distance along A's normal, `MateFace` when the normals oppose, else `Flush`; Mate lines → 0,
/// `Opposed` or `Aligned` by the axes' senses; Angle → the angle between them, `ANGLE_DEFAULT`;
/// Insert → the distance along A's axis, `Opposed` when the axes oppose; Tangent → the plane to
/// axis (or centre) distance less the radius, `Outside` on the normal side.
pub fn predict(s: &Session, kind: ConstraintKind, a: &GeoRef, b: &GeoRef) -> Result<Prediction> {
    let st = s.model.state();
    let ga = resolve_world(&s.doc, &st, a)?;
    let gb = resolve_world(&s.doc, &st, b)?;
    predict_prims(kind, &ga, &gb).ok_or_else(|| EngineError::Other(format!("{} can't join a {} and a {}", kind.word(), word(&ga), word(&gb))))
}

/// A primitive's kind as a word (`plane`, `circle`…).
pub(crate) fn word(p: &Prim) -> String {
    match serde_json::to_value(p.kind()) {
        Ok(Value::String(s)) => s,
        _ => String::new(),
    }
}

/// [`predict`] on world primitives (`None`: the kind can't join them).
pub(crate) fn predict_prims(kind: ConstraintKind, a: &Prim, b: &Prim) -> Option<Prediction> {
    use ConstraintSolution as S;
    let p = |offset: f64, solution: S| Some(Prediction { offset, solution });
    match kind {
        ConstraintKind::Mate => match (role(a), role(b)) {
            (Role::Plane(pa, na), Role::Plane(pb, nb)) => p((pb - pa).dot(na), if na.dot(nb) < 0.0 { S::MateFace } else { S::Flush }),
            (Role::Line(_, da), Role::Line(_, db)) => p(0.0, if da.dot(db) < 0.0 { S::Opposed } else { S::Aligned }),
            (Role::Plane(pa, n), Role::Line(q, _) | Role::Point(q)) | (Role::Line(q, _) | Role::Point(q), Role::Plane(pa, n)) => {
                p((q - pa).dot(n), S::MateFace)
            }
            _ => p(0.0, S::MateFace),
        },
        ConstraintKind::Angle => {
            let (da, db) = (direction(a)?, direction(b)?);
            p(da.dot(db).clamp(-1.0, 1.0).acos().to_degrees(), solidcraft_doc::constraints::ANGLE_DEFAULT)
        }
        ConstraintKind::Insert => match (*a, *b) {
            (Prim::Circle { c: ca, axis: na, .. }, Prim::Circle { c: cb, axis: nb, .. }) => {
                p((cb - ca).dot(na), if na.dot(nb) < 0.0 { S::Opposed } else { S::Aligned })
            }
            _ => None,
        },
        ConstraintKind::Tangent => {
            let ((pp, n), other) = match (*a, *b) {
                (Prim::Plane { p, n }, o) | (o, Prim::Plane { p, n }) => ((p, n), o),
                _ => return None,
            };
            let (q, r) = match other {
                Prim::Cylinder { p, r, .. } => (p, r),
                Prim::Sphere { c, r } => (c, r),
                _ => return None,
            };
            let d = (q - pp).dot(n);
            if d >= 0.0 { p(d - r, S::Outside) } else { p(d + r, S::Inside) }
        }
        ConstraintKind::Symmetry => None,
    }
}

/// How far a constraint's world picks are from meeting it at `offset` with `solution`:
/// (mm, deg). `None` when the kind can't join the picks.
pub(crate) fn gaps(kind: ConstraintKind, solution: ConstraintSolution, offset: f64, a: &Prim, b: &Prim) -> Option<(f64, f64)> {
    use ConstraintSolution as S;
    let angle = |u: Vec3, v: Vec3| u.dot(v).clamp(-1.0, 1.0).acos().to_degrees();
    // The angle a direction makes with a plane (0 when it lies in it).
    let tilt = |d: Vec3, n: Vec3| d.dot(n).abs().clamp(0.0, 1.0).asin().to_degrees();
    let from_line = |q: Vec3, p: Vec3, d: Vec3| (q - p).cross(d).len();
    match kind {
        ConstraintKind::Mate => Some(match (role(a), role(b)) {
            (Role::Plane(pa, na), Role::Plane(pb, nb)) => {
                (((pb - pa).dot(na) - offset).abs(), angle(nb, if solution == S::Flush { na } else { na * -1.0 }))
            }
            (Role::Line(pa, da), Role::Line(pb, db)) => {
                let deg = match solution {
                    S::Aligned => angle(db, da),
                    S::Undirected => angle(db, da).min(angle(db, da * -1.0)),
                    _ => angle(db, da * -1.0),
                };
                (from_line(pb, pa, da), deg)
            }
            (Role::Plane(p, n), Role::Line(q, d)) | (Role::Line(q, d), Role::Plane(p, n)) => (((q - p).dot(n) - offset).abs(), tilt(d, n)),
            (Role::Plane(p, n), Role::Point(q)) | (Role::Point(q), Role::Plane(p, n)) => (((q - p).dot(n) - offset).abs(), 0.0),
            (Role::Line(p, d), Role::Point(q)) | (Role::Point(q), Role::Line(p, d)) => (from_line(q, p, d), 0.0),
            (Role::Point(p), Role::Point(q)) => (p.dist(q), 0.0),
        }),
        ConstraintKind::Angle => Some((0.0, (angle(direction(a)?, direction(b)?) - offset).abs())),
        ConstraintKind::Insert => match (*a, *b) {
            (Prim::Circle { c: ca, axis: na, .. }, Prim::Circle { c: cb, axis: nb, .. }) => {
                let want = ca + na * offset;
                Some((cb.dist(want), angle(nb, if solution == S::Aligned { na } else { na * -1.0 })))
            }
            _ => None,
        },
        ConstraintKind::Tangent => {
            let ((pp, n), other) = match (*a, *b) {
                (Prim::Plane { p, n }, o) | (o, Prim::Plane { p, n }) => ((p, n), o),
                _ => return None,
            };
            let sign = if solution == S::Inside { -1.0 } else { 1.0 };
            match other {
                Prim::Cylinder { p, d, r } => Some((((p - pp).dot(n) - offset - sign * r).abs(), tilt(d, n))),
                Prim::Sphere { c, r } => Some((((c - pp).dot(n) - offset - sign * r).abs(), 0.0)),
                _ => None,
            }
        }
        ConstraintKind::Symmetry => None,
    }
}

/// What a pick gives a Mate (as the compile reads it): a plane, a line (a cylinder's or a
/// circle's axis) or a point (a sphere's centre).
#[derive(Clone, Copy)]
enum Role {
    Plane(Vec3, Vec3),
    Line(Vec3, Vec3),
    Point(Vec3),
}

fn role(p: &Prim) -> Role {
    match *p {
        Prim::Plane { p, n } => Role::Plane(p, n),
        Prim::Line { p, d } | Prim::Cylinder { p, d, .. } => Role::Line(p, d),
        Prim::Circle { c, axis, .. } => Role::Line(c, axis),
        Prim::Point { p } | Prim::Sphere { c: p, .. } => Role::Point(p),
    }
}

/// Is the pick an axis for a Mate (a line, a cylinder or a circle)?
pub(crate) fn is_axis(p: &Prim) -> bool {
    matches!(role(p), Role::Line(..))
}

/// The direction an Angle measures: a plane's normal or a line's direction.
fn direction(p: &Prim) -> Option<Vec3> {
    match *p {
        Prim::Plane { n, .. } => Some(n),
        Prim::Line { d, .. } | Prim::Cylinder { d, .. } => Some(d),
        Prim::Circle { axis, .. } => Some(axis),
        _ => None,
    }
}

fn xyz(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}

/// A pick as `constraint.create` takes it back: `{occurrence, face|edge: "<name>", near: [x,y,z]}`
/// (near in the component's coordinates) | `{occurrence, vertex: "<edge name>", end, near}` |
/// `{occurrence, origin}`.
pub fn pick_json(g: &GeoRef) -> Value {
    match &g.geo {
        Geo::Face { name, pick } => json!({"occurrence": g.occurrence, "face": name, "near": xyz(*pick)}),
        Geo::Edge { name, pick } => json!({"occurrence": g.occurrence, "edge": name, "near": xyz(*pick)}),
        Geo::Vertex { edge, end, pick } => json!({"occurrence": g.occurrence, "vertex": edge,
            "end": if *end == EdgeEnd::Start { "start" } else { "end" }, "near": xyz(*pick)}),
        Geo::Origin { item } => json!({"occurrence": g.occurrence, "origin": item}),
    }
}

/// A primitive as `constraint.inspect` gives it: `{kind, p|c, n|d|axis, r?}`.
pub(crate) fn prim_json(p: &Prim) -> Value {
    match *p {
        Prim::Plane { p, n } => json!({"kind": "plane", "p": xyz(p), "n": xyz(n)}),
        Prim::Line { p, d } => json!({"kind": "line", "p": xyz(p), "d": xyz(d)}),
        Prim::Point { p } => json!({"kind": "point", "p": xyz(p)}),
        Prim::Circle { c, axis, r } => json!({"kind": "circle", "c": xyz(c), "axis": xyz(axis), "r": r}),
        Prim::Cylinder { p, d, r } => json!({"kind": "cylinder", "p": xyz(p), "d": xyz(d), "r": r}),
        Prim::Sphere { c, r } => json!({"kind": "sphere", "c": xyz(c), "r": r}),
    }
}
