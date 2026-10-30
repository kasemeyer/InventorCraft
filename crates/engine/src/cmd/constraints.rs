//! Assembly constraint commands (`constraint.*`, Plan E-1, spec §5): Constrain (Mate, Angle,
//! Tangent and Insert between picks on two occurrences), edit, suppress, delete, list and
//! inspect. Every change re-solves the assembly (the constraints compile to joints in every
//! solve) through `cmd::joints::resolve`, which also refreshes the session's DOF memo with that
//! solve; each command is one undo step, and an edit that changes nothing is none.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_doc::constraints::{
    Constraint, ConstraintExprs, ConstraintKind, ConstraintLimits, ConstraintSolution, ConstraintStatus, EdgeEnd, Geo, GeoRef, MAX_CONSTRAINTS,
    OriginItem, PARALLEL_SIN, PickWhat, Prim, TANGENT_PARALLEL_SIN, angle_reference, check_name, compile_with, geo_at, next_name, owner_of, resolve,
};
use solidcraft_doc::joints::{self, Analysis, occurrence_world};
use solidcraft_doc::{Document, DocumentKind, Mat};
use solidcraft_geom::Vec3;

use super::CommandSpec;
use super::joints::{occurrence_id, typed, value};
use crate::constraints::{
    Prediction, gaps, is_axis, limits_now, offset_now, over_constrains, over_constrains_all, pick_json, predict_prims, prim_json, status_map, word,
};
use crate::params::{bad, vec3};
use crate::{Result, Session};

/// A pick, as every command that takes one reads it.
const PICK: &str = "a pick is {occurrence: id|name, face|edge: \"<name>\" | [x,y,z]} (a world point picks the nearest face/edge on that occurrence; a name may take near?: [x,y,z] in the component's coordinates), {occurrence, vertex: [x,y,z]} (or vertex: \"<edge name>\", end?: start|end, near?), or {occurrence?: id|name (default: the assembly), origin: \"xy\"|\"xz\"|\"yz\"|\"x\"|\"y\"|\"z\"|\"center\"}";

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("constraint.create", "Constrain", create).icon("joint").enabled(has_components).params(
        "type: mate|angle|tangent|insert (symmetry: second wave, refused with that reason in v1); a, b: picks; ref?: a third pick (Angle explicit vector); solution? (default per §1.2); axis?: [x,y,z] (Directed Angle only: the world direction it turns about, kept in A's frame; default a × b of the picks as made); offset?: number or expression (default 0; predict: true measures it instead); predict?: bool (default false on the command line, true from the dialog); min?, max?: number or expression; lock?: bool (Insert only); suppressed?: bool (default false); name?. A pick is {occurrence: id|name, face|edge: \"<name>\" | [x,y,z]} (a world point picks the nearest face/edge on that occurrence, as joint.create does; a name may take near?: [x,y,z] in the component's coordinates), {occurrence, vertex: [x,y,z]} (or vertex: \"<edge name>\", end?: start|end, near?), or {occurrence?: id|name (default: the assembly), origin: \"xy\"|\"xz\"|\"yz\"|\"x\"|\"y\"|\"z\"|\"center\"} → {id, name, status, dof: {<occurrence>: n}, moved: [occurrences]}",
    ),
    CommandSpec::new("constraint.edit", "Edit Constraint", edit).icon("edit").enabled(has_components).params(
        "constraint: id|name; any of type, a, b, ref, solution, axis, offset, min, max (null clears), lock, name → as create; an edit that changes nothing adds no undo step",
    ),
    CommandSpec::new("constraint.suppress", "Suppress Constraint", suppress)
        .icon("eye")
        .enabled(has_constraints)
        .params("constraint: id|name or a list; suppressed?: bool (default toggles) → {constraints: [{name, suppressed, status}]}"),
    CommandSpec::new("constraint.delete", "Delete Constraint", delete).icon("delete").enabled(has_constraints).params("constraint: id|name or a list → {deleted: n}"),
    CommandSpec::new("constraint.drive", "Drive Constraint", drive).icon("drive").noundo().enabled(has_constraints).params(
        "constraint: id|name; start? (default its value, mm or deg); end? (default start + 10); increment? (default 1) or steps? (1-9999: steps + 1 frames); repeat?: 1-1000 (default 1); cycle?: start_end|start_end_start (default start_end); stop_on_contact?: bool (default false: the assembly's contact sets, as joint.drive). At most 10,000 frames. The design is left as it was (no undo step); committing a value is constraint.edit → {frames: [{value, world: {<occurrence>: Mat}}] (the occurrences moved from where they are), stopped_by_contact?: {value: the last free value, pair: [a, b]}, stopped_by_limit?: {value, limit: min|max}, stopped_by_loop?: {value, constraint}}",
    ),
    CommandSpec::new("constraint.list", "List Constraints", list).icon("list").noundo().params(
        "occurrence? (default all) → {constraints: [{id, name, type, solution, occurrences: [a, b, ref?], offset, unit, limits, expressions?, suppressed, status}], dof: {<occurrence>: n}}",
    ),
    CommandSpec::new("constraint.inspect", "Inspect Constraint", inspect).icon("inspect").noundo().params(
        "constraint? → as list (status redundant when it is met and takes no freedom away) + per pick the resolved primitive in world (picks: [{…, resolved: {kind, p|c, n|d|axis, r?}}]), the compiled joints/equations (compiled: [{id, kind, flip, offset}]), gaps: {mm, deg} (how far the picks are from meeting it)",
    ),
    CommandSpec::new("constraint.show", "Show Constraints", show)
        .icon("eye")
        .noundo()
        .enabled(has_constraints)
        .params("occurrences: [ids|names] (the constraints picking any of them are added to the shown glyphs; an empty list clears) → {shown: [names]}"),
    CommandSpec::new("constraint.show_sick", "Show Problems", show_sick)
        .icon("inspect")
        .noundo()
        .enabled(any_sick)
        .params("→ {shown: [names]}: every sick or lost constraint, in place of the shown glyphs"),
    CommandSpec::new("constraint.hide_all", "Hide All Constraints", hide_all)
        .icon("eye")
        .noundo()
        .enabled(has_constraints)
        .params("→ {shown: []}: no constraint glyph is shown"),
];

const ASSEMBLY: &str = "needs an assembly with components";

/// A Drawing, or a design with no occurrences, has nothing to constrain.
fn has_components(s: &Session) -> std::result::Result<(), String> {
    if s.doc.kind == DocumentKind::Drawing || s.doc.occurrences.is_empty() { Err(ASSEMBLY.into()) } else { Ok(()) }
}

