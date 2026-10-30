//! Assembly constraints for the UI (Plan E-1): what `constraint.create` measures when asked to
//! predict (spec §3.4 Predict Offset and Orientation), a pick written back as the command takes
//! it, and how far a constraint is from met; each constraint's status after a solve (the
//! session's report), the over-constraint check, the dry-run preview of a draft and which picks a
//! type takes. The commands are `cmd::constraints`.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Value, json};
use solidcraft_doc::Mat;
use solidcraft_doc::constraints::{
    Compiled, Constraint, ConstraintKind, ConstraintSolution, ConstraintStatus, EdgeEnd, Geo, GeoRef, Prim, PrimKind, resolve, statuses,
};
use solidcraft_doc::joints::{self, Analysis, Solution};
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
/// A Directed or Explicit vector Angle measures signed about its reference (R149).
pub fn predict(s: &Session, c: &Constraint) -> Result<Prediction> {
    let kind = c.kind;
    let (Some(a), Some(b)) = (c.picks.first(), c.picks.get(1)) else { return Err(EngineError::Other(format!("{} takes 2 picks", kind.word()))) };
    let st = s.model.state();
    let picks = s.pick_cache.picks(&s.doc, &st);
    let ga = picks.resolve_world(a)?;
    let gb = picks.resolve_world(b)?;
    let about = if kind == ConstraintKind::Angle {
        solidcraft_doc::constraints::angle_reference(&s.doc, &st, c.solution, &c.picks, c.axis).ok().flatten()
    } else {
        None
    };
    predict_prims(kind, &ga, &gb, about).ok_or_else(|| EngineError::Other(format!("{} can't join a {} and a {}", kind.word(), word(&ga), word(&gb))))
}

/// A primitive's kind as a word (`plane`, `circle`…).
pub(crate) fn word(p: &Prim) -> String {
    match serde_json::to_value(p.kind()) {
        Ok(Value::String(s)) => s,
        _ => String::new(),
    }
}

/// [`predict`] on world primitives (`None`: the kind can't join them). An Angle measures signed
/// about `about` (the world reference of a Directed or Explicit vector Angle, R149), unsigned
/// without one.
pub(crate) fn predict_prims(kind: ConstraintKind, a: &Prim, b: &Prim, about: Option<Vec3>) -> Option<Prediction> {
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
            let (da, db) = (a.direction()?, b.direction()?);
            p(joints::measured_angle(da.normalized()?, db.normalized()?, about).to_degrees(), solidcraft_doc::constraints::ANGLE_DEFAULT)
        }
        ConstraintKind::Insert => match (*a, *b) {
            (Prim::Circle { c: ca, axis: na, .. }, Prim::Circle { c: cb, axis: nb, .. }) => {
                p((cb - ca).dot(na), if na.dot(nb) < 0.0 { S::Opposed } else { S::Aligned })
            }
            _ => None,
        },
        // Two cylinders: Outside when they are apart by more than their radii, else Inside.
        ConstraintKind::Tangent if matches!((a, b), (Prim::Cylinder { .. }, Prim::Cylinder { .. })) => {
            let (Prim::Cylinder { p: pa, d, r: ra }, Prim::Cylinder { p: pb, r: rb, .. }) = (*a, *b) else { return None };
            let apart = (pb - pa).cross(d).len();
            if apart >= ra + rb { p(apart - ra - rb, S::Outside) } else { p(apart - (ra - rb).abs(), S::Inside) }
        }
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
/// (mm, deg). An Angle measures as compile does (R149): signed about `about` (its world
/// reference) with the difference wrapped, else unsigned against the offset folded into 0..180.
/// `None` when the kind can't join the picks.
pub(crate) fn gaps(kind: ConstraintKind, solution: ConstraintSolution, offset: f64, a: &Prim, b: &Prim, about: Option<Vec3>) -> Option<(f64, f64)> {
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
        ConstraintKind::Angle => {
            let v = joints::measured_angle(a.direction()?.normalized()?, b.direction()?.normalized()?, about);
            let off = if about.is_some() {
                joints::wrapped(v - offset.to_radians()).abs()
            } else {
                let d = offset.rem_euclid(360.0);
                (v - (if d > 180.0 { 360.0 - d } else { d }).to_radians()).abs()
            };
            Some((0.0, off.to_degrees()))
        }
        ConstraintKind::Insert => match (*a, *b) {
            (Prim::Circle { c: ca, axis: na, .. }, Prim::Circle { c: cb, axis: nb, .. }) => {
                let want = ca + na * offset;
                Some((cb.dist(want), angle(nb, if solution == S::Aligned { na } else { na * -1.0 })))
            }
            _ => None,
        },
        ConstraintKind::Tangent if matches!((a, b), (Prim::Cylinder { .. }, Prim::Cylinder { .. })) => {
            let (Prim::Cylinder { p: pa, d, r: ra }, Prim::Cylinder { p: pb, d: e, r: rb }) = (*a, *b) else { return None };
            let want = if solution == S::Inside { (ra - rb).abs() } else { ra + rb } + offset;
            Some((((pb - pa).cross(d).len() - want).abs(), angle(e, d).min(angle(e, d * -1.0))))
        }
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

// Task 5: status, the session's report, the dry-run preview, pick classification.

/// One constraint as the browser and the glyphs show it (spec §6.4, §6.8).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Row {
    pub id: u64,
    pub name: String,
    pub kind: ConstraintKind,
    pub solution: ConstraintSolution,
    /// Its picks' occurrences, in pick order.
    pub occurrences: Vec<u64>,
    /// mm, or degrees for an Angle (its expression's value when it has one).
    pub offset: f64,
    /// It has a minimum or a maximum.
    pub limited: bool,
    pub status: ConstraintStatus,
    /// The two picks' points in the world, for glyphs and highlights (`None` when one no longer
    /// resolves).
    pub anchors: Option<[Vec3; 2]>,
}

