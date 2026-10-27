#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Editing placed views and regenerating them from the model (Task 8): style, scale, position,
//! orientation, label and delete, `drawing.update` in place (handles kept), `stale_views`, a
//! missing model, and model paths rebased when the drawing is saved elsewhere. VIEW1 is the
//! block's front view at [10, 8], VIEW2 the top projected at [10, 16], VIEW3 the right at [20, 8].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cadcraft_doc::{EntityKind, Handle};
use serde_json::{Value, json};
use solidcraft_doc::{CubeTarget, Document, DrawingModel, DrawingView, LineStyle};
use solidcraft_geom::Vec3;

use crate::cmd::drawing::drawing_fixture::{block_drawing, edit_model, front_view};
use crate::drawing::mapper::{self, xkey_of};
use crate::drawing::{ModelProvider, PaperPick, pick, stale_views};
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

fn bad_message(r: crate::Result<Value>) -> String {
    match r {
        Err(EngineError::BadParams { msg, .. }) => msg,
        other => panic!("expected bad params, got {other:?}"),
    }
}

/// The curves of view `id` (keyed entities, centre marks included): (handle, layer, kind).
fn curves(s: &Session, id: u64) -> Vec<(Handle, String, EntityKind)> {
    let m = model(s);
    let layout = m.paper.layout(&m.sheets[view(s, id).sheet].layout).unwrap();
    layout.entities.iter().filter(|e| xkey_of(e).is_some_and(|k| k.view == id)).map(|e| (e.handle, e.common.layer.clone(), e.kind.clone())).collect()
}

fn handles(s: &Session, id: u64) -> BTreeSet<Handle> {
    curves(s, id).into_iter().map(|(h, ..)| h).collect()
}

fn hidden(s: &Session, id: u64) -> usize {
    curves(s, id).iter().filter(|(_, layer, _)| layer == "HIDDEN").count()
}