fn has_constraints(s: &Session) -> std::result::Result<(), String> {
    has_components(s)?;
    if s.doc.assembly.constraints.is_empty() { Err("no constraints yet".into()) } else { Ok(()) }
}

fn any_sick(s: &Session) -> std::result::Result<(), String> {
    if s.constraint_report.any_sick { Ok(()) } else { Err("nothing is sick".into()) }
}

fn occurrence_name(s: &Session, id: u64) -> String {
    if id == 0 {
        return "the assembly".into();
    }
    s.doc.occurrences.iter().find(|o| o.id == id).map(|o| o.name.clone()).unwrap_or_else(|| id.to_string())
}

/// A point of a pick: [x,y,z] within ±1e7 mm (`MAX_OFFSET_MM`), the size a design can have.
fn point(v: &Value, cmd: &str, what: &str, key: &str) -> Result<Vec3> {
    let cap = solidcraft_doc::constraints::MAX_OFFSET_MM;
    let xyz: Option<Vec<f64>> = v.as_array().filter(|a| a.len() == 3).and_then(|a| a.iter().map(Value::as_f64).collect());
    match xyz {
        Some(c) if c.iter().all(|c| c.abs() <= cap) => vec3(v).ok_or_else(|| bad(cmd, format!("`{what}`: {key} must be a point [x,y,z]"))),
        Some(_) => Err(bad(cmd, format!("`{what}`: {key} must be within ±{cap} mm"))),
        None => Err(bad(cmd, format!("`{what}`: {key} must be a name or a point [x,y,z]"))),
    }
}

/// A pick parameter (`what` names it in errors), checked to resolve.
fn pick(s: &Session, v: Option<&Value>, what: &str, cmd: &str) -> Result<GeoRef> {
    let Some(v) = v.filter(|v| v.is_object()) else { return Err(bad(cmd, format!("`{what}` must be a pick: {PICK}"))) };
    let st = s.model.state();
    let g = if let Some(o) = v.get("origin").filter(|o| !o.is_null()) {
        let occurrence = match v.get("occurrence") {
            None | Some(Value::Null) => 0,
            x => occurrence_id(s, x, cmd)?,
        };
        let item: OriginItem = o
            .as_str()
            .and_then(|t| serde_json::from_value(Value::String(t.trim().to_ascii_lowercase())).ok())
            .ok_or_else(|| bad(cmd, format!("`{what}`: origin must be xy, xz, yz, x, y, z or center")))?;
        GeoRef { occurrence, geo: Geo::Origin { item } }
    } else {
        let occurrence = occurrence_id(s, v.get("occurrence"), cmd)?;
        let kinds = [("face", PickWhat::Face), ("edge", PickWhat::Edge), ("vertex", PickWhat::Vertex)];
        let Some((key, kind)) = kinds.into_iter().find(|(k, _)| v.get(*k).is_some_and(|x| !x.is_null())) else {
            return Err(bad(cmd, format!("`{what}` needs a face, edge, vertex or origin: {PICK}")));
        };
        if occurrence == 0 {
            return Err(bad(
                cmd,
                format!("`{what}`: the assembly itself has no faces, edges or vertices to pick: pick a component's, or an origin item"),
            ));
        }
        match v.get(key) {
            Some(Value::String(name)) => {
                let pick = match v.get("near") {
                    None | Some(Value::Null) => Vec3::ZERO,
                    Some(n) => point(n, cmd, what, "near")?,
                };
                let name = name.clone();
                let geo = match kind {
                    PickWhat::Face => Geo::Face { name, pick },
                    PickWhat::Edge => Geo::Edge { name, pick },
                    PickWhat::Vertex => {
                        let end = match v.get("end") {
                            None | Some(Value::Null) => EdgeEnd::Start,
                            Some(Value::String(e)) if e == "start" => EdgeEnd::Start,
                            Some(Value::String(e)) if e == "end" => EdgeEnd::End,
                            Some(_) => return Err(bad(cmd, format!("`{what}`: end must be start or end"))),
                        };
                        Geo::Vertex { edge: name, end, pick }
                    }
                };
                GeoRef { occurrence, geo }
            }
            Some(x) => {
                let p = point(x, cmd, what, key)?;
                geo_at(&s.doc, &st, occurrence, kind, p).map_err(|e| bad(cmd, format!("`{what}`: {e}")))?
            }
            None => return Err(bad(cmd, format!("`{what}` needs a face, edge, vertex or origin"))),
        }
    };
    resolve(&s.doc, &st, &g).map_err(|e| bad(cmd, format!("`{what}`: {e}")))?;
    Ok(g)
}

/// A number-or-text value in its unit (degrees for an Angle), with the expression it was typed
/// as (kept, so the constraint follows the parameters it names).
fn amount(s: &Session, v: &Value, angle: bool, key: &str, cmd: &str) -> Result<(f64, Option<String>)> {
    use solidcraft_doc::constraints::{MAX_OFFSET_DEG, MAX_OFFSET_MM};
    let (cap, unit) = if angle { (MAX_OFFSET_DEG, "deg") } else { (MAX_OFFSET_MM, "mm") };
    // A number out of range says so in the constraint's terms (the joint check would say only
    // that it is not a number).
    if let Some(x) = v.as_f64()
        && x.abs() > cap
    {
        return Err(bad(cmd, format!("{key} {x} is not a number within ±{cap} {unit}")));
    }
    let x = value(s, v, angle, cmd)?;
    Ok((if angle { x.to_degrees() } else { x }, typed(Some(v))))
}

/// A true/false parameter: absent or null is `None`; anything but a boolean is refused.
fn flag(p: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(x)) => Ok(Some(*x)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be true or false"))),
    }
}

fn kind_param(p: &Value, cmd: &str) -> Result<Option<ConstraintKind>> {
    match p.get("type") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(t)) => match ConstraintKind::parse(t) {
            Some(ConstraintKind::Symmetry) => Err(bad(cmd, "Symmetry comes in a later version")),
            Some(k) => Ok(Some(k)),
            None => Err(bad(cmd, "`type` must be mate, angle, tangent or insert")),
        },
        Some(_) => Err(bad(cmd, "`type` must be mate, angle, tangent or insert")),
    }
}

fn solution_param(p: &Value, kind: ConstraintKind, cmd: &str) -> Result<Option<ConstraintSolution>> {
    match p.get("solution") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(t)) => {
            let sol = ConstraintSolution::parse(t).ok_or_else(|| bad(cmd, format!("unknown solution `{t}`")))?;
            if !sol.fits(kind) {
                return Err(bad(cmd, format!("{} has no `{}` solution", kind.word(), t.trim())));
            }
            Ok(Some(sol))
        }
        Some(_) => Err(bad(cmd, "`solution` must be text")),
    }
}

