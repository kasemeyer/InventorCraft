//! Drawing dimensions (spec §2.7, §4.5): paper-space CADCraft `Dimension`s on layer DIMENSION in
//! the ANSI_INCH style, attached (`assoc`) to the view entities they measure, with the override
//! `linearFactor` (DIMLFAC) = 1 / the view's scale so that they read model inches. What they draw
//! is CADCraft's own formatting (`cadcraft_render::dimension_in`): no text is made here.

use std::collections::BTreeSet;

use cadcraft_doc::{AssocSnap, Common, DimAssoc, DimKind, DimMarks, Dimension, Drawing, Entity, EntityKind, Handle, Space};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Map, Value, json};
use solidcraft_doc::{DrawingModel, ViewId};

use super::assoc;
use super::mapper::xkey_of;
use super::template::DIM_STYLE;

/// The value token of a dimension's text in commands and the UI (Inventor's, spec §2.7); a
/// dimension stores it as DXF's [`DXF_TOKEN`]. [A: spec §2.7]
pub const VALUE_TOKEN: &str = "<<>>";
/// The measured value's placeholder in a stored dimension text (DXF).
pub(crate) const DXF_TOKEN: &str = "<>";

/// The layer every drawing dimension is on.
pub(crate) const DIMENSION_LAYER: &str = "DIMENSION";

/// The DimStyle field of DIMLFAC, as a dimension override.
const LINEAR_FACTOR: &str = "linearFactor";

/// Where a definition point of a new dimension sits: a point on paper, attached (or not) to a
/// view entity.
#[derive(Clone, Debug)]
pub(crate) struct Anchor {
    pub at: Vec2,
    pub handle: Handle,
    pub snap: AssocSnap,
}

/// How a new linear dimension measures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Direction {
    Horizontal,
    Vertical,
    Aligned,
}

fn v3(p: Vec2) -> Vec3 {
    Vec3::new(p.x, p.y, 0.0)
}

/// A dimension of `kind` in the drawing style with DIMLFAC 1 / `scale` and no points yet.
fn blank(kind: DimKind, scale: f64) -> Dimension {
    let mut overrides = Map::new();
    overrides.insert(LINEAR_FACTOR.into(), json!(1.0 / scale));
    Dimension {
        kind,
        defpt: Vec3::ZERO,
        text_mid: Vec3::ZERO,
        p13: Vec3::ZERO,
        p14: Vec3::ZERO,
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: DIM_STYLE.into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides,
        assoc: Vec::new(),
        marks: DimMarks::default(),
    }
}

/// A linear (horizontal or vertical) or aligned dimension from `first` to `second`, its
/// dimension line through `placement`.
pub(crate) fn linear(first: &Anchor, second: &Anchor, direction: Direction, placement: Vec2, scale: f64) -> Dimension {
    let (p1, p2) = (first.at, second.at);
    let (kind, dir) = match direction {
        Direction::Horizontal => (DimKind::Linear { rotation: 0.0 }, Vec2::X),
        Direction::Vertical => (DimKind::Linear { rotation: std::f64::consts::FRAC_PI_2 }, Vec2::new(0.0, 1.0)),
        Direction::Aligned => {
            let d = (p2 - p1).normalized();
            (DimKind::Aligned, if d == Vec2::ZERO || !d.is_finite() { Vec2::X } else { d })
        }
    };
    let foot = |p: Vec2| placement + dir * (p - placement).dot(dir);
    let mut d = blank(kind, scale);
    d.p13 = v3(p1);
    d.p14 = v3(p2);
    d.defpt = v3(foot(p2));
    d.text_mid = v3(foot(p1).mid(foot(p2)));
    d.assoc = vec![
        DimAssoc { point: "p13".into(), handle: first.handle, snap: first.snap.clone() },
        DimAssoc { point: "p14".into(), handle: second.handle, snap: second.snap.clone() },
    ];
    d
}

/// A diameter (or radius) dimension of the circle or arc `h` (centre `c`, radius `r`), its
/// leader towards `placement`, the text at `placement`. None when `h` is no circle or arc.
pub(crate) fn radial(paper: &Drawing, h: Handle, c: Vec2, r: f64, diameter: bool, placement: Vec2, scale: f64) -> Option<Dimension> {
    let u = (placement - c).normalized();
    let u = if u == Vec2::ZERO || !u.is_finite() { Vec2::X } else { u };
    let tip = c + u * r;
    let mut d = blank(if diameter { DimKind::Diameter } else { DimKind::Radius }, scale);
    let defpt = if diameter { c - u * r } else { c };
    d.defpt = v3(defpt);
    d.p15 = v3(tip);
    d.text_mid = v3(placement);
    d.user_text_pos = true;
    d.assoc = assoc::radial_assoc(paper, h, diameter, defpt, tip);
    (d.assoc.len() == 2).then_some(d)
}

