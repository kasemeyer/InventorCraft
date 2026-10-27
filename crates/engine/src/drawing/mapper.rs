//! The seam (spec §4.5, CADCraft research §3 option b): a view's curves are plain paper-space
//! entities of its sheet's layout, at view scale, in inches. Each carries the xdata key
//! `(view id, model edge id, segment)`:
//!
//! `[(1001, "TCICAD_VIEW"), (1000, "<view id>"), (1000, "<model_edge_id>"), (1071, "<segment>")]`
//!
//! (a centre mark or centreline's fourth tag is `(1000, "cm")`, R21). Regenerating a view looks
//! each key up and changes its entity in place (`modify_entity`, the handle kept), so the ported
//! `assoc::update` sees the moved source and re-solves the dimensions on it; new keys are added,
//! and keys no longer produced are removed. One `Drawing.groups` entry per view lists exactly its
//! curves, so a view selects and moves as a unit.
//!
//! CADCraft's DXF writer registers every application named by a `1001` tag in the APPID table,
//! so `TCICAD_VIEW` needs no table entry here.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;

use cadcraft_doc::{Arc as ArcEntity, Circle, Common, Entity, EntityKind, Group, Handle, Line, LwPolyline, RawTag, Space};
use cadcraft_geom::{PolyVertex, Vec2 as PaperVec2, Vec3 as PaperVec3};
use solidcraft_doc::{DrawingModel, DrawingView, ViewId};
use solidcraft_geom::Vec2;

use super::project::{PaperCurve, ViewGeometry};

pub const XDATA_APP: &str = "TCICAD_VIEW";

/// The layer of a view's visible curves, and of its hidden ones.
pub(crate) const VISIBLE_LAYER: &str = "VISIBLE";
pub(crate) const HIDDEN_LAYER: &str = "HIDDEN";

/// The fourth xdata tag of a centre mark or centreline.
const MARK_TAG: &str = "cm";

/// A curve's place along its model edge.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Seg {
    Index(u32),
    /// Centre marks and centrelines, tag "cm" (R21).
    Mark,
}

/// The key of a view entity: which view, which model edge, which piece of it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct XKey {
    pub view: ViewId,
    pub edge: String,
    pub seg: Seg,
}

impl XKey {
    /// The key as xdata tags.
    pub(crate) fn tags(&self) -> Vec<RawTag> {
        let tag = |code: i32, value: String| RawTag { code, value };
        let last = match self.seg {
            Seg::Index(i) => tag(1071, i.to_string()),
            Seg::Mark => tag(1000, MARK_TAG.into()),
        };
        vec![tag(1001, XDATA_APP.into()), tag(1000, self.view.to_string()), tag(1000, self.edge.clone()), last]
    }
}

/// The view key an entity carries, if it is a view entity.
pub(crate) fn xkey_of(e: &cadcraft_doc::Entity) -> Option<XKey> {
    let xdata = &e.common.xdata;
    let at = xdata.iter().position(|t| t.code == 1001 && t.value == XDATA_APP)?;
    let mut rest = xdata.get(at + 1..)?.iter().take_while(|t| t.code != 1001);
    let view = rest.next().filter(|t| t.code == 1000)?.value.parse().ok()?;
    let edge = rest.next().filter(|t| t.code == 1000)?.value.clone();
    let seg = match rest.next()? {
        t if t.code == 1071 => Seg::Index(t.value.parse().ok()?),
        t if t.code == 1000 && t.value == MARK_TAG => Seg::Mark,
        _ => return None,
    };
    Some(XKey { view, edge, seg })
}

/// The name of the `Drawing.groups` entry of the view whose group handle is `group`.
pub(crate) fn group_name(group: Handle) -> String {
    format!("TCI_VIEW_{}", group.hex())
}

/// A view curve (mm, centred on the view) on paper: `position + mm / 25.4 · scale` (the view
/// frame is x = dir × up, y = up, so no rotation is needed).
pub(crate) fn to_paper(c: &PaperCurve, position: Vec2, scale: f64) -> PaperCurve {
    let k = scale / 25.4;
    let p = |q: Vec2| position + q * k;
    match c {
        PaperCurve::Line { a, b } => PaperCurve::Line { a: p(*a), b: p(*b) },
        PaperCurve::Arc { c, r, start, end } => PaperCurve::Arc { c: p(*c), r: r * k, start: *start, end: *end },
        PaperCurve::Circle { c, r } => PaperCurve::Circle { c: p(*c), r: r * k },
        PaperCurve::Poly { points } => PaperCurve::Poly { points: points.iter().map(|q| p(*q)).collect() },
    }
}

