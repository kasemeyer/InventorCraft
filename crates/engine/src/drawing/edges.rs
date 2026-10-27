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
//!   by the projected x of their midpoints, then y; a silhouette is all its collinear end-to-end
//!   pieces, hidden or not, so an occluder that splits one renumbers none (R53).
//! - A tangent edge (`Mesh::smooth_edges`, or two faces whose normals agree within 1° along it,
//!   [`dihedral_tangent`], R64) is drawn, as a `Brep` line with `smooth` set, so a
//!   view can leave it out (R48); an export never draws it. Where it is also a silhouette (a
//!   tube's outline) it is marked `silhouette`, and a view always draws it (R54).
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
    /// whole silhouettes (all pieces, R53) by the projected x of their midpoints, then y. A surface the kernel splits
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
    /// A tangent edge (`Mesh::smooth_edges` or [`dihedral_tangent`]), which a view shows only when asked.
    pub smooth: bool,
    /// A silhouette, or a B-rep edge drawn where a silhouette of the same visibility runs (the
    /// merge names it after the edge): an outline, always drawn, even when `smooth` (R54).
    pub silhouette: bool,
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
    /// Per B-rep edge: tangent by the dihedral rule ([`dihedral_tangent`], R64).
    tangent: Vec<bool>,
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
        let mesh = body.mesh();
        let tangent = dihedral_tangent(&mesh);
        names.insert(i, Names { mesh, edges, faces: naming::face_names(body), tangent });
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
    // k: one surface's silhouettes in order of their midpoints' x, then y. A silhouette is a
    // surface's collinear end-to-end pieces, hidden or not (R53), so an occluder that splits
    // one does not renumber the others; all its pieces share its `k`.
    let mut silhouettes: BTreeMap<(u32, &str), Vec<usize>> = BTreeMap::new();
    for (n, line) in lines.iter().enumerate() {
        if let EdgeKind::Silhouette { face } = line.source.kind
            && let Some(name) = surfaces.get(&(line.source.placement, face))
        {
            silhouettes.entry((line.source.placement, name.as_str())).or_default().push(n);
        }
    }
    let mut k = vec![0u32; lines.len()];
    for members in silhouettes.values() {
        let pieces: Vec<(Vec2, Vec2)> = members.iter().map(|n| lines.get(*n).map_or((Vec2::ZERO, Vec2::ZERO), |l| (l.a, l.b))).collect();
        for (n, rank) in ranks(&pieces, members) {
            if let Some(slot) = k.get_mut(n) {
                *slot = rank;
            }
        }
    }
    let mut out = Vec::with_capacity(lines.len());
    for (line, k) in lines.into_iter().zip(k) {
        let placed = scene.placements.get(line.source.placement as usize).ok_or_else(|| lost("a placement"))?;
        let named = names.get(&line.source.placement).ok_or_else(|| lost(&placed.body))?;
        let (component_path, body_name) = (placed.component_path.clone(), placed.body.clone());
        let silhouette = line.silhouette || matches!(line.source.kind, EdgeKind::Silhouette { .. });
        let (source, mesh_edge, smooth) = match line.source.kind {
            EdgeKind::Brep(i) => {
                let edge_name = named.edges.get(i as usize).ok_or_else(|| lost("an edge"))?.clone();
                // The kernel's smooth flag, or tangent by the dihedral rule (R64).
                let smooth = scene.meshes.get(placed.mesh).and_then(|m| m.smooth_edges.get(i as usize)).copied().unwrap_or(false)
                    || named.tangent.get(i as usize).copied().unwrap_or(false);
                (EdgeSource::Brep { component_path, body_name, edge_name }, Some(i), smooth)
            }
            EdgeKind::Silhouette { face } => {
                let face_name = surfaces.get(&(line.source.placement, face)).ok_or_else(|| lost("a silhouette's face"))?.clone();
                (EdgeSource::Silhouette { component_path, body_name, face_name, k }, None, false)
            }
        };
        out.push(SourcedLine { a: line.a, b: line.b, hidden: line.hidden, source, mesh_edge, smooth, silhouette });
    }
    Ok(out)
}