/// Adds `d` to `layout` on DIMENSION with its measurement; its handle.
pub(crate) fn add(paper: &mut Drawing, layout: &str, mut d: Dimension) -> crate::Result<Handle> {
    d.measurement = cadcraft_render::dimension_in(paper, &d).measurement;
    let common = Common { layer: DIMENSION_LAYER.into(), ..Common::default() };
    paper.add(&Space::Paper(layout.to_string()), common, EntityKind::Dimension(d)).map_err(|e| crate::EngineError::Other(e.to_string()))
}

/// What a dimension is, in a command's result.
pub(crate) fn kind_name(d: &Dimension) -> &'static str {
    match d.kind {
        DimKind::Linear { .. } => "linear",
        DimKind::Aligned => "aligned",
        DimKind::Diameter => "diameter",
        DimKind::Radius => "radius",
        _ => "other",
    }
}

/// The dimension entity `handle` of `m`'s paper.
pub(crate) fn dimension_of(m: &DrawingModel, handle: u64) -> Option<&Dimension> {
    match &m.paper.entity(Handle(handle))?.kind {
        EntityKind::Dimension(d) => Some(d),
        _ => None,
    }
}

/// The text a dimension draws (measurement formatted by its style and overrides, prefix, user text).
pub fn dimension_text(m: &DrawingModel, handle: u64) -> Option<String> {
    dimension_of(m, handle).map(|d| cadcraft_render::dimension_in(&m.paper, d).value)
}

/// Every dimension of the paper (model space and every layout).
fn all_dimensions(paper: &Drawing) -> impl Iterator<Item = (&Entity, &Dimension)> {
    std::iter::once(&paper.model).chain(paper.layouts.iter().map(|l| &l.entities)).flat_map(|store| store.iter()).filter_map(|e| match &e.kind {
        EntityKind::Dimension(d) => Some((&**e, d)),
        _ => None,
    })
}

/// Dimensions whose referenced view entity no longer exists (drawn in the warning colour by D-2).
pub fn sick_dimensions(m: &DrawingModel) -> BTreeSet<u64> {
    all_dimensions(&m.paper).filter(|(_, d)| assoc::is_sick(&m.paper, d)).map(|(e, _)| e.handle.0).collect()
}

/// Removes every dimension attached to one of `handles` (R57: a deleted view's dimensions go
/// with it). Returns how many.
pub(crate) fn remove_attached(paper: &mut Drawing, handles: &BTreeSet<Handle>) -> usize {
    let gone: Vec<Handle> =
        all_dimensions(paper).filter(|(_, d)| d.assoc.iter().any(|a| handles.contains(&a.handle))).map(|(e, _)| e.handle).collect();
    for h in &gone {
        paper.remove_entity(*h);
    }
    gone.len()
}

/// The view whose entity a dimension is attached to (the first attachment on a view entity).
fn view_of_dimension(paper: &Drawing, d: &Dimension) -> Option<ViewId> {
    d.assoc.iter().find_map(|a| xkey_of(paper.entity(a.handle)?).map(|k| k.view))
}

/// Brings every attached dimension's DIMLFAC to 1 / its view's scale (a view rescaled keeps
/// its dimensions in model units). Returns how many changed.
pub(crate) fn sync_factors(m: &mut DrawingModel) -> usize {
    let changes: Vec<(Handle, f64)> = all_dimensions(&m.paper)
        .filter_map(|(e, d)| {
            let view = m.views.get(&view_of_dimension(&m.paper, d)?)?;
            let factor = 1.0 / view.scale;
            let current = d.overrides.get(LINEAR_FACTOR).and_then(Value::as_f64);
            (factor.is_finite() && current != Some(factor)).then_some((e.handle, factor))
        })
        .collect();
    for (h, factor) in &changes {
        let _ = m.paper.modify_entity(*h, |e| {
            if let EntityKind::Dimension(d) = &mut e.kind {
                d.overrides.insert(LINEAR_FACTOR.into(), json!(factor));
                d.block = None;
            }
        });
    }
    changes.len()
}
