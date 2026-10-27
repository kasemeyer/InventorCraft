//! The drawing tests' block: the capture's 4 x 3 x 1 in block with a drilled hole and four
//! rounded corners, in our Z-up frame (plan "The test block"; R1). Each helper lands with the
//! commands it calls (R4): Task 3 brings these four, Task 7 the view helpers.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A fresh temp folder for one test: `<temp>/tcicad-drw-<pid>-<name>/`, emptied first.
pub(crate) fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tcicad-drw-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Builds and saves `<dir>/block.tcipart`; returns its path. Box sizes are unit strings (the
/// design is in inches); placement points are mm (R1).
pub(crate) fn block_part(dir: &Path) -> String {
    let path = dir.join("block.tcipart").to_string_lossy().into_owned();
    let mut s = Session::default();
    run(&mut s, "document.new_part", json!({}));
    run(&mut s, "document.units", json!({"units": "in"}));
    run(
        &mut s,
        "solid.box",
        json!({"length": "4 in", "width": "1 in", "height": "3 in", "corner": [-50.8, 0, 0], "name": "Block", "body_name": "Block"}),
    );
    run(&mut s, "solid.hole", json!({"position": [0, 0, 38.1], "direction": [0, 1, 0], "diameter": "0.75 in", "name": "Drill"}));
    run(&mut s, "solid.fillet", json!({"edges": [[-50.8, 12.7, 0], [50.8, 12.7, 0], [-50.8, 12.7, 76.2], [50.8, 12.7, 76.2]], "radius": "0.5 in"}));
    run(&mut s, "file.save", json!({"path": path}));
    path
}

/// A Session after `drawing.new {"model": <block>}` in `temp_dir(name)`, and the block's path.
pub(crate) fn block_drawing(name: &str) -> (Session, String) {
    let dir = temp_dir(name);
    let block = block_part(&dir);
    let mut s = Session::default();
    run(&mut s, "drawing.new", json!({"model": block}));
    (s, block)
}

/// Opens the model in its own Session, runs `commands`, saves it in place.
#[allow(dead_code)] // first called by Task 8's update tests (R4)
pub(crate) fn edit_model(path: &str, commands: &[(&str, Value)]) {
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path}));
    for (id, p) in commands {
        run(&mut s, id, p.clone());
    }
    run(&mut s, "file.save", json!({"path": path}));
}

/// `drawing.view.base` of the block: front, hidden lines, unshaded, scale 1, at [10, 8]; the view id.
pub(crate) fn front_view(s: &mut Session, model: &str) -> u64 {
    let r = run(
        s,
        "drawing.view.base",
        json!({"model": model, "orientation": "front", "lines": "hidden", "shaded": false, "scale": 1, "position": [10, 8]}),
    );
    r["view"].as_u64().unwrap_or_else(|| panic!("drawing.view.base gave no view: {r}"))
}

/// The model_edge_id of the view curve nearest paper point `at`.
pub(crate) fn edge_at(s: &Session, view: u64, at: [f64; 2]) -> String {
    crate::drawing::pick::nearest_edge(s, view, at).unwrap_or_else(|| panic!("view {view} has no curves"))
}
