//! Drawing output (spec §4.7): `drawing.export.pdf` (one page per sheet, shaded views as pictures
//! under their curves) and `drawing.export.dxf` (one sheet's paper space at R2018 or R2000).
//! Exporting writes a file and changes nothing in the document (no undo step).

use serde_json::{Value, json};

use super::CommandSpec;
use crate::drawing::{self, export};
use crate::params::bad;
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("drawing.export.pdf", "Export Drawing as PDF", export_pdf).icon("export").noundo().params(
        "path: the .pdf file to write; sheets?: [index, ...] (0-based; default every sheet, in order) → path, pages, images, bytes: one page per sheet at its ANSI size (D = 2448 x 1584 pt); shaded views as pictures under their curves",
    ),
    CommandSpec::new("drawing.export.dxf", "Export Drawing as DXF", export_dxf).icon("dxf").noundo().params(
        "path: the .dxf file to write; sheet?: index (0-based; default the active sheet); version?: R2018|R2000 (default R2018) → path, sheet, version, entities, bytes: the sheet's paper space (no pictures; what the view styles suppress is left out)",
    ),
];

/// The `path` param, checked for writing.
fn path<'a>(p: &'a Value, cmd: &str) -> Result<&'a str> {
    let path =
        p.get("path").and_then(Value::as_str).filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(cmd, "path is required"))?;
    solidcraft_io::vfs::check(path).map_err(|e| EngineError::Other(format!("{cmd}: {e}")))?;
    Ok(path)
}

fn write(path: &str, bytes: &[u8], cmd: &str) -> Result<()> {
    solidcraft_io::vfs::write(path, bytes).map_err(|e| EngineError::Other(format!("{cmd}: {path}: {e}")))
}

/// A sheet index of a drawing of `count` sheets.
fn sheet_of(v: &Value, count: usize, cmd: &str) -> Result<usize> {
    let n = v.as_u64().ok_or_else(|| bad(cmd, "a sheet is an index (0, 1, ...)"))?;
    usize::try_from(n).ok().filter(|i| *i < count).ok_or_else(|| bad(cmd, format!("sheet {n} does not exist ({count} sheets)")))
}

fn export_pdf(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.export.pdf";
    let m = drawing::require(s, cmd)?;
    let path = path(p, cmd)?;
    let sheets: Vec<usize> = match p.get("sheets") {
        None | Some(Value::Null) => (0..m.sheets.len()).collect(),
        Some(Value::Array(list)) if !list.is_empty() => list.iter().map(|v| sheet_of(v, m.sheets.len(), cmd)).collect::<Result<_>>()?,
        Some(_) => return Err(bad(cmd, "`sheets` must be a list of sheet indices")),
    };
    let out = export::pdf(s, m, &sheets, &s.doc.name)?;
    write(path, &out.bytes, cmd)?;
    Ok(json!({"path": path, "pages": sheets.len(), "images": out.images, "bytes": out.bytes.len()}))
}

fn export_dxf(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "drawing.export.dxf";
    let m = drawing::require(s, cmd)?;
    let path = path(p, cmd)?;
    let sheet = match p.get("sheet") {
        None | Some(Value::Null) => s.drawing_sheet.min(m.sheets.len().saturating_sub(1)),
        Some(v) => sheet_of(v, m.sheets.len(), cmd)?,
    };
    let (version, name) = match p.get("version").filter(|v| !v.is_null()).map(Value::as_str) {
        None | Some(Some("R2018")) => (cadcraft_io::Version::R2018, "R2018"),
        Some(Some("R2000")) => (cadcraft_io::Version::R2000, "R2000"),
        Some(_) => return Err(bad(cmd, "version must be R2018 or R2000")),
    };
    // One copy of the sheet's paper: what is counted is what is written.
    let paper = export::sheet_paper(m, sheet)?;
    let entities = paper.layouts.iter().map(|l| l.entities.len()).sum::<usize>();
    let bytes = export::dxf(&paper, version)?;
    write(path, &bytes, cmd)?;
    Ok(json!({"path": path, "sheet": sheet, "version": name, "entities": entities, "bytes": bytes.len()}))
}

#[cfg(test)]
#[path = "drawing_export_tests.rs"]
mod drawing_export_tests;