/// Do the picks (world primitives) take `solution` of `kind`? Err says why not.
fn fits_picks(kind: ConstraintKind, solution: ConstraintSolution, a: &Prim, b: &Prim, cmd: &str) -> Result<()> {
    use ConstraintSolution as S;
    match kind {
        ConstraintKind::Mate => {
            let axes = is_axis(a) && is_axis(b);
            let ok = if axes { matches!(solution, S::Opposed | S::Aligned | S::Undirected) } else { matches!(solution, S::MateFace | S::Flush) };
            if ok {
                return Ok(());
            }
            let sol = serde_json::to_value(solution).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            Err(bad(cmd, format!("the `{sol}` solution does not fit a {} and a {}", word(a), word(b))))
        }
        ConstraintKind::Insert => match (a, b) {
            (Prim::Circle { .. }, Prim::Circle { .. }) => Ok(()),
            _ => Err(bad(cmd, format!("Insert takes two circular edges, not a {} and a {}", word(a), word(b)))),
        },
        ConstraintKind::Tangent => match (a, b) {
            (Prim::Cylinder { d, .. }, Prim::Cylinder { d: e, .. }) if d.cross(*e).len() > TANGENT_PARALLEL_SIN => {
                Err(bad(cmd, "skew cylinders come in a later version: pick cylinders whose axes are parallel"))
            }
            _ => predict_prims(kind, a, b, None).map(|_| ()).ok_or_else(|| bad(cmd, format!("Tangent can't join a {} and a {}", word(a), word(b)))),
        },
        _ => {
            predict_prims(kind, a, b, None).map(|_| ()).ok_or_else(|| bad(cmd, format!("{} can't join a {} and a {}", kind.word(), word(a), word(b))))
        }
    }
}

/// What `p` sets on a value that may have been typed as an expression (`base` when not given;
/// null clears it).
type Typed = (Option<f64>, Option<String>);

