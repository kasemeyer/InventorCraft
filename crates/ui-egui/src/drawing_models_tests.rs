#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Model links, refresh, export and the save location (Plan D-2 Task 11, R68, R76, R77, R89,
//! spec §8.6): the open tabs serve a drawing its model as it stands in memory; activating a
//! drawing tab and saving a drawing run `drawing.update {}` (one step when it changed something,
//! none otherwise); File › Export and the DRAWING tab's EXPORT write PDF and DXF through the save
//! picker; Save As of a new drawing starts beside its model.

use std::sync::{Arc, Mutex};

use serde_json::json;
use solidcraft_engine::Session;
use solidcraft_engine::doc::DocumentKind;

use crate::scenario::Harness;
use crate::test_support::{block_part, click, inventor};

/// The block open (saved) in tab 0 and a drawing of it in tab 1, VIEW1 its front at [10, 8]
/// (`shaded` when asked; the drawing's tab then has a test rasterizer). Tab 1 is active.
fn two_tabs(mut h: Harness, shaded: bool) -> (Harness, String, u64) {
    h.app.home.open = false;
    let block = block_part(&h.dir);
    let block = std::path::absolute(&block).unwrap().to_string_lossy().into_owned();
    h.app.open_path(&block);
    h.frames(2);
    assert_eq!(h.app.session.doc.kind, DocumentKind::Part);
    let mut s = Session::default();
    if shaded {
        s.view_rasterizer = Some(Arc::new(Solid));
    }
    s.execute("drawing.new", &json!({"model": block})).unwrap();
    let v1 = s
        .execute(
            "drawing.view.base",
            &json!({"model": block, "orientation": "front", "lines": "hidden", "shaded": shaded, "scale": 1, "position": [10, 8]}),
        )
        .unwrap()["view"]
        .as_u64()
        .unwrap();
    crate::documents::open_tab(&mut h.app, s).unwrap();
    h.frames(2);
    assert_eq!((h.app.docs.active, h.app.session.doc.kind), (1, DocumentKind::Drawing));
    (h, block, v1)
}

/// A rasterizer drawing a solid grey picture of the asked size.
struct Solid;

impl solidcraft_engine::drawing::ViewRasterizer for Solid {
    fn rasterize(
        &self,
        _: &solidcraft_engine::render::Scene,
        _: &solidcraft_engine::render::Camera,
        w: u32,
        h: u32,
    ) -> Result<solidcraft_engine::capture::Picture, String> {
        Ok(solidcraft_engine::capture::Picture { rgba: [128, 128, 128, 255].repeat((w * h) as usize), renderer: "test" })
    }
}

fn switch(h: &mut Harness, tab: usize) {
    crate::documents::switch(&mut h.app, tab);
    h.frames(2);
}

fn undo(h: &Harness) -> usize {
    h.app.session.undo.len()
}

/// The circles of the active sheet, radius in inches.
fn circle_radii(h: &Harness) -> Vec<f64> {
    let m = h.app.session.doc.drawing.as_deref().unwrap();
    let layout = m.paper.layout(&m.sheets[0].layout).unwrap();
    layout
        .entities
        .iter()
        .filter_map(|e| match &e.kind {
            cadcraft_doc::EntityKind::Circle(c) => Some((c.radius * 1e4).round() / 1e4),
            _ => None,
        })
        .collect()
}

fn view(h: &Harness, v: u64) -> solidcraft_engine::doc::drawing::DrawingView {
    h.app.session.doc.drawing.as_deref().unwrap().views[&v].clone()
}

