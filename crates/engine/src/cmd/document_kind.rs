//! Creating and reporting typed documents; a document's kind is fixed when it is created.

use serde_json::{Value, json};
use solidcraft_doc::DocumentKind;

use super::CommandSpec;
use crate::params::str_;
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("document.new_part", "New Part", new_part)
        .icon("new")
        .noundo()
        .params("name?: the design's name (default: Untitled, which takes the file's name when saved)"),
    CommandSpec::new("document.new_assembly", "New Assembly", new_assembly)
        .icon("new")
        .noundo()
        .params("name?: the design's name (default: Untitled, which takes the file's name when saved)"),
    CommandSpec::new("document.kind", "Document Kind", kind).icon("inspect").noundo().doc_only().params("→ kind: part|assembly|drawing, extension"),
];

fn new_part(s: &mut Session, p: &Value) -> Result<Value> {
    Ok(super::file::start_design(s, str_(p, "name").unwrap_or(super::file::DEFAULT_NAME), DocumentKind::Part))
}

fn new_assembly(s: &mut Session, p: &Value) -> Result<Value> {
    Ok(super::file::start_design(s, str_(p, "name").unwrap_or(super::file::DEFAULT_NAME), DocumentKind::Assembly))
}

/// Whether this design may gain new linked components: only an assembly may (a part holds bodies
/// and no component links; one that already holds some keeps them). The commands that create a
/// link are enabled by it.
pub(super) fn needs_assembly(s: &Session) -> std::result::Result<(), String> {
    if s.doc.kind == DocumentKind::Assembly {
        Ok(())
    } else {
        Err("only an assembly can hold linked components: make a new assembly (document.new_assembly) and insert this design into it".into())
    }
}

fn kind(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!({"kind": s.doc.kind, "extension": s.doc.kind.extension()}))
}

#[cfg(test)]
#[path = "document_kind_tests.rs"]
mod tests;