/// Every constraint's row after the last solve, the occurrences' degrees of freedom from the
/// last solve that counted them, and whether any is sick or lost. Session state: not saved, not
/// undone; rebuilt by every solve the session applies, after undo and redo, and after
/// `edit.restore_checkpoint`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Report {
    pub rows: Vec<Row>,
    /// Every occurrence of the assembly, constrained or not (R141), in the design's order.
    pub occurrences: Vec<OccurrenceRow>,
    /// From the last solve that counted them; empty after an undo, redo or restored checkpoint
    /// that changed the assembly, until the next one.
    pub dof: BTreeMap<u64, usize>,
    /// Some row is `Sick` or `Lost`.
    pub any_sick: bool,
}

/// What holds an occurrence (R141): nothing (`Empty`), constraints and joints that are all met
/// (`Ok`), or at least one constraint on it that is sick or lost (`Sick`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceStatus {
    Empty,
    Ok,
    Sick,
}

/// One occurrence as the browser shows its status (R141).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OccurrenceRow {
    pub id: u64,
    pub name: String,
    pub grounded: bool,
    /// Its remaining degrees of freedom: 0 grounded, 6 free and held by nothing; `None` for a
    /// held one until a solve counts them (see [`Report::dof`]).
    pub missing: Option<usize>,
    pub status: OccurrenceStatus,
}

/// Every occurrence's row from the constraint rows and the DOF counts the report holds.
fn occurrence_rows(s: &Session, rows: &[Row], dof: &BTreeMap<u64, usize>) -> Vec<OccurrenceRow> {
    let asm = &s.doc.assembly;
    let mut held: BTreeSet<u64> = asm.joints.iter().filter(|j| !j.suppressed).flat_map(|j| [j.a.occurrence, j.b.occurrence]).collect();
    let mut sick = BTreeSet::new();
    for r in rows {
        if r.status == ConstraintStatus::Suppressed {
            continue;
        }
        held.extend(r.occurrences.iter().copied());
        if matches!(r.status, ConstraintStatus::Sick { .. } | ConstraintStatus::Lost { .. }) {
            sick.extend(r.occurrences.iter().copied());
        }
    }
    s.doc
        .occurrences
        .iter()
        .map(|o| {
            let (status, missing) = if sick.contains(&o.id) {
                (OccurrenceStatus::Sick, dof.get(&o.id).copied())
            } else if held.contains(&o.id) {
                (OccurrenceStatus::Ok, if o.grounded { Some(0) } else { dof.get(&o.id).copied() })
            } else {
                (OccurrenceStatus::Empty, Some(if o.grounded { 0 } else { 6 }))
            };
            OccurrenceRow { id: o.id, name: o.name.clone(), grounded: o.grounded, missing, status }
        })
        .collect()
}