#[test]
fn an_unsaved_edit_in_the_model_tab_updates_the_drawing() {
    let (mut h, _, _) = two_tabs(inventor(DocumentKind::Part), false);
    assert_eq!(circle_radii(&h), [0.375], "the 0.75 in hole");
    let start = undo(&h);
    switch(&mut h, 0);
    h.app.run("timeline.edit", json!({"feature": "Drill", "set": {"diameter": "1 in"}})).unwrap();
    assert!(h.app.session.is_dirty(), "not saved");
    switch(&mut h, 1);
    assert_eq!(undo(&h), start + 1, "one step");
    assert_eq!(h.app.session.undo.last().unwrap().label, "Update Views");
    assert_eq!(circle_radii(&h), [0.5], "the model as its tab holds it");
}

#[test]
fn saving_the_model_updates_its_drawing() {
    let (mut h, block, _) = two_tabs(inventor(DocumentKind::Part), false);
    let start = undo(&h);
    switch(&mut h, 0);
    h.app.run("timeline.edit", json!({"feature": "Drill", "set": {"diameter": "1 in"}})).unwrap();
    h.app.run("file.save", json!({"path": block})).unwrap();
    assert!(!h.app.session.is_dirty());
    switch(&mut h, 1);
    assert_eq!(undo(&h), start + 1);
    assert_eq!(circle_radii(&h), [0.5]);
    // Nothing changed since: activating again adds no step.
    switch(&mut h, 0);
    switch(&mut h, 1);
    assert_eq!(undo(&h), start + 1, "no step when nothing changed");
}

#[test]
fn a_stale_picture_is_drawn_again() {
    let (mut h, _, v1) = two_tabs(inventor(DocumentKind::Part), true);
    let before = view(&h, v1).revision;
    assert!(view(&h, v1).raster.is_some());
    switch(&mut h, 0);
    h.app.run("timeline.edit", json!({"feature": "Drill", "set": {"diameter": "1 in"}})).unwrap();
    switch(&mut h, 1);
    let v = view(&h, v1);
    assert_ne!(v.revision, before, "projected again from the edited model");
    assert_eq!(v.raster.as_ref().unwrap().drawn_for.as_ref().unwrap().revision, v.revision, "its picture drawn for it");
    assert_eq!(crate::sheet_canvas::scene(&h.app).images.len(), 1, "the canvas draws the fresh picture");
    // Rescaled without a renderer the picture goes stale; saving the drawing draws it again.
    let rasterizer = h.app.session.view_rasterizer.take();
    h.app.run("drawing.view.set_scale", json!({"view": v1, "scale": 2})).unwrap();
    let m = h.app.session.doc.drawing.clone().unwrap();
    assert!(!solidcraft_engine::drawing::raster_fresh(&m, &m.views[&v1]));
    assert!(crate::sheet_canvas::scene(&h.app).images.is_empty());
    h.app.session.view_rasterizer = rasterizer;
    let path = h.dir.join("block.tcidrw").to_string_lossy().into_owned();
    h.app.run("file.save_as", json!({"path": path})).unwrap();
    let start = undo(&h);
    crate::toolbar::save(&mut h.app);
    h.frames(10);
    assert_eq!(undo(&h), start + 1, "the update before the save is one step");
    assert!(!h.app.session.is_dirty(), "and the save comes after it");
    let m = h.app.session.doc.drawing.clone().unwrap();
    assert!(solidcraft_engine::drawing::raster_fresh(&m, &m.views[&v1]));
    // Fresh on reopen.
    let mut again = Session::default();
    again.execute("doc.open", &json!({"path": path})).unwrap();
    let m = again.doc.drawing.clone().unwrap();
    assert!(solidcraft_engine::drawing::raster_fresh(&m, &m.views[&v1]));
}

