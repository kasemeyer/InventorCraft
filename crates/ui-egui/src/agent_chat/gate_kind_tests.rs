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