/// The report of a design nothing solves: no constraint rows, every occurrence (R141). Run by
/// the session's refresh when the assembly has no joints or constraints.
pub(crate) fn rebuild_idle(s: &mut Session) {
    let occurrences = occurrence_rows(s, &[], &BTreeMap::new());
    s.constraint_report = Report { occurrences, ..Report::default() };
    // Held by nothing: the rows' counts are exact.
    s.dof_stale = false;
    prune_shown(s);
}

/// The session's constraint report.
pub fn report(s: &Session) -> &Report {
    &s.constraint_report
}

/// Re-solve (placement only) and rebuild the report.
pub fn refresh(s: &mut Session) {
    crate::cmd::joints::place(s);
}

/// A pick's point: a plane's or line's base point, a circle's or sphere's centre.
fn anchor(p: &Prim) -> Vec3 {
    match *p {
        Prim::Plane { p, .. } | Prim::Line { p, .. } | Prim::Point { p } | Prim::Cylinder { p, .. } => p,
        Prim::Circle { c, .. } | Prim::Sphere { c, .. } => c,
    }
}

/// The offset in its unit: its expression's value when it has one that works out.
pub(crate) fn offset_now(s: &Session, c: &Constraint) -> f64 {
    in_unit(s, c, c.exprs.offset.as_deref(), c.offset)
}

/// The limits in the constraint's unit (min, max), each its expression's value when it has one
/// that works out.
pub(crate) fn limits_now(s: &Session, c: &Constraint) -> (Option<f64>, Option<f64>) {
    (c.limits.min.map(|v| in_unit(s, c, c.exprs.min.as_deref(), v)), c.limits.max.map(|v| in_unit(s, c, c.exprs.max.as_deref(), v)))
}

/// A value of `c` typed as `expr` (else the number `v`), in its unit: mm, or degrees for an Angle.
fn in_unit(s: &Session, c: &Constraint, expr: Option<&str>, v: f64) -> f64 {
    let angle = c.kind.is_angle();
    let kind = if angle { solidcraft_doc::expr::Kind::Angle } else { solidcraft_doc::expr::Kind::Length };
    match expr.map(|e| s.doc.eval(e, kind)) {
        Some(Ok(x)) if x.is_finite() => {
            if angle {
                x.to_degrees()
            } else {
                x
            }
        }
        _ => v,
    }
}

/// Each constraint's status after `sol`, a solve of the session's design (its own constraints
/// compiled there: a lost one is a solve error).
pub(crate) fn status_map(s: &Session, sol: &Solution) -> BTreeMap<u64, ConstraintStatus> {
    statuses(&s.doc.assembly, &Compiled::default(), sol)
}

