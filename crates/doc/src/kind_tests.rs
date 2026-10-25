#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::Value;

use super::*;
use crate::Document;
use crate::format::{FORMAT, format_string, read};

#[test]
fn each_kind_has_its_own_extension() {
    assert_eq!(DocumentKind::Part.extension(), "tcipart");
    assert_eq!(DocumentKind::Assembly.extension(), "tciasm");
    assert_eq!(DocumentKind::Drawing.extension(), "tcidrw");
    assert_eq!(DocumentKind::EXTENSIONS, ["tcipart", "tciasm", "tcidrw"]);
}

#[test]
fn extensions_round_trip() {
    for k in DocumentKind::ALL {
        assert_eq!(DocumentKind::from_extension(k.extension()), Some(k));
    }
}

#[test]
fn extension_matching_ignores_case_and_a_leading_dot() {
    assert_eq!(DocumentKind::from_extension(".TCIPart"), Some(DocumentKind::Part));
    assert_eq!(DocumentKind::from_extension("TciAsm"), Some(DocumentKind::Assembly));
    assert_eq!(DocumentKind::from_extension(""), None);
}

#[test]
fn the_legacy_extensions_are_not_kinds() {
    assert_eq!(DocumentKind::from_extension("solidcraft"), None);
    assert_eq!(DocumentKind::from_extension("solvecraft"), None);
    assert_eq!(DocumentKind::from_extension("json"), None);
}

#[test]
fn kinds_serialise_in_snake_case() {
    assert_eq!(serde_json::to_value(DocumentKind::Part).unwrap(), "part");
    assert_eq!(serde_json::to_value(DocumentKind::Assembly).unwrap(), "assembly");
    assert_eq!(serde_json::from_value::<DocumentKind>("drawing".into()).unwrap(), DocumentKind::Drawing);
}

#[test]
fn a_new_document_is_an_assembly() {
    assert_eq!(DocumentKind::default(), DocumentKind::Assembly);
    assert_eq!(Document::new("x").kind, DocumentKind::Assembly);
    assert_eq!(Document::new_with_kind("x", DocumentKind::Part).kind, DocumentKind::Part);
    assert_eq!(Document::new_with_kind("x", DocumentKind::Part).name, "x");
}

/// A file as format 35 wrote it: no `kind`, unless `kind` is given; no `format_pending`.
fn format_35_file(kind: Option<&str>) -> String {
    let mut v: Value = serde_json::from_str(&Document::new_with_kind("old", DocumentKind::Part).to_json()).unwrap();
    let o = v.as_object_mut().unwrap();
    o.remove("format_pending");
    o.remove("kind");
    if let Some(k) = kind {
        o.insert("kind".into(), k.into());
    }
    o.insert("format".into(), format_string(35).into());
    v.to_string()
}

#[test]
fn a_design_from_before_kinds_opens_as_an_assembly() {
    assert_eq!(read(&format_35_file(None)).unwrap().kind, DocumentKind::Assembly);
}

#[test]
fn a_kind_in_the_file_is_kept() {
    assert_eq!(read(&format_35_file(Some("part"))).unwrap().kind, DocumentKind::Part);
}

#[test]
fn the_kind_is_saved_and_listed_as_a_pending_change() {
    let file: Value = serde_json::from_str(&Document::new_with_kind("p", DocumentKind::Part).to_json()).unwrap();
    assert_eq!(file["kind"], "part");
    assert_eq!(file["format"], format_string(FORMAT));
    assert!(file["format_pending"].as_array().unwrap().iter().any(|n| n == "document-kind"), "{file}");
    assert_eq!(read(&file.to_string()).unwrap().kind, DocumentKind::Part);
}

#[test]
fn an_assembly_is_saved_with_its_kind_too() {
    let file: Value = serde_json::from_str(&Document::new("a").to_json()).unwrap();
    assert_eq!(file["kind"], "assembly", "the kind is always written, never left to the default");
}
