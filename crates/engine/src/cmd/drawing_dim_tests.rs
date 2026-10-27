#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Model-associative dimensions (Task 10): General Dimension chooses linear, aligned, diameter or
//! radius by what is picked; every dimension is on DIMENSION in ANSI_INCH with DIMLFAC = 1 /
//! view scale, so it reads model inches ("3.00", "⌀.75", "R.50"); Edit Dimension; dimensions
//! follow the model through `drawing.update` and go sick when their edge goes. VIEW1 is the
//! block's front view at [10, 8], scale 1: box [8, 6.5]-[12, 9.5], hole r .375 at [10, 8],
//! fillets r .5 at the corners.

use cadcraft_doc::{AssocSnap, DimKind, Dimension, EntityKind, Handle};
use serde_json::{Value, json};
use solidcraft_doc::DrawingModel;

use crate::cmd::drawing::drawing_fixture::{block_drawing, edge_at, edit_model, front_view};
use crate::drawing::mapper::xkey_of;
use crate::drawing::{PaperPick, dimension_text, pick, sick_dimensions};
use crate::{EngineError, Session};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn model(s: &Session) -> &DrawingModel {
    s.doc.drawing.as_deref().unwrap()
}

fn bad_message(r: crate::Result<Value>) -> String {
    match r {
        Err(EngineError::BadParams { msg, .. }) => msg,
        other => panic!("expected bad params, got {other:?}"),
    }
}

/// The dimension entity `h`: (its layer, the dimension).
fn dim(s: &Session, h: u64) -> (String, Dimension) {
    let e = model(s).paper.entity(Handle(h)).unwrap_or_else(|| panic!("no entity {h}"));
    match &e.kind {
        EntityKind::Dimension(d) => (e.common.layer.clone(), d.clone()),
        other => panic!("entity {h} is not a dimension: {other:?}"),
    }
}

/// The dimension's style with its overrides.
fn style(s: &Session, h: u64) -> cadcraft_doc::DimStyle {
    let (_, d) = dim(s, h);
    model(s).paper.dim_style(&d.style).unwrap().with_overrides(&d.overrides)
}

/// The paper handle of view `view`'s entity showing model edge `edge`.
fn handle_of_edge(s: &Session, view: u64, edge: &str) -> Handle {
    let m = model(s);
    let layout = m.paper.layout(&m.sheets[0].layout).unwrap();
    layout.entities.iter().find(|e| xkey_of(e).is_some_and(|k| k.view == view && k.edge == edge)).map(|e| e.handle).unwrap()
}

fn handle(r: &Value) -> u64 {
    r["handle"].as_u64().unwrap_or_else(|| panic!("no handle in {r}"))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6
}

/// The block drawing with VIEW1; (session, block path, view id).
fn front(name: &str) -> (Session, String, u64) {
    let (mut s, block) = block_drawing(name);
    let v = front_view(&mut s, &block);
    (s, block, v)
}

fn general(s: &mut Session, view: u64, picks: Value, placement: [f64; 2]) -> Value {
    run(s, "drawing.dim.general", json!({"view": view, "picks": picks, "placement": placement}))
}

#[test]
fn one_straight_edge_gives_its_length() {
    let (mut s, _, v) = front("one_straight_edge_gives_its_length");
    let bottom = edge_at(&s, v, [10.0, 6.5]);
    let undo = s.undo.len();
    let r = general(&mut s, v, json!([{"edge": bottom}]), [10.0, 5.8]);
    assert_eq!(s.undo.len(), undo + 1);
    assert_eq!((r["kind"].as_str(), r["text"].as_str()), (Some("linear"), Some("3.00")));
    let h = handle(&r);
    let (layer, d) = dim(&s, h);
    assert_eq!((layer.as_str(), d.style.as_str()), ("DIMENSION", "ANSI_INCH"));
    assert_eq!(d.kind, DimKind::Linear { rotation: 0.0 });
    assert_eq!(style(&s, h).linear_factor, 1.0);
    assert_eq!(dimension_text(model(&s), h).as_deref(), Some("3.00"));
    // The extension origins are the edge's ends, the dimension line through the placement.
    assert!(close(d.p13.x, 8.5) && close(d.p13.y, 6.5) && close(d.p14.x, 11.5) && close(d.p14.y, 6.5), "{d:?}");
    assert!(close(d.defpt.y, 5.8), "{d:?}");
    let edge = handle_of_edge(&s, v, &bottom);
    let points: Vec<(&str, Handle, AssocSnap)> = d.assoc.iter().map(|a| (a.point.as_str(), a.handle, a.snap.clone())).collect();
    assert_eq!(points, [("p13", edge, AssocSnap::Start), ("p14", edge, AssocSnap::End)]);
    assert!(sick_dimensions(model(&s)).is_empty());
}

