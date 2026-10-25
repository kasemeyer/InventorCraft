#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use solidcraft_doc::DocumentKind;

use crate::{EngineError, Session};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn folder(case: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tci-doc-kind-{case}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn kind(s: &mut Session) -> Value {
    run(s, "document.kind", json!({}))
}

#[test]
fn a_new_part_is_a_part() {
    let mut s = Session::default();
    assert_eq!(run(&mut s, "document.new_part", json!({"name": "leg"}))["kind"], "part");
    assert_eq!(kind(&mut s), json!({"kind": "part", "extension": "tcipart"}));
    assert_eq!(s.doc.name, "leg");
}

#[test]
fn a_new_assembly_is_an_assembly() {
    let mut s = Session::default();
    run(&mut s, "document.new_assembly", json!({"name": "porch"}));
    assert_eq!(kind(&mut s), json!({"kind": "assembly", "extension": "tciasm"}));
    assert_eq!(s.doc.name, "porch");
}

#[test]
fn a_new_document_is_clean_at_birth() {
    for (id, p) in [("document.new_part", json!({})), ("document.new_assembly", json!({})), ("file.new", json!({"kind": "assembly"}))] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
        assert!(s.is_dirty());
        run(&mut s, id, p);
        assert!(!s.is_dirty(), "{id} must not open dirty");
        assert!(s.undo.is_empty(), "{id}");
    }
}

#[test]
fn a_default_session_is_an_assembly() {
    assert_eq!(Session::default().doc.kind, DocumentKind::Assembly);
}

#[test]
fn file_new_makes_a_part_unless_told_otherwise() {
    let mut s = Session::default();
    run(&mut s, "file.new", json!({}));
    assert_eq!(s.doc.kind, DocumentKind::Part);
    run(&mut s, "file.new", json!({"kind": "assembly"}));
    assert_eq!(s.doc.kind, DocumentKind::Assembly);
    for bad in [json!("drawing"), json!("widget"), json!(7)] {
        let err = s.execute("file.new", &json!({"kind": bad})).unwrap_err();
        assert!(matches!(err, EngineError::BadParams { .. }), "{bad}: {err}");
    }
    assert_eq!(s.doc.kind, DocumentKind::Assembly, "a refused file.new leaves the design");
}