/// The constraint `create` (no `base`) or `edit` (`base`) makes from `p`: every check but the
/// over-constraint one (`crate::constraints::over_constrains`, which needs the candidate) is
/// here, so a refusal changes nothing.
pub(crate) fn build(s: &Session, p: &Value, base: Option<&Constraint>, cmd: &str) -> Result<Constraint> {
    let kind = match (kind_param(p, cmd)?, base) {
        (Some(k), _) => k,
        (None, Some(b)) => b.kind,
        (None, None) => return Err(bad(cmd, "`type` must be mate, angle, tangent or insert")),
    };
    let given = solution_param(p, kind, cmd)?;
    let mut slots: [Option<GeoRef>; 3] = [0, 1, 2].map(|i| base.and_then(|b| b.picks.get(i).cloned()));
    for (i, key) in ["a", "b", "ref"].into_iter().enumerate() {
        match p.get(key) {
            None => {}
            Some(Value::Null) if key == "ref" => slots[2] = None,
            v => slots[i] = Some(pick(s, v, key, cmd)?),
        }
    }
    // Exactly the picks its solution takes (R128), counted as `validate` counts them.
    let counted = given.or(base.filter(|b| b.kind == kind).map(|b| b.solution)).unwrap_or(kind.default_solution(false));
    let want = kind.picks(counted);
    let present = slots.iter().flatten().count();
    if let Some(missing) = ["a", "b", "ref"].into_iter().take(want).zip(&slots).find_map(|(k, g)| g.is_none().then_some(k)) {
        return Err(bad(cmd, format!("`{missing}` is missing: {} takes {want} picks", kind.word())));
    }
    if present != want {
        return Err(bad(cmd, format!("{} takes {want} picks, not {present}", kind.word())));
    }
    let picks: Vec<GeoRef> = slots.iter().flatten().cloned().collect();
    let (Some(ga), Some(gb)) = (picks.first(), picks.get(1)) else { return Err(bad(cmd, format!("{} takes {want} picks", kind.word()))) };
    if ga.occurrence == gb.occurrence {
        return Err(bad(cmd, format!("both picks are on {}: pick another component", occurrence_name(s, ga.occurrence))));
    }
    let predict = flag(p, "predict", cmd)?.unwrap_or(false);
    // An edit that keeps the picks, type and solution does not look at the geometry again (R134):
    // a Lost constraint can still be renamed, given another offset, limits, lock or suppressed.
    let same_geometry = base.is_some_and(|b| b.picks == picks && b.kind == kind && given.is_none_or(|g| g == b.solution)) && !predict;
    let given_axis = match p.get("axis") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let cap = solidcraft_doc::constraints::MAX_OFFSET_MM;
            let w = vec3(v).filter(|w| w.x.abs().max(w.y.abs()).max(w.z.abs()) <= cap);
            Some(w.and_then(Vec3::normalized).ok_or_else(|| bad(cmd, "`axis` must be a direction [x,y,z]: not zero, each within ±10000000"))?)
        }
    };
    // The picks' world primitives, when this build looks at them.
    let mut looked: Option<(Prim, Prim)> = None;
    let (solution, prediction) = match base {
        Some(b) if same_geometry => (b.solution, None),
        _ => {
            let st = s.model.state();
            let world = |g: &GeoRef, what: &str| s.pick_cache.picks(&s.doc, &st).resolve_world(g).map_err(|e| bad(cmd, format!("`{what}`: {e}")));
            let (wa, wb) = (world(ga, "a")?, world(gb, "b")?);
            looked = Some((wa, wb));
            let prediction = if predict { predict_prims(kind, &wa, &wb, None) } else { None };
            let solution = match (given, prediction, base) {
                (Some(g), ..) => g,
                (None, Some(pr), _) => pr.solution,
                (None, None, Some(b)) if b.kind == kind && fits_picks(kind, b.solution, &wa, &wb, cmd).is_ok() => b.solution,
                _ => kind.default_solution(is_axis(&wa) && is_axis(&wb)),
            };
            fits_picks(kind, solution, &wa, &wb, cmd)?;
            (solution, prediction)
        }
    };
    // An Explicit vector Angle measures about its third pick's direction: a plane or a line.
    if kind == ConstraintKind::Angle
        && solution == ConstraintSolution::ExplicitVector
        && !same_geometry
        && let Some(r) = picks.get(2)
    {
        let w = s.pick_cache.picks(&s.doc, &s.model.state()).resolve_world(r).map_err(|e| bad(cmd, format!("`ref`: {e}")))?;
        if w.direction().is_none() {
            return Err(bad(cmd, format!("`ref` must be a plane, a line or an axis to measure about, not a {}", word(&w))));
        }
    }
    if given_axis.is_some() && !(kind == ConstraintKind::Angle && solution == ConstraintSolution::Directed) {
        return Err(bad(cmd, "`axis` is for a Directed Angle only"));
    }
    let axis = if kind == ConstraintKind::Angle && solution == ConstraintSolution::Directed {
        directed_axis(s, ga, gb, given_axis, looked, base.filter(|b| b.solution == ConstraintSolution::Directed && b.picks == picks))
    } else {
        None
    };
    // A predicted Directed or Explicit vector angle is measured about its reference, as compile
    // will (R149): from −30° it predicts −30, so the solve keeps B where it is.
    let prediction = match (prediction, looked) {
        (Some(pr), Some((wa, wb))) if kind == ConstraintKind::Angle => {
            let about = angle_reference(&s.doc, &s.model.state(), solution, &picks, axis).ok().flatten();
            predict_prims(kind, &wa, &wb, about).map(|q| Prediction { solution: pr.solution, ..q })
        }
        (pr, _) => pr,
    };
    let angle = kind.is_angle();
    let (offset, offset_expr) = match p.get("offset") {
        Some(Value::Null) => return Err(bad(cmd, "`offset` must be a number or an expression")),
        Some(v) => amount(s, v, angle, "offset", cmd)?,
        None => match (prediction, base) {
            (Some(pr), _) => (pr.offset, None),
            (None, Some(b)) => (b.offset, b.exprs.offset.clone()),
            (None, None) => (0.0, None),
        },
    };
    let limit = |key: &str, was: Typed| -> Result<Typed> {
        match p.get(key) {
            None => Ok(was),
            Some(Value::Null) => Ok((None, None)),
            Some(v) => amount(s, v, angle, key, cmd).map(|(x, e)| (Some(x), e)),
        }
    };
    let (min, min_expr) = limit("min", base.map_or((None, None), |b| (b.limits.min, b.exprs.min.clone())))?;
    let (max, max_expr) = limit("max", base.map_or((None, None), |b| (b.limits.max, b.exprs.max.clone())))?;
    // A Directed Angle turns to its one signed value: no limits (spec §2.2, R152).
    if kind == ConstraintKind::Angle && solution == ConstraintSolution::Directed && (min.is_some() || max.is_some()) {
        return Err(bad(cmd, "Directed angle takes no limits"));
    }
    let lock = match p.get("lock") {
        None | Some(Value::Null) => base.is_some_and(|b| b.lock) && kind == ConstraintKind::Insert,
        Some(Value::Bool(true)) if kind != ConstraintKind::Insert => return Err(bad(cmd, format!("lock is for Insert only, not {}", kind.word()))),
        Some(Value::Bool(l)) => *l,
        Some(_) => return Err(bad(cmd, "`lock` must be true or false")),
    };
    let suppressed = flag(p, "suppressed", cmd)?.unwrap_or(base.is_some_and(|b| b.suppressed));
    let name = match p.get("name") {
        None | Some(Value::Null) => base.map_or_else(|| next_name(&s.doc.assembly, kind), |b| b.name.clone()),
        Some(Value::String(n)) => {
            check_name(&s.doc.assembly, n, base.map(|b| b.id)).map_err(|e| bad(cmd, e.to_string()))?;
            n.clone()
        }
        Some(_) => return Err(bad(cmd, "`name` must be text")),
    };
    // The solved values stay while their picks and type do; new picks or a new type start again
    // from where the parts are. A new solution keeps them: the state's signature (R132) tells the
    // solve they were solved for another compile (Mate to Flush, Opposed to Aligned), which then
    // re-seeds them.
    // A limited Mate's or Insert's offset is where its bounded value starts (Task 7): a new
    // offset starts it there again.
    let restart = matches!(kind, ConstraintKind::Mate | ConstraintKind::Insert)
        && (min.is_some() || max.is_some())
        && base.is_some_and(|b| b.offset != offset || b.exprs.offset != offset_expr);
    let (state, signatures) = match base {
        Some(b) if b.picks == picks && b.kind == kind && !restart => (b.state.clone(), b.signatures.clone()),
        _ => (Vec::new(), Vec::new()),
    };
    let c = Constraint {
        id: base.map_or(0, |b| b.id),
        name,
        kind,
        solution,
        picks,
        offset,
        limits: ConstraintLimits { min, max },
        exprs: ConstraintExprs { offset: offset_expr, min: min_expr, max: max_expr },
        suppressed,
        lock,
        axis,
        state,
        signatures,
    };
    c.validate().map_err(|e| bad(cmd, e.to_string()))?;
    Ok(c)
}

/// A Directed Angle's axis in A's frame (spec §3.5): `given` (world) when the command names one;
/// else the one `kept` (the same Directed picks) has; else frozen now as a × b of the picks'
/// directions (`looked`, or resolved here), or a direction square to a when they are parallel.
/// `None` when a pick does not resolve (the solve then reports the constraint Lost).
fn directed_axis(s: &Session, a: &GeoRef, b: &GeoRef, given: Option<Vec3>, looked: Option<(Prim, Prim)>, kept: Option<&Constraint>) -> Option<Vec3> {
    let into_a = |w: Vec3| {
        let m = if a.occurrence == 0 { solidcraft_doc::IDENTITY } else { occurrence_world(&s.doc, a.occurrence) };
        solidcraft_doc::mat_inverse(&m).and_then(|inv| solidcraft_doc::apply_vector(&inv, w).normalized())
    };
    if let Some(w) = given {
        return into_a(w);
    }
    if let Some(k) = kept.and_then(|c| c.axis) {
        return Some(k);
    }
    let (wa, wb) = match looked {
        Some(l) => l,
        None => {
            let st = s.model.state();
            {
                let picks = s.pick_cache.picks(&s.doc, &st);
                (picks.resolve_world(a).ok()?, picks.resolve_world(b).ok()?)
            }
        }
    };
    let (da, db) = (wa.direction()?, wb.direction()?);
    // Parallel (to a micro-radian: faces modelled parallel differ by rounding): any direction
    // square to a, the same one every time.
    let k = da.cross(db);
    into_a(if k.len() > PARALLEL_SIN { k } else { da.any_perp() })
}

/// Every occurrence's world placement now.
fn worlds(s: &Session) -> BTreeMap<u64, Mat> {
    s.doc.occurrences.iter().map(|o| (o.id, occurrence_world(&s.doc, o.id))).collect()
}

