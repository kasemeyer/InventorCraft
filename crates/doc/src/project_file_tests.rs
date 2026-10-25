#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use super::*;

fn root() -> PathBuf {
    std::env::temp_dir().join("tci-project-model")
}

fn sample() -> ProjectFile {
    ProjectFile {
        name: "Power Tong Assembly".into(),
        workspace: root().join("porch").to_string_lossy().into_owned(),
        libraries: vec![root().join("standard-parts").to_string_lossy().into_owned()],
        frequently_used: vec!["frames".into()],
    }
}

#[test]
fn beside_the_design_is_tried_before_the_workspace_then_the_libraries() {
    let got = sample().candidates(&root().join("porch").join("frames"), "leg.tcipart");
    assert_eq!(
        got,
        vec![
            root().join("porch").join("frames").join("leg.tcipart"),
            root().join("porch").join("leg.tcipart"),
            root().join("standard-parts").join("leg.tcipart"),
        ]
    );
}

#[test]
fn libraries_are_tried_in_order_after_the_workspace() {
    let mut p = sample();
    p.libraries.push(root().join("second").to_string_lossy().into_owned());
    let got = p.candidates(&root().join("elsewhere"), "bolt.tcipart");
    assert_eq!(got.len(), 4);
    assert_eq!(got[2], root().join("standard-parts").join("bolt.tcipart"));
    assert_eq!(got[3], root().join("second").join("bolt.tcipart"));
}

#[test]
fn a_folder_that_is_both_base_and_workspace_is_tried_once() {
    let got = sample().candidates(&root().join("porch"), "leg.tcipart");
    assert_eq!(got, vec![root().join("porch").join("leg.tcipart"), root().join("standard-parts").join("leg.tcipart")]);
}

#[test]
fn an_absolute_link_is_its_own_only_candidate() {
    let abs = root().join("somewhere").join("leg.tcipart");
    let got = sample().candidates(&root().join("porch"), &abs.to_string_lossy());
    assert_eq!(got, vec![abs]);
}

#[test]
fn a_slash_separated_link_resolves_on_every_os() {
    let got = sample().candidates(&root().join("porch"), "frames/leg.tcipart");
    assert_eq!(got[0], root().join("porch").join("frames").join("leg.tcipart"));
}

#[test]
fn resolve_picks_the_first_candidate_that_exists() {
    let p = sample();
    let in_library = root().join("standard-parts").join("leg.tcipart");
    let found = p.resolve(&root().join("porch").join("frames"), "leg.tcipart", |c: &Path| c == in_library);
    assert_eq!(found, Some(in_library));
    assert_eq!(p.resolve(&root(), "leg.tcipart", |_: &Path| false), None);
}

#[test]
fn a_relative_workspace_is_rooted_at_the_project_folder() {
    let p = ProjectFile { name: "t".into(), workspace: ".".into(), libraries: vec!["../lib".into()], frequently_used: vec!["frames".into()] };
    let dir = root().join("job");
    let r = p.rooted(&dir);
    assert_eq!(Path::new(&r.workspace), dir.join("."));
    assert_eq!(Path::new(&r.libraries[0]), dir.join("../lib"));
    assert_eq!(Path::new(&r.frequently_used[0]), dir.join("frames"));
    let abs = sample();
    assert_eq!(abs.rooted(&dir).workspace, abs.workspace, "an absolute workspace is kept");
}

#[test]
fn a_project_round_trips_through_json() {
    let p = sample();
    assert_eq!(ProjectFile::from_json(&p.to_json()).unwrap(), p);
    let bare = ProjectFile { name: "b".into(), workspace: ".".into(), ..Default::default() };
    let text = bare.to_json();
    assert!(!text.contains("libraries") && !text.contains("frequently_used"), "{text}");
    assert_eq!(ProjectFile::from_json(&text).unwrap(), bare);
}

#[test]
fn a_damaged_project_file_is_refused() {
    for text in ["", "[]", "{\"name\": 3}", "{\"workspace\": \".\"}"] {
        assert!(ProjectFile::from_json(text).is_err(), "{text:?}");
    }
}