#[test]
fn saving_without_an_extension_uses_the_kinds_own() {
    let dir = folder("save-as");
    let mut s = Session::default();
    run(&mut s, "document.new_assembly", json!({"name": "porch"}));
    let out = run(&mut s, "file.save_as", json!({"path": dir.join("porch")}));
    let written = dir.join("porch.tciasm");
    assert_eq!(out["path"], &*written.to_string_lossy());
    assert!(written.exists());
    assert_eq!(s.path.as_deref(), Some(&*written.to_string_lossy()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn file_save_without_an_extension_uses_the_kinds_own() {
    let dir = folder("save");
    let mut s = Session::default();
    run(&mut s, "document.new_part", json!({"name": "leg"}));
    run(&mut s, "file.save", json!({"path": dir.join("leg")}));
    assert!(dir.join("leg.tcipart").exists());
    assert!(!dir.join("leg").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn saving_under_another_kinds_extension_is_refused() {
    let dir = folder("mismatch");
    let mut s = Session::default();
    run(&mut s, "document.new_part", json!({"name": "leg"}));
    let wrong = dir.join("leg.tciasm");
    let err = s.execute("file.save_as", &json!({"path": wrong})).unwrap_err();
    assert!(matches!(err, EngineError::BadParams { .. }), "{err}");
    let msg = err.to_string();
    assert!(msg.contains("Part") && msg.contains("Assembly"), "names both kinds: {msg}");
    assert!(!wrong.exists());
    assert!(s.path.is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn legacy_extension_is_written_as_given() {
    let dir = folder("legacy-save");
    let mut s = Session::default();
    run(&mut s, "document.new_part", json!({"name": "old"}));
    run(&mut s, "file.save_as", json!({"path": dir.join("old.solidcraft")}));
    assert!(dir.join("old.solidcraft").exists(), "a legacy extension is never rewritten");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn opening_a_typed_file_restores_its_kind() {
    let dir = folder("reopen");
    let path = dir.join("porch.tciasm");
    let mut s = Session::default();
    run(&mut s, "document.new_assembly", json!({"name": "porch"}));
    run(&mut s, "file.save_as", json!({"path": path}));
    run(&mut s, "document.new_part", json!({}));
    run(&mut s, "doc.open", json!({"path": path}));
    assert_eq!(kind(&mut s)["kind"], "assembly");
    assert!(!s.is_dirty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_extension_names_the_kind_on_open() {
    let dir = folder("renamed");
    let saved = dir.join("porch.tciasm");
    let renamed = dir.join("porch.tcipart");
    let mut s = Session::default();
    run(&mut s, "document.new_assembly", json!({"name": "porch"}));
    run(&mut s, "file.save_as", json!({"path": saved}));
    std::fs::copy(&saved, &renamed).unwrap();
    let mut t = Session::default();
    run(&mut t, "doc.open", json!({"path": renamed}));
    assert_eq!(t.doc.kind, DocumentKind::Part, "the extension wins over the stored kind");
    assert!(!t.is_dirty(), "a kind taken from the extension is not an unsaved change");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_legacy_file_opens_with_its_stored_kind() {
    let dir = folder("legacy-open");
    let path = dir.join("bracket.solidcraft");
    let mut s = Session::default();
    run(&mut s, "document.new_part", json!({"name": "bracket"}));
    run(&mut s, "file.save_as", json!({"path": path}));
    let mut t = Session::default();
    run(&mut t, "doc.open", json!({"path": path}));
    assert_eq!(t.doc.kind, DocumentKind::Part);
    let _ = std::fs::remove_dir_all(dir);
}

/// A saved part holding one 10 x 20 x `height` box, at `path` (whose extension it keeps).
fn saved_box_part(path: &std::path::Path, height: u32) {
    let mut s = Session::default();
    run(&mut s, "document.new_part", json!({}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": height}));
    run(&mut s, "file.save_as", json!({"path": path}));
}

#[test]
fn relink_accepts_a_tcipart() {
    let dir = folder("relink");
    let (old, moved) = (dir.join("old.tcipart"), dir.join("moved.tcipart"));
    saved_box_part(&old, 30);
    saved_box_part(&moved, 30);
    let mut s = Session::default();
    let component = run(&mut s, "component.insert_design", json!({"path": old}))["component"].clone();
    let out = run(&mut s, "component.relink", json!({"component": component, "path": moved}));
    assert!(out["path"].as_str().unwrap().ends_with("moved.tcipart"), "{out}");
    assert!(s.doc.links.values().next().unwrap().path.ends_with("moved.tcipart"));
    let err = s.execute("component.relink", &json!({"component": component, "path": dir.join("x.step")})).unwrap_err();
    assert!(err.to_string().contains("design"), "a non-design file is still refused: {err}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn extract_writes_a_part_under_a_tcipart_name() {
    let dir = folder("extract");
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 30}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "file.save", json!({"path": dir.join("asm")}));
    let file = dir.join("plate.tcipart");
    let out = run(&mut s, "component.extract", json!({"component": "Plate", "path": file}));
    assert!(file.is_file(), "{out}");
    assert_eq!(s.doc.links.len(), 1);
    let mut t = Session::default();
    run(&mut t, "doc.open", json!({"path": file}));
    assert_eq!(t.doc.kind, DocumentKind::Part);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn design_path_descriptions_name_every_kind_extension() {
    for id in ["file.save", "file.save_as", "doc.open", "component.insert_design", "component.extract", "component.replace_with_link"] {
        let params = crate::find_command(id).unwrap_or_else(|| panic!("{id} is registered")).params;
        for ext in DocumentKind::EXTENSIONS {
            assert!(params.contains(&format!(".{ext}")), "{id} names .{ext}: {params}");
        }
    }
}

#[test]
fn extract_refuses_a_drawing_target() {
    let dir = folder("extract-drawing");
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 30}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "file.save", json!({"path": dir.join("asm")}));
    let file = dir.join("plate.tcidrw");
    let err = s.execute("component.extract", &json!({"component": "Plate", "path": file})).unwrap_err();
    assert!(matches!(err, EngineError::BadParams { .. }), "{err}");
    assert!(err.to_string().contains("drawing cannot hold a solid design"), "{err}");
    assert!(!file.exists(), "nothing is written for a drawing target");
    assert!(s.doc.links.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn extract_refuses_a_part_target_for_a_component_holding_links() {
    let dir = folder("extract-links");
    let leg = saved_leg(&dir);
    let mut s = Session::default();
    run(&mut s, "file.save", json!({"path": dir.join("porch")}));
    let sub = run(&mut s, "component.create", json!({"name": "Frame"}))["component"].clone();
    run(&mut s, "component.insert_design", json!({"path": leg, "parent": sub}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let links = s.doc.links.clone();
    let file = dir.join("frame.tcipart");
    let err = s.execute("component.extract", &json!({"component": "Frame", "path": file})).unwrap_err();
    assert!(matches!(err, EngineError::BadParams { .. }), "{err}");
    assert!(err.to_string().contains(".tciasm"), "{err}");
    assert!(!file.exists(), "nothing is written for a part target");
    assert_eq!(s.doc.links, links, "the design is unchanged");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_new_part_takes_its_file_name_on_save_as() {
    let dir = folder("stem-name");
    let path = dir.join("leg.tcipart");
    let mut s = Session::default();
    run(&mut s, "document.new_part", json!({}));
    run(&mut s, "file.save_as", json!({"path": path}));
    assert_eq!(s.doc.name, "leg");
    let mut t = Session::default();
    run(&mut t, "doc.open", json!({"path": path}));
    assert_eq!(t.doc.name, "leg", "after reopen");
    let mut a = Session::default();
    run(&mut a, "document.new_assembly", json!({}));
    run(&mut a, "file.save_as", json!({"path": dir.join("porch")}));
    assert_eq!(a.doc.name, "porch");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn file_save_without_a_path_keeps_a_solidcraft_extension() {
    let dir = folder("ctrl-s-legacy");
    let path = dir.join("old.solidcraft");
    saved_box_part(&path, 30);
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5}));
    let out = run(&mut s, "file.save", json!({}));
    assert_eq!(out["path"], &*path.to_string_lossy());
    assert_eq!(s.path.as_deref(), Some(&*path.to_string_lossy()));
    assert!(!dir.join("old.tcipart").exists(), "Ctrl+S on an old file does not rename it");
    let _ = std::fs::remove_dir_all(dir);
}

const LINKING: [&str; 3] = ["component.insert_design", "component.extract", "component.replace_with_link"];

/// A saved part with one box, at `<dir>/leg.tcipart`.
fn saved_leg(dir: &std::path::Path) -> PathBuf {
    let leg = dir.join("leg.tcipart");
    saved_box_part(&leg, 30); // the Task 4 helper above
    leg
}

#[test]
fn a_part_refuses_every_command_that_links_a_design() {
    let dir = folder("part-refuses");
    let leg = saved_leg(&dir);
    let mut s = Session::default();
    run(&mut s, "document.new_part", json!({"name": "holder"}));
    for id in LINKING {
        let info = crate::cmd::find_command(id).unwrap().info(&s);
        assert!(!info.enabled, "{id} must be greyed in a part");
        assert!(info.disabled_reason.as_deref().unwrap_or_default().contains("assembly"), "{id}");
        match s.execute(id, &json!({"path": leg, "component": "x", "components": ["x"]})) {
            Err(EngineError::Disabled(got, why)) => assert!(got == id && why.contains("assembly"), "{got}: {why}"),
            other => panic!("{id} in a part: {other:?}"),
        }
    }
    assert!(s.doc.links.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_assembly_accepts_a_linked_component() {
    let dir = folder("assembly-accepts");
    let leg = saved_leg(&dir);
    let mut s = Session::default();
    run(&mut s, "document.new_assembly", json!({"name": "porch"}));
    run(&mut s, "component.insert_design", json!({"path": leg}));
    assert_eq!(s.doc.links.len(), 1);
    let link = s.doc.links.values().next().unwrap();
    assert!(link.path.ends_with("leg.tcipart"), "{}", link.path);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_assembly_offers_every_command_that_links_a_design() {
    let mut s = Session::default();
    run(&mut s, "document.new_assembly", json!({}));
    for id in LINKING {
        assert!(crate::cmd::find_command(id).unwrap().info(&s).enabled, "{id}");
    }
}

#[test]
fn a_part_that_already_holds_links_opens_with_them() {
    let dir = folder("part-with-links");
    let leg = saved_leg(&dir);
    let (asm, renamed) = (dir.join("porch.tciasm"), dir.join("porch.tcipart"));
    let mut s = Session::default();
    run(&mut s, "document.new_assembly", json!({"name": "porch"}));
    run(&mut s, "component.insert_design", json!({"path": leg}));
    run(&mut s, "file.save_as", json!({"path": asm}));
    std::fs::copy(&asm, &renamed).unwrap();
    let mut t = Session::default();
    run(&mut t, "doc.open", json!({"path": renamed}));
    assert_eq!(t.doc.kind, DocumentKind::Part);
    assert_eq!(t.doc.links.len(), 1, "open never drops a link");
    assert_eq!(run(&mut t, "component.links", json!({}))["links"][0]["found"], true);
    let _ = std::fs::remove_dir_all(dir);
}
