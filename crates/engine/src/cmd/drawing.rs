//! The drawing commands (`drawing.*`, Plan D). Each one works on the active Drawing document
//! through `crate::drawing::{require, edit}`, so it fails cleanly on a part or an assembly and is
//! one undo step. Paper coordinates are inches, given as `[x, y]`.

use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_doc::{AnsiSize, Document, DocumentKind, DrawingProps};

use super::CommandSpec;
use crate::drawing::{self, template};
use crate::params::bad;
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("drawing.new", "New Drawing", new_drawing).icon("new").noundo().params(
    "model?: a part or assembly (.tcipart/.tciasm) whose name, file name and material fill the title block (a relative path is taken from the open design's folder); \
     template?: ansi_inch (the only one); size?: A|B|C|D|E (default D); landscape?: bool (default true); \
     properties?: {title, company, dwg_no, rev, drawn_by, drawn_date, checked_by, checked_date, approved_by, approved_date, material} (text, applied after the defaults) \
     → kind, sheets, size: a new, unsaved drawing with one sheet (our border and title block)",
)];

/// What a drawing's title block takes from its model: the document's name, the file's stem and
/// the material (spec §4.9).
struct ModelFacts {
    title: String,
    dwg_no: String,
    material: String,
}

/// Reads the model a new drawing is made from: a part or an assembly that exists.
fn model_facts(s: &Session, given: &str, cmd: &str) -> Result<ModelFacts> {
    const NOT_A_MODEL: &str = "a drawing view shows a part or an assembly (.tcipart or .tciasm), not a drawing";
    if given.trim().is_empty() || given.len() >= 4096 {
        return Err(bad(cmd, "`model` must be the path of a part or an assembly"));
    }
    if solidcraft_io::kind_for_path(given) == Some(DocumentKind::Drawing) {
        return Err(bad(cmd, NOT_A_MODEL));
    }
    let file = super::component_link::resolve(s.path.as_deref(), given);
    let shown = file.to_string_lossy().into_owned();
    if !file.is_file() {
        return Err(EngineError::Other(format!("{cmd}: model not found: {shown}")));
    }
    let (doc, _) = solidcraft_io::read_design_file(&shown, None::<fn(solidcraft_io::DesignPreview)>)
        .map_err(|e| EngineError::Other(format!("{cmd}: {shown}: {e}")))?;
    if doc.kind == DocumentKind::Drawing {
        return Err(bad(cmd, NOT_A_MODEL));
    }
    let stem = Path::new(&shown).file_stem().map(|x| x.to_string_lossy().trim().to_string()).unwrap_or_default();
    // A design still under the default name takes its file's, as `doc.open` shows it.
    let name = doc.name.trim();
    let title = if name.is_empty() || name == super::file::DEFAULT_NAME { stem.clone() } else { name.to_string() };
    // The model's material when it has one: every body that names a material names the same one.
    let mut materials: Vec<&String> = doc.materials.values().collect();
    materials.sort();
    materials.dedup();
    let material = match materials.as_slice() {
        [one] => one.to_string(),
        _ => String::new(),
    };
    Ok(ModelFacts { title, dwg_no: stem, material })
}

fn new_drawing(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.new";
    // Everything is read and checked first, so a refused call leaves the design as it is.
    match p.get("template") {
        None | Some(Value::Null) => {}
        Some(v) if v.as_str().is_some_and(|t| t.trim().eq_ignore_ascii_case("ansi_inch")) => {}
        Some(_) => return Err(bad(cmd, "`template` must be \"ansi_inch\" (the only template)")),
    }
    let size = match p.get("size") {
        None | Some(Value::Null) => AnsiSize::D, // (owner default; §8.1)
        Some(v) => v.as_str().and_then(AnsiSize::parse).ok_or_else(|| bad(cmd, "size must be A, B, C, D or E"))?,
    };
    let landscape = match p.get("landscape") {
        None | Some(Value::Null) => true, // (owner default; §8.1)
        Some(v) => v.as_bool().ok_or_else(|| bad(cmd, "`landscape` must be true or false"))?,
    };
    let mut props = DrawingProps { drawn_by: template::os_user(), drawn_date: template::today(), ..DrawingProps::default() };
    match p.get("model") {
        None | Some(Value::Null) => {}
        Some(v) => {
            let given = v.as_str().ok_or_else(|| bad(cmd, "`model` must be the path of a part or an assembly"))?;
            let facts = model_facts(s, given, cmd)?;
            props.title = facts.title;
            props.dwg_no = facts.dwg_no;
            props.material = facts.material;
        }
    }
    if let Some(given) = p.get("properties").filter(|v| !v.is_null()) {
        drawing::set_properties(&mut props, given, cmd)?;
    }
    let mut doc = Document::new_with_kind(super::file::DEFAULT_NAME, DocumentKind::Drawing);
    doc.drawing = Some(Arc::new(template::new_model()));
    s.replace_design(Session::new(doc));
    drawing::edit(s, cmd, |m, _| {
        template::add_sheet(m, size, landscape);
        m.properties = props;
        template::refresh_title_blocks(m);
        Ok(())
    })?;
    // A new drawing is clean, as a new part is (`document.new_part`).
    s.mark_saved();
    Ok(json!({"kind": DocumentKind::Drawing, "sheets": 1, "size": size.letter()}))
}

#[cfg(test)]
#[path = "drawing_fixture.rs"]
pub(crate) mod drawing_fixture;

#[cfg(test)]
#[path = "drawing_new_tests.rs"]
mod drawing_new_tests;