/// Rebuild the report from `sol`, the solve just applied (its DOF only when it counted them).
pub(crate) fn rebuild(s: &mut Session, sol: &Solution, analysis: Analysis) {
    let map = status_map(s, sol);
    let st = s.model.state();
    let picks = s.pick_cache.picks(&s.doc, &st);
    let rows: Vec<Row> = s
        .doc
        .assembly
        .constraints
        .iter()
        .map(|c| {
            let point = |i: usize| c.picks.get(i).and_then(|g| picks.resolve_world(g).ok()).map(|p| anchor(&p));
            Row {
                id: c.id,
                name: c.name.clone(),
                kind: c.kind,
                solution: c.solution,
                occurrences: c.picks.iter().map(|g| g.occurrence).collect(),
                offset: offset_now(s, c),
                limited: c.limits.min.is_some() || c.limits.max.is_some(),
                status: map.get(&c.id).cloned().unwrap_or(ConstraintStatus::Ok),
                anchors: point(0).zip(point(1)).map(|(a, b)| [a, b]),
            }
        })
        .collect();
    let dof = if analysis == Analysis::DegreesOfFreedom { sol.dof.clone() } else { std::mem::take(&mut s.constraint_report.dof) };
    let occurrences = occurrence_rows(s, &rows, &dof);
    let any_sick = rows.iter().any(|r| matches!(r.status, ConstraintStatus::Sick { .. } | ConstraintStatus::Lost { .. }));
    if analysis == Analysis::DegreesOfFreedom {
        s.dof_stale = false;
    } else {
        // A placement solve keeps the last counts; they are stale when what holds the parts
        // changed (a status, a grounding): the session's refresh counts again (R144).
        let was = &s.constraint_report;
        let statuses = |rows: &[Row]| rows.iter().map(|r| (r.id, r.status.clone())).collect::<Vec<_>>();
        let grounding = |o: &[OccurrenceRow]| o.iter().map(|o| (o.id, o.grounded)).collect::<Vec<_>>();
        if statuses(&was.rows) != statuses(&rows) || grounding(&was.occurrences) != grounding(&occurrences) {
            s.dof_stale = true;
        }
    }
    s.constraint_report = Report { rows, occurrences, dof, any_sick };
    prune_shown(s);
}

/// Drop shown glyphs of constraints that no longer exist.
fn prune_shown(s: &mut Session) {
    let ids: BTreeSet<u64> = s.doc.assembly.constraints.iter().map(|c| c.id).collect();
    s.shown_constraints.retain(|id| ids.contains(id));
}

/// After undo, redo or a restored checkpoint changed the design (`changed`): shown glyphs of
/// constraints that are gone are dropped; the DOF counts are marked stale, so the session's
/// refresh that follows counts them again (R144) as it rebuilds the rows; with nothing left to
/// solve the report is rebuilt here (no solve follows).
pub(crate) fn after_history(s: &mut Session, changed: bool) {
    if changed {
        s.dof_stale = true;
    }
    if !s.doc.assembly.solves() {
        rebuild_idle(s);
    }
    prune_shown(s);
}

/// The design with `constraints` in place of its own, placed by a solve with `analysis` (with a
/// copy of the session's DOF memo when it counts): the solution and every status.
fn dry_solve(s: &Session, constraints: Vec<Constraint>, analysis: Analysis) -> (Solution, BTreeMap<u64, ConstraintStatus>) {
    let mut doc = (*s.doc).clone();
    doc.assembly.constraints = constraints;
    let st = s.model.state();
    let held = joints::Held::new();
    let memo = if analysis == Analysis::DegreesOfFreedom { s.dof_memo.lock().ok().map(|m| m.clone()) } else { None };
    let sol = match memo {
        Some(mut memo) => joints::solve_cached(&doc, &st, &held, analysis, Some(&mut memo), &s.pick_cache),
        None => joints::solve_cached(&doc, &st, &held, analysis, None, &s.pick_cache),
    };
    let map = statuses(&doc.assembly, &Compiled::default(), &sol);
    (sol, map)
}

/// The constraints with `candidate` in place of constraint `replacing` (or added).
fn with_candidate(s: &Session, candidate: &Constraint, replacing: Option<u64>) -> Vec<Constraint> {
    let mut all = s.doc.assembly.constraints.clone();
    match replacing.and_then(|id| all.iter_mut().find(|c| c.id == id)) {
        Some(slot) => *slot = candidate.clone(),
        None => all.push(candidate.clone()),
    }
    all
}

/// An id for a candidate that is not in the design yet (above every constraint's).
fn candidate_id(s: &Session) -> u64 {
    s.doc.assembly.constraints.iter().map(|c| c.id).max().unwrap_or(0).saturating_add(1)
}