/// The largest angle (degrees) between two faces' normals along an edge that is tangent (R64).
pub(crate) const TANGENT_DEGREES: f64 = 1.0;

/// Per B-rep edge of `mesh`: tangent by the dihedral rule (R64), the edge bounding two
/// different faces whose normals agree within [`TANGENT_DEGREES`] at every point of its
/// polyline. A face's normal at a point is the per-corner normal (the surface's own, the
/// tessellator keeps normals per face) of each of its vertices there; a point either face has
/// no vertex at, or a mesh without normals, leaves the edge not tangent.
pub(crate) fn dihedral_tangent(mesh: &solidcraft_geom::Mesh) -> Vec<bool> {
    let mut out = vec![false; mesh.edges.len()];
    if mesh.normals.len() != mesh.positions.len() || mesh.tri_face.len() != mesh.triangles.len() {
        return out;
    }
    let scale = mesh.positions.iter().flat_map(|p| [p.x.abs(), p.y.abs(), p.z.abs()]).fold(1.0f64, f64::max);
    let tol = scale * 1e-7;
    let cell = |p: solidcraft_geom::Vec3| ((p.x / tol).floor() as i64, (p.y / tol).floor() as i64, (p.z / tol).floor() as i64);
    // Each vertex's face (vertices are per corner, so one face each), filed by position.
    let mut face_of = vec![u32::MAX; mesh.positions.len()];
    for (t, face) in mesh.triangles.iter().zip(&mesh.tri_face) {
        for v in t {
            if let Some(slot) = face_of.get_mut(*v as usize) {
                *slot = *face;
            }
        }
    }
    let mut grid: std::collections::HashMap<(i64, i64, i64), Vec<u32>> = std::collections::HashMap::new();
    for (v, p) in mesh.positions.iter().enumerate() {
        if p.is_finite() {
            grid.entry(cell(*p)).or_default().push(v as u32);
        }
    }
    // The unit normals face `face` has at `p`.
    let normals_at = |p: solidcraft_geom::Vec3, face: u32| -> Vec<solidcraft_geom::Vec3> {
        let (x, y, z) = cell(p);
        let mut found = Vec::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for v in grid.get(&(x + dx, y + dy, z + dz)).into_iter().flatten() {
                        let v = *v as usize;
                        if face_of.get(v) == Some(&face)
                            && mesh.positions.get(v).is_some_and(|q| q.dist(p) <= tol)
                            && let Some(n) = mesh.normals.get(v).and_then(|n| n.normalized())
                        {
                            found.push(n);
                        }
                    }
                }
            }
        }
        found
    };
    let cos = TANGENT_DEGREES.to_radians().cos();
    for (i, slot) in out.iter_mut().enumerate() {
        let (Some(line), Some(&[a, b])) = (mesh.edges.get(i), mesh.edge_faces.get(i).map(Vec::as_slice)) else { continue };
        if a == b || line.is_empty() {
            continue;
        }
        *slot = line.iter().all(|p| {
            let (na, nb) = (normals_at(*p, a), normals_at(*p, b));
            !na.is_empty() && !nb.is_empty() && na.iter().all(|m| nb.iter().all(|n| m.dot(*n) >= cos))
        });
    }
    out
}

/// Two pieces closer than this (mm) to each other's line, and end to end within it, are one
/// silhouette.
const SAME_SILHOUETTE_MM: f64 = 1e-6;

