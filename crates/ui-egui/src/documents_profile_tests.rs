#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use serde_json::json;
use solidcraft_engine::doc::DocumentKind::{Assembly, Part};
use solidcraft_engine::doc::ProjectFile;

use super::*;
use crate::prefs::parse_length_mm;
use crate::profile;
use crate::scenario::Harness;
use crate::test_support::inventor;
use crate::toolbar::FileAction;

/// Ctrl+N (Cmd+N on macOS), through the control channel's key path as the scenarios press it.
fn ctrl_n(h: &mut Harness) {
    h.step(&json!({"key": "N", "cmd": true})).unwrap();
    h.frames(2);
}

/// A first feature, so the active design is no longer blank and a new one opens a tab.
fn first_feature(h: &mut Harness) {
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.frames(2);
    assert!(!active_is_blank(&h.app));
}

#[test]
fn ctrl_n_makes_a_part_under_inventor() {
    let mut h = inventor(Part);
    first_feature(&mut h);
    let before = h.app.docs.count();
    ctrl_n(&mut h);
    assert_eq!(h.app.docs.count(), before + 1);
    assert_eq!(h.app.session.doc.kind, Part);
    assert!(!h.app.session.is_dirty());
    assert_eq!(h.app.ui.tab, "MODEL");
}

#[test]
fn ctrl_n_keeps_the_upstream_default_under_solidcraft() {
    let mut h = Harness::new();
    first_feature(&mut h);
    let before = h.app.docs.count();
    ctrl_n(&mut h);
    assert_eq!(h.app.docs.count(), before + 1);
    assert_eq!(h.app.session.doc.kind, Assembly);
}

#[test]
fn file_menu_new_assembly_opens_an_assembly_tab() {
    let mut h = inventor(Part);
    FileAction::NewAssembly(None).run(&mut h.app);
    h.frame();
    assert_eq!(h.app.session.doc.kind, Assembly);
    assert_eq!(h.app.ui.tab, profile::ribbon(&h.app).home());
    assert_eq!(h.app.ui.tab, "ASSEMBLE");
}

#[test]
fn a_blank_design_is_reused_with_the_new_kind() {
    let mut h = inventor(Assembly);
    assert!(active_is_blank(&h.app));
    let before = h.app.docs.count();
    new_document(&mut h.app, Part, None);
    assert_eq!(h.app.docs.count(), before);
    assert_eq!(h.app.session.doc.kind, Part);
    assert!(!h.app.session.is_dirty());
}

#[test]
fn a_new_tab_keeps_the_open_project() {
    let mut h = inventor(Part);
    let project = Arc::new(ProjectFile::default());
    h.app.session.project_file = Some(project.clone());
    first_feature(&mut h);
    let before = h.app.docs.count();
    new_document(&mut h.app, Part, None);
    assert_eq!(h.app.docs.count(), before + 1);
    let now = h.app.session.project_file.as_ref().expect("the new tab has the open project");
    assert!(Arc::ptr_eq(now, &project));
}

#[test]
fn the_inch_template_sets_inches_without_dirtying() {
    let mut h = inventor(Part);
    first_feature(&mut h);
    assert_eq!(h.app.preferences.default_units, "mm");
    new_document(&mut h.app, Part, Some("in"));
    assert_eq!(h.app.session.doc.units, "in");
    assert!(!h.app.session.is_dirty());
    assert!(h.app.session.undo.is_empty());
    assert_eq!(h.app.preferences.default_units, "mm");
}

#[test]
fn the_mm_template_overrides_an_inch_preference() {
    let mut h = inventor(Part);
    h.app.preferences.default_units = "in".into();
    FileAction::NewAssembly(Some("mm")).run(&mut h.app);
    assert_eq!(h.app.session.doc.kind, Assembly);
    assert_eq!(h.app.session.doc.units, "mm");
    assert_eq!(h.app.preferences.default_units, "in");
}

#[test]
fn ctrl_n_uses_the_preferred_units() {
    let mut h = inventor(Part);
    h.app.preferences.default_units = "in".into();
    first_feature(&mut h);
    ctrl_n(&mut h);
    assert_eq!(h.app.session.doc.kind, Part);
    assert_eq!(h.app.session.doc.units, "in");
}

#[test]
fn fields_accept_any_unit_in_an_inch_part() {
    let mut h = inventor(Part);
    first_feature(&mut h);
    new_document(&mut h.app, Part, Some("in"));
    let (k, _) = crate::prefs::unit_scale(&h.app.session.doc.units);
    let near = |text: &str, mm: f64| {
        let got = parse_length_mm(text, k).unwrap_or_else(|| panic!("{text} does not parse"));
        assert!((got - mm).abs() < 1e-9, "{text}: {got} mm, expected {mm}");
    };
    near("2", 50.8);
    near("50 mm", 50.0);
    near("1 in + 2 mm", 27.4);
}

/// A project written by `project.new` (`tag` keeps the tests' files apart).
fn project_path(tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!("tci-app-project-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("job.tciproj").to_string_lossy().into_owned()
}

/// Two open tabs: A (index 0, a feature) and B (index 1, active).
fn two_tabs() -> Harness {
    let mut h = inventor(Part);
    first_feature(&mut h);
    new_document(&mut h.app, Part, None);
    assert_eq!((h.app.docs.count(), h.app.docs.active), (2, 1));
    h
}

#[test]
fn a_project_made_in_one_tab_is_open_in_the_others() {
    let mut h = two_tabs();
    let path = project_path("new");
    h.app.run("project.new", json!({"name": "Job", "workspace": ".", "path": path, "overwrite": true})).unwrap();
    let project = h.app.session.project_file.clone().expect("project.new opens the project");
    switch(&mut h.app, 0);
    assert_eq!(h.app.docs.active, 0);
    assert!(Arc::ptr_eq(h.app.session.project_file.as_ref().expect("tab A has the project"), &project));
    switch(&mut h.app, 1);
    assert!(Arc::ptr_eq(h.app.session.project_file.as_ref().unwrap(), &project));
}

#[test]
fn a_project_opened_in_a_background_tab_replaces_the_old_one_everywhere() {
    let mut h = two_tabs();
    let (old, new) = (project_path("old"), project_path("opened"));
    h.app.run("project.new", json!({"name": "Old", "workspace": ".", "path": old, "overwrite": true})).unwrap();
    switch(&mut h.app, 0);
    h.app.run("project.new", json!({"name": "New", "workspace": ".", "path": new, "overwrite": true})).unwrap();
    h.app.run("project.open", json!({"path": new})).unwrap();
    let opened = h.app.session.project_file.clone().unwrap();
    assert_eq!(opened.name, "New");
    // B was left holding Old: on activation it shows the project opened in A.
    switch(&mut h.app, 1);
    let b = h.app.session.project_file.as_ref().expect("tab B has the project");
    assert!(Arc::ptr_eq(b, &opened));
    assert_eq!(b.name, "New");
}
