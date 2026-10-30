//! Assembly constraint commands (`constraint.*`, Plan E-1, spec §5): Constrain (Mate, Angle,
//! Tangent and Insert between picks on two occurrences), edit, suppress, delete, list and
//! inspect. Every change re-solves the assembly (the constraints compile to joints in every
//! solve) through `cmd::joints::resolve`, which also refreshes the session's DOF memo with that
//! solve; each command is one undo step, and an edit that changes nothing is none.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcraft_doc::constraints::{
    Constraint, ConstraintExprs, ConstraintKind, ConstraintLimits, ConstraintSolution, ConstraintStatus, EdgeEnd, Geo, GeoRef, MAX_CONSTRAINTS,
    OriginItem, PickWhat, Prim, check_name, compile, geo_at, next_name, owner_of, resolve, resolve_world,
};
use solidcraft_doc::joints::{self, Analysis, occurrence_world};
use solidcraft_doc::{DocumentKind, Mat};
use solidcraft_geom::Vec3;

use super::CommandSpec;
use super::joints::{occurrence_id, typed, value};
use crate::constraints::{
    direction, gaps, is_axis, offset_now, over_constrains, over_constrains_all, pick_json, predict_prims, prim_json, status_map, word,
};
use crate::params::{bad, vec3};
use crate::{Result, Session};

/// A pick, as every command that takes one reads it.
const PICK: &str = "a pick is {occurrence: id|name, face|edge: \"<name>\" | [x,y,z]} (a world point picks the nearest face/edge on that occurrence; a name may take near?: [x,y,z] in the component's coordinates), {occurrence, vertex: [x,y,z]} (or vertex: \"<edge name>\", end?: start|end, near?), or {occurrence?: id|name (default: the assembly), origin: \"xy\"|\"xz\"|\"yz\"|\"x\"|\"y\"|\"z\"|\"center\"}";

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("constraint.create", "Constrain", create).icon("constrain").enabled(has_components).params(
        "type: mate|angle|tangent|insert (symmetry: second wave, refused with that reason in v1); a, b: picks; ref?: a third pick (Angle explicit vector); solution? (default per §1.2); axis?: [x,y,z] (Directed Angle only: the world direction it turns about, kept in A's frame; default a × b of the picks as made); offset?: number or expression (default 0; predict: true measures it instead); predict?: bool (default false on the command line, true from the dialog); min?, max?: number or expression; lock?: bool (Insert only); suppressed?: bool (default false); name?. A pick is {occurrence: id|name, face|edge: \"<name>\" | [x,y,z]} (a world point picks the nearest face/edge on that occurrence, as joint.create does; a name may take near?: [x,y,z] in the component's coordinates), {occurrence, vertex: [x,y,z]} (or vertex: \"<edge name>\", end?: start|end, near?), or {occurrence?: id|name (default: the assembly), origin: \"xy\"|\"xz\"|\"yz\"|\"x\"|\"y\"|\"z\"|\"center\"} → {id, name, status, dof: {<occurrence>: n}, moved: [occurrences]}",
    ),
    CommandSpec::new("constraint.edit", "Edit Constraint", edit).icon("edit").enabled(has_components).params(
        "constraint: id|name; any of type, a, b, ref, solution, axis, offset, min, max (null clears), lock, name → as create; an edit that changes nothing adds no undo step",
    ),
    CommandSpec::new("constraint.suppress", "Suppress Constraint", suppress)
        .icon("hide")
        .enabled(has_constraints)
        .params("constraint: id|name or a list; suppressed?: bool (default toggles) → {constraints: [{name, suppressed, status}]}"),
    CommandSpec::new("constraint.delete", "Delete Constraint", delete).icon("delete").enabled(has_constraints).params("constraint: id|name or a list → {deleted: n}"),
    CommandSpec::new("constraint.list", "List Constraints", list).icon("list").noundo().params(
        "occurrence? (default all) → {constraints: [{id, name, type, solution, occurrences: [a, b, ref?], offset, unit, limits, expressions?, suppressed, status}], dof: {<occurrence>: n}}",
    ),
    CommandSpec::new("constraint.inspect", "Inspect Constraint", inspect).icon("inspect").noundo().params(
        "constraint? → as list (status redundant when it is met and takes no freedom away) + per pick the resolved primitive in world (picks: [{…, resolved: {kind, p|c, n|d|axis, r?}}]), the compiled joints/equations (compiled: [{id, kind, flip, offset}]), gaps: {mm, deg} (how far the picks are from meeting it)",
    ),
    CommandSpec::new("constraint.show", "Show Constraints", show)
        .icon("show_relationships")
        .noundo()
        .enabled(has_constraints)
        .params("occurrences: [ids|names] (the constraints picking any of them are added to the shown glyphs; an empty list clears) → {shown: [names]}"),
    CommandSpec::new("constraint.show_sick", "Show Problems", show_sick)
        .icon("show_sick")
        .noundo()
        .enabled(any_sick)
        .params("→ {shown: [names]}: every sick or lost constraint, in place of the shown glyphs"),
    CommandSpec::new("constraint.hide_all", "Hide All Constraints", hide_all)
        .icon("hide_relationships")
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
            (Prim::Cylinder { d, .. }, Prim::Cylinder { d: e, .. }) if d.cross(*e).len() > 1e-9f64.sin() => {
                Err(bad(cmd, "skew cylinders come in a later version: pick cylinders whose axes are parallel"))
            }
            _ => predict_prims(kind, a, b).map(|_| ()).ok_or_else(|| bad(cmd, format!("Tangent can't join a {} and a {}", word(a), word(b)))),
        },
        _ => predict_prims(kind, a, b).map(|_| ()).ok_or_else(|| bad(cmd, format!("{} can't join a {} and a {}", kind.word(), word(a), word(b)))),
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
            let world = |g: &GeoRef, what: &str| resolve_world(&s.doc, &st, g).map_err(|e| bad(cmd, format!("`{what}`: {e}")));
            let (wa, wb) = (world(ga, "a")?, world(gb, "b")?);
            looked = Some((wa, wb));
            let prediction = if predict { predict_prims(kind, &wa, &wb) } else { None };
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
        let w = resolve_world(&s.doc, &s.model.state(), r).map_err(|e| bad(cmd, format!("`ref`: {e}")))?;
        if direction(&w).is_none() {
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
    let (state, signatures) = match base {
        Some(b) if b.picks == picks && b.kind == kind => (b.state.clone(), b.signatures.clone()),
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

/// Directions closer to parallel than this (the sine of the angle between them) have no a × b
/// to freeze.
const PARALLEL_SIN: f64 = 1e-6;

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
            (resolve_world(&s.doc, &st, a).ok()?, resolve_world(&s.doc, &st, b).ok()?)
        }
    };
    let (da, db) = (direction(&wa)?, direction(&wb)?);
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
    v
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
    if let Some(slot) = s.doc_mut().assembly.constraints.iter_mut().find(|x| x.id == id) {
        *slot = c;
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
    let compiled = compile(&s.doc, &st, &s.doc.assembly);
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
            let world: Vec<std::result::Result<Prim, String>> =
                c.picks.iter().map(|g| resolve_world(&s.doc, &st, g).map_err(|e| e.to_string())).collect();
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
                    gaps(c.kind, c.solution, offset_now(s, c), a, b).map_or(Value::Null, |(mm, deg)| json!({"mm": mm, "deg": deg}))
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
    let without = joints::solve_holding_with_analysis(&doc, st, &joints::Held::new(), Analysis::DegreesOfFreedom);
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