fn dof_json(sol: &joints::Solution) -> Value {
    Value::Object(sol.dof.iter().map(|(o, n)| (o.to_string(), json!(n))).collect())
}

/// A status's fields (`{status: "sick", gap_mm, gap_deg, with}`) merged into `row`.
fn put_status(row: &mut Value, status: Option<&ConstraintStatus>) {
    match serde_json::to_value(status.unwrap_or(&ConstraintStatus::Ok)) {
        Ok(Value::Object(m)) => {
            for (k, v) in m {
                row[k.as_str()] = v;
            }
        }
        _ => row["status"] = json!("ok"),
    }
}

/// The answer of create and edit.
fn answer(s: &Session, id: u64, sol: &joints::Solution, before: &BTreeMap<u64, Mat>) -> Value {
    let moved: Vec<u64> = worlds(s).into_iter().filter(|(o, m)| before.get(o) != Some(m)).map(|(o, _)| o).collect();
    let c = s.doc.assembly.constraints.iter().find(|c| c.id == id);
    let mut v = json!({"id": id, "name": c.map(|c| c.name.clone()), "dof": dof_json(sol), "moved": moved});
    put_status(&mut v, status_map(s, sol).get(&id));
    if let Some(w) = c.and_then(|c| clamped(s, c)) {
        v["warnings"] = json!([w]);
    }
    v
}

/// What a limited Mate between planes, Insert or Angle says when its offset, the start of its
/// bounded value (R153 for the Angle), is outside its limits: it starts at the nearer one (as
/// `joint.create` clamps a joint's values).
pub(crate) fn clamped(s: &Session, c: &Constraint) -> Option<String> {
    if c.suppressed || c.lock || c.limits.is_empty() {
        return None;
    }
    let st = s.model.state();
    let planes = || c.picks.iter().take(2).all(|g| matches!(s.pick_cache.picks(&s.doc, &st).resolve_world(g), Ok(Prim::Plane { .. })));
    if !(matches!(c.kind, ConstraintKind::Insert | ConstraintKind::Angle) || c.kind == ConstraintKind::Mate && planes()) {
        return None;
    }
    let (offset, (min, max)) = (offset_now(s, c), limits_now(s, c));
    let at = offset.clamp(min.unwrap_or(f64::NEG_INFINITY), max.unwrap_or(f64::INFINITY));
    let round = |v: f64| (v * 1e6).round() / 1e6;
    let side = |v: Option<f64>| v.map_or("none".to_string(), |v| round(v).to_string());
    let unit = if c.kind.is_angle() { "deg" } else { "mm" };
    (at != offset).then(|| {
        format!(
            "{} starts at {} {unit}, inside its limits [{}, {}], not at its offset {} {unit}: the part moved there",
            c.name,
            round(at),
            side(min),
            side(max),
            round(offset)
        )
    })
}

/// A limited Angle's offset is a start, as a Mate's (R153): one placement solve of the design
/// with the constraint `id` unlimited at its offset clamped into the limits, so the part turns
/// there; the bounded solve that follows keeps it (any value inside meets the limits).
pub(crate) fn start_angle(s: &mut Session, id: u64) {
    let Some(c) = s.doc.assembly.constraints.iter().find(|c| c.id == id).cloned() else { return };
    if c.kind != ConstraintKind::Angle || c.suppressed || c.limits.is_empty() {
        return;
    }
    let (offset, (min, max)) = (offset_now(s, &c), limits_now(s, &c));
    let at = offset.clamp(min.unwrap_or(f64::NEG_INFINITY), max.unwrap_or(f64::INFINITY));
    if let Some(x) = s.doc_mut().assembly.constraints.iter_mut().find(|x| x.id == id) {
        x.offset = at;
        x.exprs.offset = None;
        x.limits = ConstraintLimits::default();
        x.exprs.min = None;
        x.exprs.max = None;
    }
    super::joints::place(s);
    // The record as made, its pose kept.
    if let Some(x) = s.doc_mut().assembly.constraints.iter_mut().find(|x| x.id == id) {
        *x = c;
    }
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "constraint.create";
    if s.doc.assembly.constraints.len() >= MAX_CONSTRAINTS {
        return Err(bad(cmd, format!("an assembly holds at most {MAX_CONSTRAINTS} constraints")));
    }
    let mut c = build(s, p, None, cmd)?;
    if !c.suppressed
        && let Some(why) = over_constrains(s, &c, None)
    {
        return Err(bad(cmd, why));
    }
    let before = worlds(s);
    let doc = s.doc_mut();
    c.id = doc.allocate_id()?;
    let id = c.id;
    doc.assembly.constraints.push(c);
    start_angle(s, id);
    let sol = super::joints::resolve(s);
    Ok(answer(s, id, &sol, &before))
}

/// A constraint by id or name.
fn constraint_id(s: &Session, v: &Value, cmd: &str) -> Result<u64> {
    let key = match v {
        Value::String(k) => k.clone(),
        Value::Number(n) => n.to_string(),
        _ => return Err(bad(cmd, "`constraint` must be an id or a name")),
    };
    s.doc.assembly.constraint(&key).map(|c| c.id).ok_or_else(|| bad(cmd, format!("no constraint `{key}`")))
}

/// `constraint`: one id or name, or a list of them (each once).
fn constraint_ids(s: &Session, p: &Value, cmd: &str) -> Result<Vec<u64>> {
    let mut ids = match p.get("constraint") {
        Some(Value::Array(list)) if list.len() <= MAX_CONSTRAINTS => list.iter().map(|v| constraint_id(s, v, cmd)).collect::<Result<Vec<_>>>()?,
        Some(v @ (Value::String(_) | Value::Number(_))) => vec![constraint_id(s, v, cmd)?],
        _ => return Err(bad(cmd, "`constraint` must be an id, a name or a list of them")),
    };
    if ids.is_empty() {
        return Err(bad(cmd, format!("nothing to {}: `constraint` lists no constraints", cmd.trim_start_matches("constraint."))));
    }
    let mut seen = std::collections::BTreeSet::new();
    ids.retain(|id| seen.insert(*id));
    Ok(ids)
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "constraint.edit";
    let id = match p.get("constraint") {
        Some(v) => constraint_id(s, v, cmd)?,
        None => return Err(bad(cmd, "`constraint` must be an id or a name")),
    };
    let base = s.doc.assembly.constraints.iter().find(|c| c.id == id).cloned().ok_or_else(|| bad(cmd, "no such constraint"))?;
    let c = build(s, p, Some(&base), cmd)?;
    let before = worlds(s);
    if c == base {
        // Nothing changes: no undo step, and the answer from a solve that changes nothing.
        let sol = super::joints::solution(s, &joints::Held::new(), Analysis::DegreesOfFreedom);
        return Ok(answer(s, id, &sol, &before));
    }
    if !c.suppressed
        && let Some(why) = over_constrains(s, &c, Some(id))
    {
        return Err(bad(cmd, why));
    }
    // R153: an edit that changes a limited Angle's offset starts it there.
    let started = c.offset != base.offset || c.exprs.offset != base.exprs.offset;
    if let Some(slot) = s.doc_mut().assembly.constraints.iter_mut().find(|x| x.id == id) {
        *slot = c;
    }
    if started {
        start_angle(s, id);
    }
    let sol = super::joints::resolve(s);
    Ok(answer(s, id, &sol, &before))
}