/// The pieces of one surface's silhouettes grouped into whole silhouettes: collinear pieces
/// that touch or overlap end to end (R53). Each group lists piece indices in ascending order.
fn whole_silhouettes(pieces: &[(Vec2, Vec2)]) -> Vec<Vec<usize>> {
    let joined = |(a, b): (Vec2, Vec2), (c, d): (Vec2, Vec2)| {
        let Some(dir) = (b - a).normalized() else { return a.dist(c) <= SAME_SILHOUETTE_MM || a.dist(d) <= SAME_SILHOUETTE_MM };
        let off = |p: Vec2| (p - a).cross(dir).abs();
        if off(c) > SAME_SILHOUETTE_MM || off(d) > SAME_SILHOUETTE_MM {
            return false;
        }
        let (s, t) = ((c - a).dot(dir), (d - a).dot(dir));
        let len = (b - a).dot(dir);
        s.max(t) >= -SAME_SILHOUETTE_MM && s.min(t) <= len + SAME_SILHOUETTE_MM
    };
    let mut root: Vec<usize> = (0..pieces.len()).collect();
    fn find(root: &mut [usize], mut i: usize) -> usize {
        while let Some(&r) = root.get(i).filter(|r| **r != i) {
            i = r;
        }
        i
    }
    for (i, p) in pieces.iter().enumerate() {
        for (j, q) in pieces.iter().enumerate().skip(i + 1) {
            if joined(*p, *q) && joined(*q, *p) {
                let (ri, rj) = (find(&mut root, i), find(&mut root, j));
                if let Some(slot) = root.get_mut(ri.max(rj)) {
                    *slot = ri.min(rj);
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..pieces.len() {
        groups.entry(find(&mut root, i)).or_default().push(i);
    }
    groups.into_values().collect()
}

/// The midpoint of a whole silhouette: halfway between its two outermost piece ends.
fn silhouette_mid(pieces: &[(Vec2, Vec2)], group: &[usize]) -> Vec2 {
    let ends: Vec<Vec2> = group.iter().filter_map(|i| pieces.get(*i)).flat_map(|(a, b)| [*a, *b]).collect();
    let Some(&first) = ends.first() else { return Vec2::ZERO };
    let far = |from: Vec2| ends.iter().copied().max_by(|p, q| p.dist(from).total_cmp(&q.dist(from))).unwrap_or(from);
    let a = far(first);
    (a + far(a)) * 0.5
}

/// Numbers the silhouettes in `lines` again, by the rule `sourced_lines` uses (whole
/// silhouettes of one surface by their midpoints' x, then y; R53): for a caller that moved or
/// dropped silhouette pieces (`drawing::project` snaps them to the exact surface, R52).
pub(crate) fn renumber_silhouettes(lines: &mut [SourcedLine]) {
    let mut surfaces: BTreeMap<(Vec<u64>, String, String), Vec<usize>> = BTreeMap::new();
    for (n, line) in lines.iter().enumerate() {
        if let EdgeSource::Silhouette { component_path, body_name, face_name, .. } = &line.source {
            surfaces.entry((component_path.clone(), body_name.clone(), face_name.clone())).or_default().push(n);
        }
    }
    for members in surfaces.values() {
        let pieces: Vec<(Vec2, Vec2)> = members.iter().map(|n| lines.get(*n).map_or((Vec2::ZERO, Vec2::ZERO), |l| (l.a, l.b))).collect();
        for (n, rank) in ranks(&pieces, members) {
            if let Some(EdgeSource::Silhouette { k, .. }) = lines.get_mut(n).map(|l| &mut l.source) {
                *k = rank;
            }
        }
    }
}

/// One surface's silhouette pieces (`pieces[i]` is line `members[i]`) ranked by the rule of R53:
/// whole silhouettes ([`whole_silhouettes`]) in order of their midpoints' x, then y; every piece
/// of one gets its rank. Returns (line index, rank) per piece.
fn ranks(pieces: &[(Vec2, Vec2)], members: &[usize]) -> Vec<(usize, u32)> {
    let mut groups: Vec<(Vec2, Vec<usize>)> = whole_silhouettes(pieces)
        .into_iter()
        .map(|g| (silhouette_mid(pieces, &g), g.into_iter().filter_map(|i| members.get(i).copied()).collect()))
        .collect();
    groups.sort_by(|(p, a), (q, b)| p.x.total_cmp(&q.x).then(p.y.total_cmp(&q.y)).then(a.cmp(b)));
    let mut out = Vec::with_capacity(members.len());
    for (rank, (_, group)) in groups.into_iter().enumerate() {
        let rank = u32::try_from(rank).unwrap_or(u32::MAX);
        out.extend(group.into_iter().map(|n| (n, rank)));
    }
    out
}
