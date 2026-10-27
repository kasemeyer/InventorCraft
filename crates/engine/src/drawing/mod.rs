//! Drawings (Plan D): the helpers every `drawing.*` command shares, and the API the drawing
//! environment's UI uses. A drawing is a `Document` of kind `Drawing` whose `drawing` holds the
//! [`DrawingModel`]: its paper (a vendored CADCraft drawing, in inches), sheets, views and title
//! block fields.

pub mod template;

use std::sync::Arc;

use serde_json::Value;
use solidcraft_doc::{DocumentKind, DrawingModel, DrawingProps};

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

/// Runs `f` on a copy-on-write copy of the drawing and installs the edited document; then the
/// assoc re-solve (a no-op until Task 10). One command = one undo step through `undoable`:
/// `Session::execute` records the document `f` started from.
pub(crate) fn edit<T>(s: &mut Session, cmd: &str, f: impl FnOnce(&mut DrawingModel, &Session) -> Result<T>) -> Result<T> {
    require(s, cmd)?;
    let Some(mut model) = s.doc.drawing.clone() else { return Err(EngineError::Other(format!("{cmd}: {NOT_A_DRAWING}"))) };
    // The document still holds the model, so this copies it (the paper's entity stores are
    // copy-on-write: only what `f` touches is copied again).
    let out = f(Arc::make_mut(&mut model), s)?;
    s.doc_mut().drawing = Some(model);
    Ok(out)
}

/// `<model folder>/<model stem>.tcidrw` (owner default; §8.6). Only the extension changes, so
/// the folder is written as given, with its own separators.
pub fn default_drawing_path(model_path: &str) -> String {
    let name_start = model_path.rfind(['/', '\\']).map_or(0, |i| i + 1);
    let stem_end = model_path[name_start..].rfind('.').filter(|i| *i > 0).map_or(model_path.len(), |i| name_start + i);
    format!("{}.{}", &model_path[..stem_end], DocumentKind::Drawing.extension())
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
        if !fields.contains_key(key) {
            let known: Vec<&str> = fields.keys().map(String::as_str).collect();
            return Err(crate::params::bad(cmd, format!("no title block field `{key}` (the fields: {})", known.join(", "))));
        }
        let Some(text) = value.as_str() else { return Err(crate::params::bad(cmd, format!("`properties.{key}` must be text"))) };
        fields.insert(key.clone(), Value::String(text.to_string()));
    }
    *props = serde_json::from_value(current).map_err(|e| EngineError::Other(format!("{cmd}: {e}")))?;
    Ok(())
}
