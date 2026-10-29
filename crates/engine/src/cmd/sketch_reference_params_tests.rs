//! R122: driven dimensions are reference parameters, and a driving dimension whose expression
//! names one follows what it measures (a slot dimensioned from both sides is centred).

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn name(v: &Value) -> String {
    v["param"].as_str().unwrap_or_else(|| panic!("no parameter: {v}")).to_string()
}

struct Slot {
    s: Session,
    width: String,
    height: String,
    d1: String,
    d2: String,
    start: String,
    end: String,
}

/// A 100 × 50 rectangle at the origin with a 20 long slot at y = 25: `d1` (driving, 10) from
/// the left side to the slot, `d2` (driven) from the slot to the right side.
fn slot() -> Slot {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 50]}));
    let (bl, br) = (r["corners"]["bottom_left"].as_str().unwrap().to_string(), r["corners"]["bottom_right"].as_str().unwrap().to_string());
    let _ = s.execute("sketch.constraint.coincident", &json!({"a": bl, "b": "origin"}));
    let width = name(&run(&mut s, "sketch.dimension", json!({"entities": [r["sides"]["bottom"]], "value": "100 mm"})));
    let height = name(&run(&mut s, "sketch.dimension", json!({"entities": [r["sides"]["right"]], "value": "50 mm"})));
    let l = run(&mut s, "sketch.line", json!({"points": [[10, 25], [30, 25]], "infer": true, "connect": false}))["curves"][0]
        .as_str()
        .unwrap()
        .to_string();
    let (start, end) = (format!("{l}.start"), format!("{l}.end"));
    run(&mut s, "sketch.dimension", json!({"entities": [l], "value": "20 mm"}));
    run(&mut s, "sketch.dimension", json!({"entities": [bl, start], "type": "vertical", "value": "25 mm"}));
    let d1 = name(&run(&mut s, "sketch.dimension", json!({"entities": [bl, start], "type": "horizontal", "value": "10 mm"})));
    let d2 = run(&mut s, "sketch.dimension", json!({"entities": [end, br], "type": "horizontal", "driven": true}));
    assert_eq!(d2["reference"], true, "{d2}");
    let d2 = name(&d2);
    Slot { s, width, height, d1, d2, start, end }
}

impl Slot {
    fn sketch(&self) -> solidcraft_sketch::Sketch {
        self.s.doc.sketch(self.s.active_sketch.unwrap()).unwrap().clone()
    }
    fn x(&self, point: &str) -> f64 {
        let sk = self.sketch();
        sk.resolve_point(point).and_then(|i| sk.point(i)).unwrap().x
    }
    /// A dimension's value in the stored sketch, by parameter name.
    fn value(&self, param: &str) -> f64 {
        self.sketch().constraints.iter().find(|c| c.param.as_deref() == Some(param)).and_then(|c| c.kind.value()).unwrap()
    }
    fn assert_centred(&self, width: f64) {
        let half = (width - 20.0) / 2.0;
        assert!((self.x(&self.start) - half).abs() < 1e-6, "start {} for width {width}", self.x(&self.start));
        assert!((self.x(&self.end) - (half + 20.0)).abs() < 1e-6, "end {}", self.x(&self.end));
        assert!((self.value(&self.d1) - self.value(&self.d2)).abs() < 1e-6, "{} vs {}", self.value(&self.d1), self.value(&self.d2));
        // The reference parameter holds what its dimension measures.
        let vals = self.s.doc.param_values().0;
        assert!((vals[&self.d2].v - half).abs() < 1e-6, "{:?}", vals[&self.d2]);
    }
}

#[test]
fn a_driven_dimension_is_a_reference_parameter_listed_as_one() {
    let mut t = slot();
    let p = t.s.doc.param(&t.d2).cloned().unwrap();
    assert!(p.reference && p.model, "{p:?}");
    assert_eq!(p.expr, "70 mm");
    let list = run(&mut t.s, "parameters.list", json!({}));
    let row = list["parameters"].as_array().unwrap().iter().find(|r| r["name"] == t.d2.as_str()).cloned().unwrap();
    assert_eq!(row["reference"], true, "{row}");
    assert_eq!(row["value"], 70.0, "{row}");
    let d1 = list["parameters"].as_array().unwrap().iter().find(|r| r["name"] == t.d1.as_str()).cloned().unwrap();
    assert!(d1.get("reference").is_none(), "{d1}");
    // Read-only: a change is refused with a message, and leaves no undo step.
    let undo = t.s.undo.len();
    let e = t.s.execute("parameters.change", &json!({"name": t.d2, "expression": "30 mm"})).unwrap_err().to_string();
    assert!(e.contains("reference parameter") && e.contains(&t.d2), "{e}");
    assert_eq!(t.s.doc.param(&t.d2).unwrap().expr, "70 mm");
    assert_eq!(t.s.undo.len(), undo, "a refused change leaves no undo step");
}