#[test]
fn a_side_edge_reads_2_00() {
    let (mut s, _, v) = front("a_side_edge_reads_2_00");
    let right = edge_at(&s, v, [12.0, 8.0]);
    let r = general(&mut s, v, json!([{"edge": right}]), [13.0, 8.0]);
    assert_eq!((r["kind"].as_str(), r["text"].as_str()), (Some("linear"), Some("2.00")));
    assert_eq!(dim(&s, handle(&r)).1.kind, DimKind::Linear { rotation: std::f64::consts::FRAC_PI_2 });
    // The same edge picked by a point on the paper.
    let r = general(&mut s, v, json!([{"at": [12.0, 8.0]}]), [13.5, 8.0]);
    assert_eq!(r["text"].as_str(), Some("2.00"));
    assert_eq!(dim(&s, handle(&r)).1.assoc[0].handle, handle_of_edge(&s, v, &right));
}

/// Final wave 15: a forced orientation across which the picks have no extent (a vertical edge
/// measured horizontally) is refused, not drawn as "0.00"; one along the edge is drawn.
#[test]
fn a_forced_orientation_that_measures_nothing_is_refused() {
    let (mut s, _, v) = front("a_forced_orientation_that_measures_nothing_is_refused");
    let right = edge_at(&s, v, [12.0, 8.0]);
    let bottom = edge_at(&s, v, [10.0, 6.5]);
    let undo = s.undo.len();
    for (cmd, edge, orientation) in [
        ("drawing.dim.general", &right, "horizontal"),
        ("drawing.dim.linear", &right, "horizontal"),
        ("drawing.dim.general", &bottom, "vertical"),
        ("drawing.dim.linear", &bottom, "vertical"),
    ] {
        let r = s.execute(cmd, &json!({"view": v, "picks": [{"edge": edge}], "placement": [13, 5], "orientation": orientation}));
        assert_eq!(bad_message(r), format!("a {orientation} dimension here would measure nothing: give another orientation"), "{cmd}");
    }
    assert_eq!(s.undo.len(), undo);
    let r = run(&mut s, "drawing.dim.linear", json!({"view": v, "picks": [{"edge": right}], "placement": [13, 8], "orientation": "vertical"}));
    assert_eq!(r["text"].as_str(), Some("2.00"));
}

#[test]
fn a_circle_gives_a_diameter() {
    let (mut s, _, v) = front("a_circle_gives_a_diameter");
    let hole = edge_at(&s, v, [10.375, 8.0]);
    let r = general(&mut s, v, json!([{"edge": hole}]), [11.0, 8.6]);
    assert_eq!((r["kind"].as_str(), r["text"].as_str()), (Some("diameter"), Some("⌀.75")));
    let (layer, d) = dim(&s, handle(&r));
    assert_eq!((layer.as_str(), d.kind), ("DIMENSION", DimKind::Diameter));
    let circle = handle_of_edge(&s, v, &hole);
    assert!(d.assoc.iter().all(|a| a.handle == circle && matches!(a.snap, AssocSnap::OnCircle { .. })), "{:?}", d.assoc);
    assert_eq!(d.assoc.iter().map(|a| a.point.as_str()).collect::<Vec<_>>(), ["defpt", "p15"]);
    // The leader end is on the circle towards the placement, defpt opposite.
    assert!(
        close(d.p15.xy().dist(cadcraft_geom::Vec2::new(10.0, 8.0)), 0.375) && close(d.defpt.xy().dist(cadcraft_geom::Vec2::new(10.0, 8.0)), 0.375)
    );
    assert!(d.p15.x > 10.0 && d.p15.y > 8.0 && d.defpt.x < 10.0 && d.defpt.y < 8.0, "{d:?}");
}

