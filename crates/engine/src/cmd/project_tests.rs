#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn folder(case: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tci-project-{case}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_project_saves_and_opens() {
    let dir = folder("save-open");
    let path = dir.join("porch.tciproj");
    let mut s = Session::default();
    run(&mut s, "project.new", json!({"name": "porch", "workspace": dir, "path": path, "frequently_used": ["frames"]}));
    assert!(path.exists(), "project.new writes the file");
    let mut t = Session::default();
    let got = run(&mut t, "project.open", json!({"path": path}));
    assert_eq!(got["name"], "porch");
    assert_eq!(got["path"], &*path.to_string_lossy());
    assert_eq!(PathBuf::from(got["workspace"].as_str().unwrap()), dir);
    assert_eq!(PathBuf::from(got["frequently_used"][0].as_str().unwrap()), dir.join("frames"));
    assert_eq!(t.project_file.as_ref().map(|p| p.name.as_str()), Some("porch"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_project_path_without_an_extension_gets_tciproj() {
    let dir = folder("ext");
    let mut s = Session::default();
    let out = run(&mut s, "project.new", json!({"name": "p", "workspace": ".", "path": dir.join("job")}));
    assert!(dir.join("job.tciproj").exists());
    assert_eq!(out["path"], &*dir.join("job.tciproj").to_string_lossy());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn project_new_writes_nothing_unless_name_workspace_and_path_are_all_given() {
    let dir = folder("partial");
    let path = dir.join("half.tciproj");
    let mut s = Session::default();
    for p in [json!({"path": path}), json!({"name": "n", "path": path}), json!({"workspace": ".", "path": path})] {
        assert!(s.execute("project.new", &p).is_err(), "{p}");
    }
    assert!(!path.exists());
    assert!(s.project_file.is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn opening_roots_a_relative_workspace_at_the_project_folder() {
    let dir = folder("relative");
    std::fs::create_dir_all(dir.join("parts")).unwrap();
    std::fs::write(dir.join("parts").join("leg.tcipart"), "{}").unwrap();
    let path = dir.join("job.tciproj");
    let mut s = Session::default();
    run(&mut s, "project.new", json!({"name": "job", "workspace": "parts", "path": path}));
    let mut t = Session::default();
    run(&mut t, "project.open", json!({"path": path}));
    let found = run(&mut t, "project.resolve", json!({"link": "leg.tcipart", "base": std::env::temp_dir()}));
    assert_eq!(PathBuf::from(found["path"].as_str().unwrap()), dir.join("parts").join("leg.tcipart"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn resolve_refuses_a_link_found_nowhere() {
    let dir = folder("missing");
    let path = dir.join("job.tciproj");
    let mut s = Session::default();
    run(&mut s, "project.new", json!({"name": "job", "workspace": dir, "path": path}));
    let err = s.execute("project.resolve", &json!({"link": "nope.tcipart"})).unwrap_err().to_string();
    assert!(err.contains("nope.tcipart"), "{err}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn resolve_without_a_project_is_refused() {
    let mut s = Session::default();
    let err = s.execute("project.resolve", &json!({"link": "leg.tcipart"})).unwrap_err().to_string();
    assert!(err.contains("no project"), "{err}");
}

#[test]
fn an_open_project_survives_new_and_open() {
    let dir = folder("survives");
    let design = dir.join("bracket.tcipart");
    let mut s = Session::default();
    run(&mut s, "document.new_part", json!({"name": "bracket"}));
    run(&mut s, "file.save_as", json!({"path": design}));
    run(&mut s, "project.new", json!({"name": "job", "workspace": dir, "path": dir.join("job.tciproj")}));
    for (id, p) in
        [("file.new", json!({})), ("document.new_assembly", json!({})), ("document.new_part", json!({})), ("doc.open", json!({"path": design}))]
    {
        run(&mut s, id, p);
        assert_eq!(s.project_file.as_ref().map(|p| p.name.as_str()), Some("job"), "after {id}");
    }
    assert_eq!(s.scratch().project_file, s.project_file, "a scratch copy sees the same project");
    let _ = std::fs::remove_dir_all(dir);
}
