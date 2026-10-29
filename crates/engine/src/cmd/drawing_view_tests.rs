#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Placing drawing views (Task 7): base and projected views as paper entities keyed by
//! (view, model edge, segment) xdata, third-angle projection and alignment, picking and the
//! preview. The drawing tests' block, front view at [10, 8] (the plan's paper facts).

use std::collections::BTreeSet;

use cadcraft_doc::{EntityKind, Handle};
use serde_json::{Value, json};
use solidcraft_doc::{CubeTarget, DrawingModel, DrawingView, LineStyle};

use crate::cmd::drawing::drawing_fixture::{block_drawing, block_part, front_view, temp_dir};
use crate::drawing::mapper::{self, Seg, XKey, xkey_of};
use crate::drawing::{CurveKind, PaperPick, dimension_text, pick, preview_view, sick_dimensions, suppressed_annotations};
use crate::{EngineError, Session};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn model(s: &Session) -> &DrawingModel {
    s.doc.drawing.as_deref().unwrap()
}

fn view(s: &Session, id: u64) -> &DrawingView {
    model(s).views.get(&id).unwrap()
}

/// The entities of view `id` on its sheet's layout: (handle, layer, key, kind).
fn entities_of(s: &Session, id: u64) -> Vec<(Handle, String, XKey, EntityKind)> {
    let m = model(s);
    let layout = m.paper.layout(&m.sheets[view(s, id).sheet].layout).unwrap();
    layout
        .entities
        .iter()
        .filter_map(|e| xkey_of(e).filter(|k| k.view == id).map(|k| (e.handle, e.common.layer.clone(), k, e.kind.clone())))
        .collect()
}

/// The paper box of view `id`'s entities.
fn paper_box(s: &Session, id: u64) -> ([f64; 2], [f64; 2]) {
    let m = model(s);
    let mut b = cadcraft_doc::geom::Bounds2::EMPTY;
    for (h, ..) in entities_of(s, id) {
        b = b.union(&cadcraft_doc::entity_bounds(&m.paper, m.paper.entity(h).unwrap(), 0));
    }
    ([b.min.x, b.min.y], [b.max.x, b.max.y])
}

