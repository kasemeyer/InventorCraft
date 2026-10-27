//! Edge identity for drawing views: the hidden-line removal of `file.export_view_dxf`, run so
//! that every projected line names the model edge it came from, a B-rep edge by its persistent
//! name (`doc::naming`) or a silhouette by its face and its place among that face's
//! silhouettes. Dimensions attach to these names, so they survive a regeneration.
//!
//! The naming rules:
//! - A B-rep edge is named by `naming::edge_names` (`<face A>|<face B>`, `~k` when repeated). A
//!   name the naming worked out from geometry (unverified) is used as it is: `EdgeSource` does
//!   not mark it.
//! - A silhouette has no B-rep edge. Its chord carries the B-rep face of its front-facing
//!   triangle (`EdgeKind::Silhouette`); it is named after that face's whole surface: the pieces
//!   joined to it across hidden seams (`Mesh::surface_patch`; a cylinder's side is two halves),
//!   by the piece name that sorts first (R49). `k` numbers that surface's silhouettes in the view
//!   by the projected x of their midpoints, then y.
//! - A tangent edge (`Mesh::smooth_edges`) is drawn, as a `Brep` line with `smooth` set, so a
//!   view can leave it out (R48); an export never draws it.
// Drawing views (Task 6) are the first caller outside the tests.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::BTreeMap;
use std::sync::Arc;

use solidcraft_doc::naming;
use solidcraft_geom::Vec2;
use solidcraft_render::Camera;

use crate::cmd::view_dxf::meshes::{self, Keep, Scene};
use crate::cmd::view_dxf::projection::{self, EdgeKind, Line};
use crate::{EngineError, Result, Session};

/// The model edge a drawn line comes from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeSource {
    /// A B-rep edge, by its persistent name.
    Brep { component_path: Vec<u64>, body_name: String, edge_name: String },
    /// A silhouette of a curved face, which depends on the view: `k` numbers one face's
    /// silhouettes by the projected x of their midpoints, then y. A surface the kernel splits
    /// into pieces joined across hidden seams (a cylinder's two halves) is one face here, named
    /// after its piece whose name sorts first.
    Silhouette { component_path: Vec<u64>, body_name: String, face_name: String, k: u32 },
}

impl EdgeSource {
    /// The canonical id: `"{path}/{body}/e/{edge_name}"` or `"{path}/{body}/s/{face_name}/{k}"`,
    /// `path` being the occurrence ids joined by `.` (`"0"` for the root).
    pub fn model_edge_id(&self) -> String {
        let path = |p: &[u64]| if p.is_empty() { "0".to_string() } else { p.iter().map(u64::to_string).collect::<Vec<_>>().join(".") };
        match self {
            Self::Brep { component_path, body_name, edge_name } => format!("{}/{body_name}/e/{edge_name}", path(component_path)),
            Self::Silhouette { component_path, body_name, face_name, k } => format!("{}/{body_name}/s/{face_name}/{k}", path(component_path)),
        }
    }
}

/// One projected line of a drawing view, in mm on the view plane.
#[derive(Clone, Debug)]
pub(crate) struct SourcedLine {
    pub a: Vec2,
    pub b: Vec2,
    pub hidden: bool,
    pub source: EdgeSource,
    /// The mesh (= B-rep) edge index of a `Brep` source.
    pub mesh_edge: Option<u32>,
    /// A tangent edge (`Mesh::smooth_edges`), which a view shows only when asked.
    pub smooth: bool,
}

/// The HLR of `model`'s visible bodies seen by `cam`, each line naming its source.
pub(crate) fn sourced_lines(model: &Session, cam: &Camera, tolerance_mm: f64) -> Result<Vec<SourcedLine>> {
    if !cam.is_valid() || !(0.0001..=1.0).contains(&tolerance_mm) {
        return Err(EngineError::Other("drawing view: invalid camera or tolerance".into()));
    }
    let scene = meshes::prepare(model, tolerance_mm, Keep::Identity)?;
    let drawing = projection::drawing(&scene, &[], cam).map_err(|e| EngineError::Other(format!("drawing view: {e}")))?;
    name_lines(model, &scene, drawing.lines)
}

