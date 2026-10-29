//! Assembly constraint commands (`constraint.*`, Plan E-1, spec §5): Constrain (Mate, Angle,
//! Tangent and Insert between picks on two occurrences), edit, suppress, delete, list and
//! inspect. Every change re-solves the assembly (the constraints compile to joints in every
//! solve) through `cmd::joints::resolve`, which also refreshes the session's DOF memo with that
//! solve; each command is one undo step, and an edit that changes nothing is none.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcraft_doc::constraints::{
    Constraint, ConstraintExprs, ConstraintKind, ConstraintLimits, ConstraintSolution, EdgeEnd, Geo, GeoRef, MAX_CONSTRAINTS, OriginItem, PickWhat,
    Prim, check_name, compile, geo_at, next_name, owner_of, resolve, resolve_world,
};
use solidcraft_doc::joints::{self, Analysis, occurrence_world};
use solidcraft_doc::{DocumentKind, Mat};
use solidcraft_geom::Vec3;

use super::CommandSpec;
use super::joints::{occurrence_id, typed, value};
use crate::constraints::{gaps, is_axis, pick_json, predict_prims, prim_json, word};
use crate::params::{bad, bool_, str_, vec3};
use crate::{Result, Session};

/// A pick, as every command that takes one reads it.
const PICK: &str = "a pick is {occurrence: id|name, face|edge: \"<name>\" | [x,y,z]} (a world point picks the nearest face/edge on that occurrence; a name may take near?: [x,y,z] in the component's coordinates), {occurrence, vertex: [x,y,z]} (or vertex: \"<edge name>\", end?: start|end, near?), or {occurrence?: id|name (default: the assembly), origin: \"xy\"|\"xz\"|\"yz\"|\"x\"|\"y\"|\"z\"|\"center\"}";

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("constraint.create", "Constrain", create).icon("constrain").enabled(has_components).params(
        "type: mate|angle|tangent|insert (symmetry: second wave, refused with that reason in v1); a, b: picks; ref?: a third pick (Angle explicit vector); solution? (default per §1.2); offset?: number or expression (default 0; predict: true measures it instead); predict?: bool (default false on the command line, true from the dialog); min?, max?: number or expression; lock?: bool (Insert only); suppressed?: bool (default false); name?. A pick is {occurrence: id|name, face|edge: \"<name>\" | [x,y,z]} (a world point picks the nearest face/edge on that occurrence, as joint.create does; a name may take near?: [x,y,z] in the component's coordinates), {occurrence, vertex: [x,y,z]} (or vertex: \"<edge name>\", end?: start|end, near?), or {occurrence?: id|name (default: the assembly), origin: \"xy\"|\"xz\"|\"yz\"|\"x\"|\"y\"|\"z\"|\"center\"} → {id, name, status, dof: {<occurrence>: n}, moved: [occurrences]}",
    ),
    CommandSpec::new("constraint.edit", "Edit Constraint", edit).icon("edit").enabled(has_components).params(
        "constraint: id|name; any of type, a, b, ref, solution, offset, min, max (null clears), lock, name → as create; an edit that changes nothing adds no undo step",
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
        "constraint? → as list + per pick the resolved primitive in world (picks: [{…, resolved: {kind, p|c, n|d|axis, r?}}]), the compiled joints/equations (compiled: [{id, kind, flip, offset}]), gaps: {mm, deg}",
    ),
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

fn occurrence_name(s: &Session, id: u64) -> String {
    if id == 0 {
        return "the assembly".into();
    }
    s.doc.occurrences.iter().find(|o| o.id == id).map(|o| o.name.clone()).unwrap_or_else(|| id.to_string())
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
                    Some(n) => vec3(n).ok_or_else(|| bad(cmd, format!("`{what}`: near must be [x,y,z]")))?,
                };
                let name = name.clone();
                let geo = match kind {
                    PickWhat::Face => Geo::Face { name, pick },
                    PickWhat::Edge => Geo::Edge { name, pick },
                    PickWhat::Vertex => {
                        let end = match str_(v, "end") {
                            None | Some("start") => EdgeEnd::Start,
                            Some("end") => EdgeEnd::End,
                            Some(_) => return Err(bad(cmd, format!("`{what}`: end must be start or end"))),
                        };
                        Geo::Vertex { edge: name, end, pick }
                    }
                };
                GeoRef { occurrence, geo }
            }
            Some(x) => {
                let p = vec3(x).ok_or_else(|| bad(cmd, format!("`{what}`: {key} must be a name or a point [x,y,z]")))?;
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
fn amount(s: &Session, v: &Value, angle: bool, cmd: &str) -> Result<(f64, Option<String>)> {
    let x = value(s, v, angle, cmd)?;
    Ok((if angle { x.to_degrees() } else { x }, typed(Some(v))))
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
        _ => predict_prims(kind, a, b).map(|_| ()).ok_or_else(|| bad(cmd, format!("{} can't join a {} and a {}", kind.word(), word(a), word(b)))),
    }
}

