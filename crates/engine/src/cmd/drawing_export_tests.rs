#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Drawing output (Task 12, spec §4.7, §7.4): PDF pages at the ANSI sheet size with shaded views
//! as pictures under their curves (fresh pictures only), stroke text and no font (R60); DXF of
//! one sheet's paper space at R2018 and R2000, read back with `cadcraft_io::read` and with our
//! own `read_dxf` (R35), keys chunked past 255 bytes (R62), suppressed annotations left out (R61).
//! VIEW1 is the block's front view at [10, 8]: box [8, 6.5]-[12, 9.5], hole r .375 at [10, 8];
//! its dimensions are the hole's diameter ("⌀.75") and the bottom edge's straight run between
//! the corner fillets ("3.00").

use std::collections::BTreeMap;
use std::ops::Deref;
use std::sync::{Arc, Mutex};

use cadcraft_doc::{Drawing, EntityKind};
use craft_pdf::syntax::object::{Dict, Name, Stream};
use serde_json::{Value, json};
use solidcraft_doc::DrawingModel;
use solidcraft_render::{Camera, Rgb, Scene};

use crate::capture::Picture;
use crate::cmd::drawing::drawing_fixture::{block_drawing, edge_at, edit_model, front_view, temp_dir};
use crate::drawing::mapper::{XKey, xkey_of};
use crate::drawing::{FLAT_GREY, ViewRasterizer, dimension_text, sick_dimensions, suppressed_annotations};
use crate::{EngineError, Session};

#[path = "../../../cadcraft-io/tests/support/pdf_read.rs"]
mod pdf_read;

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

/// Records the picture sizes asked for and draws flat grey.
#[derive(Default)]
struct Recording(Mutex<Vec<(u32, u32)>>);

impl Recording {
    fn calls(&self) -> Vec<(u32, u32)> {
        self.0.lock().unwrap().clone()
    }
}

impl ViewRasterizer for Recording {
    fn rasterize(&self, _: &Scene, _: &Camera, px_w: u32, px_h: u32) -> Result<Picture, String> {
        self.0.lock().unwrap().push((px_w, px_h));
        let Rgb(r, g, b) = FLAT_GREY;
        Ok(Picture { rgba: [r, g, b, 255].repeat((px_w * px_h) as usize), renderer: "gpu" })
    }
}

/// The block drawing with VIEW1 (no rasterizer unless a test sets one), its diameter dimension
/// and its bottom edge's dimension: (session, block, VIEW1, diameter, bottom, temp folder).
fn dimensioned(name: &str) -> (Session, String, u64, u64, u64, std::path::PathBuf) {
    let (mut s, block) = block_drawing(name);
    s.view_rasterizer = None;
    let v = front_view(&mut s, &block);
    let hole = edge_at(&s, v, [10.375, 8.0]);
    let bottom = edge_at(&s, v, [10.0, 6.5]);
    let dia = run(&mut s, "drawing.dim.general", json!({"view": v, "picks": [{"edge": hole}], "placement": [11.5, 9.0]}));
    let lin = run(&mut s, "drawing.dim.general", json!({"view": v, "picks": [{"edge": bottom}], "placement": [10.0, 5.8]}));
    assert_eq!((dia["text"].as_str(), lin["text"].as_str()), (Some("⌀.75"), Some("3.00")));
    let dir = temp_dir(&format!("{name}-out"));
    (s, block, v, dia["handle"].as_u64().unwrap(), lin["handle"].as_u64().unwrap(), dir)
}

fn out(dir: &std::path::Path, file: &str) -> String {
    dir.join(file).to_string_lossy().into_owned()
}

/// Exports the PDF; (the command's result, the file's bytes).
fn pdf(s: &mut Session, path: &str, extra: Value) -> (Value, Vec<u8>) {
    let mut p = json!({"path": path});
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    let r = run(s, "drawing.export.pdf", p);
    (r, std::fs::read(path).unwrap())
}

