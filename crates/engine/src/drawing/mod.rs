//! Drawings (Plan D): the helpers every `drawing.*` command shares, and the API the drawing
//! environment's UI uses. A drawing is a `Document` of kind `Drawing` whose `drawing` holds the
//! [`DrawingModel`]: its paper (a vendored CADCraft drawing, in inches), sheets, views and title
//! block fields.

pub(crate) mod assoc;
pub(crate) mod centermark;
pub mod dims;
pub(crate) mod edges;
pub(crate) mod export;
pub(crate) mod mapper;
pub(crate) mod model_ref;
pub(crate) mod notes;
pub mod orient;
pub mod pick;
pub(crate) mod project;
pub mod raster;
pub mod template;

use std::sync::Arc;

use serde_json::Value;
use solidcraft_doc::{DocumentKind, DrawingModel, DrawingProps};

pub use centermark::MARK_OVERSHOOT_IN;
pub use dims::{VALUE_TOKEN, dimension_text, sick_dimensions};
pub(crate) use model_ref::rebased_models;
pub use model_ref::{ModelProvider, stale_views};
pub use notes::{LEADER_ARROW, NOTE_HEIGHT_IN};
pub use orient::{Projection, aligned_position, projected_orientation, projection_at};
pub use pick::{CurveKind, PaperPick, pick, preview_view};
pub use project::{PaperCurve, camera_for};
pub use raster::{CaptureRasterizer, FLAT_GREY, MAX_RASTER_SIDE, PDF_PX_PER_IN, SCREEN_PX_PER_IN, ViewRasterizer, raster_rect};
pub use template::*;

use crate::{EngineError, Result, Session};

pub(crate) const NOT_A_DRAWING: &str = "the active document is not a drawing (drawing.new makes one)";

/// The active drawing, or Err("<cmd>: " + NOT_A_DRAWING).
pub(crate) fn require<'a>(s: &'a Session, cmd: &str) -> Result<&'a DrawingModel> {
    match (&s.doc.kind, s.doc.drawing.as_deref()) {
        (DocumentKind::Drawing, Some(m)) => Ok(m),
        _ => Err(EngineError::Other(format!("{cmd}: {NOT_A_DRAWING}"))),
    }
}

/// Runs `f` on a copy-on-write copy of the drawing, re-solves the associative dimensions
/// (`assoc::update` against the drawing `f` started from, then each attached dimension's
/// DIMLFAC to 1 / its view's scale), puts attached leaders' arrow ends back on their edges
/// (`notes::follow`) and installs the edited document. One command = one undo step through
/// `undoable`: `Session::execute` records the document `f` started from.
pub(crate) fn edit<T>(s: &mut Session, cmd: &str, f: impl FnOnce(&mut DrawingModel, &Session) -> Result<T>) -> Result<T> {
    require(s, cmd)?;
    let Some(mut model) = s.doc.drawing.clone() else { return Err(EngineError::Other(format!("{cmd}: {NOT_A_DRAWING}"))) };
    // The document still holds the model, so this copies it (the paper's entity stores are
    // copy-on-write: only what `f` touches is copied again).
    let edited = Arc::make_mut(&mut model);
    let out = f(edited, s)?;
    if let Some(before) = s.doc.drawing.as_deref() {
        assoc::update(&mut edited.paper, &before.paper);
    }
    dims::sync_factors(edited);
    // Attached leaders follow their edges (`assoc::update` handles dimensions only).
    notes::follow(&mut edited.paper);
    sync_suppressed(&mut edited.paper);
    s.doc_mut().drawing = Some(model);
    Ok(out)
}

