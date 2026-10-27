//! Placing drawing views (`drawing.view.*`, Plan D): a base view of a part or an assembly, and
//! views projected from it (third angle, spec §2.4). Each placed view is one command and one undo
//! step; its curves become paper entities through `drawing::mapper::place` (the seam, spec §4.5).
//! A placed view is then edited (style, scale, position, orientation, label, delete) and
//! regenerated from its model in place (`drawing.update`, spec §4.6).

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};
use solidcraft_doc::{
    CubeTarget, DocumentKind, DrawingModel, DrawingView, LineStyle, ModelKind, ModelRef, ModelRevision, Orientation, ViewId, parse_scale,
};
use solidcraft_geom::Vec2;

use super::CommandSpec;
use crate::drawing::mapper;
use crate::drawing::model_ref::{self, LoadedModel, Loads, absolute, load};
use crate::drawing::orient::{Projection, aligned_position, orientation_from, projected_orientation, projection_at};
use crate::drawing::project::{PaperCurve, ProjectOptions, ViewGeometry, project_view};
use crate::drawing::{self, template};
use crate::params::{bad, vec2, vec3};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("drawing.view.base", "Base View", base).icon("camera").params(
        "model: a part or assembly (.tcipart/.tciasm; a relative path is taken from the drawing's folder); \
         orientation?: a view cube target (front (default), top, right, back, bottom, left, iso_top_right, iso_top_left, iso_bottom_right, iso_bottom_left, or any other edge or corner such as front_top) or {dir: [x, y, z], up: [x, y, z]} (model directions); \
         lines?: hidden (default) or removed; shaded?: bool (default false); scale?: a number > 0 or \"1:2\" (default 1); position: [x, y], the view's centre on the sheet in inches; \
         sheet?: index (default the active sheet); identifier?: text (default the next VIEW<n>); tangent_edges?: bool (default false); label_visible?: bool (default false) \
         → view: its id, identifier, entities: {visible, hidden}, raster",
    ),
    CommandSpec::new("drawing.view.projected", "Projected View", projected).icon("project").params(
        "parent: a view id; position: [x, y] in inches. The direction from the parent's centre picks the view (third angle): up = top, down = bottom, left, right, a diagonal = an isometric; \
         an orthographic view stays aligned with its parent; style and scale come from the parent → view, identifier, entities: {visible, hidden}, raster",
    ),
    CommandSpec::new("drawing.view.set_style", "View Style", set_style).icon("appearance").params(
        "view: a view id; lines?: hidden or removed; shaded?: bool; style_from_base?: bool (a projected view takes its parent's style; giving lines or shaded stops it). \
         Projected views that take their style from this one follow → view, changed: the views changed",
    ),
    CommandSpec::new("drawing.view.set_scale", "View Scale", set_scale).icon("scale").params(
        "view: a view id; scale: a number > 0 or \"1:2\", or scale_from_base: bool (a projected view takes its parent's scale). \
         Views keep their centres; projected views that take their scale from this one follow → view, changed",
    ),
    CommandSpec::new("drawing.view.set_position", "Move View", set_position).icon("move").params(
        "view: a view id; position: [x, y], the view's centre in inches. A projected top, bottom, left or right view stays aligned with its parent; \
         the views projected from this one move with it → view, position: where it went",
    ),
    CommandSpec::new("drawing.view.set_orientation", "View Orientation", set_orientation).icon("orbit").params(
        "view: a base view's id; orientation: a view cube target (front, top, iso_top_right, ...) or {dir: [x, y, z], up: [x, y, z]}. \
         The views projected from it turn with it; all are projected again from the model → view, changed",
    ),
    CommandSpec::new("drawing.view.set_label", "View Label", set_label).icon("rename").params(
        "view: a view id; identifier?: text not used by another view; label_visible?: bool (the label is the identifier over SCALE <scale>, under the view) \
         → view, identifier, label_visible",
    ),
    CommandSpec::new("drawing.view.delete", "Delete View", delete).icon("delete").params(
        "view: a view id; children?: delete (default: the views projected from it go too) or keep (they become base views) → deleted: view ids, kept",
    ),
    CommandSpec::new("drawing.update", "Update Views", update).icon("update").params(
        "views?: view ids (default: every view whose model changed since it was drawn). Each is projected again from its model, its entities changed in place \
         → updated: view ids, orphans: dimensions whose edge is gone, missing: the stored paths of models not found (their views keep their entities)",
    ),
];