fn near(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
    (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol
}

fn assert_box(s: &Session, id: u64, min: [f64; 2], max: [f64; 2]) {
    let (lo, hi) = paper_box(s, id);
    assert!(near(lo, min, 1e-9) && near(hi, max, 1e-9), "{lo:?} {hi:?}");
}

fn bad_message(r: crate::Result<Value>) -> String {
    match r {
        Err(EngineError::BadParams { msg, .. }) => msg,
        other => panic!("expected bad params, got {other:?}"),
    }
}

/// `drawing.view.base` params of the block's front view at `position`.
fn front_params(block: &str, position: [f64; 2]) -> Value {
    json!({"model": block, "orientation": "front", "lines": "hidden", "shaded": false, "scale": 1, "position": position})
}

#[test]
fn base_view_places_view1_at_the_click() {
    let (mut s, block) = block_drawing("base_view_places_view1_at_the_click");
    let r = s.execute("drawing.view.base", &front_params(&block, [10.0, 8.0])).unwrap();
    assert_eq!(r["identifier"], "VIEW1");
    assert_eq!(r["entities"], json!({"visible": 9, "hidden": 0}));
    assert_eq!(r["raster"], "none");
    let id = r["view"].as_u64().unwrap();
    let v = view(&s, id);
    assert_eq!((v.identifier.as_str(), v.parent, v.scale), ("VIEW1", None, 1.0));
    assert_eq!(v.orientation.preset, CubeTarget::from_name("front"));
    assert_eq!(model(&s).next_view_number, 2);
    let placed = entities_of(&s, id);
    assert_eq!(placed.len(), 9);
    assert!(placed.iter().all(|(_, layer, k, _)| layer == "VISIBLE" && k.edge.starts_with("0/Block/") && matches!(k.seg, Seg::Index(_))));
    // The group lists exactly those handles.
    let group = model(&s).paper.groups.iter().find(|g| g.name == mapper::group_name(v.group)).unwrap();
    let members: BTreeSet<Handle> = group.members.iter().copied().collect();
    assert_eq!(members, placed.iter().map(|(h, ..)| *h).collect::<BTreeSet<_>>());
    assert_eq!(group.members.len(), 9);
    // Every key is unique, and each names its handle.
    let keys: BTreeSet<XKey> = placed.iter().map(|(_, _, k, _)| k.clone()).collect();
    assert_eq!(keys.len(), 9);
    for (h, _, k, _) in &placed {
        assert_eq!(mapper::handle_of(model(&s), k), Some(*h));
    }
    // The xdata as the spec writes it.
    let line = placed.iter().find(|(.., kind)| matches!(kind, EntityKind::Line(_))).unwrap();
    let xdata = &model(&s).paper.entity(line.0).unwrap().common.xdata;
    let tags: Vec<(i32, &str)> = xdata.iter().map(|t| (t.code, t.value.as_str())).collect();
    assert_eq!(tags, [(1001, "TCICAD_VIEW"), (1000, id.to_string().as_str()), (1000, line.2.edge.as_str()), (1071, "0")]);
    assert_box(&s, id, [8.0, 6.5], [12.0, 9.5]);
    // The hole circle: centre [10, 8], r = 0.375.
    let circle = placed.iter().find_map(|(.., kind)| if let EntityKind::Circle(c) = kind { Some(c.clone()) } else { None }).unwrap();
    assert!(near([circle.center.x, circle.center.y], [10.0, 8.0], 1e-9) && (circle.radius - 0.375).abs() <= 1e-9, "{circle:?}");
    // A second base view is VIEW2.
    assert_eq!(front_view(&mut s, &block), id + 1);
    assert_eq!(view(&s, id + 1).identifier, "VIEW2");
}

#[test]
fn scale_half_halves_the_view() {
    let (mut s, block) = block_drawing("scale_half_halves_the_view");
    let mut p = front_params(&block, [10.0, 8.0]);
    p["scale"] = json!(0.5);
    let id = run(&mut s, "drawing.view.base", p)["view"].as_u64().unwrap();
    assert_eq!(view(&s, id).scale, 0.5);
    assert_box(&s, id, [9.0, 7.25], [11.0, 8.75]);
}

#[test]
fn projected_views_follow_third_angle() {
    let (mut s, block) = block_drawing("projected_views_follow_third_angle");
    let base = front_view(&mut s, &block);
    let project = |s: &mut Session, at: [f64; 2]| -> (u64, Value) {
        let r = run(s, "drawing.view.projected", json!({"parent": base, "position": at}));
        (r["view"].as_u64().unwrap(), r)
    };
    let (top, r) = project(&mut s, [11.0, 16.0]);
    assert_eq!(r["identifier"], "VIEW2");
    assert_eq!(r["entities"]["hidden"], 2);
    let v = view(&s, top);
    assert_eq!((v.parent, v.orientation.preset), (Some(base), CubeTarget::from_name("top")));
    assert_eq!((v.position.x, v.position.y), (10.0, 16.0));
    let (right, r) = project(&mut s, [20.0, 8.4]);
    assert_eq!(r["entities"]["hidden"], 2);
    let v = view(&s, right);
    assert_eq!(v.orientation.preset, CubeTarget::from_name("right"));
    assert_eq!((v.position.x, v.position.y), (20.0, 8.0));
    let (left, _) = project(&mut s, [2.0, 8.0]);
    assert_eq!(view(&s, left).orientation.preset, CubeTarget::from_name("left"));
    assert_eq!((view(&s, left).position.x, view(&s, left).position.y), (2.0, 8.0));
    let (bottom, _) = project(&mut s, [10.0, 2.0]);
    assert_eq!(view(&s, bottom).orientation.preset, CubeTarget::from_name("bottom"));
    assert_eq!((view(&s, bottom).position.x, view(&s, bottom).position.y), (10.0, 2.0));
    let (iso, _) = project(&mut s, [18.0, 14.0]);
    assert_eq!(view(&s, iso).orientation.preset, CubeTarget::from_name("iso_top_right"));
    assert_eq!((view(&s, iso).position.x, view(&s, iso).position.y), (18.0, 14.0));
    assert_eq!(view(&s, iso).identifier, "VIEW6");
    // The top view: 4 x 1 in about [10, 16].
    assert_box(&s, top, [8.0, 15.5], [12.0, 16.5]);
}

#[test]
fn projected_views_inherit_style_and_scale() {
    let (mut s, block) = block_drawing("projected_views_inherit_style_and_scale");
    let mut p = front_params(&block, [10.0, 8.0]);
    p["scale"] = json!(0.5);
    p["lines"] = json!("removed");
    let base = run(&mut s, "drawing.view.base", p)["view"].as_u64().unwrap();
    let r = run(&mut s, "drawing.view.projected", json!({"parent": base, "position": [10, 14]}));
    assert_eq!(r["entities"], json!({"visible": r["entities"]["visible"], "hidden": 0}));
    let v = view(&s, r["view"].as_u64().unwrap());
    assert_eq!((v.scale, v.lines, v.style_from_base, v.scale_from_base), (0.5, LineStyle::Removed, true, true));
    assert_eq!(v.sheet, 0);
}

#[test]
fn each_placed_view_is_one_undo_step() {
    let (mut s, block) = block_drawing("each_placed_view_is_one_undo_step");
    let undo = s.undo.len();
    let base = front_view(&mut s, &block);
    run(&mut s, "drawing.view.projected", json!({"parent": base, "position": [10, 16]}));
    run(&mut s, "drawing.view.projected", json!({"parent": base, "position": [20, 8]}));
    assert_eq!((model(&s).views.len(), s.undo.len()), (3, undo + 3));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(model(&s).views.len(), 2);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(model(&s).views.len(), 1);
    run(&mut s, "edit.undo", json!({}));
    let m = model(&s);
    assert!(m.views.is_empty());
    let layout = m.paper.layout(&m.sheets[0].layout).unwrap();
    assert_eq!(layout.entities.handles(), vec![m.sheets[0].border, m.sheets[0].title_block]);
    assert!(m.paper.groups.is_empty());
}

#[test]
fn orientation_as_dir_and_up() {
    let (mut s, block) = block_drawing("orientation_as_dir_and_up");
    let mut p = front_params(&block, [10.0, 8.0]);
    p["orientation"] = json!({"dir": [0, 0, -1], "up": [0, 1, 0]});
    let top = run(&mut s, "drawing.view.base", p.clone())["view"].as_u64().unwrap();
    assert_eq!(view(&s, top).orientation.preset, CubeTarget::from_name("top"));
    p["orientation"] = json!({"dir": [0.3, 0.9, 0], "up": [0, 0, 1]});
    p["position"] = json!([20, 8]);
    let r = run(&mut s, "drawing.view.base", p);
    let v = view(&s, r["view"].as_u64().unwrap());
    assert_eq!(v.orientation.preset, None);
    let d = v.orientation.dir;
    assert!((d.len() - 1.0).abs() <= 1e-12 && (d.x - 0.3 / 0.9_f64.hypot(0.3)).abs() <= 1e-12, "{d:?}");
    assert!(r["entities"]["visible"].as_u64().unwrap() > 0);
}

#[test]
fn degenerate_orientations_and_scales_are_refused() {
    let (mut s, block) = block_drawing("degenerate_orientations_and_scales_are_refused");
    let base = |change: &dyn Fn(&mut Value)| {
        let mut p = front_params(&block, [10.0, 8.0]);
        change(&mut p);
        p
    };
    let cases: Vec<(&str, Value, String)> = vec![
        ("drawing.view.base", base(&|p| p["scale"] = json!(0)), "scale must be a number > 0".into()),
        ("drawing.view.base", base(&|p| p["scale"] = json!(-1)), "scale must be a number > 0".into()),
        ("drawing.view.base", base(&|p| p["scale"] = json!("NaN")), "scale must be a number > 0".into()),
        ("drawing.view.base", base(&|p| p["position"] = json!([f64::NAN, 1])), "position must be two finite numbers".into()),
        ("drawing.view.base", base(&|p| p["orientation"] = json!({"dir": [0, 0, 0], "up": [0, 0, 1]})), "dir must be a non-zero vector".into()),
        ("drawing.view.base", base(&|p| p["orientation"] = json!({"dir": [0, 1, 0], "up": [0, 2, 0]})), "up must not be parallel to dir".into()),
        ("drawing.view.base", base(&|p| p["orientation"] = json!("sideways")), "unknown orientation sideways".into()),
        ("drawing.view.projected", json!({"parent": 99, "position": [10, 16]}), "no view 99".into()),
    ];
    let undo = s.undo.len();
    for (cmd, p, message) in cases {
        assert_eq!(bad_message(s.execute(cmd, &p)), message, "{cmd} {p}");
        assert!(model(&s).views.is_empty(), "{cmd} {p}");
    }
    assert_eq!(s.undo.len(), undo);
    // A part is not a drawing: both commands fail cleanly.
    let mut part = Session::default();
    run(&mut part, "document.new_part", json!({}));
    assert!(part.execute("drawing.view.base", &front_params(&block, [10.0, 8.0])).is_err());
    assert!(part.execute("drawing.view.projected", &json!({"parent": 1, "position": [1, 1]})).is_err());
}

#[test]
fn pick_finds_the_edge_under_the_point() {
    let (mut s, block) = block_drawing("pick_finds_the_edge_under_the_point");
    let id = front_view(&mut s, &block);
    let edge = |p: Option<PaperPick>| match p {
        Some(PaperPick::ViewEdge { view, curve, edge, handle }) => {
            assert_eq!(view, id);
            let e = model(&s).paper.entity(Handle(handle)).unwrap();
            assert_eq!(xkey_of(e).unwrap().edge, edge);
            curve
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(edge(pick(&s, [10.0, 6.52], 0.05)), CurveKind::Line);
    assert_eq!(edge(pick(&s, [10.375, 8.0], 0.05)), CurveKind::Circle);
    assert_eq!(edge(pick(&s, [8.146, 6.646], 0.05)), CurveKind::Arc);
    assert_eq!(pick(&s, [30.0, 2.0], 0.05), None);
    // Inside the view, away from its curves: the view.
    assert_eq!(pick(&s, [9.0, 7.5], 0.05), Some(PaperPick::View { view: id }));
    assert_eq!(crate::cmd::drawing::drawing_fixture::edge_at(&s, id, [10.0, 6.52]), edge_id(&s, id, CurveKind::Line, [10.0, 6.5]));
}

/// The edge of view `id`'s curve of `kind` through `at`.
fn edge_id(s: &Session, id: u64, kind: CurveKind, at: [f64; 2]) -> String {
    match pick(s, at, 1e-6) {
        Some(PaperPick::ViewEdge { view, curve, edge, .. }) if view == id && curve == kind => edge,
        other => panic!("{other:?}"),
    }
}

#[test]
fn preview_matches_the_placed_view_and_changes_nothing() {
    let (mut s, block) = block_drawing("preview_matches_the_placed_view_and_changes_nothing");
    let (undo, revision) = (s.undo.len(), s.revision);
    let curves = preview_view(&s, "drawing.view.base", &front_params(&block, [10.0, 8.0])).unwrap();
    assert_eq!(curves.len(), 9);
    assert!(curves.iter().all(|(_, hidden)| !hidden));
    assert_eq!((s.undo.len(), s.revision), (undo, revision));
    assert!(model(&s).views.is_empty());
    // The same curves as the placed view.
    let id = front_view(&mut s, &block);
    let placed = entities_of(&s, id);
    for (curve, _) in &curves {
        let found = placed.iter().any(|(.., kind)| match (curve, kind) {
            (crate::drawing::PaperCurve::Line { a, b }, EntityKind::Line(l)) => {
                near([a.x, a.y], [l.a.x, l.a.y], 1e-9) && near([b.x, b.y], [l.b.x, l.b.y], 1e-9)
            }
            (crate::drawing::PaperCurve::Circle { c, r }, EntityKind::Circle(e)) => {
                near([c.x, c.y], [e.center.x, e.center.y], 1e-9) && (r - e.radius).abs() <= 1e-9
            }
            (crate::drawing::PaperCurve::Arc { c, r, .. }, EntityKind::Arc(e)) => {
                near([c.x, c.y], [e.center.x, e.center.y], 1e-9) && (r - e.radius).abs() <= 1e-9
            }
            _ => false,
        });
        assert!(found, "{curve:?}");
    }
    // A projected preview from it: the top view's 2 hidden lines.
    let top = preview_view(&s, "drawing.view.projected", &json!({"parent": id, "position": [10, 16]})).unwrap();
    assert_eq!(top.iter().filter(|(_, hidden)| *hidden).count(), 2);
    assert_eq!(model(&s).views.len(), 1);
}

#[test]
fn assembly_views_prefix_component_paths() {
    let dir = temp_dir("assembly_views_prefix_component_paths");
    let block = block_part(&dir);
    let asm = dir.join("asm.tciasm").to_string_lossy().into_owned();
    let mut a = Session::default();
    run(&mut a, "component.insert_design", json!({"path": block}));
    run(&mut a, "file.save", json!({"path": asm}));
    let mut s = Session::default();
    run(&mut s, "drawing.new", json!({"model": asm}));
    let r = run(&mut s, "drawing.view.base", front_params(&asm, [10.0, 8.0]));
    let id = r["view"].as_u64().unwrap();
    assert_eq!(view(&s, id).model.kind, solidcraft_doc::ModelKind::Assembly);
    let placed = entities_of(&s, id);
    assert_eq!(placed.len(), 9, "{placed:?}");
    assert!(placed.iter().all(|(_, _, k, _)| !k.edge.starts_with("0/")), "{placed:?}");
}

#[test]
fn placing_again_changes_entities_in_place() {
    // The seam's regeneration (spec §4.5): the same keys keep their handles and move in place, a
    // key no longer produced is removed, and the group follows.
    let (mut s, block) = block_drawing("placing_again_changes_entities_in_place");
    let id = front_view(&mut s, &block);
    let planned = super::plan(&s, "drawing.view.base", &front_params(&block, [10.0, 8.0])).unwrap();
    let mut m = model(&s).clone();
    let mut v = view(&s, id).clone();
    let before: BTreeSet<Handle> = entities_of(&s, id).into_iter().map(|(h, ..)| h).collect();
    let layout_len = |m: &DrawingModel| m.paper.layout(&m.sheets[0].layout).unwrap().entities.len();
    let count = layout_len(&m);
    v.position = solidcraft_geom::Vec2::new(12.0, 9.0);
    mapper::place(&mut m, &v, &planned.geometry);
    assert_eq!(layout_len(&m), count);
    let moved: Vec<(Handle, XKey)> =
        m.paper.layout(&m.sheets[0].layout).unwrap().entities.iter().filter_map(|e| xkey_of(e).map(|k| (e.handle, k))).collect();
    assert_eq!(moved.iter().map(|(h, _)| *h).collect::<BTreeSet<_>>(), before);
    let circle = moved.iter().find_map(|(h, _)| match &m.paper.entity(*h).unwrap().kind {
        EntityKind::Circle(c) => Some(c.center),
        _ => None,
    });
    let c = circle.unwrap();
    assert!(near([c.x, c.y], [12.0, 9.0], 1e-9), "{c:?}");
    // Drop the first curve: its entity goes, the others stay.
    let mut fewer = planned.geometry.clone();
    let gone = fewer.curves.remove(0);
    mapper::place(&mut m, &v, &fewer);
    assert_eq!(layout_len(&m), count - 1);
    let key = XKey { view: id, edge: gone.edge, seg: Seg::Index(gone.segment) };
    assert_eq!(mapper::handle_of(&m, &key), None);
    let group = m.paper.groups.iter().find(|g| g.name == mapper::group_name(v.group)).unwrap();
    assert_eq!(group.members.len(), 8);
}

/// View `id`'s Lines on VISIBLE: (drawn, invisible). R94: tangent edges turned off stay on paper
/// as invisible entities.
fn visible_lines(s: &Session, id: u64) -> (usize, usize) {
    let m = model(s);
    let layout = m.paper.layout(&m.sheets[view(s, id).sheet].layout).unwrap();
    let on: Vec<bool> = layout
        .entities
        .iter()
        .filter(|e| xkey_of(e).is_some_and(|k| k.view == id && k.seg != Seg::Mark))
        .filter(|e| e.common.layer == "VISIBLE" && matches!(e.kind, EntityKind::Line(_)))
        .map(|e| e.common.visible)
        .collect();
    (on.iter().filter(|v| **v).count(), on.iter().filter(|v| !**v).count())
}

/// The block's front view at [10, 8] and its top view projected at [10, 16].
fn front_and_top(name: &str) -> (Session, u64, u64) {
    let (mut s, block) = block_drawing(name);
    let v1 = front_view(&mut s, &block);
    let v2 = run(&mut s, "drawing.view.projected", json!({"parent": v1, "position": [10, 16]}))["view"].as_u64().unwrap();
    (s, v1, v2)
}

/// R66, R73: `set_style`'s `tangent_edges` re-projects the view and the views that follow its
/// style, in one step. The top view's joins at sheet x = 8.5 and 11.5 (R64) are drawn only when
/// on; when off they stay on paper, invisible (R94).
#[test]
fn set_style_turns_tangent_edges_on_and_off() {
    let (mut s, v1, v2) = front_and_top("set_style_turns_tangent_edges_on_and_off");
    assert_eq!(visible_lines(&s, v2), (10, 2));
    let off: BTreeSet<Handle> = entities_of(&s, v2).into_iter().map(|(h, ..)| h).collect();
    let undo = s.undo.len();
    let r = run(&mut s, "drawing.view.set_style", json!({"view": v1, "tangent_edges": true}));
    assert_eq!(r["changed"], json!([v1, v2]));
    assert_eq!(r["suppressed"], json!(0));
    assert_eq!((s.undo.len(), s.undo.last().map(|u| u.label.as_str())), (undo + 1, Some("View Style")));
    assert!(view(&s, v1).tangent_edges && view(&s, v2).tangent_edges && view(&s, v2).style_from_base);
    assert_eq!(visible_lines(&s, v2), (12, 0));
    // The same handles: the joins were on paper all along.
    assert_eq!(entities_of(&s, v2).into_iter().map(|(h, ..)| h).collect::<BTreeSet<_>>(), off);
    let joins: Vec<f64> = entities_of(&s, v2)
        .into_iter()
        .filter_map(|(_, _, k, kind)| match kind {
            EntityKind::Line(l) if k.edge.contains("F1:+z|F3:blend") => Some(l.a.x),
            _ => None,
        })
        .collect();
    assert_eq!(joins.len(), 2, "{joins:?}");
    assert!(joins.iter().any(|x| (x - 8.5).abs() <= 1e-9) && joins.iter().any(|x| (x - 11.5).abs() <= 1e-9), "{joins:?}");
    // Again: nothing changes, no step.
    let r = run(&mut s, "drawing.view.set_style", json!({"view": v1, "tangent_edges": true}));
    assert_eq!(r["changed"], json!([]));
    assert_eq!(s.undo.len(), undo + 1);
    // Off on the projected view alone: it stops following its parent.
    let r = run(&mut s, "drawing.view.set_style", json!({"view": v2, "tangent_edges": false}));
    assert_eq!(r["changed"], json!([v2]));
    assert_eq!(visible_lines(&s, v2), (10, 2));
    assert!(!view(&s, v2).style_from_base && !view(&s, v2).tangent_edges && view(&s, v1).tangent_edges);
    assert_eq!(
        bad_message(s.execute("drawing.view.set_style", &json!({"view": v1, "tangent_edges": "yes"}))),
        "`tangent_edges` must be true or false"
    );
    assert_eq!(bad_message(s.execute("drawing.view.set_style", &json!({"view": v1}))), "name lines, shaded, tangent_edges or style_from_base");
    assert_eq!(
        bad_message(s.execute("drawing.view.set_style", &json!({"view": v2, "style_from_base": true, "tangent_edges": true}))),
        "style_from_base: true takes the parent's style; give lines, shaded or tangent_edges without it"
    );
    // Following the parent again takes its tangent edges.
    run(&mut s, "drawing.view.set_style", json!({"view": v2, "style_from_base": true}));
    assert_eq!(visible_lines(&s, v2), (12, 0));
}

/// R94 (overrides R73): a dimension on a tangent edge is suppressed, not sick, while tangent
/// edges are off (its edge stays on paper, invisible, as a hidden line does under Hidden Line
/// Removed, R61), and comes back with them.
#[test]
fn a_dimension_on_a_tangent_edge_is_suppressed_when_they_turn_off() {
    let (mut s, v1, v2) = front_and_top("a_dimension_on_a_tangent_edge_is_suppressed_when_they_turn_off");
    run(&mut s, "drawing.view.set_style", json!({"view": v1, "tangent_edges": true}));
    let r = run(&mut s, "drawing.dim.general", json!({"view": v2, "picks": [{"at": [8.5, 16]}], "placement": [7.5, 16]}));
    assert_eq!(r["text"], "1.00");
    let dim = r["handle"].as_u64().unwrap();
    let r = run(&mut s, "drawing.view.set_style", json!({"view": v1, "tangent_edges": false}));
    assert_eq!(r["suppressed"], json!(1));
    let m = model(&s);
    assert_eq!(suppressed_annotations(m), BTreeSet::from([dim]));
    assert!(sick_dimensions(m).is_empty());
    assert!(!m.paper.entity(Handle(dim)).unwrap().common.visible);
    assert_eq!(dimension_text(m, dim).as_deref(), Some("1.00"));
    // Picking there finds no edge: the join is not drawn.
    assert!(s.execute("drawing.dim.general", &json!({"view": v2, "picks": [{"at": [8.5, 16]}], "placement": [7.5, 16]})).is_err());
    let r = run(&mut s, "drawing.update", json!({}));
    assert_eq!(r["orphans"], json!(0));
    let r = run(&mut s, "drawing.view.set_style", json!({"view": v1, "tangent_edges": true}));
    assert_eq!(r["suppressed"], json!(0));
    let m = model(&s);
    assert!(suppressed_annotations(m).is_empty() && sick_dimensions(m).is_empty());
    assert!(m.paper.entity(Handle(dim)).unwrap().common.visible);
    assert_eq!(dimension_text(m, dim).as_deref(), Some("1.00"));
}

/// Wave 3 item 3 (R100): the default scale of a new base view measures the model (4 x 1 x 3 in,
/// fitting at 1:1 on D and on an A portrait sheet) on the active sheet.
#[test]
fn the_default_scale_measures_the_model_on_the_active_sheet() {
    use crate::drawing::{default_scale, model_extents_mm};
    let mm = |x: f64, y: f64, z: f64| [x * 25.4, y * 25.4, z * 25.4];
    let (mut s, block) = block_drawing("the_default_scale_measures_the_model");
    let e = model_extents_mm(&s, &block).unwrap();
    for (got, want) in e.iter().zip(mm(4.0, 1.0, 3.0)) {
        assert!((got - want).abs() < 1e-6, "{e:?}");
    }
    assert_eq!(default_scale(&s, &block), Some(1.0));
    // An A portrait sheet (8.5 × 11, printable 7.5 × 10): across (4 + 1)·1 + 1 = 6 ≤ 7.5, up (3 + 1)·1 + 1 = 5 ≤ 10 → 1:1.
    s.execute("drawing.sheet.set_size", &json!({"size": "A", "landscape": false})).unwrap();
    assert_eq!(default_scale(&s, &block), Some(1.0));
    assert_eq!(default_scale(&s, "no/such/model.tcipart"), None);
}

/// Fix wave 3 item 9: `model_ref::load` finds a model by its path and reads whatever document is
/// there (the ref's kind is not consulted), so an assembly (`.tciasm`) measures as its
/// components do: the block inserted once, 4 × 1 × 3 in.
#[test]
fn the_default_scale_measures_an_assembly() {
    use crate::drawing::{default_scale, model_extents_mm};
    let dir = temp_dir("the_default_scale_measures_an_assembly");
    let block = block_part(&dir);
    let asm = dir.join("asm.tciasm").to_string_lossy().into_owned();
    let mut a = Session::default();
    run(&mut a, "component.insert_design", json!({"path": block}));
    run(&mut a, "file.save", json!({"path": asm}));
    let mut s = Session::default();
    run(&mut s, "drawing.new", json!({"model": asm}));
    let e = model_extents_mm(&s, &asm).expect("the assembly measures");
    for (got, want) in e.iter().zip([4.0 * 25.4, 25.4, 3.0 * 25.4]) {
        assert!((got - want).abs() < 1e-6, "{e:?}");
    }
    assert_eq!(default_scale(&s, &asm), Some(1.0));
}