fn suppress(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "constraint.suppress";
    let ids = constraint_ids(s, p, cmd)?;
    let to = flag(p, "suppressed", cmd)?;
    let sol = if s.doc.assembly.constraints.iter().any(|c| ids.contains(&c.id) && to != Some(c.suppressed)) {
        let mut next = s.doc.assembly.constraints.clone();
        for c in next.iter_mut().filter(|c| ids.contains(&c.id)) {
            c.suppressed = to.unwrap_or(!c.suppressed);
        }
        // Unsuppressing into a conflict is refused (owner Q16), as creating one is.
        let woken: Vec<&Constraint> =
            next.iter().filter(|c| !c.suppressed && s.doc.assembly.constraints.iter().any(|b| b.id == c.id && b.suppressed)).collect();
        if let Some(first) = woken.first()
            && let Some(why) = over_constrains_all(s, next.clone(), first)
        {
            return Err(bad(cmd, why));
        }
        s.doc_mut().assembly.constraints = next;
        super::joints::resolve(s)
    } else {
        super::joints::solution(s, &joints::Held::new(), Analysis::Placement)
    };
    let map = status_map(s, &sol);
    let rows: Vec<Value> = ids
        .iter()
        .filter_map(|id| s.doc.assembly.constraints.iter().find(|c| c.id == *id))
        .map(|c| {
            let mut v = json!({"name": c.name, "suppressed": c.suppressed});
            put_status(&mut v, map.get(&c.id));
            v
        })
        .collect();
    Ok(json!({"constraints": rows}))
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "constraint.delete";
    let ids = constraint_ids(s, p, cmd)?;
    s.doc_mut().assembly.constraints.retain(|c| !ids.contains(&c.id));
    super::joints::resolve(s);
    Ok(json!({"deleted": ids.len()}))
}

/// Most frames one drive makes (spec §5).
const MAX_FRAMES: usize = 10_000;

/// A whole number `key` in 1..=`max` (`default` when absent).
fn count(p: &Value, key: &str, max: usize, default: usize, cmd: &str) -> Result<usize> {
    match p.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_f64()
            .filter(|x| x.fract() == 0.0 && (1.0..=max as f64).contains(x))
            .map(|x| x as usize)
            .ok_or_else(|| bad(cmd, format!("`{key}` must be a whole number from 1 to {max}"))),
    }
}

/// A number or expression `key` in the constraint's unit (`None` when absent; null refused).
fn drive_amount(s: &Session, p: &Value, key: &str, angle: bool, cmd: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::Null) => Err(bad(cmd, format!("`{key}` must be a number or an expression"))),
        Some(v) => amount(s, v, angle, key, cmd).map(|(x, _)| Some(x)),
    }
}

/// The values one drive steps through, from its parameters (spec §5): one pass from start to
/// end by `increment` (the last step short when it doesn't divide) or in `steps`, back again
/// for start_end_start, `repeat` times.
fn drive_values(s: &Session, c: &Constraint, p: &Value, cmd: &str) -> Result<Vec<f64>> {
    let angle = c.kind.is_angle();
    let start = drive_amount(s, p, "start", angle, cmd)?.unwrap_or_else(|| offset_now(s, c));
    let end = drive_amount(s, p, "end", angle, cmd)?.unwrap_or(start + 10.0);
    let increment = drive_amount(s, p, "increment", angle, cmd)?;
    if increment.is_some() && p.get("steps").is_some() {
        return Err(bad(cmd, "give `increment` or `steps`, not both"));
    }
    let steps = count(p, "steps", MAX_FRAMES - 1, 0, cmd)?;
    let repeat = count(p, "repeat", 1000, 1, cmd)?;
    let back = match p.get("cycle") {
        None => false,
        Some(Value::String(c)) if c == "start_end" => false,
        Some(Value::String(c)) if c == "start_end_start" => true,
        Some(_) => return Err(bad(cmd, "`cycle` must be start_end or start_end_start")),
    };
    let too_many = || bad(cmd, "at most 10,000 frames");
    let span = end - start;
    let mut pass: Vec<f64> = if steps > 0 {
        (0..=steps).map(|i| start + span * i as f64 / steps as f64).collect()
    } else {
        let inc = increment.unwrap_or(1.0);
        if inc <= 0.0 {
            return Err(bad(cmd, "`increment` must be above 0"));
        }
        let n = span.abs() / inc;
        if !(n < MAX_FRAMES as f64) {
            return Err(too_many());
        }
        // Whole steps (a rounding hair short of one counts), then the end when they miss it.
        let whole = (n * (1.0 + 1e-12)).floor() as usize;
        let mut v: Vec<f64> = (0..=whole).map(|i| start + span.signum() * inc * i as f64).collect();
        if v.last().is_some_and(|l| (l - end).abs() > 1e-9 * end.abs().max(1.0)) {
            v.push(end);
        }
        v
    };
    if back {
        let rev: Vec<f64> = pass.iter().rev().skip(1).copied().collect();
        pass.extend(rev);
    }
    if pass.len().saturating_mul(repeat) > MAX_FRAMES {
        return Err(too_many());
    }
    Ok(pass.iter().copied().cycle().take(pass.len() * repeat).collect())
}