/// The part an occurrence places, by its component's name (`B`, not the occurrence `B:1`; `the
/// assembly` for 0).
fn part_name(s: &Session, id: u64) -> String {
    if id == 0 {
        return "the assembly".into();
    }
    let Some(o) = s.doc.occurrences.iter().find(|o| o.id == id) else { return id.to_string() };
    s.doc.components.iter().find(|c| c.id == o.component).map_or_else(|| o.name.clone(), |c| c.name.clone())
}

/// Would the design with `constraints` in place of its own over-constrain (spec §3.7, owner Q6,
/// Q16)? Two placement solves, now and with them: every constraint `Sick` with them that was not
/// before is newly sick. `Some("over-constrains <B>: <newly sick, comma-separated> off by
/// <largest gap> mm")` (deg when only an angle is off), B being `blame`'s second pick's
/// occurrence. Only loop-closing members can be sick (R110), so a new constraint that disagrees
/// names itself; one between two fixed occurrences that is not met closes a loop, so it is
/// refused the same way. Redundant constraints that agree are not sick, so they pass.
pub(crate) fn over_constrains_all(s: &Session, constraints: Vec<Constraint>, blame: &Constraint) -> Option<String> {
    let (_, before) = dry_solve(s, s.doc.assembly.constraints.clone(), Analysis::Placement);
    let (_, after) = dry_solve(s, constraints.clone(), Analysis::Placement);
    let (mut mm, mut deg) = (0.0_f64, 0.0_f64);
    let mut names = Vec::new();
    for c in &constraints {
        let Some(ConstraintStatus::Sick { gap_mm, gap_deg, .. }) = after.get(&c.id) else { continue };
        if matches!(before.get(&c.id), Some(ConstraintStatus::Sick { .. })) {
            continue;
        }
        names.push(c.name.clone());
        mm = mm.max(*gap_mm);
        deg = deg.max(*gap_deg);
    }
    if names.is_empty() {
        return None;
    }
    let b = blame.picks.get(1).map_or(0, |g| g.occurrence);
    let gap = if mm > 1e-6 || deg <= 1e-6 { format!("{mm:.3} mm") } else { format!("{deg:.3} deg") };
    Some(format!("over-constrains {}: {} off by {gap}", part_name(s, b), names.join(", ")))
}

/// [`over_constrains_all`] for one candidate: added (`replacing: None`) or in place of constraint
/// `replacing` (an edit, an unsuppress). Used by create, edit and unsuppress.
pub fn over_constrains(s: &Session, candidate: &Constraint, replacing: Option<u64>) -> Option<String> {
    let mut c = candidate.clone();
    if replacing.is_none() {
        c.id = candidate_id(s);
    }
    over_constrains_all(s, with_candidate(s, &c, replacing), &c)
}

/// A constraint being made or edited in the Place Constraint panel (spec §6.3).
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub kind: ConstraintKind,
    /// `None`: the type's default (or the predicted one when `offset` is `None`).
    pub solution: Option<ConstraintSolution>,
    pub a: Option<GeoRef>,
    pub b: Option<GeoRef>,
    pub reference: Option<GeoRef>,
    /// A number or an expression; `None` predicts it from where the parts are.
    pub offset: Option<String>,
    pub min: Option<String>,
    pub max: Option<String>,
    pub lock: bool,
    pub suppressed: bool,
    pub name: Option<String>,
    /// The constraint being edited (`None`: a new one).
    pub editing: Option<u64>,
    /// The record as the edit began ([`Draft::edit`]): `params` then sends only what differs
    /// from it (R147), so an unchanged pick or type is not looked at again (R134).
    pub record: Option<Box<Constraint>>,
}

/// By hand (R116): a Mate with its type's default solution and nothing picked.
impl Default for Draft {
    fn default() -> Draft {
        Draft {
            kind: ConstraintKind::Mate,
            solution: None,
            a: None,
            b: None,
            reference: None,
            offset: None,
            min: None,
            max: None,
            lock: false,
            suppressed: false,
            name: None,
            editing: None,
            record: None,
        }
    }
}