#[test]
fn an_arc_gives_a_radius() {
    let (mut s, _, v) = front("an_arc_gives_a_radius");
    let fillet = edge_at(&s, v, [8.146, 6.646]);
    let r = general(&mut s, v, json!([{"edge": fillet}]), [7.5, 6.0]);
    assert_eq!((r["kind"].as_str(), r["text"].as_str()), (Some("radius"), Some("R.50")));
    let (_, d) = dim(&s, handle(&r));
    assert_eq!(d.kind, DimKind::Radius);
    // defpt at the arc's centre [8.5, 7], p15 on the arc.
    assert!(close(d.defpt.x, 8.5) && close(d.defpt.y, 7.0), "{d:?}");
    assert!(close(d.p15.xy().dist(d.defpt.xy()), 0.5));
    let arc = handle_of_edge(&s, v, &fillet);
    assert_eq!(d.assoc[0].snap, AssocSnap::Center);
    assert!(d.assoc.iter().all(|a| a.handle == arc));
}

#[test]
fn two_picks_are_oriented_by_the_placement() {
    let (mut s, _, v) = front("two_picks_are_oriented_by_the_placement");
    let (left, right) = (edge_at(&s, v, [8.0, 8.0]), edge_at(&s, v, [12.0, 8.0]));
    let r = general(&mut s, v, json!([{"edge": left, "snap": "mid"}, {"edge": right, "snap": "mid"}]), [10.0, 10.0]);
    assert_eq!((r["kind"].as_str(), r["text"].as_str()), (Some("linear"), Some("4.00")));
    let (_, d) = dim(&s, handle(&r));
    assert_eq!(d.kind, DimKind::Linear { rotation: 0.0 });
    assert!(close(d.p13.x, 8.0) && close(d.p13.y, 8.0) && close(d.p14.x, 12.0) && close(d.p14.y, 8.0), "{d:?}");
    assert_eq!(d.assoc.iter().map(|a| a.snap.clone()).collect::<Vec<_>>(), [AssocSnap::Mid, AssocSnap::Mid]);
    let (top, bottom) = (edge_at(&s, v, [10.0, 9.5]), edge_at(&s, v, [10.0, 6.5]));
    let r = general(&mut s, v, json!([{"edge": top, "snap": "mid"}, {"edge": bottom, "snap": "mid"}]), [13.0, 8.0]);
    assert_eq!((r["kind"].as_str(), r["text"].as_str()), (Some("linear"), Some("3.00")));
    assert_eq!(dim(&s, handle(&r)).1.kind, DimKind::Linear { rotation: std::f64::consts::FRAC_PI_2 });
    // Diagonal picks: above them horizontal, unless `orientation` says otherwise.
    let picks = json!([{"edge": top, "snap": "mid"}, {"edge": right, "snap": "mid"}]);
    assert_eq!(general(&mut s, v, picks.clone(), [11.0, 10.5])["text"].as_str(), Some("2.00"));
    let r = run(&mut s, "drawing.dim.general", json!({"view": v, "picks": picks, "placement": [11.0, 10.5], "orientation": "vertical"}));
    assert_eq!(r["text"].as_str(), Some("1.50"));
}

#[test]
fn half_scale_still_reads_model_units() {
    let (mut s, block) = block_drawing("half_scale_still_reads_model_units");
    let v = run(&mut s, "drawing.view.base", json!({"model": block, "orientation": "front", "scale": 0.5, "position": [10, 8]}))["view"]
        .as_u64()
        .unwrap();
    let bottom = edge_at(&s, v, [10.0, 7.25]);
    let r = general(&mut s, v, json!([{"edge": bottom}]), [10.0, 6.8]);
    assert_eq!((r["kind"].as_str(), r["text"].as_str()), (Some("linear"), Some("3.00")));
    let (_, d) = dim(&s, handle(&r));
    assert!(close(d.p13.x, 9.25) && close(d.p14.x, 10.75), "{d:?}");
    assert_eq!(style(&s, handle(&r)).linear_factor, 2.0);
}

#[test]
fn rescaling_a_view_keeps_its_dimensions_in_model_units() {
    let (mut s, _, v) = front("rescaling_a_view_keeps_its_dimensions_in_model_units");
    let bottom = edge_at(&s, v, [10.0, 6.5]);
    let h = handle(&general(&mut s, v, json!([{"edge": bottom}]), [10.0, 5.8]));
    run(&mut s, "drawing.view.set_scale", json!({"view": v, "scale": 0.5}));
    assert_eq!(style(&s, h).linear_factor, 2.0);
    let (_, d) = dim(&s, h);
    assert!(close(d.p13.x, 9.25) && close(d.p14.x, 10.75), "{d:?}");
    assert_eq!(dimension_text(model(&s), h).as_deref(), Some("3.00"));
    // A move carries the whole dimension along.
    run(&mut s, "drawing.view.set_position", json!({"view": v, "position": [12, 8]}));
    let (_, d) = dim(&s, h);
    assert!(close(d.p13.x, 11.25) && close(d.p14.x, 12.75), "{d:?}");
    assert_eq!(dimension_text(model(&s), h).as_deref(), Some("3.00"));
}

