//! Constraints compiled to joints (spec §3.4): every solve turns each unsuppressed Mate, Tangent,
//! Insert and Angle into transient joints between its picks' frames (ids from
//! [`COMPILED_BASE`]), seeded from the constraint's `state`, or, when that does not fit, by the
//! values that move B least from where it is; and, where a joint can't say it (spec §3.5), loop
//! equations: a signed or bounded Angle, the distance between two cylinders' axes. A constraint
//! whose picks or offset no longer resolve, or whose pick kinds it can't join, is `lost` and
//! compiles to nothing.
//!
//! Which side moves is the solver's: it places out from the fixed occurrences along either side
//! of a joint, so a grounded B with a free A moves A and the joint is kept as written.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use solidcraft_geom::Vec3;

use super::geo::{Prim, PrimKind, resolve};
use super::{COMPILED_BASE, Constraint, ConstraintKind, ConstraintSolution, StateSignature};
use crate::joints::{self, Assembly, Joint, JointKind, JointOrigin, Snap, Solution, nearest_values, occurrence_world};
use crate::{Document, ModelState, apply_vector};

/// What one solve adds to the saved joints: the constraints' joints and equations, and the
/// constraints that compile to nothing, with why.
#[derive(Clone, Debug, Default)]
pub struct Compiled {
    pub joints: Vec<Joint>,
    /// The loop equations (Angle with a sense or limits, Tangent between cylinders).
    pub equations: Vec<joints::Eq>,
    /// (constraint id, why).
    pub lost: Vec<(u64, String)>,
}

/// Largest offset an expression may give a constraint (mm), as a joint's.
const MAX_EXPR_VALUE: f64 = 1e9;

/// The id of constraint `constraint`'s `k`-th compiled member (`k < 4`).
pub fn compiled_id(constraint: u64, k: u64) -> u64 {
    debug_assert!(k < 4, "a constraint compiles to at most 4 members");
    COMPILED_BASE.saturating_add(constraint.saturating_mul(4)).saturating_add(k.min(3))
}

/// The constraint a compiled id belongs to (`None` for a saved joint's id).
pub fn owner_of(id: u64) -> Option<u64> {
    id.checked_sub(COMPILED_BASE).map(|k| k / 4)
}

/// Compile every unsuppressed constraint of `asm` (Symmetry waits for the second wave).
pub fn compile(doc: &Document, st: &ModelState, asm: &Assembly) -> Compiled {
    let mut out = Compiled::default();
    for c in asm.constraints.iter().filter(|c| !c.suppressed) {
        if c.kind == ConstraintKind::Symmetry {
            continue;
        }
        match one(doc, st, c) {
            Ok((js, eqs)) => {
                out.joints.extend(js);
                out.equations.extend(eqs);
            }
            Err(why) => out.lost.push((c.id, why)),
        }
    }
    out
}

/// One side of a compiled joint: the occurrence and its frame (origin, z) in its coordinates.
type Side = (u64, Vec3, Vec3);

/// What a pick gives a Mate: a plane, a line (a cylinder's or a circle's axis) or a point (a
/// sphere's centre).
enum Role {
    Plane(Vec3, Vec3),
    Line(Vec3, Vec3),
    Point(Vec3),
}

fn mate_role(p: &Prim) -> Role {
    match *p {
        Prim::Plane { p, n } => Role::Plane(p, n),
        Prim::Line { p, d } | Prim::Cylinder { p, d, .. } => Role::Line(p, d),
        Prim::Circle { c, axis, .. } => Role::Line(c, axis),
        Prim::Point { p } | Prim::Sphere { c: p, .. } => Role::Point(p),
    }
}

fn word(k: PrimKind) -> &'static str {
    match k {
        PrimKind::Plane => "plane",
        PrimKind::Line => "line",
        PrimKind::Point => "point",
        PrimKind::Circle => "circle",
        PrimKind::Cylinder => "cylinder",
        PrimKind::Sphere => "sphere",
    }
}

/// A value typed as `expr` (its unit's kind) or as the number `v` (mm, or degrees for an Angle):
/// mm or radians, below `MAX_EXPR_VALUE`.
fn amount(doc: &Document, c: &Constraint, what: &str, expr: Option<&str>, v: f64) -> Result<f64, String> {
    let angle = c.kind.is_angle();
    let Some(e) = expr else { return Ok(if angle { v.to_radians() } else { v }) };
    let kind = if angle { crate::expr::Kind::Angle } else { crate::expr::Kind::Length };
    let v = doc.eval(e, kind).map_err(|err| format!("{what} `{e}`: {err}"))?;
    if v.is_finite() && v.abs() < MAX_EXPR_VALUE { Ok(v) } else { Err(format!("{what} `{e}` gives {v}")) }
}

