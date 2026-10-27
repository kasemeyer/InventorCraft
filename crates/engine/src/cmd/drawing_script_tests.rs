#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The two-view drawing as a CLI script (Task 13, spec §7.2 headless): `examples/drawing/two-view-block.json`,
//! run by `solidcraft-cli run --results`, builds the test block (4 x 3 x 1 in, a ⌀.75 through-hole,
//! R.5 fillets on the vertical edges; R1), saves it, makes a drawing of it with VIEW1 (front, hidden
//! lines, scale 1, at [10, 8]: box [8, 6.5]-[12, 9.5], hole r .375 at [10, 8]) and VIEW2 (top, at
//! [10, 16]), dimensions the bottom straight run, the right side, the hole and a fillet arc, marks and
//! notes the hole, saves the drawing and exports a PDF and DXF (R2018 and R2000). Here the script runs
//! through `Session::run_script` with its file names taken into a temp folder.

use std::collections::BTreeMap;

use cadcraft_doc::{Drawing, EntityKind, Handle};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};
use solidcraft_doc::{CubeTarget, DrawingModel};

use crate::Session;
use crate::cmd::drawing::drawing_fixture::temp_dir;
use crate::drawing::mapper::{Seg, XKey, xkey_of};
use crate::drawing::{dimension_text, sick_dimensions};

// CADCraft's own PDF reader (R31), as `drawing_export_tests` loads it.
#[allow(clippy::duplicate_mod)]
#[path = "../../../cadcraft-io/tests/support/pdf_read.rs"]
mod pdf_read;

const SCRIPT: &str = include_str!("../../../../examples/drawing/two-view-block.json");

fn model(s: &Session) -> &DrawingModel {
    s.doc.drawing.as_deref().unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6
}

fn near(p: Vec2, q: [f64; 2]) -> bool {
    close(p.x, q[0]) && close(p.y, q[1])
}

/// The script with every `path` and `model` file name put in `dir` (the CLI takes them from the
/// working folder).
fn script_in(dir: &std::path::Path) -> Value {
    let mut script: Value = serde_json::from_str(SCRIPT).unwrap();
    for step in script["commands"].as_array_mut().unwrap() {
        for key in ["path", "model"] {
            if let Some(name) = step["params"][key].as_str() {
                assert!(!name.contains(['/', '\\']), "the script names files in its own folder: {name}");
                step["params"][key] = json!(dir.join(name).to_string_lossy());
            }
        }
    }
    script
}

/// The results of the script's steps running `command`, in order.
fn results_of<'a>(script: &Value, results: &'a [Value], command: &str) -> Vec<&'a Value> {
    script["commands"].as_array().unwrap().iter().zip(results).filter(|(c, _)| c["command"] == command).map(|(_, r)| r).collect()
}

fn per_layer(d: &Drawing, layout: &str) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for e in d.layout(layout).unwrap().entities.iter() {
        *out.entry(e.common.layer.clone()).or_insert(0) += 1;
    }
    out
}

fn keys(d: &Drawing, layout: &str) -> Vec<XKey> {
    let mut out: Vec<XKey> = d.layout(layout).unwrap().entities.iter().filter_map(|e| xkey_of(e)).collect();
    out.sort();
    out
}