#[test]
fn edit_sets_precision_and_text() {
    let (mut s, _, v) = front("edit_sets_precision_and_text");
    let bottom = edge_at(&s, v, [10.0, 6.5]);
    let h = handle(&general(&mut s, v, json!([{"edge": bottom}]), [10.0, 5.8]));
    let undo = s.undo.len();
    let r = run(&mut s, "drawing.dim.edit", json!({"dim": h, "precision": 3}));
    assert_eq!((r["handle"].as_u64(), r["kind"].as_str(), r["text"].as_str()), (Some(h), Some("linear"), Some("3.000")));
    assert_eq!(s.undo.len(), undo + 1);
    assert_eq!(style(&s, h).decimals, 3);
    let r = run(&mut s, "drawing.dim.edit", json!({"dim": h, "hide_value": true, "text": "TYP"}));
    assert_eq!(r["text"].as_str(), Some("TYP"));
    let r = run(&mut s, "drawing.dim.edit", json!({"dim": h, "hide_value": false, "text": "<<>> TYP", "precision": 2}));
    assert_eq!(r["text"].as_str(), Some("3.00 TYP"));
    // Stored as DXF's "<>".
    assert_eq!(dim(&s, h).1.text, "<> TYP");
    assert_eq!(dimension_text(model(&s), h).as_deref(), Some("3.00 TYP"));
    let undo = s.undo.len();
    assert_eq!(bad_message(s.execute("drawing.dim.edit", &json!({"dim": h, "precision": 7}))), "precision must be 0 to 6");
    assert_eq!(s.undo.len(), undo);
    assert_eq!(dimension_text(model(&s), h).as_deref(), Some("3.00 TYP"));
    // A view curve is not a dimension.
    let edge = handle_of_edge(&s, v, &bottom).0;
    assert_eq!(bad_message(s.execute("drawing.dim.edit", &json!({"dim": edge, "precision": 2}))), format!("{edge} is not a dimension"));
}

#[test]
fn extension_lines_hide_and_show() {
    let (mut s, _, v) = front("extension_lines_hide_and_show");
    let bottom = edge_at(&s, v, [10.0, 6.5]);
    let h = handle(&general(&mut s, v, json!([{"edge": bottom}]), [10.0, 5.8]));
    run(&mut s, "drawing.dim.edit", json!({"dim": h, "ext_lines": {"first": false, "second": true}}));
    assert_eq!((style(&s, h).suppress_ext1, style(&s, h).suppress_ext2), (true, false));
    run(&mut s, "drawing.dim.edit", json!({"dim": h, "ext_lines": {"first": true, "second": true}}));
    assert_eq!((style(&s, h).suppress_ext1, style(&s, h).suppress_ext2), (false, false));
}

#[test]
fn dimensions_follow_the_model() {
    let (mut s, block, v) = front("dimensions_follow_the_model");
    let hole = edge_at(&s, v, [10.375, 8.0]);
    let h = handle(&general(&mut s, v, json!([{"edge": hole}]), [11.0, 8.6]));
    edit_model(&block, &[("timeline.edit", json!({"feature": "Drill", "set": {"diameter": "1 in"}}))]);
    let r = run(&mut s, "drawing.update", json!({}));
    assert_eq!(r["orphans"].as_u64(), Some(0));
    assert_eq!(dimension_text(model(&s), h).as_deref(), Some("⌀1.00"));
    let (_, d) = dim(&s, h);
    assert!(close(d.p15.xy().dist(cadcraft_geom::Vec2::new(10.0, 8.0)), 0.5), "{d:?}");
    assert!(sick_dimensions(model(&s)).is_empty());
    // One undo step puts the old size back.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(dimension_text(model(&s), h).as_deref(), Some("⌀.75"));
}