/// A value as a draft holds it: its expression, else the number.
fn text(expr: &Option<String>, v: Option<f64>) -> Option<String> {
    expr.clone().or_else(|| v.map(|v| v.to_string()))
}

impl Draft {
    /// The draft of an edit of `c`: its kind, solution, picks, offset, limits, lock, suppressed
    /// and name as they are (R147).
    pub fn edit(c: &Constraint) -> Draft {
        Draft {
            kind: c.kind,
            solution: Some(c.solution),
            a: c.picks.first().cloned(),
            b: c.picks.get(1).cloned(),
            reference: c.picks.get(2).cloned(),
            offset: text(&c.exprs.offset, Some(c.offset)),
            min: text(&c.exprs.min, c.limits.min),
            max: text(&c.exprs.max, c.limits.max),
            lock: c.lock,
            suppressed: c.suppressed,
            name: Some(c.name.clone()),
            editing: Some(c.id),
            record: Some(Box::new(c.clone())),
        }
    }

    /// The parameters of the command that makes it: `constraint.create`, or `constraint.edit`
    /// when `editing` (a pick left out then keeps the constraint's). An edit begun with
    /// [`Draft::edit`] sends only the fields that differ from the record (R147).
    pub fn params(&self) -> Value {
        if let Some(r) = self.record.as_deref() {
            return self.changes(r);
        }
        let mut p = json!({"type": self.kind, "suppressed": self.suppressed});
        for (key, g) in [("a", &self.a), ("b", &self.b), ("ref", &self.reference)] {
            if let Some(g) = g {
                p[key] = pick_json(g);
            }
        }
        if let Some(sol) = self.solution {
            p["solution"] = json!(sol);
        }
        match &self.offset {
            Some(o) => p["offset"] = json!(o),
            None => p["predict"] = json!(true),
        }
        for (key, v) in [("min", &self.min), ("max", &self.max)] {
            if let Some(v) = v {
                p[key] = json!(v);
            }
        }
        if self.lock {
            p["lock"] = json!(true);
        }
        if let Some(n) = &self.name {
            p["name"] = json!(n);
        }
        if let Some(id) = self.editing {
            p["constraint"] = json!(id);
        }
        p
    }

    /// `constraint.edit`'s parameters for what differs from `r`.
    fn changes(&self, r: &Constraint) -> Value {
        let mut p = json!({"constraint": self.editing.unwrap_or(r.id)});
        if self.kind != r.kind {
            p["type"] = json!(self.kind);
        }
        if let Some(sol) = self.solution.filter(|s| *s != r.solution) {
            p["solution"] = json!(sol);
        }
        for (i, (key, g)) in [("a", &self.a), ("b", &self.b), ("ref", &self.reference)].into_iter().enumerate() {
            match (g, r.picks.get(i)) {
                (Some(g), was) if was != Some(g) => p[key] = pick_json(g),
                (None, Some(_)) if key == "ref" => p[key] = Value::Null,
                _ => {}
            }
        }
        match &self.offset {
            None => p["predict"] = json!(true),
            Some(o) if Some(o) != text(&r.exprs.offset, Some(r.offset)).as_ref() => p["offset"] = json!(o),
            Some(_) => {}
        }
        for (key, v, was) in [("min", &self.min, text(&r.exprs.min, r.limits.min)), ("max", &self.max, text(&r.exprs.max, r.limits.max))] {
            if *v != was {
                p[key] = v.as_ref().map_or(Value::Null, |v| json!(v));
            }
        }
        if self.lock != r.lock {
            p["lock"] = json!(self.lock);
        }
        if self.suppressed != r.suppressed {
            p["suppressed"] = json!(self.suppressed);
        }
        if let Some(n) = self.name.as_ref().filter(|n| **n != r.name) {
            p["name"] = json!(n);
        }
        p
    }
}