/// The offset, from its expression when it has one: mm, or radians for an Angle.
fn offset_of(doc: &Document, c: &Constraint) -> Result<f64, String> {
    amount(doc, c, "offset", c.exprs.offset.as_deref(), c.offset)
}

/// The limits as an equation's bounds (mm, or radians for an Angle), `None` without any; a
/// missing side is unbounded.
fn bounds_of(doc: &Document, c: &Constraint) -> Result<Option<(f64, f64)>, String> {
    let side = |what: &str, expr: &Option<String>, v: Option<f64>, none: f64| match v {
        Some(v) => amount(doc, c, what, expr.as_deref(), v),
        None => Ok(none),
    };
    if c.limits.is_empty() {
        return Ok(None);
    }
    let lo = side("min", &c.exprs.min, c.limits.min, f64::NEG_INFINITY)?;
    let hi = side("max", &c.exprs.max, c.limits.max, f64::INFINITY)?;
    Ok(Some((lo.min(hi), hi.max(lo))))
}

/// The direction an Angle measures on a pick: a plane's normal, a line's, an axis's.
fn direction(p: &Prim) -> Option<Vec3> {
    match *p {
        Prim::Plane { n, .. } => Some(n),
        Prim::Line { d, .. } | Prim::Cylinder { d, .. } => Some(d),
        Prim::Circle { axis, .. } => Some(axis),
        Prim::Point { .. } | Prim::Sphere { .. } => None,
    }
}

/// An undirected angle within 0..π: 330° is 30°.
fn unsigned(a: f64) -> f64 {
    let w = a.rem_euclid(std::f64::consts::TAU);
    if w > std::f64::consts::PI { std::f64::consts::TAU - w } else { w }
}

/// Directions closer to parallel than this (the sine of the angle between them) are parallel
/// for an Angle's start.
const PARALLEL_SIN: f64 = 1e-6;

/// A direction with its largest component positive (the first of equal ones).
fn canonical(d: Vec3) -> Vec3 {
    let c = [d.x, d.y, d.z];
    let big = c.iter().fold(0.0f64, |m, v| if v.abs() > m.abs() + 1e-12 { *v } else { m });
    if big < 0.0 { d * -1.0 } else { d }
}

/// Cylinders whose axes are further from parallel than this (radians) can't be tangent in v1.
const PARALLEL_RAD: f64 = 1e-9;

/// A direction of occurrence `occ` in the world.
fn world_dir(doc: &Document, occ: u64, d: Vec3) -> Vec3 {
    if occ == 0 { d } else { apply_vector(&occurrence_world(doc, occ), d) }
}