#[test]
fn a_deleted_hole_leaves_a_sick_dimension() {
    let (mut s, block, v) = front("a_deleted_hole_leaves_a_sick_dimension");
    let hole = edge_at(&s, v, [10.375, 8.0]);
    let h = handle(&general(&mut s, v, json!([{"edge": hole}]), [11.0, 8.6]));
    let bottom = edge_at(&s, v, [10.0, 6.5]);
    let healthy = handle(&general(&mut s, v, json!([{"edge": bottom}]), [10.0, 5.8]));
    let (_, before) = dim(&s, h);
    edit_model(&block, &[("timeline.delete", json!({"feature": "Drill"}))]);
    let r = run(&mut s, "drawing.update", json!({}));
    assert_eq!(r["orphans"].as_u64(), Some(1));
    assert_eq!(sick_dimensions(model(&s)).into_iter().collect::<Vec<_>>(), [h]);
    assert_eq!(dimension_text(model(&s), h).as_deref(), Some("⌀.75"));
    // It keeps its geometry and its (dangling) association.
    assert_eq!(dim(&s, h).1, before);
    assert_eq!(dimension_text(model(&s), healthy).as_deref(), Some("3.00"));
}

#[test]
fn direct_commands_check_their_geometry() {
    let (mut s, _, v) = front("direct_commands_check_their_geometry");
    let hole = edge_at(&s, v, [10.375, 8.0]);
    let r = run(&mut s, "drawing.dim.radius", json!({"view": v, "picks": [{"edge": hole}], "placement": [11.0, 8.6]}));
    assert_eq!((r["kind"].as_str(), r["text"].as_str()), (Some("radius"), Some("R.38")));
    let bottom = edge_at(&s, v, [10.0, 6.5]);
    let undo = s.undo.len();
    let refused = s.execute("drawing.dim.diameter", &json!({"view": v, "picks": [{"edge": bottom}], "placement": [10.0, 5.8]}));
    assert_eq!(bad_message(refused), "a diameter needs a circle or an arc");
    assert_eq!(s.undo.len(), undo);
    // An arc measures as its circle (R51).
    let fillet = edge_at(&s, v, [8.146, 6.646]);
    let r = run(&mut s, "drawing.dim.diameter", json!({"view": v, "picks": [{"edge": fillet}], "placement": [7.5, 6.0]}));
    assert_eq!(r["text"].as_str(), Some("⌀1.00"));
    // Aligned on an edge, linear (horizontal by the placement) on the same edge.
    let r = run(&mut s, "drawing.dim.aligned", json!({"view": v, "picks": [{"edge": bottom}], "placement": [10.0, 5.8]}));
    assert_eq!((r["kind"].as_str(), r["text"].as_str()), (Some("aligned"), Some("3.00")));
    let r = run(&mut s, "drawing.dim.linear", json!({"view": v, "picks": [{"edge": bottom}], "placement": [10.0, 5.8]}));
    assert_eq!((r["kind"].as_str(), r["text"].as_str()), (Some("linear"), Some("3.00")));
    assert_eq!(
        bad_message(s.execute("drawing.dim.radius", &json!({"view": v, "picks": [{"edge": bottom}], "placement": [10.0, 5.8]}))),
        "a radius needs a circle or an arc"
    );
    assert_eq!(
        bad_message(s.execute("drawing.dim.general", &json!({"view": v, "picks": [{"edge": "no/such/edge"}], "placement": [10.0, 5.8]}))),
        "view 1 shows no edge no/such/edge"
    );
}

#[test]
fn a_dimension_over_an_edge_does_not_hide_the_edge_from_pick() {
    let (mut s, _, v) = front("a_dimension_over_an_edge_does_not_hide_the_edge_from_pick");
    let bottom = edge_at(&s, v, [10.0, 6.5]);
    let h = handle(&general(&mut s, v, json!([{"edge": bottom}]), [10.0, 5.8]));
    // The dimension's box (text and arrow room) covers the bottom edge: a point just off the
    // edge is in the box but is the edge's.
    match pick(&s, [10.0, 6.52], 0.05) {
        Some(PaperPick::ViewEdge { edge, .. }) => assert_eq!(edge, bottom),
        other => panic!("expected the bottom edge, got {other:?}"),
    }
    // Away from every view edge, the dimension is picked.
    assert_eq!(pick(&s, [10.0, 5.8], 0.05), Some(PaperPick::Dimension { handle: h }));
}

