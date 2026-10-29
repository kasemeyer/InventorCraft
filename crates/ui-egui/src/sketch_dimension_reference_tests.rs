//! R106/R122: while a dimension's value is edited, a click on any other dimension, driving or
//! driven, inserts its parameter name; `d1 = d2` with d2 driven centres a slot. Both profiles.
use crate::scenario::Harness;
use crate::test_support::clicks_expire;
use serde_json::{Value, json};

/// The slot of the engine's R122 tests: a 100 × 50 rectangle, a 20 long slot at y = 25 with
/// `d1` (driving, 10) from the left side and `d2` (driven) from the right side. Returns the
/// harness, d1, d2 and d2's constraint id.
fn slot(inventor: bool) -> (Harness, String, String, String) {
    crate::dim_view::cancel();
    let mut h = if inventor { crate::test_support::inventor(solidcraft_engine::doc::DocumentKind::Part) } else { Harness::new() };
    let run = |h: &mut Harness, id: &str, p: Value| h.app.run(id, p).unwrap_or_else(|e| panic!("{id}: {e}"));
    run(&mut h, "sketch.create", json!({"plane": "XY"}));
    let r = run(&mut h, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 50]}));
    let (bl, br) = (r["corners"]["bottom_left"].as_str().unwrap().to_string(), r["corners"]["bottom_right"].as_str().unwrap().to_string());
    let _ = h.app.run("sketch.constraint.coincident", json!({"a": bl, "b": "origin"}));
    run(&mut h, "sketch.dimension", json!({"entities": [r["sides"]["bottom"]], "value": "100 mm", "text_at": [50, -12]}));
    run(&mut h, "sketch.dimension", json!({"entities": [r["sides"]["right"]], "value": "50 mm", "text_at": [112, 25]}));
    let l = run(&mut h, "sketch.line", json!({"points": [[10, 25], [30, 25]], "infer": true, "connect": false}))["curves"][0]
        .as_str()
        .unwrap()
        .to_string();
    run(&mut h, "sketch.dimension", json!({"entities": [l], "value": "20 mm", "text_at": [20, 32]}));
    run(&mut h, "sketch.dimension", json!({"entities": [bl, format!("{l}.start")], "type": "vertical", "value": "25 mm", "text_at": [4, 12]}));
    let d1 =
        run(&mut h, "sketch.dimension", json!({"entities": [bl, format!("{l}.start")], "type": "horizontal", "value": "10 mm", "text_at": [5, 40]}));
    let d2 =
        run(&mut h, "sketch.dimension", json!({"entities": [format!("{l}.end"), br], "type": "horizontal", "driven": true, "text_at": [65, 40]}));
    let (d1, d2) = (d1["param"].as_str().unwrap().to_string(), d2["param"].as_str().unwrap().to_string());
    let sk = h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    let d2_id = sk.constraints.iter().find(|c| c.param.as_deref() == Some(d2.as_str())).unwrap().id.clone();
    h.call("ui.view", json!({"view": "top"}));
    h.app.cam.distance = 260.0;
    h.app.cam.target = solidcraft_engine::geom::Vec3::new(50.0, 25.0, 0.0);
    h.settle();
    (h, d1, d2, d2_id)
}

fn slot_start_x(h: &Harness) -> f64 {
    let sk = h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
    let start = sk.constraints.iter().find_map(|c| match c.kind {
        solidcraft_engine::sketch::ConstraintKind::DistanceX { q, value, .. } if c.param.is_some() && !c.driven && (value - 10.0).abs() < 1e-6 => {
            Some(q)
        }
        _ => None,
    });
    start.and_then(|q| sk.point(q)).unwrap().x
}

#[test]
fn picking_a_driven_dimension_while_editing_inserts_its_name_and_centres_the_slot() {
    for inventor in [false, true] {
        let (mut h, d1, d2, d2_id) = slot(inventor);
        assert!((slot_start_x(&h) - 10.0).abs() < 1e-6);
        // Outside the edit a click on the driven dimension inserts nothing (it selects, as before).
        h.step(&json!({"click": {"dimension": d2}})).unwrap();
        assert!(!crate::dim_view::editing(), "inventor {inventor}");
        assert_eq!(crate::dim_view::reference_tip(&d2_id), None, "no tooltip outside the edit");
        assert_eq!(h.app.session.doc.param(&d1).unwrap().expr, "10 mm");
        clicks_expire(&mut h);
        // Edit d1, pick d2: the box reads "d2" and stays open.
        crate::dim_view::edit_param(&h.app, &d1);
        h.frames(5);
        assert!(crate::dim_view::editing());
        assert_eq!(crate::dim_view::reference_tip(&d2_id).as_deref(), Some(d2.as_str()), "the tooltip names the parameter");
        h.step(&json!({"click": {"dimension": d2}})).unwrap();
        assert!(crate::dim_view::editing(), "the pick does not end the edit (inventor {inventor})");
        assert_eq!(crate::dim_view::edit_text().as_deref(), Some(d2.as_str()));
        h.frames(2);
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(egui::Id::new("sc_dim_edit")), "the value keeps the keyboard");
        h.step(&json!({"key": "Enter"})).unwrap();
        assert!(!crate::dim_view::editing(), "{}", h.call("ui.inspect", json!({})));
        assert_eq!(h.app.session.doc.param(&d1).unwrap().expr, d2);
        let sk = h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
        let value = |p: &str| sk.constraints.iter().find(|c| c.param.as_deref() == Some(p)).and_then(|c| c.kind.value()).unwrap();
        assert!((value(&d1) - 40.0).abs() < 1e-6 && (value(&d2) - 40.0).abs() < 1e-6, "centred: {} {}", value(&d1), value(&d2));
        crate::dim_view::cancel();
    }
}