/// The joints and equations one constraint compiles to (the compile table, spec §3.4, §3.5),
/// the joints seeded.
fn one(doc: &Document, st: &ModelState, c: &Constraint) -> Result<(Vec<Joint>, Vec<joints::Eq>), String> {
    use ConstraintSolution as S;
    let (Some(pa), Some(pb)) = (c.picks.first(), c.picks.get(1)) else { return Err(format!("{} takes 2 picks", c.kind.word())) };
    if pa.occurrence == pb.occurrence {
        return Err("both picks are on one occurrence".into());
    }
    let ga = resolve(doc, st, pa).map_err(|e| e.to_string())?;
    let gb = resolve(doc, st, pb).map_err(|e| e.to_string())?;
    let offset = offset_of(doc, c)?;
    let (oa, ob) = (pa.occurrence, pb.occurrence);
    let cant = || format!("{} can't join a {} and a {}", c.kind.word(), word(ga.kind()), word(gb.kind()));
    let aligned = matches!(c.solution, S::Flush | S::Aligned);
    let eq = |k: u64, eq: joints::Equation| joints::Eq { id: compiled_id(c.id, k), name: c.name.clone(), eq };
    let side = |occurrence: u64, prim: Prim| joints::Side { occurrence, prim };
    // (kind, A side, B side, flip, offset)
    let (kind, a, b, flip, offset): (JointKind, Side, Side, bool, f64) = match c.kind {
        ConstraintKind::Mate => match (mate_role(&ga), mate_role(&gb)) {
            (Role::Plane(p, n), Role::Plane(q, m)) => (JointKind::Planar, (oa, p, n), (ob, q, m), aligned, offset),
            (Role::Line(p, d), Role::Line(q, e)) => {
                // Undirected: the sense the axes have now, chosen again on every compile.
                let flip = if c.solution == S::Undirected { world_dir(doc, oa, d).dot(world_dir(doc, ob, e)) > 0.0 } else { aligned };
                (JointKind::Cylindrical, (oa, p, d), (ob, q, e), flip, 0.0)
            }
            (Role::Plane(p, n), Role::Line(q, e)) => (JointKind::LineOnPlane, (oa, p, n), (ob, q, e), false, offset),
            (Role::Line(q, e), Role::Plane(p, n)) => (JointKind::LineOnPlane, (ob, p, n), (oa, q, e), false, offset),
            (Role::Plane(p, n), Role::Point(q)) => (JointKind::PointOnPlane, (oa, p, n), (ob, q, Vec3::Z), false, offset),
            (Role::Point(q), Role::Plane(p, n)) => (JointKind::PointOnPlane, (ob, p, n), (oa, q, Vec3::Z), false, offset),
            (Role::Line(p, d), Role::Point(q)) => (JointKind::PointOnLine, (oa, p, d), (ob, q, Vec3::Z), false, 0.0),
            (Role::Point(q), Role::Line(p, d)) => (JointKind::PointOnLine, (ob, p, d), (oa, q, Vec3::Z), false, 0.0),
            (Role::Point(p), Role::Point(q)) => (JointKind::Ball, (oa, p, Vec3::Z), (ob, q, Vec3::Z), false, 0.0),
        },
        // Two cylinders (axes parallel): their axes kept parallel by a joint, the distance between
        // them by an equation, r1 + r2 (Outside) or |r1 − r2| (Inside) plus the offset, the radii
        // read on every compile.
        ConstraintKind::Tangent if matches!((ga, gb), (Prim::Cylinder { .. }, Prim::Cylinder { .. })) => {
            let (Prim::Cylinder { p, d, r: ra }, Prim::Cylinder { p: q, d: e, r: rb }) = (ga, gb) else { return Err(cant()) };
            let (wd, we) = (world_dir(doc, oa, d), world_dir(doc, ob, e));
            if wd.cross(we).len() > PARALLEL_RAD.sin() {
                return Err("skew cylinders come in a later version".into());
            }
            let target = if c.solution == S::Inside { (ra - rb).abs() } else { ra + rb } + offset;
            let bounds = bounds_of(doc, c)?.map(|(lo, hi)| (target - offset + lo, target - offset + hi));
            let parallel = seeded(doc, st, c, 0, JointKind::Parallel, (oa, p, d), (ob, q, e), wd.dot(we) > 0.0, 0.0, 0.0);
            let apart = eq(1, joints::Equation::AxisDistance { a: side(oa, ga), b: side(ob, gb), target, bounds });
            return Ok((vec![parallel], vec![apart]));
        }
        ConstraintKind::Angle => {
            let (Some(da), Some(db)) = (direction(&ga), direction(&gb)) else { return Err(cant()) };
            let bounds = bounds_of(doc, c)?;
            let axis = match c.solution {
                // The frozen a × b in A's frame (set when it became Directed).
                S::Directed => {
                    let k = c.axis.and_then(Vec3::normalized).ok_or_else(|| "a Directed Angle needs its axis: edit it to set one".to_string())?;
                    Some(side(oa, Prim::Line { p: Vec3::ZERO, d: k }))
                }
                S::ExplicitVector => {
                    let r = c.picks.get(2).ok_or_else(|| "an Explicit vector Angle takes a third pick".to_string())?;
                    let g = resolve(doc, st, r).map_err(|e| e.to_string())?;
                    if direction(&g).is_none() {
                        return Err(format!("Angle can't measure about a {}", word(g.kind())));
                    }
                    // An edge runs whichever way its B-rep curve does: its sense is its largest
                    // component's (in its component's coordinates), so +30 turns the same way
                    // after any rebuild. A plane's outward normal and an axis keep their own.
                    let g = match g {
                        Prim::Line { p, d } => Prim::Line { p, d: canonical(d) },
                        other => other,
                    };
                    Some(side(r.occurrence, g))
                }
                // Undirected with limits, from two directions parallel now: the unsigned angle
                // has no slope at 0, so this solve turns b about a fixed direction square to a
                // (the next, from where it lands, measures unsigned again).
                _ if bounds.is_some() && world_dir(doc, oa, da).cross(world_dir(doc, ob, db)).len() <= PARALLEL_SIN => {
                    Some(side(oa, Prim::Line { p: Vec3::ZERO, d: da.any_perp() }))
                }
                _ => None,
            };
            // Undirected with no limits: an angle relation (B's z at the angle from A's, either
            // sense); anything else an equation.
            // Undirected: 0..π, folded in degrees when typed as a number (330 is exactly 30).
            let folded = if c.exprs.offset.is_none() {
                {
                    let d = c.offset.rem_euclid(360.0);
                    (if d > 180.0 { 360.0 - d } else { d }).to_radians()
                }
            } else {
                unsigned(offset)
            };
            if axis.is_none() && bounds.is_none() {
                let j = seeded(doc, st, c, 0, JointKind::Angle, (oa, prim_point(&ga), da), (ob, prim_point(&gb), db), true, 0.0, folded);
                return Ok((vec![j], Vec::new()));
            }
            let target = if axis.is_none() { folded } else { offset };
            return Ok((Vec::new(), vec![eq(0, joints::Equation::Angle { a: side(oa, ga), b: side(ob, gb), axis, target, bounds })]));
        }
        ConstraintKind::Tangent => {
            // Outside: the cylinder or sphere on the plane's outward side; Inside: through it.
            // The radius is read from the face on every compile, so a part edit moves it.
            let sign = if c.solution == S::Inside { -1.0 } else { 1.0 };
            let ((op, p, n), other, oo) = match (ga, gb) {
                (Prim::Plane { p, n }, other) => ((oa, p, n), other, ob),
                (other, Prim::Plane { p, n }) => ((ob, p, n), other, oa),
                _ => return Err(cant()),
            };
            match other {
                Prim::Cylinder { p: q, d, r } => (JointKind::LineOnPlane, (op, p, n), (oo, q, d), false, offset + sign * r),
                Prim::Sphere { c: q, r } => (JointKind::PointOnPlane, (op, p, n), (oo, q, Vec3::Z), false, offset + sign * r),
                _ => return Err(cant()),
            }
        }
        ConstraintKind::Insert => match (ga, gb) {
            (Prim::Circle { c: p, axis: n, .. }, Prim::Circle { c: q, axis: m, .. }) => {
                (JointKind::Revolute, (oa, p, n), (ob, q, m), aligned, offset)
            }
            _ => return Err(cant()),
        },
        ConstraintKind::Symmetry => return Err(format!("{} compiles in a later wave", c.kind.word())),
    };
    Ok((vec![seeded(doc, st, c, 0, kind, a, b, flip, offset, 0.0)], Vec::new()))
}

