//! The drawing commands (`drawing.*`, Plan D). Each one works on the active Drawing document
//! through `crate::drawing::{require, edit}`, so it fails cleanly on a part or an assembly and is
//! one undo step. Paper coordinates are inches, given as `[x, y]`.

use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_doc::{AnsiSize, Document, DocumentKind, DrawingModel, DrawingProps};

use super::CommandSpec;
use crate::drawing::{self, template};
use crate::params::bad;
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("drawing.new", "New Drawing", new_drawing).icon("new").noundo().params(
    "model?: a part or assembly (.tcipart/.tciasm) whose name, file name and material fill the title block (a relative path is taken from the open design's folder); \
     template?: ansi_inch (the only one); size?: A|B|C|D|E (default D); landscape?: bool (default true); \
     properties?: {title, company, dwg_no, rev, drawn_by, drawn_date, checked_by, checked_date, approved_by, approved_date, material} (text, applied after the defaults) \
     → kind, sheets, size: a new, unsaved drawing with one sheet (our border and title block)",
    ),
    CommandSpec::new("drawing.sheet.add", "New Sheet", sheet_add).icon("new").params(
        "size?: A|B|C|D|E and landscape?: bool (default: the active sheet's) → sheet: the new sheet's index (0-based), now the active sheet; every title block's SHEET is renumbered",
    ),
    CommandSpec::new("drawing.sheet.activate", "Activate Sheet", sheet_activate)
        .icon("activate")
        .noundo()
        .params("sheet: the sheet's index (0-based) → sheet: the active sheet (view state, no undo step)"),
    CommandSpec::new("drawing.sheet.set_size", "Sheet Size", sheet_set_size).icon("settings").params(
        "sheet?: index (0-based; default the active sheet); size: A|B|C|D|E; landscape?: bool (default the sheet's) → sheet, size, landscape: the page, border and title block follow the new size",
    ),
    CommandSpec::new("drawing.sheet.set_name", "Sheet Name", sheet_set_name).icon("settings").params(
        "sheet?: index (0-based; default the active sheet); name: text (not empty, at most 100 characters; the browser shows <name>:<n>) → sheet, name",
    ),
    CommandSpec::new("drawing.properties.set", "Drawing Properties", properties_set).icon("settings").params(
        "title?, company?, dwg_no?, rev?, drawn_by?, drawn_date?, checked_by?, checked_date?, approved_by?, approved_date?, material?: text → properties: every title block field; SIZE, SCALE and SHEET are computed and refused",
    ),
];

/// The `sheet` param: an index of an existing sheet of `m`.
fn sheet_index(m: &DrawingModel, p: &Value, cmd: &str) -> Result<usize> {
    let count = m.sheets.len();
    let n = match p.get("sheet") {
        Some(Value::Number(n)) => n.as_u64().ok_or_else(|| bad(cmd, "`sheet` must be a sheet index (0, 1, ...)"))?,
        _ => return Err(bad(cmd, "`sheet` must be a sheet index (0, 1, ...)")),
    };
    usize::try_from(n).ok().filter(|i| *i < count).ok_or_else(|| bad(cmd, format!("sheet {n} does not exist ({count} sheets)")))
}

/// The active sheet's index (`Session::drawing_sheet`, kept inside the drawing's sheets).
fn active_sheet(s: &Session, m: &DrawingModel) -> usize {
    s.drawing_sheet.min(m.sheets.len().saturating_sub(1))
}

/// The optional `size` and `landscape` params, each defaulting to the given value.
fn size_params(p: &Value, cmd: &str, size: AnsiSize, landscape: bool) -> Result<(AnsiSize, bool)> {
    let size = match p.get("size") {
        None | Some(Value::Null) => size,
        Some(v) => v.as_str().and_then(AnsiSize::parse).ok_or_else(|| bad(cmd, "size must be A, B, C, D or E"))?,
    };
    let landscape = match p.get("landscape") {
        None | Some(Value::Null) => landscape,
        Some(v) => v.as_bool().ok_or_else(|| bad(cmd, "`landscape` must be true or false"))?,
    };
    Ok((size, landscape))
}