/// Drive a constraint's offset through values (spec §5, §6.6): each frame sets it on the
/// session's design and places the assembly (no DOF count); the frames say where the moved
/// occurrences go. It stops past a limit (a last frame at the bound), where a loop the drive
/// opens no longer closes, and, with `stop_on_contact`, at the last value free of contact
/// (bisected to 1e-4 of the unit). The design, its revision and the constraint report are
/// restored before it answers (owner Q17): no undo step.
fn drive(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "constraint.drive";
    let id = match p.get("constraint") {
        Some(v) => constraint_id(s, v, cmd)?,
        None => return Err(bad(cmd, "`constraint` must be an id or a name")),
    };
    let c = s.doc.assembly.constraints.iter().find(|c| c.id == id).cloned().ok_or_else(|| bad(cmd, "no such constraint"))?;
    if c.suppressed {
        return Err(bad(cmd, format!("{} is suppressed: unsuppress it to drive it", c.name)));
    }
    if let Some((_, why)) = compile_with(&s.pick_cache.picks(&s.doc, &s.model.state()), &s.doc.assembly).lost.into_iter().find(|(x, _)| *x == id) {
        return Err(bad(cmd, format!("{} is lost ({why}): edit it to drive it", c.name)));
    }
    let values = drive_values(s, &c, p, cmd)?;
    let contact = flag(p, "stop_on_contact", cmd)?.unwrap_or(false);
    let (min, max) = limits_now(s, &c);
    let saved = (s.doc.clone(), s.revision, s.constraint_report.clone(), s.dof_stale);
    let before = worlds(s);
    // What is already open or touching at the start does not stop the drive.
    let open_at_start: BTreeSet<u64> = super::joints::solution(s, &joints::Held::new(), Analysis::Placement).conflicts.iter().map(|c| c.0).collect();
    // The contact sets' bodies tessellated once for the whole drive, and what already passes
    // through each other there, which does not stop it (R158).
    let check = if contact { super::motion::ContactCheck::new(s) } else { super::motion::ContactCheck::off() };
    // The design `from` (the last placed frame's) with the offset at `v`, placed. The driven
    // constraint goes without its limits for the sweep (R154), so every frame compiles to the
    // exact value (a Mate's Planar, an Angle's target); the stop above uses the limits it had.
    let set = |s: &mut Session, from: &Arc<Document>, v: f64| {
        s.doc = from.clone();
        if let Some(x) = s.doc_mut().assembly.constraints.iter_mut().find(|x| x.id == id) {
            x.offset = v;
            x.exprs.offset = None;
            x.limits = ConstraintLimits::default();
            x.exprs.min = None;
            x.exprs.max = None;
        }
        super::joints::place(s)
    };
    let frame = |s: &Session, v: f64| {
        let world: serde_json::Map<String, Value> =
            worlds(s).into_iter().filter(|(o, m)| before.get(o) != Some(m)).map(|(o, m)| (o.to_string(), json!(m))).collect();
        json!({"value": v, "world": world})
    };
    let hit = |s: &Session| check.hit(s);
    let mut frames: Vec<Value> = Vec::new();
    // The last frame's value and design.
    let mut last: Option<(f64, Arc<Document>)> = None;
    let mut out = json!({});
    for v in values {
        let from = last.as_ref().map_or_else(|| saved.0.clone(), |(_, d)| d.clone());
        let past = match (min, max) {
            (Some(lo), _) if v < lo => Some((lo, "min")),
            (_, Some(hi)) if v > hi => Some((hi, "max")),
            _ => None,
        };
        if let Some((bound, which)) = past {
            if last.as_ref().is_none_or(|(l, _)| *l != bound) {
                set(s, &from, bound);
                frames.push(frame(s, bound));
            }
            out["stopped_by_limit"] = json!({"value": bound, "limit": which});
            break;
        }
        let sol = set(s, &from, v);
        if let Some(opened) = sol.conflicts.iter().find(|x| !open_at_start.contains(&x.0) && owner_of(x.0) != Some(id)) {
            out["stopped_by_loop"] = json!({"value": last.as_ref().map(|(l, _)| *l), "constraint": opened.1});
            break;
        }
        if contact && let Some(mut pair) = hit(s) {
            // Bisect between the last free value and this one, to 1e-4 of the unit.
            let mut lo = last.as_ref().map(|(l, _)| *l);
            if let Some((mut at, mut at_doc)) = last.clone() {
                let mut hi = v;
                while (hi - at).abs() > 1e-4 {
                    let mid = (at + hi) / 2.0;
                    set(s, &at_doc, mid);
                    match hit(s) {
                        Some(p) => (hi, pair) = (mid, p),
                        None => (at, at_doc) = (mid, s.doc.clone()),
                    }
                }
                s.doc = at_doc;
                if last.as_ref().is_some_and(|(l, _)| *l != at) {
                    frames.push(frame(s, at));
                }
                lo = Some(at);
            }
            out["stopped_by_contact"] = json!({"value": lo, "pair": [pair.0, pair.1]});
            break;
        }
        frames.push(frame(s, v));
        last = Some((v, s.doc.clone()));
    }
    (s.doc, s.revision, s.constraint_report, s.dof_stale) = saved;
    out["frames"] = json!(frames);
    Ok(out)
}