/// What `p` sets on a value that may have been typed as an expression (`base` when not given;
/// null clears it).
type Typed = (Option<f64>, Option<String>);

/// The constraint `create` (no `base`) or `edit` (`base`) makes from `p`: every check but the
/// over-constraint one (Task 5) is here, so a refusal changes nothing.
fn build(s: &Session, p: &Value, base: Option<&Constraint>, cmd: &str) -> Result<Constraint> {
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
    let picks: Vec<GeoRef> = slots.iter().map_while(Clone::clone).collect();
    if present != want || picks.len() != present {
        return Err(bad(cmd, format!("{} takes {want} picks, not {present}", kind.word())));
    }
    let (Some(ga), Some(gb)) = (picks.first(), picks.get(1)) else { return Err(bad(cmd, format!("{} takes {want} picks", kind.word()))) };
    if ga.occurrence == gb.occurrence {
        return Err(bad(cmd, format!("both picks are on {}: pick another component", occurrence_name(s, ga.occurrence))));
    }
    let st = s.model.state();
    let world = |g: &GeoRef, what: &str| resolve_world(&s.doc, &st, g).map_err(|e| bad(cmd, format!("`{what}`: {e}")));
    let (wa, wb) = (world(ga, "a")?, world(gb, "b")?);
    let prediction = if bool_(p, "predict").unwrap_or(false) { predict_prims(kind, &wa, &wb) } else { None };
    let solution = match (given, prediction, base) {
        (Some(g), ..) => g,
        (None, Some(pr), _) => pr.solution,
        (None, None, Some(b)) if b.kind == kind && fits_picks(kind, b.solution, &wa, &wb, cmd).is_ok() => b.solution,
        _ => kind.default_solution(is_axis(&wa) && is_axis(&wb)),
    };
    fits_picks(kind, solution, &wa, &wb, cmd)?;
    let angle = kind.is_angle();
    let (offset, offset_expr) = match p.get("offset") {
        Some(Value::Null) => return Err(bad(cmd, "`offset` must be a number or an expression")),
        Some(v) => amount(s, v, angle, cmd)?,
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
            Some(v) => amount(s, v, angle, cmd).map(|(x, e)| (Some(x), e)),
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
    let suppressed = match p.get("suppressed") {
        None | Some(Value::Null) => base.is_some_and(|b| b.suppressed),
        Some(Value::Bool(x)) => *x,
        Some(_) => return Err(bad(cmd, "`suppressed` must be true or false")),
    };
    let name = match p.get("name") {
        None | Some(Value::Null) => base.map_or_else(|| next_name(&s.doc.assembly, kind), |b| b.name.clone()),
        Some(Value::String(n)) => {
            check_name(&s.doc.assembly, n, base.map(|b| b.id)).map_err(|e| bad(cmd, e.to_string()))?;
            n.clone()
        }
        Some(_) => return Err(bad(cmd, "`name` must be text")),
    };
    // The solved values stay while what they belong to does; new picks, a new type or a new
    // solution start again from where the parts are (R132 also re-seeds a changed compile).
    let state = match base {
        Some(b) if b.picks == picks && b.kind == kind && b.solution == solution => b.state.clone(),
        _ => Vec::new(),
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
        axis: base.and_then(|b| b.axis),
        state,
    };
    c.validate().map_err(|e| bad(cmd, e.to_string()))?;
    Ok(c)
}

/// Every occurrence's world placement now.
fn worlds(s: &Session) -> BTreeMap<u64, Mat> {
    s.doc.occurrences.iter().map(|o| (o.id, occurrence_world(&s.doc, o.id))).collect()
}

fn dof_json(sol: &joints::Solution) -> Value {
    Value::Object(sol.dof.iter().map(|(o, n)| (o.to_string(), json!(n))).collect())
}

/// Until Task 5's report: suppressed, else ok.
fn status(c: &Constraint) -> &'static str {
    if c.suppressed { "suppressed" } else { "ok" }
}