/// A point on the pick for a joint frame: a plane's or line's point, a circle's centre.
fn prim_point(p: &Prim) -> Vec3 {
    match *p {
        Prim::Plane { p, .. } | Prim::Line { p, .. } | Prim::Cylinder { p, .. } | Prim::Point { p } => p,
        Prim::Circle { c, .. } | Prim::Sphere { c, .. } => c,
    }
}

/// Constraint `c`'s `k`-th compiled member, a joint of `kind` between frames `a` and `b`
/// (its first joint member when `k` is 0), seeded from its state or the nearest values.
#[allow(clippy::too_many_arguments)]
fn seeded(doc: &Document, st: &ModelState, c: &Constraint, k: u64, kind: JointKind, a: Side, b: Side, flip: bool, offset: f64, angle: f64) -> Joint {
    let side = |(occurrence, origin, z): Side| JointOrigin { occurrence, snap: Snap::Frame { origin, z, x: None } };
    let mut j = Joint {
        id: compiled_id(c.id, k),
        name: c.name.clone(),
        kind,
        a: side(a),
        b: side(b),
        values: Vec::new(),
        limits: Vec::new(),
        flip,
        offset,
        angle,
        suppressed: false,
        exprs: Default::default(),
    };
    // The saved state when it fits (an Insert keeps its turn, locked or not); else the values
    // that move B least (a new constraint, changed picks, a NaN read back from a file).
    // R132: only a state solved for this kind and flip (a locked Insert's is its Rigid's).
    let n = kind.dofs().len();
    let locked = c.kind == ConstraintKind::Insert && c.lock;
    let sig = StateSignature { kind: if locked { JointKind::Rigid } else { kind }, flip };
    // Every joint member so far is the constraint's first (`k` 0): a Tangent between cylinders
    // adds an equation, not a joint.
    j.values = match c.state_for(0, n, sig) {
        Some(q) => q.to_vec(),
        None => nearest_values(doc, st, &j).unwrap_or_else(|| vec![0.0; n]),
    };
    if locked {
        // Locked at the turn it holds (new: the turn it has now): a rigid joint at that turn.
        j.angle = j.values.first().copied().unwrap_or(0.0);
        j.kind = JointKind::Rigid;
        j.values = Vec::new();
    }
    j
}