#[test]
fn a_missing_model_says_so() {
    let mut h = inventor(DocumentKind::Part);
    h.app.home.open = false;
    let dir = h.dir.join("gone");
    std::fs::create_dir_all(&dir).unwrap();
    let block = block_part(&dir);
    let block = std::path::absolute(&block).unwrap().to_string_lossy().into_owned();
    let drawing = h.dir.join("orphan.tcidrw").to_string_lossy().into_owned();
    let mut s = Session::default();
    s.execute("drawing.new", &json!({"model": block})).unwrap();
    s.execute("drawing.view.base", &json!({"model": block, "orientation": "front", "scale": 1, "position": [10, 8]})).unwrap();
    s.execute("file.save_as", &json!({"path": drawing})).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    h.app.open_path(&drawing);
    h.frames(2);
    assert_eq!(h.app.session.doc.kind, DocumentKind::Drawing);
    let (text, _, _) = h.app.status.clone().unwrap();
    // The path as the drawing stores it (relative to the drawing's folder).
    assert!(text.contains("gone/block.tcipart"), "the stored model path: {text}");
    assert!(h.app.dialog.is_none() && h.app.drawing.panel.is_none(), "no dialog");
    // The browser's suffix says so too (R77).
    let m = h.app.session.doc.drawing.clone().unwrap();
    let v = m.views.values().next().unwrap();
    assert_eq!(crate::browser::drawing::missing(&mut h.app), [v.id]);
}

/// A save picker answering `path`, recording the names it was asked with.
fn answer_saves(h: &mut Harness, path: Option<String>) -> Arc<Mutex<Vec<(String, Vec<String>)>>> {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let log = asked.clone();
    h.app.services.pick_save = Some(Box::new(move |name: &str, exts: &[&str]| {
        log.lock().unwrap().push((name.to_string(), exts.iter().map(|e| (*e).to_string()).collect()));
        Box::pin(std::future::ready(path.clone()))
    }));
    asked
}

fn labels(rows: &[(&'static str, &'static str, crate::toolbar::FileAction)]) -> Vec<&'static str> {
    rows.iter().map(|(label, _, _)| *label).collect()
}

#[test]
fn export_offers_pdf_and_dxf_for_drawings() {
    let mut part = inventor(DocumentKind::Part);
    part.app.home.open = false;
    assert_eq!(
        labels(&crate::toolbar::export_rows(&part.app)),
        ["Export STL…", "Export 3MF…", "Export STEP…", "Export Current View as DXF…", "Export OBJ…", "Export URDF…", "Export Design as JSON…"],
        "a part's list is unchanged"
    );
    let (mut h, _, _) = two_tabs(Harness::new(), false);
    assert_eq!(labels(&crate::toolbar::export_rows(&h.app)), ["Export PDF…", "Export DXF…"]);
    // The DRAWING tab's EXPORT › PDF: the save picker, not "path is required".
    let pdf = h.dir.join("out.pdf").to_string_lossy().into_owned();
    let asked = answer_saves(&mut h, Some(pdf.clone()));
    h.app.ui.tab = "DRAWING".into();
    h.frames(2);
    let start = undo(&h);
    click(&mut h, "toolbar:drawing.export.pdf");
    h.frames(3);
    let (name, exts) = asked.lock().unwrap()[0].clone();
    assert_eq!(exts, ["pdf"]);
    assert!(name.ends_with(".pdf"), "{name}");
    assert!(h.app.status.as_ref().is_none_or(|(text, _, error)| !*error && !text.contains("path is required")), "{:?}", h.app.status);
    assert!(std::fs::read(&pdf).is_ok_and(|b| b.starts_with(b"%PDF")), "written after the status painted");
    assert_eq!(undo(&h), start, "an export is not an edit");
    // Step by step: the frame after the answer shows the status, the next one writes.
    let dxf = h.dir.join("out.dxf").to_string_lossy().into_owned();
    answer_saves(&mut h, Some(dxf.clone()));
    crate::drawing_models::export_dxf(&mut h.app);
    h.frame();
    assert!(h.app.drawing.export.is_some(), "answered, waiting for the status to paint");
    assert_eq!(h.app.status.as_ref().map(|(t, _, _)| t.clone()), Some(crate::i18n::tr("ui.drawing.exporting").to_string()));
    assert!(std::fs::metadata(&dxf).is_err(), "not yet");
    h.frame();
    assert!(std::fs::read(&dxf).is_ok_and(|b| !b.is_empty()));
    assert!(h.app.drawing.export.is_none());
    // PDF: one page per sheet.
    h.app.run("drawing.sheet.add", json!({})).unwrap();
    let two = h.dir.join("two.pdf").to_string_lossy().into_owned();
    answer_saves(&mut h, Some(two.clone()));
    crate::drawing_models::export_pdf(&mut h.app);
    h.frames(3);
    let text = h.app.status.as_ref().map(|(t, _, _)| t.clone()).unwrap();
    assert!(text.contains(&two) && text.contains('2'), "{text}");
    assert!(std::fs::read(&two).is_ok_and(|b| b.starts_with(b"%PDF")));
}