#[test]
fn dimension_commands_need_a_drawing() {
    let mut s = Session::default();
    run(&mut s, "document.new_part", json!({}));
    for id in ["drawing.dim.general", "drawing.dim.linear", "drawing.dim.aligned", "drawing.dim.diameter", "drawing.dim.radius", "drawing.dim.edit"] {
        let e = s.execute(id, &json!({"view": 1, "picks": [{"edge": "e"}], "placement": [0, 0], "dim": 1})).unwrap_err();
        assert!(e.to_string().contains("the active document is not a drawing"), "{id}: {e}");
    }
}

/// R56: two arcs of one geometry under one (edge, segment) key take keys from their kind and
/// arc order, so when the model feature that produced the first goes, the second never takes
/// over the first's handle: the dimensions on both go sick, neither silently re-attaches.
#[test]
fn same_key_arcs_never_trade_handles() {
    use crate::drawing::mapper;
    use crate::drawing::project::{PaperCurve, ViewCurve, ViewGeometry};
    use solidcraft_geom::Vec2 as MmVec2;

    let (mut s, _, v) = front("same_key_arcs_never_trade_handles");
    let arc = |start: f64, end: f64| ViewCurve {
        edge: "0/Block/e/F9:rim".into(),
        sources: vec!["0/Block/e/F9:rim".into()],
        segment: 0,
        hidden: false,
        curve: PaperCurve::Arc { c: MmVec2::new(0.0, 0.0), r: 9.525, start, end },
    };
    // The first arc (from the feature that will go) is listed first.
    let first = arc(0.0, 1.0);
    let second = arc(3.0, 4.0);
    let geometry =
        |curves: Vec<ViewCurve>| ViewGeometry { curves, min: MmVec2::new(-10.0, -10.0), max: MmVec2::new(10.0, 10.0), suppressed: Vec::new() };
    let both = geometry(vec![first.clone(), second.clone()]);
    crate::drawing::edit(&mut s, "test", |m, _| {
        let view = m.views.get(&v).unwrap().clone();
        mapper::place(m, &view, &both);
        Ok(())
    })
    .unwrap();
    let arcs = |s: &Session| -> Vec<(Handle, f64)> {
        let m = model(s);
        let layout = m.paper.layout(&m.sheets[0].layout).unwrap();
        layout
            .entities
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::Arc(a) if xkey_of(e).is_some_and(|k| k.view == v) => Some((e.handle, a.start)),
                _ => None,
            })
            .collect()
    };
    let placed = arcs(&s);
    assert_eq!(placed.len(), 2, "{placed:?}");
    let on_first = placed.iter().find(|(_, start)| start.abs() < 1e-9).unwrap().0;
    let on_second = placed.iter().find(|(_, start)| (start - 3.0).abs() < 1e-9).unwrap().0;
    let radius = |s: &mut Session, h: Handle| -> u64 {
        let m = model(s);
        let EntityKind::Arc(a) = &m.paper.entity(h).unwrap().kind else { panic!() };
        let (c, r) = (a.center.xy(), a.radius);
        let d = crate::drawing::dims::radial(&m.paper, h, c, r, false, c + cadcraft_geom::Vec2::new(1.0, 1.0), 1.0).unwrap();
        let layout = m.sheets[0].layout.clone();
        crate::drawing::edit(s, "test", |m, _| crate::drawing::dims::add(&mut m.paper, &layout, d)).unwrap().0
    };
    let dim_first = radius(&mut s, on_first);
    let dim_second = radius(&mut s, on_second);
    assert!(sick_dimensions(model(&s)).is_empty());
    // The feature that produced the first arc is gone: only the second is projected.
    let only_second = geometry(vec![second]);
    crate::drawing::edit(&mut s, "test", |m, _| {
        let view = m.views.get(&v).unwrap().clone();
        mapper::place(m, &view, &only_second);
        Ok(())
    })
    .unwrap();
    let now = arcs(&s);
    assert_eq!(now.len(), 1, "{now:?}");
    assert!(now[0].0 != on_first, "the second arc took over the first arc's handle");
    assert_eq!(sick_dimensions(model(&s)).into_iter().collect::<Vec<_>>(), [dim_first, dim_second]);
}