/// Every dimension's DIMLFAC override on the sheet.
fn dimension_factors(d: &Drawing, layout: &str) -> Vec<Option<f64>> {
    d.layout(layout)
        .unwrap()
        .entities
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Dimension(dm) => Some(dm.overrides.get("linearFactor").and_then(Value::as_f64)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_example_script_makes_the_two_view_drawing() {
    let dir = temp_dir("the_example_script_makes_the_two_view_drawing");
    let script = script_in(&dir);
    let mut s = Session::default();
    let results = s.run_script(&script).unwrap_or_else(|e| panic!("the script fails: {e}"));
    assert_eq!(results.len(), script["commands"].as_array().unwrap().len());

    // Steps 1-3: the block, a new D drawing of it, VIEW1 and the top view above it.
    let new = results_of(&script, &results, "drawing.new");
    assert_eq!(new.len(), 1);
    assert_eq!((new[0]["kind"].as_str(), new[0]["sheets"].as_u64(), new[0]["size"].as_str()), (Some("drawing"), Some(1), Some("D")));
    let base = results_of(&script, &results, "drawing.view.base")[0]["view"].as_u64().unwrap();
    let top = results_of(&script, &results, "drawing.view.projected")[0]["view"].as_u64().unwrap();
    assert_eq!((base, top), (1, 2), "the script names VIEW1 by id 1");
    let m = model(&s);
    assert_eq!(m.views.len(), 2);
    assert_eq!(m.views[&top].parent, Some(base));
    assert_eq!(m.views[&top].orientation.preset, CubeTarget::from_name("top"));
    let at = m.views[&top].position;
    assert!(close(at.x, 10.0) && close(at.y, 16.0), "{at:?}");

    // Step 4: the four dimensions of spec §7.2, in order.
    let dims = results_of(&script, &results, "drawing.dim.general");
    let texts: Vec<&str> = dims.iter().map(|r| r["text"].as_str().unwrap()).collect();
    assert_eq!(texts, ["3.00", "2.00", "⌀.75", "R.50"]);
    let kinds: Vec<&str> = dims.iter().map(|r| r["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["linear", "linear", "diameter", "radius"]);
    for (r, text) in dims.iter().zip(texts) {
        assert_eq!(dimension_text(m, r["handle"].as_u64().unwrap()).as_deref(), Some(text));
    }
    assert!(sick_dimensions(m).is_empty());

    // Step 5: the centre mark (two legs on CENTER, each 2 x (.375 + .125) = 1.0 long, crossing at
    // the hole's centre, one across and one up) and the hole note.
    let marks = results_of(&script, &results, "drawing.centermark");
    let legs: Vec<u64> = marks[0]["handles"].as_array().unwrap().iter().map(|h| h.as_u64().unwrap()).collect();
    assert_eq!(legs.len(), 2);
    let hole = dims[2]["handle"].as_u64().unwrap();
    let hole_edge = match &m.paper.entity(Handle(hole)).unwrap().kind {
        EntityKind::Dimension(d) => m.paper.entity(d.assoc[0].handle).and_then(|e| xkey_of(e)).unwrap().edge,
        other => panic!("not a dimension: {other:?}"),
    };
    let ends: Vec<(Vec2, Vec2)> = legs
        .iter()
        .map(|h| {
            let e = m.paper.entity(Handle(*h)).unwrap();
            assert_eq!(e.common.layer, "CENTER");
            let key = xkey_of(e).unwrap();
            assert_eq!((key.view, key.edge.as_str(), &key.seg), (base, hole_edge.as_str(), &Seg::Mark));
            match &e.kind {
                EntityKind::Line(l) => (l.a.xy(), l.b.xy()),
                other => panic!("a centre mark leg is not a line: {other:?}"),
            }
        })
        .collect();
    for (a, b) in &ends {
        assert!(close(a.dist(*b), 1.0) && near(a.mid(*b), [10.0, 8.0]), "{a:?} {b:?}");
    }
    assert!(close(ends[0].0.y, ends[0].1.y) && close(ends[1].0.x, ends[1].1.x), "{ends:?}");
    let note = results_of(&script, &results, "drawing.note.hole");
    assert_eq!(note[0]["text"].as_str(), Some("⌀.75 THRU"));

    // Step 6: the drawing saved, and it opens again with the same dimensions.
    let drawing = dir.join("block.tcidrw");
    assert!(dir.join("block.tcipart").is_file() && drawing.is_file());
    let mut back = Session::default();
    back.execute("doc.open", &json!({"path": drawing.to_string_lossy()})).unwrap();
    let reopened: Vec<String> = dims.iter().map(|r| dimension_text(model(&back), r["handle"].as_u64().unwrap()).unwrap()).collect();
    assert_eq!(reopened, ["3.00", "2.00", "⌀.75", "R.50"]);

    // The PDF: one D landscape page (2448 x 1584 pt), no pictures (the views are not shaded).
    let pdf = results_of(&script, &results, "drawing.export.pdf");
    assert_eq!((pdf[0]["pages"].as_u64(), pdf[0]["images"].as_u64()), (Some(1), Some(0)));
    let doc = pdf_read::read(&std::fs::read(dir.join("block.pdf")).unwrap());
    assert_eq!(doc.pages.len(), 1);
    assert_eq!(doc.pages[0].media, [0.0, 0.0, 2448.0, 1584.0]);

    // The DXF, at R2018 and R2000: the sheet's entities per layer, 4 dimensions in model inches
    // (DIMLFAC 1) and every key, read back with cadcraft-io (R63).
    let layout = m.sheets[0].layout.clone();
    let paper = crate::drawing::export::sheet_paper(m, 0).unwrap();
    let expected: BTreeMap<String, usize> = EXPECTED_LAYERS.iter().map(|(l, n)| (l.to_string(), *n)).collect();
    assert_eq!(per_layer(&paper, &layout), expected);
    let dxf = results_of(&script, &results, "drawing.export.dxf");
    assert_eq!(dxf.len(), 2);
    for (r, (file, version)) in dxf.iter().zip([("block.dxf", "R2018"), ("block-r2000.dxf", "R2000")]) {
        assert_eq!(r["version"].as_str(), Some(version));
        assert_eq!(r["entities"].as_u64(), Some(EXPECTED_LAYERS.iter().map(|(_, n)| *n as u64).sum()), "{version}");
        let text = std::fs::read_to_string(dir.join(file)).unwrap();
        let read = cadcraft_io::read(text.as_bytes(), "x.dxf").unwrap();
        assert_eq!(per_layer(&read, &layout), expected, "{version}");
        assert_eq!(dimension_factors(&read, &layout), [Some(1.0); 4], "{version}");
        assert_eq!(keys(&read, &layout), keys(&paper, &layout), "{version}");
    }
}

/// The sheet's entities per layer: the border and title block (2 inserts); the centre mark's 2 legs;
/// the 4 dimensions; the hole's 2 hidden lines in the top view (spec §7.3); the hole note's leader
/// and text; and 19 visible curves: VIEW1's 4 lines, 1 circle and 4 arcs (§7.3's front golden) and
/// the top view's 10 lines (`drawing_projection_tests::top_view_has_two_hidden_lines_for_the_hole`):
/// the outline's sides (2), front (3 pieces: the fillets' edges and the top face's) and back (5
/// pieces: the same, the left end split where two fillet edges' names alternate). The two lines
/// where the top face meets the fillets (x = 8.5 and 11.5) are tangent (R64) and the views leave
/// tangent edges off.
const EXPECTED_LAYERS: &[(&str, usize)] = &[("BORDER", 2), ("CENTER", 2), ("DIMENSION", 4), ("HIDDEN", 2), ("NOTE", 2), ("VISIBLE", 19)];