/// What the design would be with a draft (spec §6.3 live preview): the placements and DOF of a
/// dry solve, the draft's status, the predicted offset and solution when `offset` is `None`, and
/// why it can't be made (`refused`; then `world` is empty).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preview {
    pub world: BTreeMap<u64, Mat>,
    pub dof: BTreeMap<u64, usize>,
    pub status: Option<ConstraintStatus>,
    pub predicted: Option<Prediction>,
    pub refused: Option<String>,
    /// What the command would warn (R160): a limited constraint whose offset, its start, is
    /// outside its limits starts at the nearer one.
    pub warning: Option<String>,
}

/// A dry solve of the design with draft `d`, through every check its command makes (the
/// over-constraint one too). Never changes `s`.
pub fn preview(s: &Session, d: &Draft) -> Preview {
    let refused = |why: String| Preview { refused: Some(why), ..Preview::default() };
    let base = match d.editing {
        Some(id) => match s.doc.assembly.constraints.iter().find(|c| c.id == id) {
            Some(c) => Some(c.clone()),
            None => return refused(format!("no constraint {id}")),
        },
        None => None,
    };
    let cmd = if base.is_some() { "constraint.edit" } else { "constraint.create" };
    let mut c = match crate::cmd::constraints::build(s, &d.params(), base.as_ref(), cmd) {
        Ok(c) => c,
        Err(EngineError::BadParams { msg, .. }) => return refused(msg),
        Err(e) => return refused(e.to_string()),
    };
    if base.is_none() {
        c.id = candidate_id(s);
    }
    let all = with_candidate(s, &c, d.editing);
    if !c.suppressed
        && let Some(why) = over_constrains_all(s, all.clone(), &c)
    {
        return refused(why);
    }
    let predicted = if d.offset.is_none() { predict(s, &c).ok() } else { None };
    // R160: a limited Angle made, or its offset edited, starts at its offset first (R153), as
    // the command does, so the preview shows the pose the command makes.
    let starts = c.kind == ConstraintKind::Angle
        && !c.suppressed
        && !c.limits.is_empty()
        && base.as_ref().is_none_or(|b| b.offset != c.offset || b.exprs.offset != c.exprs.offset);
    let (sol, map) = if starts {
        let mut t = s.scratch();
        t.doc_mut().assembly.constraints = all;
        crate::cmd::constraints::start_angle(&mut t, c.id);
        let all = t.doc.assembly.constraints.clone();
        dry_solve(&t, all, Analysis::DegreesOfFreedom)
    } else {
        dry_solve(s, all, Analysis::DegreesOfFreedom)
    };
    let warning = crate::cmd::constraints::clamped(s, &c);
    Preview { world: sol.world, dof: sol.dof, status: map.get(&c.id).cloned(), predicted, refused: None, warning }
}

/// What a pick is (`None` when it does not resolve).
pub fn classify(s: &Session, g: &GeoRef) -> Option<PrimKind> {
    resolve(&s.doc, &s.model.state(), g).ok().map(|p| p.kind())
}

/// May a constraint of `kind` take a `candidate` pick in `slot` (0, 1; 2 for an Angle's
/// reference), `first` being slot 0's pick? Mate: any of the six; Angle: planes, lines and
/// cylinders; Tangent: a plane, cylinder or sphere, then a curved one after a plane and a plane or
/// cylinder after a curved one; Insert: circles only.
pub fn accepts(kind: ConstraintKind, slot: usize, first: Option<PrimKind>, candidate: PrimKind) -> bool {
    use PrimKind as K;
    match (kind, slot) {
        (ConstraintKind::Mate, 0 | 1) => true,
        (ConstraintKind::Angle, 0..=2) => matches!(candidate, K::Plane | K::Line | K::Cylinder),
        (ConstraintKind::Tangent, 0) => matches!(candidate, K::Plane | K::Cylinder | K::Sphere),
        (ConstraintKind::Tangent, 1) => match first {
            Some(K::Plane) => matches!(candidate, K::Cylinder | K::Sphere),
            Some(K::Cylinder | K::Sphere) => matches!(candidate, K::Plane | K::Cylinder),
            _ => matches!(candidate, K::Plane | K::Cylinder | K::Sphere),
        },
        (ConstraintKind::Insert, 0 | 1) => candidate == K::Circle,
        _ => false,
    }
}