/// The names one placed body's lines need, looked up once.
struct Names {
    mesh: Arc<solidcraft_geom::Mesh>,
    edges: Vec<String>,
    faces: Arc<Vec<String>>,
}
impl Names {
    /// The name of B-rep face `face`'s whole surface: the first name of the pieces joined to it
    /// across hidden seams (`Mesh::surface_patch`).
    fn surface(&self, face: u32) -> Option<String> {
        let own = self.faces.get(face as usize)?;
        Some(self.mesh.surface_patch(face as usize).iter().filter_map(|f| self.faces.get(*f)).min().unwrap_or(own).clone())
    }
}

/// Names each line after the HLR (never in its loop): edge names and face names are resolved
/// once per placement, and each face's silhouettes are numbered.
fn name_lines(model: &Session, scene: &Scene, lines: Vec<Line>) -> Result<Vec<SourcedLine>> {
    let lost = |what: &str| EngineError::Other(format!("drawing view: {what} lost its identity"));
    let world = model.world_state();
    let mut names: BTreeMap<u32, Names> = BTreeMap::new();
    for line in &lines {
        let i = line.source.placement;
        if names.contains_key(&i) {
            continue;
        }
        let placed = scene.placements.get(i as usize).ok_or_else(|| lost("a placement"))?;
        let body = world.body(&placed.body).ok_or_else(|| lost(&placed.body))?;
        let edges = naming::edge_names(body);
        if scene.meshes.get(placed.mesh).is_none_or(|m| m.edges.len() != edges.len()) {
            return Err(lost(&placed.body));
        }
        names.insert(i, Names { mesh: body.mesh(), edges, faces: naming::face_names(body) });
    }
    // Each silhouette's face, by its surface's name (looked up once per face).
    let mut surfaces: BTreeMap<(u32, u32), String> = BTreeMap::new();
    for line in &lines {
        if let EdgeKind::Silhouette { face } = line.source.kind
            && !surfaces.contains_key(&(line.source.placement, face))
        {
            let named = names.get(&line.source.placement).ok_or_else(|| lost("a placement"))?;
            surfaces.insert((line.source.placement, face), named.surface(face).ok_or_else(|| lost("a silhouette's face"))?);
        }
    }
    // k: one face's silhouettes in order of their midpoints' x, then y.
    let mut silhouettes: BTreeMap<(u32, &str), Vec<usize>> = BTreeMap::new();
    for (n, line) in lines.iter().enumerate() {
        if let EdgeKind::Silhouette { face } = line.source.kind
            && let Some(name) = surfaces.get(&(line.source.placement, face))
        {
            silhouettes.entry((line.source.placement, name.as_str())).or_default().push(n);
        }
    }
    let mut k = vec![0u32; lines.len()];
    for members in silhouettes.values_mut() {
        let mid = |n: &usize| lines.get(*n).map_or(Vec2::ZERO, |l| (l.a + l.b) * 0.5);
        members.sort_by(|a, b| {
            let (p, q) = (mid(a), mid(b));
            p.x.total_cmp(&q.x).then(p.y.total_cmp(&q.y)).then(a.cmp(b))
        });
        for (rank, n) in members.iter().enumerate() {
            if let Some(slot) = k.get_mut(*n) {
                *slot = u32::try_from(rank).map_err(|_| lost("a silhouette"))?;
            }
        }
    }
    let mut out = Vec::with_capacity(lines.len());
    for (line, k) in lines.into_iter().zip(k) {
        let placed = scene.placements.get(line.source.placement as usize).ok_or_else(|| lost("a placement"))?;
        let named = names.get(&line.source.placement).ok_or_else(|| lost(&placed.body))?;
        let (component_path, body_name) = (placed.component_path.clone(), placed.body.clone());
        let (source, mesh_edge, smooth) = match line.source.kind {
            EdgeKind::Brep(i) => {
                let edge_name = named.edges.get(i as usize).ok_or_else(|| lost("an edge"))?.clone();
                let smooth = scene.meshes.get(placed.mesh).and_then(|m| m.smooth_edges.get(i as usize)).copied().unwrap_or(false);
                (EdgeSource::Brep { component_path, body_name, edge_name }, Some(i), smooth)
            }
            EdgeKind::Silhouette { face } => {
                let face_name = surfaces.get(&(line.source.placement, face)).ok_or_else(|| lost("a silhouette's face"))?.clone();
                (EdgeSource::Silhouette { component_path, body_name, face_name, k }, None, false)
            }
        };
        out.push(SourcedLine { a: line.a, b: line.b, hidden: line.hidden, source, mesh_edge, smooth });
    }
    Ok(out)
}