/// A paper curve as a CADCraft entity kind.
fn kind_of(c: &PaperCurve) -> EntityKind {
    let v3 = |p: Vec2| PaperVec3::new(p.x, p.y, 0.0);
    match c {
        PaperCurve::Line { a, b } => EntityKind::Line(Line { a: v3(*a), b: v3(*b) }),
        PaperCurve::Arc { c, r, start, end } => {
            EntityKind::Arc(ArcEntity { center: v3(*c), radius: *r, start: start.rem_euclid(TAU), end: end.rem_euclid(TAU) })
        }
        PaperCurve::Circle { c, r } => EntityKind::Circle(Circle { center: v3(*c), radius: *r }),
        PaperCurve::Poly { points } => EntityKind::LwPolyline(LwPolyline {
            vertices: points.iter().map(|p| PolyVertex { p: PaperVec2::new(p.x, p.y), ..PolyVertex::default() }).collect(),
            closed: false,
            const_width: 0.0,
            elevation: 0.0,
            plinegen: false,
        }),
    }
}

/// The handles of view `view`'s entities on `layout`, by key.
fn placed(m: &DrawingModel, layout: &str, view: ViewId) -> BTreeMap<XKey, Handle> {
    m.paper
        .layout(layout)
        .map(|l| l.entities.iter().filter_map(|e| xkey_of(e).filter(|k| k.view == view).map(|k| (k, e.handle))).collect())
        .unwrap_or_default()
}

/// Writes `g` for view `v` into its sheet's layout: modify_entity for keys that exist, Drawing::add for new
/// keys, remove_entity for keys no longer produced; `v.group` then lists exactly the view's entities.
///
/// Centre marks and centrelines (`Seg::Mark`) are not curves of `g`: they are kept as they are
/// and stay in the group.
pub(crate) fn place(m: &mut DrawingModel, v: &DrawingView, g: &ViewGeometry) {
    let Some(layout) = m.sheets.get(v.sheet).map(|s| s.layout.clone()) else { return };
    let space = Space::Paper(layout.clone());
    let before = placed(m, &layout, v.id);
    let position = v.position;
    let mut members: Vec<Handle> = Vec::new();
    let mut produced: BTreeSet<XKey> = BTreeSet::new();
    for curve in &g.curves {
        // A key is unique per view: a curve whose (edge, segment) is taken (a joined circle on
        // a piece of its first edge, R51) takes the edge's next free segment.
        let mut segment = curve.segment;
        let mut key = XKey { view: v.id, edge: curve.edge.clone(), seg: Seg::Index(segment) };
        while produced.contains(&key) {
            segment = segment.saturating_add(1);
            key.seg = Seg::Index(segment);
        }
        let kind = kind_of(&to_paper(&curve.curve, position, v.scale));
        let layer = if curve.hidden { HIDDEN_LAYER } else { VISIBLE_LAYER };
        let handle = match before.get(&key) {
            Some(&h) => {
                let same = m.paper.entity(h).is_some_and(|e| e.kind == kind && e.common.layer == layer);
                if !same {
                    let _ = m.paper.modify_entity(h, |e| {
                        e.kind = kind;
                        e.common.layer = layer.to_string();
                    });
                }
                Some(h)
            }
            None => {
                let common = Common { layer: layer.to_string(), xdata: key.tags(), ..Common::default() };
                m.paper.add(&space, common, kind).ok()
            }
        };
        if let Some(h) = handle {
            members.push(h);
        }
        produced.insert(key);
    }
    for (key, h) in &before {
        match key.seg {
            Seg::Mark => members.push(*h),
            Seg::Index(_) if !produced.contains(key) => {
                m.paper.remove_entity(*h);
            }
            Seg::Index(_) => {}
        }
    }
    let name = group_name(v.group);
    match m.paper.groups.iter_mut().find(|gr| gr.name == name) {
        Some(group) => group.members = members,
        None => m.paper.groups.push(Group { name, description: v.identifier.clone(), selectable: true, members }),
    }
}

/// The handle of the entity keyed `key`, on its view's sheet.
// The dimension commands (Task 10) are its first caller outside the tests.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn handle_of(m: &DrawingModel, key: &XKey) -> Option<cadcraft_doc::Handle> {
    let layout = m.sheets.get(m.views.get(&key.view)?.sheet)?.layout.as_str();
    m.paper.layout(layout)?.entities.iter().find(|e| xkey_of(e).as_ref() == Some(key)).map(|e: &std::sync::Arc<Entity>| e.handle)
}