/// A constraint's state after a solve (spec §3.7): computed per solve, never saved.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ConstraintStatus {
    Ok,
    /// Met, and takes no freedom away (only `constraint.inspect` works this out).
    Redundant,
    /// Not met: how far it is off (the largest over its compiled members) and the other
    /// loop-closing constraints and joints of its loop group that are off too (R110: the members
    /// of the tree that placed the parts are never named).
    Sick {
        gap_mm: f64,
        gap_deg: f64,
        with: Vec<String>,
    },
    /// A pick or its offset no longer works out.
    Lost {
        reason: String,
    },
    /// Its loop group was too large to solve (a cap): checked, not solved.
    Unsolved,
    Suppressed,
}

/// Every constraint's status after solve `sol` of the constraints `compiled` from `asm`:
/// `Suppressed`; `Lost` when it compiled to nothing or a member is a solve error; `Unsolved` when
/// a member is in a capped loop group; `Sick` when a member is in `sol.conflicts`; else `Ok`.
/// Only loop-closing members can conflict, so the newest constraint of a loop is the sick one
/// (R110), as Inventor marks the one just added.
pub fn statuses(asm: &Assembly, compiled: &Compiled, sol: &Solution) -> BTreeMap<u64, ConstraintStatus> {
    let lost: BTreeMap<u64, &str> = compiled
        .lost
        .iter()
        .map(|(c, why)| (*c, why.as_str()))
        .chain(sol.errors.iter().filter_map(|(id, _, why)| owner_of(*id).map(|c| (c, why.as_str()))))
        .collect();
    let capped: BTreeSet<u64> = sol.groups.iter().filter(|g| g.capped).flat_map(|g| g.members.iter().copied()).collect();
    let conflicts: BTreeMap<u64, &str> = sol.conflicts.iter().map(|(id, name, _)| (*id, name.as_str())).collect();
    let members = |c: u64| (0..4).map(move |k| compiled_id(c, k));
    asm.constraints
        .iter()
        .map(|c| {
            let status = if c.suppressed {
                ConstraintStatus::Suppressed
            } else if let Some(why) = lost.get(&c.id) {
                ConstraintStatus::Lost { reason: (*why).to_string() }
            } else if members(c.id).any(|id| capped.contains(&id)) {
                ConstraintStatus::Unsolved
            } else if members(c.id).any(|id| conflicts.contains_key(&id)) {
                let (mut gap_mm, mut gap_deg) = (0.0_f64, 0.0_f64);
                // As the solver measured them (R145); a loop that meets but can't stay closed on
                // the way has no gap (NaN, which `max` passes over).
                for (mm, rad) in members(c.id).filter_map(|id| sol.conflict_gaps.get(&id)) {
                    gap_mm = gap_mm.max(*mm);
                    gap_deg = gap_deg.max(rad.to_degrees());
                }
                // The other closing members of its loop groups that are off too.
                let own: BTreeSet<u64> = members(c.id).collect();
                let with: Vec<String> = sol
                    .groups
                    .iter()
                    .filter(|g| g.members.iter().any(|id| own.contains(id)))
                    .flat_map(|g| g.members.iter())
                    .filter(|id| !own.contains(id))
                    .filter_map(|id| conflicts.get(id).map(|name| (*name).to_string()))
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                ConstraintStatus::Sick { gap_mm, gap_deg, with }
            } else {
                ConstraintStatus::Ok
            };
            (c.id, status)
        })
        .collect()
}