#[test]
fn save_as_starts_next_to_the_model() {
    // (owner default; §8.6)
    let mut h = inventor(DocumentKind::Part);
    h.app.home.open = false;
    let block = block_part(&h.dir);
    h.app.open_path(&block);
    h.frames(2);
    crate::documents::create_drawing_view(&mut h.app);
    h.frames(2);
    assert_eq!(h.app.session.doc.kind, DocumentKind::Drawing);
    let asked = answer_saves(&mut h, None);
    crate::toolbar::save_as(&mut h.app);
    h.frames(2);
    let expected = std::path::absolute(std::path::Path::new(&h.dir).join("block.tcidrw")).unwrap();
    let (name, exts) = asked.lock().unwrap()[0].clone();
    assert_eq!(std::path::PathBuf::from(&name), expected);
    assert_eq!(exts, ["tcidrw"]);
    // Placed views name the model too.
    h.app.drawing.panel = None;
    let model = std::path::absolute(&block).unwrap().to_string_lossy().into_owned();
    h.app.run("drawing.view.base", json!({"model": model, "orientation": "front", "scale": 1, "position": [10, 8]})).unwrap();
    crate::toolbar::save_as(&mut h.app);
    h.frames(2);
    assert_eq!(std::path::PathBuf::from(&asked.lock().unwrap()[1].0), expected);
}

#[test]
fn a_failed_export_says_why() {
    // Final wave item 32: an export the engine refuses reports its error, no "Exporting…" left.
    let (mut h, _, _) = two_tabs(Harness::new(), false);
    let bad = h.dir.join("no-such-folder").join("out.pdf").to_string_lossy().into_owned();
    answer_saves(&mut h, Some(bad.clone()));
    crate::drawing_models::export_pdf(&mut h.app);
    h.frames(3);
    assert!(h.app.drawing.export.is_none());
    let (text, _, error) = h.app.status.clone().expect("a status");
    assert!(error, "the failure is an error: {text}");
    assert_ne!(text, crate::i18n::tr("ui.drawing.exporting"), "no Exporting… left");
    assert!(!text.is_empty());
    assert!(std::fs::metadata(&bad).is_err());
    // Refused before the engine runs (another file operation under way): still said.
    let later = h.dir.join("later.pdf").to_string_lossy().into_owned();
    h.app.services.pick_save = Some(Box::new(|_: &str, _: &[&str]| Box::pin(std::future::pending())));
    h.app.pick_save("other.pdf", &["pdf"], |_, _| {});
    h.app.drawing.export = Some(crate::drawing_models::PendingExport { command: "drawing.export.pdf", path: later.clone(), painted: false });
    h.app.set_status(crate::i18n::tr("ui.drawing.exporting").to_string(), false);
    crate::drawing_models::tick(&mut h.app);
    crate::drawing_models::tick(&mut h.app);
    assert!(h.app.drawing.export.is_none());
    let (text, _, error) = h.app.status.clone().expect("a status");
    assert!(error && text.contains("file operation"), "{text}");
    assert!(std::fs::metadata(&later).is_err());
}