/// R58, R61: a dimension or an attached note on a curve the view's style suppresses (an
/// invisible view entity, Hidden Line Removed) is suppressed too: kept with its handle and its
/// attachment, invisible, never sick or orphaned; it is shown again with the curve. Returns how
/// many entities changed.
pub(crate) fn sync_suppressed(paper: &mut cadcraft_doc::Drawing) -> usize {
    use std::collections::BTreeMap;

    use cadcraft_doc::{EntityKind, Handle};
    let hidden = |h: Handle| paper.entity(h).is_some_and(|t| !t.common.visible);
    // Each annotation's visibility as it should be.
    let mut want: BTreeMap<Handle, bool> = BTreeMap::new();
    for store in std::iter::once(&paper.model).chain(paper.layouts.iter().map(|l| &l.entities)) {
        for e in store.iter() {
            if let EntityKind::Dimension(d) = &e.kind {
                if !d.assoc.is_empty() {
                    want.insert(e.handle, !d.assoc.iter().any(|a| hidden(a.handle)));
                }
            } else if let Some(n) = notes::note_of(e)
                && let Some(a) = n.attach
            {
                let shown = !hidden(a.handle);
                for h in std::iter::once(e.handle).chain(n.partner) {
                    let w = want.entry(h).or_insert(true);
                    *w = *w && shown;
                }
            }
        }
    }
    let changes: Vec<(Handle, bool)> = want.into_iter().filter(|(h, shown)| paper.entity(*h).is_some_and(|e| e.common.visible != *shown)).collect();
    for (h, shown) in &changes {
        let _ = paper.modify_entity(*h, |e| e.common.visible = *shown);
    }
    changes.len()
}

/// The annotations (dimensions, notes and leaders, centre marks and centrelines) the view styles
/// suppress now (R58, R61): kept, invisible, not sick ([`sick_dimensions`] never lists them).
pub fn suppressed_annotations(m: &DrawingModel) -> std::collections::BTreeSet<u64> {
    let paper = &m.paper;
    std::iter::once(&paper.model)
        .chain(paper.layouts.iter().map(|l| &l.entities))
        .flat_map(|s| s.iter())
        .filter(|e| !e.common.visible)
        .filter(|e| {
            matches!(e.kind, cadcraft_doc::EntityKind::Dimension(_))
                || notes::note_of(e).is_some()
                || mapper::xkey_of(e).is_some_and(|k| k.seg == mapper::Seg::Mark)
        })
        .map(|e| e.handle.0)
        .collect()
}

/// `<model folder>/<model stem>.tcidrw` (owner default; §8.6). Only the extension changes, so
/// the folder is written as given, with its own separators.
pub fn default_drawing_path(model_path: &str) -> String {
    let name_start = model_path.rfind(['/', '\\']).map_or(0, |i| i + 1);
    let stem_end = model_path[name_start..].rfind('.').filter(|i| *i > 0).map_or(model_path.len(), |i| name_start + i);
    format!("{}.{}", &model_path[..stem_end], DocumentKind::Drawing.extension())
}

/// What sets a computed title block field (`COMPUTED_FIELDS`), named in any case; None for the
/// others.
fn computed_by(key: &str) -> Option<&'static str> {
    let tag = key.to_ascii_uppercase();
    COMPUTED_FIELDS.contains(&tag.as_str()).then_some(match tag.as_str() {
        "SIZE" => "drawing.sheet.set_size",
        "SCALE" => "the first base view's scale",
        _ => "the sheet's number and the sheet count",
    })
}

/// Sets the title block fields named in `given` (an object of `DrawingProps` fields, each text)
/// on `props`; the computed fields (`COMPUTED_FIELDS`) are not properties and are refused.
pub(crate) fn set_properties(props: &mut DrawingProps, given: &Value, cmd: &str) -> Result<()> {
    let Value::Object(given) = given else {
        return Err(crate::params::bad(cmd, "`properties` must be an object of title block fields"));
    };
    let mut current = serde_json::to_value(&*props).map_err(|e| EngineError::Other(format!("{cmd}: {e}")))?;
    let Some(fields) = current.as_object_mut() else { return Err(EngineError::Other(format!("{cmd}: the title block fields are not an object"))) };
    for (key, value) in given {
        if let Some(why) = computed_by(key) {
            return Err(crate::params::bad(cmd, format!("{} is computed ({why})", key.to_ascii_uppercase())));
        }
        if !fields.contains_key(key) {
            return Err(crate::params::bad(cmd, format!("unknown title block field {key}")));
        }
        let Some(text) = value.as_str() else { return Err(crate::params::bad(cmd, format!("`properties.{key}` must be text"))) };
        fields.insert(key.clone(), Value::String(text.to_string()));
    }
    *props = serde_json::from_value(current).map_err(|e| EngineError::Other(format!("{cmd}: {e}")))?;
    Ok(())
}