/// The circle of view `id`: (handle, centre, radius).
fn circle(s: &Session, id: u64) -> (Handle, [f64; 2], f64) {
    curves(s, id)
        .into_iter()
        .find_map(|(h, _, k)| match k {
            EntityKind::Circle(c) => Some((h, [c.center.x, c.center.y], c.radius)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("view {id} has no circle"))
}

fn paper_box(s: &Session, id: u64) -> ([f64; 2], [f64; 2]) {
    let m = model(s);
    let mut b = cadcraft_doc::geom::Bounds2::EMPTY;
    for (h, ..) in curves(s, id) {
        b = b.union(&cadcraft_doc::entity_bounds(&m.paper, m.paper.entity(h).unwrap(), 0));
    }
    ([b.min.x, b.min.y], [b.max.x, b.max.y])
}

fn near(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
    (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol
}

fn assert_box(s: &Session, id: u64, min: [f64; 2], max: [f64; 2]) {
    let (lo, hi) = paper_box(s, id);
    assert!(near(lo, min, 1e-6) && near(hi, max, 1e-6), "view {id}: {lo:?} {hi:?}");
}

fn position(s: &Session, id: u64) -> [f64; 2] {
    let p = view(s, id).position;
    [p.x, p.y]
}

/// The block drawing with VIEW1 (front, [10, 8]), VIEW2 (top, [10, 16]) and VIEW3 (right, [20, 8]).
fn three_views(name: &str) -> (Session, String, [u64; 3]) {
    let (mut s, block) = block_drawing(name);
    let v1 = front_view(&mut s, &block);
    let v2 = run(&mut s, "drawing.view.projected", json!({"parent": v1, "position": [10, 16]}))["view"].as_u64().unwrap();
    let v3 = run(&mut s, "drawing.view.projected", json!({"parent": v1, "position": [20, 8]}))["view"].as_u64().unwrap();
    (s, block, [v1, v2, v3])
}

/// The texts on NOTE that belong to no view curve: (value, insertion point).
fn notes(s: &Session) -> Vec<(String, [f64; 2])> {
    let m = model(s);
    let layout = m.paper.layout(&m.sheets[0].layout).unwrap();
    layout
        .entities
        .iter()
        .filter(|e| e.common.layer == "NOTE")
        .filter_map(|e| match &e.kind {
            EntityKind::Text(t) => Some((t.value.clone(), [t.insert.x, t.insert.y])),
            _ => None,
        })
        .collect()
}

#[test]
fn set_style_children_follow() {
    let (mut s, _, [v1, v2, v3]) = three_views("set_style_children_follow");
    assert_eq!((hidden(&s, v2), hidden(&s, v3)), (2, 2));
    let undo = s.undo.len();
    run(&mut s, "drawing.view.set_style", json!({"view": v1, "lines": "removed"}));
    assert_eq!(s.undo.len(), undo + 1);
    assert_eq!((view(&s, v1).lines, view(&s, v2).lines, view(&s, v3).lines), (LineStyle::Removed, LineStyle::Removed, LineStyle::Removed));
    assert_eq!((hidden(&s, v2), hidden(&s, v3)), (0, 0));
    run(&mut s, "drawing.view.set_style", json!({"view": v2, "style_from_base": false, "lines": "hidden"}));
    assert_eq!((view(&s, v2).style_from_base, view(&s, v2).lines, hidden(&s, v2)), (false, LineStyle::Hidden, 2));
    // V1 changes again: V3 follows, V2 no longer does.
    run(&mut s, "drawing.view.set_style", json!({"view": v1, "lines": "hidden"}));
    assert_eq!((hidden(&s, v2), hidden(&s, v3)), (2, 2));
    run(&mut s, "drawing.view.set_style", json!({"view": v1, "lines": "removed"}));
    assert_eq!((hidden(&s, v2), hidden(&s, v3)), (2, 0));
    assert_eq!(view(&s, v2).lines, LineStyle::Hidden);
    assert_eq!(s.undo.len(), undo + 4);
    // Nothing to set is refused.
    assert_eq!(bad_message(s.execute("drawing.view.set_style", &json!({"view": v1}))), "name lines, shaded or style_from_base");
}

#[test]
fn set_scale_follows_scale_from_base() {
    let (mut s, _, [v1, v2, v3]) = three_views("set_scale_follows_scale_from_base");
    let before = handles(&s, v2);
    run(&mut s, "drawing.view.set_scale", json!({"view": v1, "scale": 0.5}));
    assert_eq!((view(&s, v1).scale, view(&s, v2).scale, view(&s, v3).scale), (0.5, 0.5, 0.5));
    // The top view: 4 x 1 in at half scale, about its unchanged centre.
    assert_box(&s, v2, [9.0, 15.75], [11.0, 16.25]);
    assert_box(&s, v1, [9.0, 7.25], [11.0, 8.75]);
    assert_eq!((position(&s, v1), position(&s, v2), position(&s, v3)), ([10.0, 8.0], [10.0, 16.0], [20.0, 8.0]));
    assert_eq!(handles(&s, v2), before);
    // A child's own scale stops it following.
    run(&mut s, "drawing.view.set_scale", json!({"view": v3, "scale": "2:1"}));
    assert_eq!((view(&s, v3).scale, view(&s, v3).scale_from_base), (2.0, false));
    run(&mut s, "drawing.view.set_scale", json!({"view": v1, "scale": 1}));
    assert_eq!((view(&s, v2).scale, view(&s, v3).scale), (1.0, 2.0));
    run(&mut s, "drawing.view.set_scale", json!({"view": v3, "scale_from_base": true}));
    assert_eq!((view(&s, v3).scale, view(&s, v3).scale_from_base), (1.0, true));
    assert_eq!(bad_message(s.execute("drawing.view.set_scale", &json!({"view": v1, "scale": 0}))), "scale must be a number > 0");
}

#[test]
fn moving_views_keeps_alignment() {
    let (mut s, _, [v1, v2, v3]) = three_views("moving_views_keeps_alignment");
    let (h1, h2, h3) = (handles(&s, v1), handles(&s, v2), handles(&s, v3));
    run(&mut s, "drawing.view.set_position", json!({"view": v2, "position": [14, 16]}));
    assert!(near(position(&s, v2), [10.0, 16.0], 1e-6));
    run(&mut s, "drawing.view.set_position", json!({"view": v1, "position": [12, 8]}));
    assert!(near(position(&s, v1), [12.0, 8.0], 1e-6));
    assert!(near(position(&s, v2), [12.0, 16.0], 1e-6), "{:?}", position(&s, v2));
    assert!(near(position(&s, v3), [22.0, 8.0], 1e-6), "{:?}", position(&s, v3));
    // The entities moved with them, in place.
    assert_box(&s, v1, [10.0, 6.5], [14.0, 9.5]);
    assert_box(&s, v2, [10.0, 15.5], [14.0, 16.5]);
    let (_, c, r) = circle(&s, v1);
    assert!(near(c, [12.0, 8.0], 1e-6) && (r - 0.375).abs() <= 1e-9);
    assert_eq!((handles(&s, v1), handles(&s, v2), handles(&s, v3)), (h1, h2, h3));
    assert_eq!(
        bad_message(s.execute("drawing.view.set_position", &json!({"view": v1, "position": [f64::NAN, 1]}))),
        "position must be two finite numbers"
    );
}

#[test]
fn only_base_views_turn() {
    let (mut s, _, [v1, v2, _]) = three_views("only_base_views_turn");
    let undo = s.undo.len();
    assert_eq!(
        bad_message(s.execute("drawing.view.set_orientation", &json!({"view": v2, "orientation": "top"}))),
        "only a base view's orientation can be set (VIEW2 is projected from VIEW1)"
    );
    assert_eq!(s.undo.len(), undo);
    run(&mut s, "drawing.view.set_orientation", json!({"view": v1, "orientation": "top"}));
    assert_eq!(s.undo.len(), undo + 1);
    assert_eq!(view(&s, v1).orientation.preset, CubeTarget::from_name("top"));
    let o = &view(&s, v2).orientation;
    let close = |a: Vec3, b: Vec3| (a - b).len() <= 1e-9;
    assert!(close(o.dir, Vec3::new(0.0, -1.0, 0.0)) && close(o.up, Vec3::new(0.0, 0.0, -1.0)), "{o:?}");
    assert_eq!(o.preset, None);
    // Both regenerated: V1 is the 4 x 1 in top; V2 looks through the hole, 4 x 3 in.
    assert_box(&s, v1, [8.0, 7.5], [12.0, 8.5]);
    assert_box(&s, v2, [8.0, 14.5], [12.0, 17.5]);
    // The hole's far rim: arcs of r 0.375 about [10, 16] (the kernel splits that rim, so it
    // is not joined into one circle).
    let rim: Vec<([f64; 2], f64)> = curves(&s, v2)
        .into_iter()
        .filter_map(|(.., k)| match k {
            EntityKind::Arc(a) if (a.radius - 0.375).abs() <= 1e-9 => Some(([a.center.x, a.center.y], a.radius)),
            EntityKind::Circle(c) if (c.radius - 0.375).abs() <= 1e-9 => Some(([c.center.x, c.center.y], c.radius)),
            _ => None,
        })
        .collect();
    assert!(!rim.is_empty() && rim.iter().all(|(c, _)| near(*c, [10.0, 16.0], 1e-6)), "{rim:?}");
}

#[test]
fn labels_show_name_and_scale() {
    let (mut s, _, [_, v2, v3]) = three_views("labels_show_name_and_scale");
    run(&mut s, "drawing.view.set_label", json!({"view": v3, "identifier": "SIDE", "label_visible": true}));
    assert_eq!((view(&s, v3).identifier.as_str(), view(&s, v3).label_visible), ("SIDE", true));
    let texts = notes(&s);
    let values: Vec<&str> = texts.iter().map(|(v, _)| v.as_str()).collect();
    assert_eq!(values, ["SIDE", "SCALE 1:1"]);
    // Under the view (the right view is [19.5, 6.5]-[20.5, 9.5]), centred on it.
    assert!(texts.iter().all(|(_, at)| (at[0] - 20.0).abs() <= 1e-9 && at[1] < 6.5), "{texts:?}");
    assert!(texts[0].1[1] > texts[1].1[1]);
    // The view's group carries the new identifier.
    let group = model(&s).paper.groups.iter().find(|g| g.name == mapper::group_name(view(&s, v3).group)).unwrap();
    assert_eq!(group.description, "SIDE");
    assert_eq!(bad_message(s.execute("drawing.view.set_label", &json!({"view": v2, "identifier": "SIDE"}))), "identifier SIDE is taken");
    // Moving the view moves its label; hiding it removes the texts.
    run(&mut s, "drawing.view.set_position", json!({"view": v3, "position": [22, 8]}));
    assert!(notes(&s).iter().all(|(_, at)| (at[0] - 22.0).abs() <= 1e-9), "{:?}", notes(&s));
    run(&mut s, "drawing.view.set_label", json!({"view": v3, "label_visible": false}));
    assert!(notes(&s).is_empty());
}

#[test]
fn delete_takes_children_unless_kept() {
    let (mut s, _, [v1, v2, v3]) = three_views("delete_takes_children_unless_kept");
    let kept = (handles(&s, v2), handles(&s, v3));
    run(&mut s, "drawing.view.delete", json!({"view": v1}));
    let m = model(&s);
    assert!(m.views.is_empty());
    let layout = m.paper.layout(&m.sheets[0].layout).unwrap();
    assert_eq!(layout.entities.handles(), vec![m.sheets[0].border, m.sheets[0].title_block]);
    assert!(m.paper.groups.is_empty());
    // No view shows the block any more: its loaded model is dropped (Task 7's cache concern).
    assert!(s.loaded_models.lock().unwrap().is_empty());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(model(&s).views.len(), 3);
    run(&mut s, "drawing.view.delete", json!({"view": v1, "children": "keep"}));
    assert_eq!(model(&s).views.keys().copied().collect::<Vec<_>>(), [v2, v3]);
    for id in [v2, v3] {
        let v = view(&s, id);
        assert_eq!((v.parent, v.style_from_base, v.scale_from_base), (None, false, false));
    }
    assert_eq!((handles(&s, v2), handles(&s, v3)), kept);
    let m = model(&s);
    assert!(m.paper.layout(&m.sheets[0].layout).unwrap().entities.iter().all(|e| mapper::view_of(e) != Some(v1)));
    assert_eq!(model(&s).paper.groups.len(), 2);
    assert_eq!(bad_message(s.execute("drawing.view.delete", &json!({"view": v2, "children": "orphan"}))), "`children` must be delete or keep");
}

/// The block's hole made 1 in across, in the file.
fn grow_the_hole(block: &str) {
    edit_model(block, &[("timeline.edit", json!({"feature": "Drill", "set": {"diameter": "1 in"}}))]);
}

#[test]
fn update_keeps_every_handle_when_the_hole_grows() {
    let (mut s, block, ids) = three_views("update_keeps_every_handle_when_the_hole_grows");
    let before: Vec<BTreeSet<Handle>> = ids.iter().map(|id| handles(&s, *id)).collect();
    let (hole, ..) = circle(&s, ids[0]);
    assert!(stale_views(&s).is_empty());
    grow_the_hole(&block);
    assert_eq!(stale_views(&s), ids);
    let undo = s.undo.len();
    let r = run(&mut s, "drawing.update", json!({}));
    assert_eq!(r, json!({"updated": ids, "orphans": 0, "missing": []}));
    assert_eq!(s.undo.len(), undo + 1);
    let after: Vec<BTreeSet<Handle>> = ids.iter().map(|id| handles(&s, *id)).collect();
    assert_eq!(after, before);
    let (h, c, r) = circle(&s, ids[0]);
    assert_eq!(h, hole);
    assert!(near(c, [10.0, 8.0], 1e-6) && (r - 0.5).abs() <= 1e-9, "{c:?} {r}");
    assert!(stale_views(&s).is_empty());
}

#[test]
fn nothing_stale_nothing_updated() {
    let (mut s, block, ids) = three_views("nothing_stale_nothing_updated");
    grow_the_hole(&block);
    run(&mut s, "drawing.update", json!({}));
    let (undo, doc) = (s.undo.len(), s.doc.clone());
    let r = run(&mut s, "drawing.update", json!({}));
    assert_eq!(r, json!({"updated": [], "orphans": 0, "missing": []}));
    assert_eq!(s.undo.len(), undo);
    assert!(Arc::ptr_eq(&s.doc, &doc));
    assert_eq!(bad_message(s.execute("drawing.update", &json!({"views": [99]}))), "no view 99");
    let _ = ids;
}

#[test]
fn undoing_an_update_restores_the_old_views() {
    let (mut s, block, ids) = three_views("undoing_an_update_restores_the_old_views");
    grow_the_hole(&block);
    run(&mut s, "drawing.update", json!({}));
    let (hole, _, r) = circle(&s, ids[0]);
    assert!((r - 0.5).abs() <= 1e-9);
    run(&mut s, "edit.undo", json!({}));
    let (h, _, r) = circle(&s, ids[0]);
    assert_eq!(h, hole);
    assert!((r - 0.375).abs() <= 1e-9, "{r}");
    assert_eq!(stale_views(&s), ids);
    run(&mut s, "edit.redo", json!({}));
    let (h, _, r) = circle(&s, ids[0]);
    assert_eq!(h, hole);
    assert!((r - 0.5).abs() <= 1e-9, "{r}");
}

#[test]
fn a_missing_model_keeps_the_stored_views() {
    let (mut s, block, ids) = three_views("a_missing_model_keeps_the_stored_views");
    let dir = Path::new(&block).parent().unwrap().to_path_buf();
    let counts: Vec<usize> = ids.iter().map(|id| curves(&s, *id).len()).collect();
    let drw = dir.join("drw.tcidrw").to_string_lossy().into_owned();
    run(&mut s, "file.save", json!({"path": drw}));
    assert_eq!(view(&s, ids[0]).model.path, "block.tcipart");
    std::fs::rename(&block, dir.join("away.tcipart")).unwrap();
    run(&mut s, "doc.open", json!({"path": drw}));
    assert_eq!(model(&s).views.keys().copied().collect::<Vec<_>>(), ids);
    assert_eq!(ids.iter().map(|id| curves(&s, *id).len()).collect::<Vec<_>>(), counts);
    assert!(stale_views(&s).is_empty());
    let undo = s.undo.len();
    let r = run(&mut s, "drawing.update", json!({}));
    assert_eq!(r, json!({"updated": [], "orphans": 0, "missing": ["block.tcipart"]}));
    assert_eq!(s.undo.len(), undo);
    assert_eq!(ids.iter().map(|id| curves(&s, *id).len()).collect::<Vec<_>>(), counts);
}

/// Copies the folder `from` (files and folders) into `to`.
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn a_drawing_saved_elsewhere_still_finds_its_model() {
    let (mut s, block, ids) = three_views("a_drawing_saved_elsewhere_still_finds_its_model");
    let dir = Path::new(&block).parent().unwrap().to_path_buf();
    std::fs::create_dir_all(dir.join("out")).unwrap();
    let r = run(&mut s, "file.save", json!({"path": dir.join("out").join("drw").to_string_lossy()}));
    assert!(r["path"].as_str().unwrap().ends_with("drw.tcidrw"), "{r}");
    assert_eq!(view(&s, ids[0]).model.path, "../block.tcipart");
    // Undo history keeps finding the model too.
    assert!(s.undo.iter().filter_map(|snap| snap.doc.drawing.as_deref()).flat_map(|m| m.views.values()).all(|v| v.model.path == "../block.tcipart"));
    let copy = crate::cmd::drawing::drawing_fixture::temp_dir("a_drawing_saved_elsewhere_still_finds_its_model-copy");
    copy_dir(&dir, &copy);
    let copied_block = copy.join("block.tcipart").to_string_lossy().into_owned();
    let mut c = Session::default();
    run(&mut c, "doc.open", json!({"path": copy.join("out").join("drw.tcidrw").to_string_lossy()}));
    grow_the_hole(&copied_block);
    let r = run(&mut c, "drawing.update", json!({}));
    assert_eq!(r, json!({"updated": ids, "orphans": 0, "missing": []}));
    let (_, _, radius) = circle(&c, ids[0]);
    assert!((radius - 0.5).abs() <= 1e-9, "{radius}");
    // The original's model is untouched.
    assert!(stale_views(&s).is_empty());
}

/// A host whose open tab holds `doc` at `path`.
struct Tab(PathBuf, Arc<Document>);

impl ModelProvider for Tab {
    fn document(&self, path: &Path) -> Option<Arc<Document>> {
        (path == self.0).then(|| self.1.clone())
    }
}

#[test]
fn an_open_model_tab_overrides_the_file() {
    let (mut s, block, [v1, v2, _]) = three_views("an_open_model_tab_overrides_the_file");
    let mut tab = Session::default();
    run(&mut tab, "doc.open", json!({"path": block}));
    run(&mut tab, "timeline.edit", json!({"feature": "Drill", "set": {"diameter": "1 in"}}));
    s.model_provider = Some(Arc::new(Tab(crate::drawing::model_ref::absolute(Path::new(&block)), tab.doc.clone())));
    let r = run(&mut s, "drawing.update", json!({"views": [v1]}));
    assert_eq!(r["updated"], json!([v1]));
    let (_, _, radius) = circle(&s, v1);
    assert!((radius - 0.5).abs() <= 1e-9, "{radius}");
    // The file still says 0.75; V2 was not asked for, and is stale against the tab.
    assert!(stale_views(&s).contains(&v2));
}

#[test]
fn a_view_inside_another_views_box_is_picked_first() {
    // Task 7 review: the smallest view box containing the point wins (ties: the nearest centre).
    let (mut s, block) = block_drawing("a_view_inside_another_views_box_is_picked_first");
    let v1 = front_view(&mut s, &block);
    let iso = run(&mut s, "drawing.view.projected", json!({"parent": v1, "position": [11.5, 9]}))["view"].as_u64().unwrap();
    run(&mut s, "drawing.view.set_scale", json!({"view": iso, "scale": 0.5}));
    let (a, b) = (paper_box(&s, v1), paper_box(&s, iso));
    let (lo, hi) = ([a.0[0].max(b.0[0]), a.0[1].max(b.0[1])], [a.1[0].min(b.1[0]), a.1[1].min(b.1[1])]);
    assert!(lo[0] < hi[0] && lo[1] < hi[1], "the boxes do not overlap: {a:?} {b:?}");
    let mut seen = 0;
    for i in 1..20 {
        for j in 1..20 {
            let at = [lo[0] + (hi[0] - lo[0]) * f64::from(i) / 20.0, lo[1] + (hi[1] - lo[1]) * f64::from(j) / 20.0];
            if let Some(PaperPick::View { view }) = pick(&s, at, 0.01) {
                assert_eq!(view, iso, "{at:?}");
                seen += 1;
            }
        }
    }
    assert!(seen > 0);
}
