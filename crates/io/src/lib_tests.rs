#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use solidcraft_doc::DocumentKind;

use super::*;

#[test]
fn every_kind_extension_is_a_design_path_and_format() {
    for ext in DocumentKind::EXTENSIONS {
        assert!(is_design_path(&format!("x.{ext}")), "{ext}");
        assert!(is_design_path(&format!("X.{}", ext.to_ascii_uppercase())), "{ext} in capitals");
        assert_eq!(Format::from_name(&format!("x.{ext}")).unwrap(), Format::Design, "{ext}");
        assert_eq!(formats::for_path(&format!("x.{ext}")).map(|f| f.id), Some("design"), "{ext}");
    }
}

#[test]
fn design_exts_are_the_kinds_then_the_legacy_extensions() {
    assert_eq!(DESIGN_EXTS[..3], DocumentKind::EXTENSIONS);
    assert_eq!(DESIGN_EXTS[3..], [DESIGN_EXT, LEGACY_DESIGN_EXT]);
}

#[test]
fn a_path_reports_the_kind_its_extension_names() {
    assert_eq!(kind_for_path("jobs/porch/leg.tcipart"), Some(DocumentKind::Part));
    assert_eq!(kind_for_path("porch.TCIASM"), Some(DocumentKind::Assembly));
    assert_eq!(kind_for_path("sheet.tcidrw"), Some(DocumentKind::Drawing));
}

#[test]
fn a_legacy_or_unknown_path_reports_no_kind() {
    assert_eq!(kind_for_path("old.solidcraft"), None);
    assert_eq!(kind_for_path("older.solvecraft"), None);
    assert_eq!(kind_for_path("plain.json"), None);
    assert_eq!(kind_for_path("noextension"), None);
}