/// A constraint as `constraint.list` gives it, with its status after `sol`'s solve.
fn row(s: &Session, c: &Constraint, status: Option<&ConstraintStatus>) -> Value {
    let mut limits = serde_json::Map::new();
    for (k, v) in [("min", c.limits.min), ("max", c.limits.max)] {
        if let Some(v) = v {
            limits.insert(k.into(), json!(v));
        }
    }
    let mut v = json!({"id": c.id, "name": c.name, "type": c.kind, "solution": c.solution,
        "occurrences": c.picks.iter().map(|g| g.occurrence).collect::<Vec<_>>(), "offset": offset_now(s, c),
        "unit": if c.kind.is_angle() { "deg" } else { "mm" }, "limits": limits, "suppressed": c.suppressed});
    put_status(&mut v, status);
    if !c.exprs.is_empty() {
        v["expressions"] = json!(c.exprs);
    }
    v
}

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "constraint.list";
    has_components(s).map_err(|why| bad(cmd, why))?;
    let only = match p.get("occurrence") {
        None | Some(Value::Null) => None,
        v => Some(occurrence_id(s, v, cmd)?),
    };
    let sol = super::joints::solution(s, &joints::Held::new(), Analysis::DegreesOfFreedom);
    let map = status_map(s, &sol);
    let rows: Vec<Value> = s
        .doc
        .assembly
        .constraints
        .iter()
        .filter(|c| only.is_none_or(|o| c.picks.iter().any(|g| g.occurrence == o)))
        .map(|c| row(s, c, map.get(&c.id)))
        .collect();
    Ok(json!({"constraints": rows, "dof": dof_json(&sol)}))
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "constraint.inspect";
    has_components(s).map_err(|why| bad(cmd, why))?;
    let ids: Vec<u64> = match p.get("constraint") {
        None | Some(Value::Null) => s.doc.assembly.constraints.iter().map(|c| c.id).collect(),
        Some(v) => vec![constraint_id(s, v, cmd)?],
    };
    let st = s.model.state();
    let picks = s.pick_cache.picks(&s.doc, &st);
    let compiled = compile_with(&picks, &s.doc.assembly);
    let sol = super::joints::solution(s, &joints::Held::new(), Analysis::DegreesOfFreedom);
    let map = status_map(s, &sol);
    let total: usize = sol.dof.values().sum();
    let out: Vec<Value> = ids
        .iter()
        .filter_map(|id| s.doc.assembly.constraints.iter().find(|c| c.id == *id))
        .map(|c| {
            // Redundant: met, and the design without it keeps the same freedom (spec §3.7).
            let status = match map.get(&c.id) {
                Some(ConstraintStatus::Ok) if redundant(s, &st, c.id, total) => Some(ConstraintStatus::Redundant),
                other => other.cloned(),
            };
            let mut v = row(s, c, status.as_ref());
            let world: Vec<std::result::Result<Prim, String>> = c.picks.iter().map(|g| picks.resolve_world(g).map_err(|e| e.to_string())).collect();
            v["picks"] = c
                .picks
                .iter()
                .zip(&world)
                .map(|(g, w)| {
                    let mut j = pick_json(g);
                    match w {
                        Ok(prim) => j["resolved"] = prim_json(prim),
                        Err(e) => j["error"] = json!(e),
                    }
                    j
                })
                .collect();
            let equations = compiled.equations.iter().filter(|e| owner_of(e.id) == Some(c.id)).map(|e| match &e.eq {
                joints::Equation::Angle { axis, target, bounds, .. } => {
                    json!({"id": e.id, "kind": if axis.is_some() { "signed_angle" } else { "angle" },
                    "target": target.to_degrees(), "bounds": bounds.map(|(lo, hi)| [lo.to_degrees(), hi.to_degrees()])})
                }
                joints::Equation::AxisDistance { target, bounds, .. } => {
                    json!({"id": e.id, "kind": "axis_distance", "target": target, "bounds": bounds.map(|(lo, hi)| [lo, hi])})
                }
            });
            v["compiled"] = compiled
                .joints
                .iter()
                .filter(|j| owner_of(j.id) == Some(c.id))
                .map(|j| json!({"id": j.id, "kind": j.kind, "flip": j.flip, "offset": j.offset}))
                .chain(equations)
                .collect();
            if let Some((_, why)) = compiled.lost.iter().find(|(id, _)| *id == c.id) {
                v["lost"] = json!(why);
            }
            v["gaps"] = match (world.first(), world.get(1)) {
                (Some(Ok(a)), Some(Ok(b))) => {
                    let about = angle_reference(&s.doc, &st, c.solution, &c.picks, c.axis).ok().flatten();
                    gaps(c.kind, c.solution, offset_now(s, c), a, b, about).map_or(Value::Null, |(mm, deg)| json!({"mm": mm, "deg": deg}))
                }
                _ => Value::Null,
            };
            v
        })
        .collect();
    Ok(json!({"constraints": out, "dof": dof_json(&sol)}))
}

/// Is constraint `id` redundant: does the design without it keep `total` degrees of freedom (the
/// sum over the occurrences, with it)?
fn redundant(s: &Session, st: &solidcraft_doc::ModelState, id: u64, total: usize) -> bool {
    let mut doc = (*s.doc).clone();
    doc.assembly.constraints.retain(|c| c.id != id);
    let without = joints::solve_cached(&doc, st, &joints::Held::new(), Analysis::DegreesOfFreedom, None, &s.pick_cache);
    without.dof.values().sum::<usize>() == total
}

/// The constraints whose glyphs are shown, by name, in the design's order.
fn shown(s: &Session) -> Value {
    let names: Vec<&str> = s.doc.assembly.constraints.iter().filter(|c| s.shown_constraints.contains(&c.id)).map(|c| c.name.as_str()).collect();
    json!({"shown": names})
}

/// Most occurrences `constraint.show` takes in one list.
const MAX_SHOW: usize = 100_000;

fn show(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "constraint.show";
    let list = match p.get("occurrences") {
        Some(Value::Array(list)) if list.len() <= MAX_SHOW => list,
        Some(Value::Array(_)) => return Err(bad(cmd, format!("at most {MAX_SHOW} occurrences"))),
        _ => return Err(bad(cmd, "`occurrences` must be a list of occurrence ids or names")),
    };
    let occurrences = list.iter().map(|v| occurrence_id(s, Some(v), cmd)).collect::<Result<std::collections::BTreeSet<u64>>>()?;
    if occurrences.is_empty() {
        s.shown_constraints.clear();
    } else {
        let add: Vec<u64> =
            s.doc.assembly.constraints.iter().filter(|c| c.picks.iter().any(|g| occurrences.contains(&g.occurrence))).map(|c| c.id).collect();
        s.shown_constraints.extend(add);
    }
    Ok(shown(s))
}

fn show_sick(s: &mut Session, _p: &Value) -> Result<Value> {
    let sick = s
        .constraint_report
        .rows
        .iter()
        .filter(|r| matches!(r.status, ConstraintStatus::Sick { .. } | ConstraintStatus::Lost { .. }))
        .map(|r| r.id)
        .collect();
    s.shown_constraints = sick;
    Ok(shown(s))
}

fn hide_all(s: &mut Session, _p: &Value) -> Result<Value> {
    s.shown_constraints.clear();
    Ok(shown(s))
}

#[cfg(test)]
#[path = "constraints_geo_tests.rs"]
mod constraints_geo_tests;

#[cfg(test)]
#[path = "constraints_compile_tests.rs"]
mod constraints_compile_tests;

#[cfg(test)]
#[path = "constraints_create_tests.rs"]
mod constraints_create_tests;

#[cfg(test)]
#[path = "constraints_copy_tests.rs"]
mod constraints_copy_tests;

#[cfg(test)]
#[path = "constraints_hostile_tests.rs"]
mod constraints_hostile_tests;

#[cfg(test)]
#[path = "constraints_status_tests.rs"]
mod constraints_status_tests;

#[cfg(test)]
#[path = "constraints_angle_tests.rs"]
mod constraints_angle_tests;

#[cfg(test)]
#[path = "constraints_limits_tests.rs"]
mod constraints_limits_tests;

#[cfg(test)]
#[path = "constraints_drive_tests.rs"]
mod constraints_drive_tests;

#[cfg(test)]
#[path = "constraints_script_tests.rs"]
mod constraints_script_tests;

#[cfg(test)]
#[path = "constraints_cache_tests.rs"]
mod constraints_cache_tests;
