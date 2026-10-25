#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};

use super::*;

fn exec(id: &str, params: Value) -> Verdict {
    request("engine.execute", &json!({"command": id, "params": params}), 5)
}

#[test]
fn starting_a_typed_document_asks_like_file_new() {
    assert!(matches!(exec("file.new", json!({})), Verdict::Ask(_)));
    for id in ["document.new_part", "document.new_assembly"] {
        assert!(matches!(exec(id, json!({"name": "leg"})), Verdict::Ask(a) if a.contains(id)), "{id}");
    }
}

#[test]
fn writing_a_project_file_asks_like_save_as() {
    for path in ["job.tciproj", "/jobs/porch/job.tciproj"] {
        let p = json!({"name": "job", "workspace": ".", "path": path});
        assert!(matches!(exec("file.save_as", p.clone()), Verdict::Ask(_)), "{path}");
        assert!(matches!(exec("project.new", p), Verdict::Ask(_)), "project.new {path}");
    }
}

#[test]
fn a_bare_typed_design_name_is_a_file_for_link_commands() {
    for id in ["component.extract", "component.relink", "component.replace_with_link"] {
        for path in ["x.tcipart", "x.TCIASM", "x.tcidrw", "job.tciproj"] {
            assert!(matches!(exec(id, json!({"component": "Plate", "path": path})), Verdict::Ask(_)), "{id} {path}");
        }
    }
    assert!(!matches!(exec("component.extract", json!({"component": "Plate", "path": "c1"})), Verdict::Ask(_)), "a curve id is not a file");
}