/// The chord tolerance of a drawing view's meshes, mm.
const TOLERANCE_MM: f64 = 0.01;

/// A view worked out from a command's parameters, not placed yet: `view` has no id, group or
/// identifier until it is placed.
pub(crate) struct Planned {
    pub view: DrawingView,
    pub geometry: ViewGeometry,
    /// The identifier asked for, if any.
    pub identifier: Option<String>,
}

impl Planned {
    /// The view's curves on paper (inches), each with whether it is hidden.
    pub(crate) fn paper_curves(&self) -> Vec<(PaperCurve, bool)> {
        self.geometry.curves.iter().map(|c| (mapper::to_paper(&c.curve, self.view.position, self.view.scale), c.hidden)).collect()
    }
}

/// `position`: two finite numbers.
fn position(p: &Value, cmd: &str) -> Result<Vec2> {
    p.get("position").and_then(vec2).ok_or_else(|| bad(cmd, "position must be two finite numbers"))
}

fn bool_param(p: &Value, key: &str, default: bool, cmd: &str) -> Result<bool> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_bool().ok_or_else(|| bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// `scale`: a number > 0 or a typed scale (`1:2`, `0.5`); default 1.
fn scale(p: &Value, cmd: &str) -> Result<f64> {
    let refused = || bad(cmd, "scale must be a number > 0");
    let v = match p.get("scale") {
        None | Some(Value::Null) => return Ok(1.0),
        Some(Value::Number(n)) => n.as_f64().ok_or_else(refused)?,
        Some(Value::String(t)) => parse_scale(t).ok_or_else(refused)?,
        Some(_) => return Err(refused()),
    };
    if v.is_finite() && v > 0.0 && v <= 1e6 { Ok(v) } else { Err(refused()) }
}

/// `orientation`: a cube target's name or `{dir, up}`; default front.
fn orientation(p: &Value, cmd: &str) -> Result<Orientation> {
    match p.get("orientation") {
        None | Some(Value::Null) => Ok(CubeTarget { x: 0, y: -1, z: 0 }.orientation()), // front
        Some(Value::String(name)) => {
            CubeTarget::from_name(name.trim()).map(CubeTarget::orientation).ok_or_else(|| bad(cmd, format!("unknown orientation {name}")))
        }
        Some(o @ Value::Object(_)) => {
            let dir = o.get("dir").and_then(vec3).ok_or_else(|| bad(cmd, "dir must be a non-zero vector"))?;
            let up = o.get("up").and_then(vec3).ok_or_else(|| bad(cmd, "up must not be parallel to dir"))?;
            orientation_from(dir, up).map_err(|why| bad(cmd, why))
        }
        Some(_) => Err(bad(cmd, "`orientation` must be a view cube target (front, top, iso_top_right, ...) or {dir, up}")),
    }
}

/// `lines`: hidden (default) or removed.
fn lines(p: &Value, cmd: &str) -> Result<LineStyle> {
    match p.get("lines") {
        None | Some(Value::Null) => Ok(LineStyle::Hidden),
        Some(v) => match v.as_str().map(str::trim) {
            Some("hidden") => Ok(LineStyle::Hidden),
            Some("removed") => Ok(LineStyle::Removed),
            _ => Err(bad(cmd, "`lines` must be hidden or removed")),
        },
    }
}

/// `identifier`: text not taken by another view of the drawing (`except`: the view being renamed).
fn identifier(m: &DrawingModel, p: &Value, cmd: &str, except: Option<ViewId>) -> Result<Option<String>> {
    match p.get("identifier") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let text = v.as_str().map(str::trim).filter(|t| !t.is_empty() && t.len() <= 255).ok_or_else(|| bad(cmd, "`identifier` must be text"))?;
            if m.views.values().any(|v| v.identifier == text && Some(v.id) != except) {
                return Err(bad(cmd, format!("identifier {text} is taken")));
            }
            Ok(Some(text.to_string()))
        }
    }
}

