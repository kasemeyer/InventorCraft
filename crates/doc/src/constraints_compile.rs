//! Constraints compiled to joints (spec §3.4): every solve turns each unsuppressed Mate, Tangent
//! and Insert into one transient joint between its picks' frames (ids from [`COMPILED_BASE`]),
//! seeded from the constraint's `state`, or, when that does not fit, by the values that move B
//! least from where it is. A constraint whose picks or offset no longer resolve, or whose pick
//! kinds it can't join, is `lost` and compiles to nothing.
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
    /// Empty until the Angle and Tangent equations (Task 6).
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

/// Compile every unsuppressed constraint of `asm` (Angle and Symmetry wait for Task 6).
pub fn compile(doc: &Document, st: &ModelState, asm: &Assembly) -> Compiled {
    let mut out = Compiled::default();
    for c in asm.constraints.iter().filter(|c| !c.suppressed) {
        if !matches!(c.kind, ConstraintKind::Mate | ConstraintKind::Tangent | ConstraintKind::Insert) {
            continue;
        }
        match one(doc, st, c) {
            Ok(j) => out.joints.push(j),
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

/// The offset, from its expression when it has one (mm, below `MAX_EXPR_VALUE`).
fn offset_of(doc: &Document, c: &Constraint) -> Result<f64, String> {
    let Some(e) = c.exprs.offset.as_deref() else { return Ok(c.offset) };
    let v = doc.eval(e, crate::expr::Kind::Length).map_err(|err| format!("offset `{e}`: {err}"))?;
    if v.is_finite() && v.abs() < MAX_EXPR_VALUE { Ok(v) } else { Err(format!("offset `{e}` gives {v}")) }
}

/// A direction of occurrence `occ` in the world.
fn world_dir(doc: &Document, occ: u64, d: Vec3) -> Vec3 {
    if occ == 0 { d } else { apply_vector(&occurrence_world(doc, occ), d) }
}

/// The joint one constraint compiles to (the compile table, spec §3.4), seeded.
fn one(doc: &Document, st: &ModelState, c: &Constraint) -> Result<Joint, String> {
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
        ConstraintKind::Angle | ConstraintKind::Symmetry => return Err(format!("{} compiles in a later wave", c.kind.word())),
    };
    let side = |(occurrence, origin, z): Side| JointOrigin { occurrence, snap: Snap::Frame { origin, z, x: None } };
    let mut j = Joint {
        id: compiled_id(c.id, 0),
        name: c.name.clone(),
        kind,
        a: side(a),
        b: side(b),
        values: Vec::new(),
        limits: Vec::new(),
        flip,
        offset,
        angle: 0.0,
        suppressed: false,
        exprs: Default::default(),
    };
    // The saved state when it fits (an Insert keeps its turn, locked or not); else the values
    // that move B least (a new constraint, changed picks, a NaN read back from a file).
    // R132: only a state solved for this kind and flip (a locked Insert's is its Rigid's).
    let n = kind.dofs().len();
    let locked = c.kind == ConstraintKind::Insert && c.lock;
    let sig = StateSignature { kind: if locked { JointKind::Rigid } else { kind }, flip };
    j.values = match c.state_for(n, sig) {
        Some(q) => q.clone(),
        None => nearest_values(doc, st, &j).unwrap_or_else(|| vec![0.0; n]),
    };
    if locked {
        // Locked at the turn it holds (new: the turn it has now): a rigid joint at that turn.
        j.angle = j.values.first().copied().unwrap_or(0.0);
        j.kind = JointKind::Rigid;
        j.values = Vec::new();
    }
    Ok(j)
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

/// The gap a conflict's message gives ("off by X mm and Y° …"): (mm, deg); (0, 0) for a loop
/// that meets but can't stay closed on the way.
fn gap_of(why: &str) -> (f64, f64) {
    let Some(rest) = why.strip_prefix("off by ") else { return (0.0, 0.0) };
    let mut words = rest.split_whitespace();
    let mm = words.next().and_then(|w| w.parse::<f64>().ok()).unwrap_or(0.0);
    let deg = words.nth(2).and_then(|w| w.trim_end_matches('°').parse::<f64>().ok()).unwrap_or(0.0);
    (mm, deg)
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
    let conflicts: BTreeMap<u64, (&str, &str)> = sol.conflicts.iter().map(|(id, name, why)| (*id, (name.as_str(), why.as_str()))).collect();
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
                for (_, why) in members(c.id).filter_map(|id| conflicts.get(&id)) {
                    let (mm, deg) = gap_of(why);
                    gap_mm = gap_mm.max(mm);
                    gap_deg = gap_deg.max(deg);
                }
                // The other closing members of its loop groups that are off too.
                let own: BTreeSet<u64> = members(c.id).collect();
                let with: Vec<String> = sol
                    .groups
                    .iter()
                    .filter(|g| g.members.iter().any(|id| own.contains(id)))
                    .flat_map(|g| g.members.iter())
                    .filter(|id| !own.contains(id))
                    .filter_map(|id| conflicts.get(id).map(|(name, _)| (*name).to_string()))
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