/// Deleting a reference parameter is refused with the change's message: the sync would
/// otherwise recreate it at once, so the delete would silently do nothing.
#[test]
fn deleting_a_reference_parameter_is_refused() {
    let mut t = slot();
    let undo = t.s.undo.len();
    let change = t.s.execute("parameters.change", &json!({"name": t.d2, "expression": "30 mm"})).unwrap_err().to_string();
    let e = t.s.execute("parameters.delete", &json!({"name": t.d2})).unwrap_err().to_string();
    assert_eq!(e, change, "the delete is refused with the change's message");
    let e = t.s.execute("parameters.change", &json!({"delete": t.d2})).unwrap_err().to_string();
    assert_eq!(e, change, "so is a delete through parameters.change");
    assert_eq!(t.s.undo.len(), undo, "a refused delete leaves no undo step");
    assert_eq!(t.s.doc.param(&t.d2).unwrap().expr, "70 mm");
}

#[test]
fn a_driving_dimension_set_to_a_driven_one_centres_the_slot_and_keeps_it_centred() {
    let mut t = slot();
    let undo = t.s.undo.len();
    run(&mut t.s, "parameters.change", json!({"name": t.d1, "expression": t.d2}));
    assert_eq!(t.s.undo.len(), undo + 1, "one undo step for the edit");
    assert_eq!(t.s.doc.param(&t.d1).unwrap().expr, t.d2);
    t.assert_centred(100.0);
    assert!((t.x(&t.start) - 40.0).abs() < 1e-6);
    // The rectangle's width changes: still equal.
    run(&mut t.s, "parameters.change", json!({"name": t.width, "expression": "140 mm"}));
    t.assert_centred(140.0);
    assert!((t.x(&t.start) - 60.0).abs() < 1e-6);
    // An offset: d1 = d2 + 4.
    run(&mut t.s, "parameters.change", json!({"name": t.d1, "expression": format!("{} + 4 mm", t.d2)}));
    assert!((t.value(&t.d1) - t.value(&t.d2) - 4.0).abs() < 1e-6, "{} {}", t.value(&t.d1), t.value(&t.d2));
    // Undo takes the edits back, one at a time.
    run(&mut t.s, "edit.undo", json!({}));
    t.assert_centred(140.0);
}

#[test]
fn a_cycle_between_driving_expressions_is_still_refused() {
    let mut t = slot();
    run(&mut t.s, "parameters.change", json!({"name": t.d1, "expression": t.height}));
    let e = t.s.execute("parameters.change", &json!({"name": t.height, "expression": t.d1})).unwrap_err().to_string();
    assert!(e.contains("circular"), "{e}");
    assert_eq!(t.s.doc.param(&t.height).unwrap().expr, "50 mm");
}

#[test]
fn a_design_saved_before_reference_parameters_names_its_driven_dimensions_on_load() {
    let t = slot();
    let mut v: Value = serde_json::from_str(&t.s.doc.to_json()).unwrap();
    // As saved before R122: the driven dimension has no parameter.
    v["params"].as_array_mut().unwrap().retain(|p| p["name"] != t.d2.as_str());
    let mut stripped = 0;
    for f in v["features"].as_array_mut().unwrap() {
        for c in f.pointer_mut("/sketch/constraints").and_then(Value::as_array_mut).into_iter().flatten() {
            if c["driven"] == true {
                c.as_object_mut().unwrap().remove("param");
                stripped += 1;
            }
        }
    }
    assert_eq!(stripped, 1, "{v}");
    let doc = solidcraft_doc::Document::from_json(&v.to_string()).unwrap();
    let sk = doc.features.iter().find_map(|f| match &f.kind {
        solidcraft_doc::FeatureKind::Sketch { sketch, .. } => Some(sketch),
        _ => None,
    });
    let c = sk.unwrap().constraints.iter().find(|c| c.driven).unwrap();
    let n = c.param.clone().expect("named on load");
    assert_eq!(n, t.d2, "the next free name, deterministically");
    let p = doc.param(&n).unwrap();
    assert!(p.reference && p.expr == "70 mm", "{p:?}");
    assert!(doc.reference_params_in_sync());
}
