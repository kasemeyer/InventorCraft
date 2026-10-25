//! Creating and reporting typed documents; a document's kind is fixed when it is created.

use serde_json::{Value, json};
use solidcraft_doc::DocumentKind;

use super::CommandSpec;
use crate::params::str_;
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("document.new_part", "New Part", new_part).icon("new").noundo().params("name?: the design's name (default: Part)"),
    CommandSpec::new("document.new_assembly", "New Assembly", new_assembly)
        .icon("new")
        .noundo()
        .params("name?: the design's name (default: Assembly)"),
    CommandSpec::new("document.kind", "Document Kind", kind).icon("inspect").noundo().doc_only().params("→ kind: part|assembly|drawing, extension"),
];

fn new_part(s: &mut Session, p: &Value) -> Result<Value> {
    Ok(super::file::start_design(s, str_(p, "name").unwrap_or("Part"), DocumentKind::Part))
}

fn new_assembly(s: &mut Session, p: &Value) -> Result<Value> {
    Ok(super::file::start_design(s, str_(p, "name").unwrap_or("Assembly"), DocumentKind::Assembly))
}

fn kind(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!({"kind": s.doc.kind, "extension": s.doc.kind.extension()}))
}

#[cfg(test)]
#[path = "document_kind_tests.rs"]
mod tests;