/// The view of `planned`'s model and orientation, projected.
fn geometry(s: &Session, view: &DrawingView) -> Result<(ViewGeometry, ModelRevision, ModelKind)> {
    let loaded = load(s, &view.model)?;
    geometry_of(&loaded, view)
}

/// `view` of the model `loaded`, projected.
fn geometry_of(loaded: &LoadedModel, view: &DrawingView) -> Result<(ViewGeometry, ModelRevision, ModelKind)> {
    let kind = if loaded.session.doc.kind == DocumentKind::Part { ModelKind::Part } else { ModelKind::Assembly };
    let opts = ProjectOptions { lines: view.lines, tangent_edges: view.tangent_edges, tolerance_mm: TOLERANCE_MM };
    let g =
        project_view(&loaded.session, &view.orientation, &opts).map_err(|e| crate::EngineError::Other(format!("{}: {e}", loaded.path.display())))?;
    Ok((g, loaded.revision.clone(), kind))
}

/// The model reference of `given`, a path from the drawing's folder (the DesignLink rule: stored
/// relative to the drawing once it is saved, with `/`), and, with a project open, from the
/// project's workspace when the model is inside it.
fn model_ref(s: &Session, given: &str, cmd: &str) -> Result<ModelRef> {
    const NOT_A_MODEL: &str = "a drawing view shows a part or an assembly (.tcipart or .tciasm), not a drawing";
    if given.trim().is_empty() || given.len() >= 4096 {
        return Err(bad(cmd, "`model` must be the path of a part or an assembly"));
    }
    if solidcraft_io::kind_for_path(given) == Some(DocumentKind::Drawing) {
        return Err(bad(cmd, NOT_A_MODEL));
    }
    let file = absolute(&super::component_link::resolve(s.path.as_deref(), given));
    let project_path = s.project_file.as_ref().and_then(|project| {
        let rel = file.strip_prefix(absolute(Path::new(&project.workspace))).ok()?;
        Some(rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"))
    });
    let kind = match solidcraft_io::kind_for_path(given) {
        Some(DocumentKind::Part) => ModelKind::Part,
        _ => ModelKind::Assembly,
    };
    Ok(ModelRef { path: super::component_link::stored(s.path.as_deref(), &file), project_path, kind })
}

/// A view not placed yet (no id, group or identifier).
fn unplaced(model: ModelRef, orientation: Orientation, at: Vec2) -> DrawingView {
    DrawingView {
        id: 0,
        identifier: String::new(),
        sheet: 0,
        parent: None,
        model,
        model_state: None,
        orientation,
        scale: 1.0,
        scale_from_base: false,
        lines: LineStyle::Hidden,
        shaded: false,
        style_from_base: false,
        tangent_edges: false,
        position: at,
        label_visible: false,
        group: cadcraft_doc::Handle(0),
        revision: ModelRevision::default(),
        raster: None,
    }
}

/// What `cmd` (drawing.view.base or drawing.view.projected) with `p` would place, every
/// parameter checked before the model is loaded.
pub(crate) fn plan(s: &Session, cmd: &str, p: &Value) -> Result<Planned> {
    let m = drawing::require(s, cmd)?;
    let (mut view, identifier) = match cmd {
        "drawing.view.base" => {
            let scale = scale(p, cmd)?;
            let at = position(p, cmd)?;
            let orientation = orientation(p, cmd)?;
            let lines = lines(p, cmd)?;
            let shaded = bool_param(p, "shaded", false, cmd)?;
            let tangent_edges = bool_param(p, "tangent_edges", false, cmd)?;
            let label_visible = bool_param(p, "label_visible", false, cmd)?;
            let sheet = match p.get("sheet") {
                None | Some(Value::Null) => s.drawing_sheet.min(m.sheets.len().saturating_sub(1)),
                Some(v) => v
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .filter(|n| *n < m.sheets.len())
                    .ok_or_else(|| bad(cmd, format!("`sheet` must be a sheet index (0 to {})", m.sheets.len().saturating_sub(1))))?,
            };
            if m.sheets.is_empty() {
                return Err(bad(cmd, "the drawing has no sheets"));
            }
            let identifier = identifier(m, p, cmd, None)?;
            let given = p.get("model").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`model` must be the path of a part or an assembly"))?;
            let model = model_ref(s, given, cmd)?;
            let mut v = unplaced(model, orientation, at);
            (v.sheet, v.scale, v.lines, v.shaded, v.tangent_edges, v.label_visible) = (sheet, scale, lines, shaded, tangent_edges, label_visible);
            (v, identifier)
        }
        "drawing.view.projected" => {
            let id = match p.get("parent") {
                Some(Value::Number(n)) => n.as_u64().ok_or_else(|| bad(cmd, "`parent` must be a view id"))?,
                _ => return Err(bad(cmd, "`parent` must be a view id")),
            };
            let parent = m.views.get(&id).ok_or_else(|| bad(cmd, format!("no view {id}")))?;
            let at = position(p, cmd)?;
            let from = [parent.position.x, parent.position.y];
            let projection = projection_at(from, [at.x, at.y]);
            let [x, y] = aligned_position(from, projection, [at.x, at.y]);
            let mut v = unplaced(parent.model.clone(), projected_orientation(&parent.orientation, projection), Vec2::new(x, y));
            v.sheet = parent.sheet;
            v.parent = Some(parent.id);
            v.model_state.clone_from(&parent.model_state);
            (v.scale, v.lines, v.shaded, v.tangent_edges) = (parent.scale, parent.lines, parent.shaded, parent.tangent_edges);
            (v.style_from_base, v.scale_from_base) = (true, true);
            (v, None)
        }
        _ => return Err(bad(cmd, "a view preview is of drawing.view.base or drawing.view.projected")),
    };
    let (geometry, revision, kind) = geometry(s, &view).map_err(|e| crate::EngineError::Other(format!("{cmd}: {e}")))?;
    view.revision = revision;
    view.model.kind = kind;
    Ok(Planned { view, geometry, identifier })
}

/// Places `planned` (one undo step): its id, group and identifier, its entities, and the title
/// block's SCALE. Returns the command's result.
fn place(s: &mut Session, cmd: &str, planned: Planned) -> Result<Value> {
    let Planned { mut view, geometry, identifier } = planned;
    let hidden = geometry.curves.iter().filter(|c| c.hidden).count();
    let visible = geometry.curves.len() - hidden;
    let (id, name) = drawing::edit(s, cmd, |m, _| {
        view.id = m.views.keys().next_back().map_or(1, |last| last + 1);
        view.group = m.paper.new_handle();
        view.identifier = match identifier {
            Some(text) => text,
            None => {
                let mut n = m.next_view_number.max(1);
                while m.views.values().any(|v| v.identifier == format!("VIEW{n}")) {
                    n += 1;
                }
                m.next_view_number = n + 1;
                format!("VIEW{n}")
            }
        };
        mapper::place(m, &view, &geometry);
        let out = (view.id, view.identifier.clone());
        m.views.insert(view.id, view);
        template::refresh_title_blocks(m);
        Ok(out)
    })?;
    Ok(json!({"view": id, "identifier": name, "entities": {"visible": visible, "hidden": hidden}, "raster": "none"}))
}

fn base(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.view.base";
    let planned = plan(s, cmd, p)?;
    place(s, cmd, planned)
}

fn projected(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.view.projected";
    let planned = plan(s, cmd, p)?;
    place(s, cmd, planned)
}

// --- Editing placed views and regenerating them (Task 8) ---

/// The `view` param: the id of a view of `m`.
fn view_param(m: &DrawingModel, p: &Value, cmd: &str) -> Result<ViewId> {
    let id = match p.get("view") {
        Some(Value::Number(n)) => n.as_u64().ok_or_else(|| bad(cmd, "`view` must be a view id"))?,
        _ => return Err(bad(cmd, "`view` must be a view id")),
    };
    if m.views.contains_key(&id) { Ok(id) } else { Err(bad(cmd, format!("no view {id}"))) }
}

/// The views projected from `id`, from `id`'s children down, each after its parent.
fn descendants(views: &BTreeMap<ViewId, DrawingView>, id: ViewId) -> Vec<ViewId> {
    let mut out: Vec<ViewId> = Vec::new();
    let mut next = vec![id];
    while let Some(parent) = next.pop() {
        let children: Vec<ViewId> = views.values().filter(|v| v.parent == Some(parent) && v.id != id && !out.contains(&v.id)).map(|v| v.id).collect();
        out.extend(children.iter().copied());
        next.extend(children.into_iter().rev());
    }
    out
}

/// The projection that places `child` beside `parent`.
fn projection_of(parent: &DrawingView, child: &DrawingView) -> Projection {
    projection_at([parent.position.x, parent.position.y], [child.position.x, child.position.y])
}

/// `views` projected again from their (possibly changed) models, each with its new revision
/// and kind; the model of each is loaded once.
fn regenerated(s: &Session, views: Vec<DrawingView>, cmd: &str) -> Result<Vec<(DrawingView, ViewGeometry)>> {
    let mut loads = Loads::default();
    views
        .into_iter()
        .map(|mut v| {
            let loaded = loads.get(s, &v.model).ok_or_else(|| crate::EngineError::Other(format!("{cmd}: model not found: {}", v.model.path)))?;
            let (g, revision, kind) = geometry_of(&loaded, &v).map_err(|e| crate::EngineError::Other(format!("{cmd}: {e}")))?;
            (v.revision, v.model.kind) = (revision, kind);
            Ok((v, g))
        })
        .collect()
}

/// Writes `changed` views into `m`: regenerated ones through `mapper::place`, the others as
/// stored; every one's label follows; the title blocks' SCALE is refreshed.
fn install(m: &mut DrawingModel, changed: Vec<DrawingView>, regenerated: Vec<(DrawingView, ViewGeometry)>) {
    for v in changed {
        mapper::place_label(m, &v);
        m.views.insert(v.id, v);
    }
    for (v, g) in regenerated {
        mapper::place(m, &v, &g);
        mapper::place_label(m, &v);
        m.views.insert(v.id, v);
    }
    template::refresh_title_blocks(m);
}

fn set_style(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.view.set_style";
    let m = drawing::require(s, cmd)?;
    let id = view_param(m, p, cmd)?;
    let given_lines = match p.get("lines") {
        None | Some(Value::Null) => None,
        Some(_) => Some(lines(p, cmd)?),
    };
    let given_shaded = match p.get("shaded") {
        None | Some(Value::Null) => None,
        Some(_) => Some(bool_param(p, "shaded", false, cmd)?),
    };
    let follow = match p.get("style_from_base") {
        None | Some(Value::Null) => None,
        Some(_) => Some(bool_param(p, "style_from_base", false, cmd)?),
    };
    if given_lines.is_none() && given_shaded.is_none() && follow.is_none() {
        return Err(bad(cmd, "name lines, shaded or style_from_base"));
    }
    let mut views = m.views.clone();
    let parent = views.get(&id).and_then(|v| v.parent).and_then(|p| views.get(&p)).map(|p| (p.lines, p.shaded));
    let Some(v) = views.get_mut(&id) else { return Err(bad(cmd, format!("no view {id}"))) };
    match (follow, parent) {
        (Some(true), None) => return Err(bad(cmd, format!("{} is a base view: it has no parent style to follow", v.identifier))),
        (Some(true), Some(_)) if given_lines.is_some() || given_shaded.is_some() => {
            return Err(bad(cmd, "style_from_base: true takes the parent's style; give lines or shaded without it"));
        }
        (Some(true), Some((lines, shaded))) => (v.style_from_base, v.lines, v.shaded) = (true, lines, shaded),
        _ => {
            v.style_from_base = false;
            v.lines = given_lines.unwrap_or(v.lines);
            v.shaded = given_shaded.unwrap_or(v.shaded);
        }
    }
    for child in descendants(&views, id) {
        let Some(parent) = views.get(&child).and_then(|c| c.parent).and_then(|p| views.get(&p)).map(|p| (p.lines, p.shaded)) else { continue };
        if let Some(c) = views.get_mut(&child).filter(|c| c.style_from_base) {
            (c.lines, c.shaded) = parent;
        }
    }
    let (mut redraw, mut changed) = (Vec::new(), Vec::new());
    for (vid, v) in &views {
        match m.views.get(vid) {
            Some(old) if old.lines != v.lines => redraw.push(v.clone()),
            Some(old) if old != v => changed.push(v.clone()),
            _ => {}
        }
    }
    let regenerated = regenerated(s, redraw, cmd)?;
    let updated: Vec<ViewId> = changed.iter().map(|v| v.id).chain(regenerated.iter().map(|(v, _)| v.id)).collect();
    drawing::edit(s, cmd, |m, _| {
        install(m, changed, regenerated);
        Ok(())
    })?;
    Ok(json!({"view": id, "changed": updated}))
}

fn set_scale(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.view.set_scale";
    let m = drawing::require(s, cmd)?;
    let id = view_param(m, p, cmd)?;
    let given = match p.get("scale") {
        None | Some(Value::Null) => None,
        Some(_) => Some(scale(p, cmd)?),
    };
    let follow = match p.get("scale_from_base") {
        None | Some(Value::Null) => None,
        Some(_) => Some(bool_param(p, "scale_from_base", false, cmd)?),
    };
    let mut views = m.views.clone();
    let parent = views.get(&id).and_then(|v| v.parent).and_then(|p| views.get(&p)).map(|p| p.scale);
    let Some(v) = views.get_mut(&id) else { return Err(bad(cmd, format!("no view {id}"))) };
    match (given, follow, parent) {
        (None, None, _) => return Err(bad(cmd, "give scale or scale_from_base")),
        (_, Some(true), None) => return Err(bad(cmd, format!("{} is a base view: it has no parent scale to follow", v.identifier))),
        (Some(_), Some(true), _) => return Err(bad(cmd, "scale_from_base: true takes the parent's scale; give scale without it")),
        (None, Some(true), Some(scale)) => (v.scale_from_base, v.scale) = (true, scale),
        (given, _, _) => {
            v.scale_from_base = false;
            v.scale = given.unwrap_or(v.scale);
        }
    }
    for child in descendants(&views, id) {
        let Some(scale) = views.get(&child).and_then(|c| c.parent).and_then(|p| views.get(&p)).map(|p| p.scale) else { continue };
        if let Some(c) = views.get_mut(&child).filter(|c| c.scale_from_base) {
            c.scale = scale;
        }
    }
    let changed: Vec<(DrawingView, f64)> =
        views.into_values().filter_map(|v| m.views.get(&v.id).filter(|old| **old != v).map(|old| (v.clone(), old.scale))).collect();
    let ids: Vec<ViewId> = changed.iter().map(|(v, _)| v.id).collect();
    drawing::edit(s, cmd, |m, _| {
        // The paper curves scale about the view's centre (the model is not read).
        for (v, old) in &changed {
            if *old != v.scale {
                mapper::transform_view(m, v, &mapper::move_and_scale(v.position, v.position, v.scale / old));
            }
        }
        install(m, changed.into_iter().map(|(v, _)| v).collect(), Vec::new());
        Ok(())
    })?;
    Ok(json!({"view": id, "changed": ids}))
}

fn set_position(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.view.set_position";
    let m = drawing::require(s, cmd)?;
    let id = view_param(m, p, cmd)?;
    let at = position(p, cmd)?;
    let Some(v) = m.views.get(&id) else { return Err(bad(cmd, format!("no view {id}"))) };
    // A projected orthographic view stays on its alignment axis.
    let to = match v.parent.and_then(|p| m.views.get(&p)) {
        Some(parent) => {
            let [x, y] = aligned_position([parent.position.x, parent.position.y], projection_of(parent, v), [at.x, at.y]);
            Vec2::new(x, y)
        }
        None => at,
    };
    let delta = to - v.position;
    let moved: Vec<DrawingView> = std::iter::once(id)
        .chain(descendants(&m.views, id))
        .filter_map(|vid| m.views.get(&vid).cloned())
        .map(|mut v| {
            v.position += delta;
            v
        })
        .collect();
    drawing::edit(s, cmd, |m, _| {
        for v in &moved {
            if let Some(old) = m.views.get(&v.id).map(|o| o.position) {
                mapper::transform_view(m, v, &mapper::move_and_scale(old, v.position, 1.0));
            }
        }
        install(m, moved, Vec::new());
        Ok(())
    })?;
    Ok(json!({"view": id, "position": [to.x, to.y]}))
}

fn set_orientation(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.view.set_orientation";
    let m = drawing::require(s, cmd)?;
    let id = view_param(m, p, cmd)?;
    let Some(v) = m.views.get(&id) else { return Err(bad(cmd, format!("no view {id}"))) };
    if let Some(parent) = v.parent.and_then(|p| m.views.get(&p)) {
        return Err(bad(cmd, format!("only a base view's orientation can be set ({} is projected from {})", v.identifier, parent.identifier)));
    }
    if p.get("orientation").is_none_or(Value::is_null) {
        return Err(bad(cmd, "`orientation` is required: a view cube target (front, top, iso_top_right, ...) or {dir, up}"));
    }
    let mut views = m.views.clone();
    let o = orientation(p, cmd)?;
    if let Some(v) = views.get_mut(&id) {
        v.orientation = o;
    }
    let order = descendants(&views, id);
    for child in &order {
        let Some(c) = views.get(child) else { continue };
        let Some(parent) = c.parent.and_then(|p| views.get(&p)) else { continue };
        let derived = projected_orientation(&parent.orientation, projection_of(parent, c));
        if let Some(c) = views.get_mut(child) {
            c.orientation = derived;
        }
    }
    let redraw: Vec<DrawingView> = std::iter::once(id).chain(order).filter_map(|vid| views.remove(&vid)).collect();
    let regenerated = regenerated(s, redraw, cmd)?;
    let ids: Vec<ViewId> = regenerated.iter().map(|(v, _)| v.id).collect();
    drawing::edit(s, cmd, |m, _| {
        install(m, Vec::new(), regenerated);
        Ok(())
    })?;
    Ok(json!({"view": id, "changed": ids}))
}

fn set_label(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.view.set_label";
    let m = drawing::require(s, cmd)?;
    let id = view_param(m, p, cmd)?;
    let name = identifier(m, p, cmd, Some(id))?;
    let visible = match p.get("label_visible") {
        None | Some(Value::Null) => None,
        Some(_) => Some(bool_param(p, "label_visible", false, cmd)?),
    };
    if name.is_none() && visible.is_none() {
        return Err(bad(cmd, "give identifier or label_visible"));
    }
    let Some(mut v) = m.views.get(&id).cloned() else { return Err(bad(cmd, format!("no view {id}"))) };
    if let Some(name) = name {
        v.identifier = name;
    }
    v.label_visible = visible.unwrap_or(v.label_visible);
    let out = json!({"view": id, "identifier": v.identifier, "label_visible": v.label_visible});
    drawing::edit(s, cmd, |m, _| {
        install(m, vec![v], Vec::new());
        Ok(())
    })?;
    Ok(out)
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.view.delete";
    let m = drawing::require(s, cmd)?;
    let id = view_param(m, p, cmd)?;
    let keep = match p.get("children") {
        None | Some(Value::Null) => false,
        Some(v) => match v.as_str().map(str::trim) {
            Some("delete") => false,
            Some("keep") => true,
            _ => return Err(bad(cmd, "`children` must be delete or keep")),
        },
    };
    let removed: Vec<ViewId> = if keep { vec![id] } else { std::iter::once(id).chain(descendants(&m.views, id)).collect() };
    let freed: Vec<DrawingView> = if keep {
        m.views
            .values()
            .filter(|c| c.parent == Some(id))
            .map(|c| {
                let mut c = c.clone();
                (c.parent, c.style_from_base, c.scale_from_base) = (None, false, false);
                c
            })
            .collect()
    } else {
        Vec::new()
    };
    let kept: Vec<ViewId> = freed.iter().map(|v| v.id).collect();
    drawing::edit(s, cmd, |m, _| {
        for vid in &removed {
            if let Some(v) = m.views.remove(vid) {
                mapper::remove_view(m, &v);
            }
        }
        install(m, freed, Vec::new());
        Ok(())
    })?;
    model_ref::evict_unreferenced(s);
    Ok(json!({"deleted": removed, "kept": kept}))
}

/// `drawing.update`: the given views (default: every view whose model has changed) projected
/// again from their models in place, handles kept. Views whose model cannot be found keep their
/// stored entities and are reported by the path they store.
fn update(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.update";
    let m = drawing::require(s, cmd)?;
    let asked: Option<Vec<ViewId>> = match p.get("views") {
        None | Some(Value::Null) => None,
        Some(Value::Array(ids)) => Some(
            ids.iter()
                .map(|id| {
                    let id = id.as_u64().ok_or_else(|| bad(cmd, "`views` must be an array of view ids"))?;
                    if m.views.contains_key(&id) { Ok(id) } else { Err(bad(cmd, format!("no view {id}"))) }
                })
                .collect::<Result<Vec<_>>>()?,
        ),
        Some(_) => return Err(bad(cmd, "`views` must be an array of view ids")),
    };
    let targets: Vec<&DrawingView> = match &asked {
        Some(ids) => m.views.values().filter(|v| ids.contains(&v.id)).collect(),
        None => m.views.values().collect(),
    };
    let mut loads = Loads::default();
    let (mut missing, mut redraw): (Vec<String>, Vec<(DrawingView, std::sync::Arc<model_ref::LoadedModel>)>) = (Vec::new(), Vec::new());
    for v in targets {
        match loads.get(s, &v.model) {
            None => {
                if !missing.contains(&v.model.path) {
                    missing.push(v.model.path.clone());
                }
            }
            Some(loaded) if asked.is_some() || loaded.revision != v.revision => redraw.push((v.clone(), loaded)),
            Some(_) => {}
        }
    }
    let mut regenerated = Vec::new();
    for (mut v, loaded) in redraw {
        let (g, revision, kind) = geometry_of(&loaded, &v).map_err(|e| crate::EngineError::Other(format!("{cmd}: {e}")))?;
        (v.revision, v.model.kind) = (revision, kind);
        regenerated.push((v, g));
    }
    let updated: Vec<ViewId> = regenerated.iter().map(|(v, _)| v.id).collect();
    if !regenerated.is_empty() {
        drawing::edit(s, cmd, |m, _| {
            install(m, Vec::new(), regenerated);
            Ok(())
        })?;
    }
    model_ref::evict_unreferenced(s);
    // Orphaned dimensions are counted once dimensions exist (Task 10).
    Ok(json!({"updated": updated, "orphans": 0, "missing": missing}))
}

#[cfg(test)]
#[path = "drawing_view_tests.rs"]
mod drawing_view_tests;

#[cfg(test)]
#[path = "drawing_update_tests.rs"]
mod drawing_update_tests;