/// Exports sheet `sheet` as DXF at `version`; (the command's result, the file's text).
fn dxf(s: &mut Session, path: &str, sheet: usize, version: &str) -> (Value, String) {
    let r = run(s, "drawing.export.dxf", json!({"path": path, "sheet": sheet, "version": version}));
    (r, std::fs::read_to_string(path).unwrap())
}

/// Each page's image XObjects: (width, height) in pixels.
fn images(bytes: &[u8]) -> Vec<Vec<(f64, f64)>> {
    let pdf = craft_pdf::open_slice(bytes, None).expect("the PDF opens");
    pdf.pages()
        .iter()
        .map(|page| {
            let raw = page.raw();
            let Some(xo) = raw.get::<Dict<'_>>(b"Resources").and_then(|r| r.get::<Dict<'_>>(b"XObject")) else { return Vec::new() };
            let keys: Vec<Vec<u8>> = xo.keys().map(|k| k.deref().to_vec()).collect();
            keys.iter()
                .filter_map(|k| xo.get::<Stream<'_>>(k.as_slice()))
                .filter(|st| st.dict().get::<Name<'_>>(b"Subtype").is_some_and(|n| n.as_str() == "Image"))
                .map(|st| (st.dict().get::<f64>(b"Width").unwrap_or(0.0), st.dict().get::<f64>(b"Height").unwrap_or(0.0)))
                .collect()
        })
        .collect()
}

/// The path-painting operators (`S`, `s`, `f`, `F`, `f*`, `B`, `B*`, `b`, `b*`) drawn inside the
/// marked content of the PDF layer named `layer` on `page`.
fn layer_paths(page: &pdf_read::Page, layer: &str) -> usize {
    let resources: Vec<&str> = page.properties.iter().filter(|(_, name)| name == layer).map(|(res, _)| res.as_str()).collect();
    assert!(!resources.is_empty(), "no PDF layer {layer}: {:?}", page.properties);
    let mut depth_in: Option<usize> = None;
    let mut depth = 0usize;
    let mut count = 0;
    for op in pdf_read::ops(&page.content) {
        match op.op.as_str() {
            "BDC" | "BMC" => {
                depth += 1;
                if depth_in.is_none()
                    && op.names.first().map(String::as_str) == Some("OC")
                    && op.names.get(1).is_some_and(|n| resources.contains(&n.as_str()))
                {
                    depth_in = Some(depth);
                }
            }
            "EMC" => {
                if depth_in == Some(depth) {
                    depth_in = None;
                }
                depth = depth.saturating_sub(1);
            }
            "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" if depth_in.is_some() => count += 1,
            _ => {}
        }
    }
    count
}

/// Entities per layer on the paper's layout `layout`.
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

fn dimensions(d: &Drawing, layout: &str) -> Vec<cadcraft_doc::Dimension> {
    d.layout(layout)
        .unwrap()
        .entities
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Dimension(dm) => Some(dm.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn pdf_has_one_page_per_sheet() {
    let (mut s, _, _, _, _, dir) = dimensioned("pdf_has_one_page_per_sheet");
    run(&mut s, "drawing.sheet.add", json!({}));
    let undo = s.undo.len();
    let (r, bytes) = pdf(&mut s, &out(&dir, "two.pdf"), json!({}));
    assert_eq!(s.undo.len(), undo, "an export is no undo step");
    assert_eq!((r["pages"].as_u64(), r["images"].as_u64()), (Some(2), Some(0)));
    let doc = pdf_read::read(&bytes);
    assert_eq!(doc.pages.len(), 2);
    assert_eq!(doc.pages[0].media, [0.0, 0.0, 2448.0, 1584.0]);
    assert_eq!(doc.pages[1].media, [0.0, 0.0, 2448.0, 1584.0]);
    // `sheets` picks and orders the pages.
    let (r, bytes) = pdf(&mut s, &out(&dir, "one.pdf"), json!({"sheets": [1]}));
    assert_eq!(r["pages"].as_u64(), Some(1));
    assert_eq!(pdf_read::read(&bytes).pages.len(), 1);
}

#[test]
fn pdf_pages_follow_their_sheet_size() {
    let (mut s, _, _, _, _, dir) = dimensioned("pdf_pages_follow_their_sheet_size");
    run(&mut s, "drawing.sheet.add", json!({}));
    run(&mut s, "drawing.sheet.set_size", json!({"sheet": 1, "size": "B"}));
    let (_, bytes) = pdf(&mut s, &out(&dir, "sizes.pdf"), json!({}));
    let doc = pdf_read::read(&bytes);
    assert_eq!(doc.pages[0].media, [0.0, 0.0, 2448.0, 1584.0]);
    assert_eq!(doc.pages[1].media, [0.0, 0.0, 1224.0, 792.0]);
}

/// R60 (amends R20): searchable text is deferred, so the PDF holds no font at all (no system
/// font, not even for a hole note's ↧, R59) and the dimension, note and title block text is there
/// as strokes on its PDF layer.
#[test]
fn pdf_text_is_strokes_and_no_font() {
    let (mut s, block, v, _, _, dir) = dimensioned("pdf_text_is_strokes_and_no_font");
    let hole = edge_at(&s, v, [10.375, 8.0]);
    let note = run(&mut s, "drawing.note.hole", json!({"view": v, "edge": hole, "placement": [12, 10]}));
    edit_model(&block, &[("timeline.edit", json!({"feature": "Drill", "set": {"depth": "0.5 in"}}))]);
    run(&mut s, "drawing.update", json!({}));
    let text = crate::drawing::notes::text_of(model(&s), cadcraft_doc::Handle(note["handle"].as_u64().unwrap())).unwrap();
    let EntityKind::MText(t) = &model(&s).paper.entity(text).unwrap().kind else { panic!("the hole note's text is not an MText") };
    assert_eq!(t.contents, "⌀.75 ↧.50");
    let (_, bytes) = pdf(&mut s, &out(&dir, "text.pdf"), json!({}));
    let doc = pdf_read::read(&bytes);
    let page = &doc.pages[0];
    assert!(page.fonts.is_empty(), "the PDF embeds fonts: {:?}", page.fonts.iter().map(|f| &f.base_font).collect::<Vec<_>>());
    assert!(!page.content.contains(" Tf"), "the page sets a font");
    for layer in ["DIMENSION", "NOTE", "BORDER", "VISIBLE"] {
        assert!(layer_paths(page, layer) > 0, "nothing drawn on {layer}");
    }
}

/// R59: the depth symbol is a stroke-font glyph (our own, three strokes), so a hole note never
/// falls back to a system font: no outline glyph and no missing-glyph box for any of its
/// characters.
#[test]
fn the_depth_symbol_is_a_stroke_glyph() {
    let sh = cadcraft_fonts::shape_stroke("↧", 6.0, 1.0, 0.0);
    let pts = |v: &[(f64, f64)]| v.iter().map(|(x, y)| cadcraft_geom::Vec2::new(*x, *y)).collect::<Vec<_>>();
    assert_eq!(sh.strokes, vec![pts(&[(2.0, 6.0), (2.0, 1.0)]), pts(&[(0.0, 3.0), (2.0, 1.0), (4.0, 3.0)]), pts(&[(0.0, 0.0), (4.0, 0.0)])]);
    for c in "⌀.75 ↧.50".chars().filter(|c| !c.is_whitespace()) {
        let sh = cadcraft_fonts::shape_stroke(&c.to_string(), 6.0, 1.0, 0.0);
        assert!(sh.glyphs.is_empty() && sh.text.is_empty(), "{c:?} is drawn from an outline font");
        let missing_box = sh.strokes.len() == 1 && sh.strokes[0].len() == 5;
        assert!(!sh.strokes.is_empty() && !missing_box, "{c:?} is not in the stroke font");
    }
}

#[test]
fn a_shaded_view_adds_exactly_one_image() {
    let (mut s, _, v, _, _, dir) = dimensioned("a_shaded_view_adds_exactly_one_image");
    s.view_rasterizer = Some(Arc::new(Recording::default()));
    run(&mut s, "drawing.view.set_style", json!({"view": v, "shaded": true}));
    let (r, bytes) = pdf(&mut s, &out(&dir, "shaded.pdf"), json!({}));
    assert_eq!(r["images"].as_u64(), Some(1));
    assert_eq!(images(&bytes).iter().map(Vec::len).collect::<Vec<_>>(), [1]);
    // The picture is drawn before (under) the first stroke.
    let doc = pdf_read::read(&bytes);
    let ops = pdf_read::ops(&doc.pages[0].content);
    let image = ops.iter().position(|o| o.op == "Do").unwrap();
    assert!(image < ops.iter().position(|o| o.op == "S").unwrap());
    run(&mut s, "drawing.view.set_style", json!({"view": v, "shaded": false}));
    let (r, bytes) = pdf(&mut s, &out(&dir, "unshaded.pdf"), json!({}));
    assert_eq!(r["images"].as_u64(), Some(0));
    assert_eq!(images(&bytes), vec![Vec::<(f64, f64)>::new()]);
}

#[test]
fn pdf_asks_for_300_px_per_inch() {
    let (mut s, _, v, _, _, dir) = dimensioned("pdf_asks_for_300_px_per_inch");
    let rec = Arc::new(Recording::default());
    s.view_rasterizer = Some(rec.clone());
    run(&mut s, "drawing.view.set_style", json!({"view": v, "shaded": true}));
    assert_eq!(rec.calls(), [(800, 600)]);
    let (_, bytes) = pdf(&mut s, &out(&dir, "300.pdf"), json!({}));
    assert_eq!(rec.calls(), [(800, 600), (1200, 900)]);
    assert_eq!(images(&bytes), vec![vec![(1200.0, 900.0)]]);
    // No rasterizer: the stored (fresh) 800 x 600 picture is embedded.
    s.view_rasterizer = None;
    let (_, bytes) = pdf(&mut s, &out(&dir, "stored.pdf"), json!({}));
    assert_eq!(images(&bytes), vec![vec![(800.0, 600.0)]]);
}

/// Check 1: only fresh pictures print; a stale one (kept from before a change nothing could
/// draw) is never stretched over the view, and a shaded view without a picture prints as lines.
#[test]
fn stale_or_missing_pictures_print_as_lines_only() {
    let (mut s, block, v, _, _, dir) = dimensioned("stale_or_missing_pictures_print_as_lines_only");
    s.view_rasterizer = Some(Arc::new(Recording::default()));
    run(&mut s, "drawing.view.set_style", json!({"view": v, "shaded": true}));
    s.view_rasterizer = None;
    let r = run(&mut s, "drawing.view.set_scale", json!({"view": v, "scale": 0.5}));
    assert_eq!(r["raster"], "stale");
    let stored = model(&s).views[&v].raster.as_ref().map(|r| (r.width, r.height));
    assert_eq!(stored, Some((800, 600)), "the stale picture is kept");
    let (r, bytes) = pdf(&mut s, &out(&dir, "stale.pdf"), json!({}));
    assert_eq!(r["images"].as_u64(), Some(0));
    assert_eq!(images(&bytes), vec![Vec::<(f64, f64)>::new()]);
    // A shaded view placed with nothing to draw it has no picture at all.
    let r = run(
        &mut s,
        "drawing.view.base",
        json!({"model": block, "orientation": "top", "lines": "hidden", "shaded": true, "scale": 1, "position": [24, 8]}),
    );
    assert_eq!(r["raster"], "stale");
    let (r, _) = pdf(&mut s, &out(&dir, "none.pdf"), json!({}));
    assert_eq!(r["images"].as_u64(), Some(0));
}

/// Final wave 20: a stored picture is fresh only for the model revision, orientation and scale
/// it was drawn at: turned front to back with nothing to draw it, the view keeps an 800 x 600
/// picture of the same size as the view's box, and it is still not printed.
#[test]
fn a_same_size_stale_picture_is_not_reused() {
    let (mut s, _, v, _, _, dir) = dimensioned("a_same_size_stale_picture_is_not_reused");
    s.view_rasterizer = Some(Arc::new(Recording::default()));
    run(&mut s, "drawing.view.set_style", json!({"view": v, "shaded": true}));
    let drawn_for = model(&s).views[&v].raster.as_ref().and_then(|r| r.drawn_for.clone()).unwrap();
    assert_eq!((drawn_for.orientation, drawn_for.scale), (model(&s).views[&v].orientation, 1.0));
    assert_eq!(drawn_for.revision, model(&s).views[&v].revision);
    assert!(crate::drawing::raster::fresh(model(&s), &model(&s).views[&v]));
    let (r, _) = pdf(&mut s, &out(&dir, "fresh.pdf"), json!({}));
    assert_eq!(r["images"].as_u64(), Some(1));
    s.view_rasterizer = None;
    run(&mut s, "drawing.view.set_orientation", json!({"view": v, "orientation": "back"}));
    let view = &model(&s).views[&v];
    assert_eq!(view.raster.as_ref().map(|r| (r.width, r.height)), Some((800, 600)));
    let rect = crate::drawing::raster_rect(model(&s), view).unwrap();
    assert!(crate::drawing::raster::fits(view.raster.as_ref().unwrap(), rect), "the same pixel size: {rect:?}");
    assert!(!crate::drawing::raster::fresh(model(&s), view));
    let (r, _) = pdf(&mut s, &out(&dir, "turned.pdf"), json!({}));
    assert_eq!(r["images"].as_u64(), Some(0));
}

#[test]
fn dxf_round_trips_at_r2018_and_r2000() {
    let (mut s, _, v, dia, _, dir) = dimensioned("dxf_round_trips_at_r2018_and_r2000");
    // A 0.5-scale view: its dimensions read model inches through DIMLFAC 2.0.
    run(&mut s, "drawing.view.set_scale", json!({"view": v, "scale": 0.5}));
    let layout = model(&s).sheets[0].layout.clone();
    let paper = crate::drawing::export::sheet_paper(model(&s), 0).unwrap();
    assert_eq!(dimensions(&paper, &layout).len(), 2);
    for version in ["R2018", "R2000"] {
        let (r, text) = dxf(&mut s, &out(&dir, &format!("{version}.dxf")), 0, version);
        assert_eq!(r["version"], version);
        assert_eq!(r["entities"].as_u64(), Some(paper.layout(&layout).unwrap().entities.len() as u64));
        let back = cadcraft_io::read(text.as_bytes(), "x.dxf").unwrap();
        assert_eq!(back.layouts.len(), 1, "{version}: one sheet's paper space");
        assert_eq!(per_layer(&back, &layout), per_layer(&paper, &layout), "{version}");
        let dims = dimensions(&back, &layout);
        assert_eq!(dims.len(), 2, "{version}");
        for d in &dims {
            assert_eq!(d.overrides.get("linearFactor").and_then(Value::as_f64), Some(2.0), "{version}: {:?}", d.overrides);
        }
        assert_eq!(keys(&back, &layout), keys(&paper, &layout), "{version}");
        assert!(!keys(&back, &layout).is_empty());
        // Check 8 (Review Focus): the diameter sign survives (R2000 is not UTF-8).
        let mut m = model(&s).clone();
        m.paper = back;
        assert_eq!(dimension_text(&m, dia).as_deref(), Some("⌀.75"), "{version}");
        assert!(sick_dimensions(&m).is_empty(), "{version}");
    }
}

/// R35, as it stands: `solidcraft_io::read_dxf` reads our DXF without error, but it reads model
/// space only (an entity with group 67 = 1 is skipped: "paper space is not the drawing",
/// `io/src/dxf.rs`), and a sheet is paper space, so it finds no geometry. The line work is in the
/// file, in paper space: every LINE, ARC and CIRCLE of the sheet with 67 = 1, the hole's r .375
/// circle among them. (Reported: reading a sheet with our reader needs a ruling.)
#[test]
fn our_dxf_reader_reads_the_file_but_not_paper_space() {
    let (mut s, _, _, _, _, dir) = dimensioned("our_dxf_reader_reads_the_file_but_not_paper_space");
    let (_, text) = dxf(&mut s, &out(&dir, "lines.dxf"), 0, "R2018");
    assert_eq!(solidcraft_io::read_dxf(&text).unwrap().len(), 0);
    let m = model(&s);
    let paper = crate::drawing::export::sheet_paper(m, 0).unwrap();
    let layout = paper.layout(&m.sheets[0].layout).unwrap();
    let mut expected = [0usize; 3];
    for e in layout.entities.iter() {
        match e.kind {
            EntityKind::Line(_) => expected[0] += 1,
            EntityKind::Arc(_) => expected[1] += 1,
            EntityKind::Circle(_) => expected[2] += 1,
            _ => {}
        }
    }
    // The ENTITIES section's entities: (kind, groups).
    let pairs: Vec<(String, String)> =
        text.lines().collect::<Vec<_>>().chunks(2).map(|c| (c[0].trim().to_string(), c.get(1).unwrap_or(&"").trim().to_string())).collect();
    let start = pairs.iter().position(|(c, v)| c == "2" && v == "ENTITIES").unwrap();
    let mut found = [0usize; 3];
    let mut radii = Vec::new();
    let mut current: Option<(String, Vec<(String, String)>)> = None;
    let mut done = |e: Option<(String, Vec<(String, String)>)>| {
        let Some((kind, groups)) = e else { return };
        let in_paper = groups.iter().any(|(c, v)| c == "67" && v == "1");
        let slot = match kind.as_str() {
            "LINE" => 0,
            "ARC" => 1,
            "CIRCLE" => 2,
            _ => return,
        };
        assert!(in_paper, "a {kind} outside paper space");
        found[slot] += 1;
        if slot == 2 {
            radii.push(groups.iter().find(|(c, _)| c == "40").map(|(_, v)| v.parse::<f64>().unwrap()).unwrap());
        }
    };
    for (c, v) in pairs.iter().skip(start + 1) {
        if c == "0" {
            done(current.take());
            if v == "ENDSEC" {
                break;
            }
            current = Some((v.clone(), Vec::new()));
        } else if let Some((_, groups)) = current.as_mut() {
            groups.push((c.clone(), v.clone()));
        }
    }
    println!("paper-space lines/arcs/circles: {found:?}, expected {expected:?}");
    assert_eq!(found, expected);
    assert!(radii.iter().any(|r| (r - 0.375).abs() < 1e-9), "{radii:?}");
}

#[test]
fn dxf_skips_shaded_rasters() {
    let (mut s, _, v, _, _, dir) = dimensioned("dxf_skips_shaded_rasters");
    s.view_rasterizer = Some(Arc::new(Recording::default()));
    run(&mut s, "drawing.view.set_style", json!({"view": v, "shaded": true}));
    assert!(model(&s).views[&v].raster.is_some());
    let (_, text) = dxf(&mut s, &out(&dir, "shaded.dxf"), 0, "R2018");
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    assert!(!lines.windows(2).any(|w| w[0] == "0" && (w[1] == "IMAGE" || w[1] == "IMAGEDEF")), "an IMAGE in the DXF");
    let back = cadcraft_io::read(text.as_bytes(), "x.dxf").unwrap();
    assert!(back.layouts.iter().flat_map(|l| l.entities.iter()).all(|e| !matches!(e.kind, EntityKind::Image(_))));
}

#[test]
fn exports_check_their_input() {
    let (mut s, _, _, _, _, dir) = dimensioned("exports_check_their_input");
    let undo = s.undo.len();
    assert_eq!(bad_message(s.execute("drawing.export.pdf", &json!({}))), "path is required");
    assert_eq!(bad_message(s.execute("drawing.export.dxf", &json!({"version": "R2018"}))), "path is required");
    assert_eq!(
        bad_message(s.execute("drawing.export.dxf", &json!({"path": out(&dir, "x.dxf"), "version": "R12"}))),
        "version must be R2018 or R2000"
    );
    assert_eq!(bad_message(s.execute("drawing.export.dxf", &json!({"path": out(&dir, "x.dxf"), "sheet": 3}))), "sheet 3 does not exist (1 sheets)");
    assert_eq!(
        bad_message(s.execute("drawing.export.pdf", &json!({"path": out(&dir, "x.pdf"), "sheets": [0, 1]}))),
        "sheet 1 does not exist (1 sheets)"
    );
    assert_eq!(
        bad_message(s.execute("drawing.export.pdf", &json!({"path": out(&dir, "x.pdf"), "sheets": "all"}))),
        "`sheets` must be a list of sheet indices"
    );
    assert_eq!(s.undo.len(), undo);
    assert!(!dir.join("x.dxf").exists() && !dir.join("x.pdf").exists());
    let mut part = Session::default();
    run(&mut part, "document.new_part", json!({}));
    for (id, p) in [("drawing.export.pdf", json!({"path": out(&dir, "p.pdf")})), ("drawing.export.dxf", json!({"path": out(&dir, "p.dxf")}))] {
        let e = part.execute(id, &p).unwrap_err().to_string();
        assert!(e.contains("is not a drawing"), "{id}: {e}");
    }
}

/// Check 3 (R62): a view key longer than a DXF `1000` string holds (255 bytes) is written as
/// consecutive strings, none longer than 255 bytes, and read back whole; the fixture's longest
/// key fits in one.
#[test]
fn long_keys_round_trip_through_dxf() {
    use crate::drawing::mapper;
    use crate::drawing::project::{PaperCurve, ViewCurve, ViewGeometry};
    use solidcraft_geom::Vec2 as MmVec2;

    let (mut s, _, v1, _, _, dir) = dimensioned("long_keys_round_trip_through_dxf");
    let v3 = run(&mut s, "drawing.view.projected", json!({"parent": v1, "position": [20, 8]}))["view"].as_u64().unwrap();
    let a = edge_at(&s, v3, [20.0, 7.625]);
    let b = edge_at(&s, v3, [20.0, 8.375]);
    run(&mut s, "drawing.centerline.bisector", json!({"view": v3, "a": a, "b": b}));
    let layout = model(&s).sheets[0].layout.clone();
    let longest = keys(&model(&s).paper, &layout).iter().map(|k| k.edge.len()).max().unwrap();
    println!("the fixture's longest key: {longest} bytes");
    assert!(longest <= 255);
    // A synthetic 300-byte key, with two-byte characters across the 255-byte boundary, then
    // astral characters (R2000 writes each as a surrogate pair `\U+XXXX\U+XXXX`, 14 bytes).
    let long: String = format!("0/Block/e/x{}{}", "F9:ø".repeat(58), "\u{1F600}".repeat(40));
    assert!(long.len() >= 300 && !long.is_char_boundary(255), "{}", long.len());
    let curve = ViewCurve {
        edge: long.clone(),
        sources: vec![long.clone()],
        segment: 0,
        hidden: false,
        curve: PaperCurve::Line { a: MmVec2::new(0.0, 0.0), b: MmVec2::new(10.0, 0.0) },
    };
    let geometry = ViewGeometry { curves: vec![curve], min: MmVec2::new(0.0, 0.0), max: MmVec2::new(10.0, 0.0), suppressed: Vec::new() };
    crate::drawing::edit(&mut s, "test", |m, _| {
        let view = m.views.get(&v1).unwrap().clone();
        mapper::place(m, &view, &geometry);
        Ok(())
    })
    .unwrap();
    let before = keys(&model(&s).paper, &layout);
    assert!(before.iter().any(|k| k.edge == long));
    for version in ["R2018", "R2000"] {
        let (_, text) = dxf(&mut s, &out(&dir, &format!("long-{version}.dxf")), 0, version);
        let lines: Vec<&str> = text.lines().collect();
        let longest_1000 = lines.windows(2).filter(|w| w[0].trim() == "1000").map(|w| w[1].len()).max().unwrap();
        assert!(longest_1000 <= 255, "{version}: a 1000 string of {longest_1000} bytes");
        let back = cadcraft_io::read(text.as_bytes(), "x.dxf").unwrap();
        assert_eq!(keys(&back, &layout), before, "{version}");
    }
}

/// Check 4 (R46): a D landscape sheet goes to DXF as portrait media with the landscape flag, and
/// reads back as a 34 x 22 in sheet.
#[test]
fn a_landscape_sheet_reads_back_landscape() {
    let (mut s, _, _, _, _, dir) = dimensioned("a_landscape_sheet_reads_back_landscape");
    let (_, text) = dxf(&mut s, &out(&dir, "d.dxf"), 0, "R2018");
    let back = cadcraft_io::read(text.as_bytes(), "x.dxf").unwrap();
    let page = &back.layout(&model(&s).sheets[0].layout).unwrap().page;
    assert!(page.landscape);
    assert!((page.width_mm - 558.8).abs() < 1e-6 && (page.height_mm - 863.6).abs() < 1e-6, "{} x {}", page.width_mm, page.height_mm);
    let sheet = cadcraft_render::Sheet::from_page(page, 25.4);
    assert!((sheet.size.x - 34.0).abs() < 1e-9 && (sheet.size.y - 22.0).abs() < 1e-9, "{:?}", sheet.size);
}

/// Check 5 (R61): what the view styles suppress (hidden curves under Hidden Line Removed, and the
/// annotations on them) is not written; everything drawn is.
#[test]
fn dxf_leaves_out_suppressed_annotations() {
    let (mut s, _, v1, _, _, dir) = dimensioned("dxf_leaves_out_suppressed_annotations");
    let v3 = run(&mut s, "drawing.view.projected", json!({"parent": v1, "position": [20, 8]}))["view"].as_u64().unwrap();
    let a = edge_at(&s, v3, [20.0, 7.625]);
    let b = edge_at(&s, v3, [20.0, 8.375]);
    let cl = run(&mut s, "drawing.centerline.bisector", json!({"view": v3, "a": a, "b": b}))["handle"].as_u64().unwrap();
    let dim = run(&mut s, "drawing.dim.general", json!({"view": v3, "picks": [{"edge": a}], "placement": [20, 6]}))["handle"].as_u64().unwrap();
    run(&mut s, "drawing.view.set_style", json!({"view": v3, "lines": "removed"}));
    let suppressed = suppressed_annotations(model(&s));
    assert!(suppressed.contains(&cl) && suppressed.contains(&dim));
    let layout = model(&s).sheets[0].layout.clone();
    let invisible = model(&s).paper.layout(&layout).unwrap().entities.iter().filter(|e| !e.common.visible).count();
    assert_eq!(invisible, 4, "2 hidden curves, the centreline and the dimension");
    let (r, text) = dxf(&mut s, &out(&dir, "suppressed.dxf"), 0, "R2018");
    let back = cadcraft_io::read(text.as_bytes(), "x.dxf").unwrap();
    let entities = &back.layout(&layout).unwrap().entities;
    assert_eq!(r["entities"].as_u64(), Some(entities.len() as u64));
    assert_eq!(entities.len(), model(&s).paper.layout(&layout).unwrap().entities.len() - invisible);
    assert!(entities.iter().all(|e| e.common.visible));
    assert!(entities.iter().all(|e| e.handle.0 != cl && e.handle.0 != dim));
    assert_eq!(entities.iter().filter(|e| e.common.layer == "HIDDEN").count(), 0);
    assert_eq!(dimensions(&back, &layout).len(), 2, "VIEW1's two dimensions are written");
}