/// Every view key's segment as written (group 1071) and its key, for the view entities of `d`'s
/// layout `layout`.
fn written_keys(d: &cadcraft_doc::Drawing, layout: &str) -> Vec<(crate::drawing::mapper::XKey, String)> {
    let l = d.layout(layout).unwrap_or_else(|| panic!("no layout {layout}"));
    let mut out: Vec<_> = l
        .entities
        .iter()
        .filter_map(|e| {
            let key = xkey_of(e)?;
            let raw = e.common.xdata.iter().find(|t| t.code == 1071)?.value.clone();
            Some((key, raw))
        })
        .collect();
    out.sort();
    out
}

/// Task 11 review carry: a segment is DXF group 1071, a signed 32-bit integer, so bumped keys
/// (R51 collisions) must stay at most `i32::MAX`, and round-trip through DXF write → read
/// (ASCII and binary, R2018 and R2000) with the same value. Synthetic worst case: 258 polylines
/// on one (edge, segment 0xFFFFF): ranks clamp at 0xFF, so the largest is `i32::MAX` exactly and
/// the two past it take the overflow loop, which wraps inside the bumped range.
#[test]
fn bumped_keys_fit_dxf_1071_and_round_trip() {
    use crate::drawing::mapper::{self, Seg};
    use crate::drawing::project::{PaperCurve, ViewCurve, ViewGeometry};
    use solidcraft_geom::Vec2 as MmVec2;

    let (mut s, block, v) = front("bumped_keys_fit_dxf_1071_and_round_trip");
    // The fixture's own collisions: the back view's far rim (R51) and an iso view.
    for (orientation, at) in [("back", [24, 8]), ("iso_top_right", [24, 18])] {
        run(
            &mut s,
            "drawing.view.base",
            json!({"model": block, "orientation": orientation, "lines": "hidden", "shaded": false, "scale": 1, "position": at}),
        );
    }
    let layout = model(&s).sheets[0].layout.clone();
    let fixture_max = written_keys(&model(&s).paper, &layout)
        .iter()
        .filter_map(|(k, _)| match k.seg {
            Seg::Index(i) => Some(i),
            Seg::Mark => None,
        })
        .max()
        .unwrap();
    println!("largest fixture segment: {fixture_max:#x}");
    assert!(fixture_max <= i32::MAX as u32, "fixture segment {fixture_max:#x} overflows DXF 1071");

    let polys: Vec<ViewCurve> = (0..258)
        .map(|i| ViewCurve {
            edge: "0/Block/e/F9:synthetic".into(),
            sources: vec!["0/Block/e/F9:synthetic".into()],
            segment: 0xF_FFFF,
            hidden: false,
            curve: PaperCurve::Poly { points: vec![MmVec2::new(f64::from(i), 0.0), MmVec2::new(f64::from(i), 1.0)] },
        })
        .collect();
    let geometry = ViewGeometry { curves: polys, min: MmVec2::new(0.0, 0.0), max: MmVec2::new(258.0, 1.0), suppressed: Vec::new() };
    crate::drawing::edit(&mut s, "test", |m, _| {
        let view = m.views.get(&v).unwrap().clone();
        mapper::place(m, &view, &geometry);
        Ok(())
    })
    .unwrap();
    let before = written_keys(&model(&s).paper, &layout);
    let synthetic: Vec<u32> = before
        .iter()
        .filter(|(k, _)| k.view == v)
        .filter_map(|(k, _)| match k.seg {
            Seg::Index(i) => Some(i),
            Seg::Mark => None,
        })
        .collect();
    assert_eq!(synthetic.len(), 258, "every polyline keeps a distinct key");
    assert_eq!(synthetic.iter().max(), Some(&(i32::MAX as u32)));
    for (k, raw) in &before {
        assert!(raw.parse::<i32>().is_ok(), "{k:?} writes 1071 {raw}, not a signed 32-bit integer");
    }

    for version in [cadcraft_io::Version::R2018, cadcraft_io::Version::R2000] {
        for binary in [false, true] {
            let bytes = cadcraft_io::write_with(&model(&s).paper, "keys.dxf", &cadcraft_io::SaveOptions { version: Some(version), binary }).unwrap();
            let back = cadcraft_io::read(&bytes, "keys.dxf").unwrap();
            let after: Vec<_> = written_keys(&back, &layout).into_iter().map(|(k, _)| k).collect();
            let expected: Vec<_> = before.iter().map(|(k, _)| k.clone()).collect();
            assert_eq!(after, expected, "{version:?} binary {binary}");
        }
    }
}