/// The answer of create and edit.
fn answer(s: &Session, id: u64, sol: &joints::Solution, before: &BTreeMap<u64, Mat>) -> Value {
    let moved: Vec<u64> = worlds(s).into_iter().filter(|(o, m)| before.get(o) != Some(m)).map(|(o, _)| o).collect();
    let c = s.doc.assembly.constraints.iter().find(|c| c.id == id);
    json!({"id": id, "name": c.map(|c| c.name.clone()), "status": c.map_or("ok", status), "dof": dof_json(sol), "moved": moved})
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "constraint.create";
    if s.doc.assembly.constraints.len() >= MAX_CONSTRAINTS {
        return Err(bad(cmd, format!("an assembly holds at most {MAX_CONSTRAINTS} constraints")));
    }
    let mut c = build(s, p, None, cmd)?;
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
        _ => return Err(bad(cmd, "`constraint` must be an id, a name or a list of them")),
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
    if let Some(slot) = s.doc_mut().assembly.constraints.iter_mut().find(|x| x.id == id) {
        *slot = c;
    }
    let sol = super::joints::resolve(s);
    Ok(answer(s, id, &sol, &before))
}

fn suppress(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "constraint.suppress";
    let ids = constraint_ids(s, p, cmd)?;
    let to = match p.get("suppressed") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(x)) => Some(*x),
        Some(_) => return Err(bad(cmd, "`suppressed` must be true or false")),
    };
    if s.doc.assembly.constraints.iter().any(|c| ids.contains(&c.id) && to != Some(c.suppressed)) {
        for c in s.doc_mut().assembly.constraints.iter_mut().filter(|c| ids.contains(&c.id)) {
            c.suppressed = to.unwrap_or(!c.suppressed);
        }
        super::joints::resolve(s);
    }
    let rows: Vec<Value> = ids
        .iter()
        .filter_map(|id| s.doc.assembly.constraints.iter().find(|c| c.id == *id))
        .map(|c| json!({"name": c.name, "suppressed": c.suppressed, "status": status(c)}))
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

/// The offset in its unit: its expression's value when it has one that works out.
fn offset_now(s: &Session, c: &Constraint) -> f64 {
    let angle = c.kind.is_angle();
    let kind = if angle { solidcraft_doc::expr::Kind::Angle } else { solidcraft_doc::expr::Kind::Length };
    match c.exprs.offset.as_deref().map(|e| s.doc.eval(e, kind)) {
        Some(Ok(v)) if v.is_finite() => {
            if angle {
                v.to_degrees()
            } else {
                v
            }
        }
        _ => c.offset,
    }
}

/// A constraint as `constraint.list` gives it.
fn row(s: &Session, c: &Constraint) -> Value {
    let mut limits = serde_json::Map::new();
    for (k, v) in [("min", c.limits.min), ("max", c.limits.max)] {
        if let Some(v) = v {
            limits.insert(k.into(), json!(v));
        }
    }
    let mut v = json!({"id": c.id, "name": c.name, "type": c.kind, "solution": c.solution,
        "occurrences": c.picks.iter().map(|g| g.occurrence).collect::<Vec<_>>(), "offset": offset_now(s, c),
        "unit": if c.kind.is_angle() { "deg" } else { "mm" }, "limits": limits, "suppressed": c.suppressed, "status": status(c)});
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
    let rows: Vec<Value> =
        s.doc.assembly.constraints.iter().filter(|c| only.is_none_or(|o| c.picks.iter().any(|g| g.occurrence == o))).map(|c| row(s, c)).collect();
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
    let out: Vec<Value> = ids
        .iter()
        .filter_map(|id| s.doc.assembly.constraints.iter().find(|c| c.id == *id))
        .map(|c| {
            let mut v = row(s, c);
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
            v["compiled"] = compiled
                .joints
                .iter()
                .filter(|j| owner_of(j.id) == Some(c.id))
                .map(|j| json!({"id": j.id, "kind": j.kind, "flip": j.flip, "offset": j.offset}))
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
#[path = "constraints_hostile_tests.rs"]
mod constraints_hostile_tests;
