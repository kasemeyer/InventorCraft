//! Placing drawing views (`drawing.view.*`, Plan D): a base view of a part or an assembly, and
//! views projected from it (third angle, spec §2.4). Each placed view is one command and one undo
//! step; its curves become paper entities through `drawing::mapper::place` (the seam, spec §4.5).

use std::path::Path;

use serde_json::{Value, json};
use solidcraft_doc::{CubeTarget, DocumentKind, DrawingModel, DrawingView, LineStyle, ModelKind, ModelRef, ModelRevision, Orientation, parse_scale};
use solidcraft_geom::Vec2;

use super::CommandSpec;
use crate::drawing::mapper;
use crate::drawing::model_ref::{absolute, load};
use crate::drawing::orient::{aligned_position, orientation_from, projected_orientation, projection_at};
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

/// `identifier`: text not taken by another view of the drawing.
fn identifier(m: &DrawingModel, p: &Value, cmd: &str) -> Result<Option<String>> {
    match p.get("identifier") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let text = v.as_str().map(str::trim).filter(|t| !t.is_empty() && t.len() <= 255).ok_or_else(|| bad(cmd, "`identifier` must be text"))?;
            if m.views.values().any(|v| v.identifier == text) {
                return Err(bad(cmd, format!("identifier {text} is taken")));
            }
            Ok(Some(text.to_string()))
        }
    }
}

/// The view of `planned`'s model and orientation, projected.
fn geometry(s: &Session, view: &DrawingView) -> Result<(ViewGeometry, ModelRevision, ModelKind)> {
    let loaded = load(s, &view.model)?;
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
            let identifier = identifier(m, p, cmd)?;
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

#[cfg(test)]
#[path = "drawing_view_tests.rs"]
mod drawing_view_tests;