/// Appends a sheet the size of the active one (R7: it becomes the active sheet).
fn sheet_add(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.sheet.add";
    let m = drawing::require(s, cmd)?;
    let (size, landscape) = match m.sheets.get(active_sheet(s, m)) {
        Some(sheet) => (sheet.size, sheet.landscape),
        None => (AnsiSize::D, true), // (owner default; §8.1)
    };
    let (size, landscape) = size_params(p, cmd, size, landscape)?;
    let index = drawing::edit(s, cmd, |m, _| {
        let index = template::add_sheet(m, size, landscape);
        template::refresh_title_blocks(m);
        Ok(index)
    })?;
    s.drawing_sheet = index;
    Ok(json!({"sheet": index}))
}

fn sheet_activate(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.sheet.activate";
    let index = sheet_index(drawing::require(s, cmd)?, p, cmd)?;
    s.drawing_sheet = index;
    Ok(json!({"sheet": index}))
}

fn sheet_set_size(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.sheet.set_size";
    let m = drawing::require(s, cmd)?;
    let index = match p.get("sheet") {
        None | Some(Value::Null) => active_sheet(s, m),
        Some(_) => sheet_index(m, p, cmd)?,
    };
    let Some(sheet) = m.sheets.get(index) else { return Err(bad(cmd, "the drawing has no sheets")) };
    if p.get("size").is_none_or(Value::is_null) {
        return Err(bad(cmd, "size must be A, B, C, D or E"));
    }
    let (size, landscape) = size_params(p, cmd, sheet.size, sheet.landscape)?;
    drawing::edit(s, cmd, |m, _| {
        template::resize_sheet(m, index, size, landscape).map_err(|e| EngineError::Other(format!("{cmd}: {e}")))?;
        template::refresh_title_blocks(m);
        Ok(())
    })?;
    Ok(json!({"sheet": index, "size": size.letter(), "landscape": landscape}))
}

/// Most characters of a sheet's name.
const MAX_SHEET_NAME: usize = 100;

/// `drawing.sheet.set_name` (Edit Sheet's Name, R104): one undo step.
fn sheet_set_name(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.sheet.set_name";
    let m = drawing::require(s, cmd)?;
    let index = match p.get("sheet") {
        None | Some(Value::Null) => active_sheet(s, m),
        Some(_) => sheet_index(m, p, cmd)?,
    };
    if m.sheets.get(index).is_none() {
        return Err(bad(cmd, "the drawing has no sheets"));
    }
    let name = p.get("name").and_then(Value::as_str).map(str::trim).ok_or_else(|| bad(cmd, "`name` must be text"))?;
    if name.is_empty() {
        return Err(bad(cmd, "a sheet needs a name"));
    }
    if name.chars().count() > MAX_SHEET_NAME || name.chars().any(char::is_control) {
        return Err(bad(cmd, format!("a sheet's name is at most {MAX_SHEET_NAME} characters on one line")));
    }
    let name = name.to_string();
    let shown = name.clone();
    drawing::edit(s, cmd, |m, _| {
        if let Some(sheet) = m.sheets.get_mut(index) {
            sheet.name = name;
        }
        Ok(())
    })?;
    Ok(json!({"sheet": index, "name": shown}))
}

fn properties_set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.properties.set";
    let mut props = drawing::require(s, cmd)?.properties.clone();
    if p.as_object().is_none_or(serde_json::Map::is_empty) {
        return Err(bad(cmd, "name at least one title block field, e.g. {\"title\": \"BRACKET\"}"));
    }
    drawing::set_properties(&mut props, p, "", cmd)?;
    let shown = serde_json::to_value(&props).map_err(|e| EngineError::Other(format!("{cmd}: {e}")))?;
    drawing::edit(s, cmd, |m, _| {
        m.properties = props;
        template::refresh_title_blocks(m);
        Ok(())
    })?;
    Ok(json!({"properties": shown}))
}

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
        drawing::set_properties(&mut props, given, "properties.", cmd)?;
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

#[cfg(test)]
#[path = "drawing_sheet_tests.rs"]
mod drawing_sheet_tests;

#[cfg(test)]
#[path = "drawing_edges_tests.rs"]
mod drawing_edges_tests;

#[cfg(test)]
#[path = "drawing_projection_tests.rs"]
mod drawing_projection_tests;

#[cfg(test)]
#[path = "drawing_script_tests.rs"]
mod drawing_script_tests;
