#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};

use cadcraft_doc::{EntityKind, Lineweight};
use serde_json::{Value, json};
use solidcraft_doc::{AnsiSize, DocumentKind, DrawingModel, TemplateId};

use super::drawing_fixture::{block_drawing, block_part, temp_dir};
use crate::drawing::{self, BORDER_INSET_IN, DIM_STYLE, LAYERS, TITLE_FIELDS, default_drawing_path};
use crate::{EngineError, Session};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn err(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} should fail, gave {v}"),
        Err(e) => e.to_string(),
    }
}

fn model(s: &Session) -> &DrawingModel {
    s.doc.drawing.as_deref().expect("a drawing model")
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// The first sheet's title block attributes, tag → value, in their order.
fn attribs(m: &DrawingModel) -> Vec<(String, String)> {
    let e = m.paper.entity(m.sheets[0].title_block).expect("the title block insert");
    let EntityKind::Insert(ins) = &e.kind else { panic!("the title block is an Insert: {:?}", e.kind) };
    ins.attribs.iter().map(|a| (a.tag.clone(), a.text.value.clone())).collect()
}

fn attrib_map(m: &DrawingModel) -> BTreeMap<String, String> {
    attribs(m).into_iter().collect()
}

#[test]
fn new_drawing_is_d_landscape_with_border_and_title_block() {
    let mut s = Session::default();
    let r = run(&mut s, "drawing.new", json!({}));
    assert_eq!(r, json!({"kind": "drawing", "sheets": 1, "size": "D"}));
    assert_eq!(s.doc.kind, DocumentKind::Drawing);
    assert_eq!(s.drawing_sheet, 0);
    assert!(!s.is_dirty(), "a new drawing is clean, as a new part is");
    let m = model(&s);
    assert_eq!(m.template, TemplateId::AnsiInch);
    assert_eq!(m.sheets.len(), 1);
    let sheet = &m.sheets[0];
    assert_eq!((sheet.size, sheet.landscape, sheet.layout.as_str()), (AnsiSize::D, true, "Sheet:1"));
    assert_eq!(m.paper.layouts.len(), 1);
    let layout = m.paper.layout("Sheet:1").unwrap();
    // The portrait media size plus the landscape flag (CADCraft's convention; R46).
    assert!(close(layout.page.width_mm, 558.8) && close(layout.page.height_mm, 863.6), "{:?}", layout.page);
    assert!(layout.page.landscape);
    // The renderer still lays the sheet out 34 x 22 in.
    let rendered = cadcraft_render::Sheet::from_page(&layout.page, 25.4);
    assert!(close(rendered.size.x, 34.0) && close(rendered.size.y, 22.0), "{:?}", rendered.size);
    assert_eq!(layout.page.paper, "ANSI D (22.00 x 34.00 Inches)");
    // CADCraft draws the sheet 34 x 22 paper units (inches).
    let drawn = cadcraft_render::sheet(&m.paper, "Sheet:1").unwrap();
    assert!(close(drawn.size.x, 34.0) && close(drawn.size.y, 22.0), "{:?}", drawn.size);
    // One Insert of each block, and they are the sheet's border and title block.
    let inserts: Vec<(cadcraft_doc::Handle, String, [f64; 2])> = layout
        .entities
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Insert(i) => Some((e.handle, i.block.clone(), [i.insert.x, i.insert.y])),
            _ => None,
        })
        .collect();
    assert_eq!(inserts.len(), 2, "{inserts:?}");
    assert_eq!(inserts[0], (sheet.border, "TCI_BORDER_D".to_string(), [0.0, 0.0]));
    assert_eq!(inserts[1].0, sheet.title_block);
    assert_eq!(inserts[1].1, "TCI_TITLE_ANSI");
    // The title block's lower-right corner sits on the frame's: (34 - 0.5, 0.5).
    assert_eq!(inserts[1].2, [34.0 - BORDER_INSET_IN, BORDER_INSET_IN]);
    // The border's zone texts: columns 1..8, rows A..D.
    let border = m.paper.block("TCI_BORDER_D").unwrap();
    let zones: BTreeSet<String> = border
        .entities
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Text(t) => Some(t.value.clone()),
            _ => None,
        })
        .collect();
    let want: BTreeSet<String> = ["1", "2", "3", "4", "5", "6", "7", "8", "A", "B", "C", "D"].iter().map(|z| z.to_string()).collect();
    assert_eq!(zones, want);
    // Each zone is marked on both sides: 8 numbers top and bottom, 4 letters left and right.
    let texts = border.entities.iter().filter(|e| matches!(e.kind, EntityKind::Text(_))).count();
    assert_eq!(texts, 2 * 8 + 2 * 4);
    // The frame is inset half an inch: a closed rectangle from (0.5, 0.5) to (33.5, 21.5).
    let frame = border
        .entities
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::LwPolyline(p) if p.closed => Some(p.vertices.iter().map(|v| [v.p.x, v.p.y]).collect::<Vec<_>>()),
            _ => None,
        })
        .unwrap();
    assert_eq!(frame, vec![[0.5, 0.5], [33.5, 0.5], [33.5, 21.5], [0.5, 21.5]]);
}

#[test]
fn title_block_fields_are_the_spec_list() {
    let mut s = Session::default();
    run(&mut s, "drawing.new", json!({}));
    let m = model(&s);
    let tags: Vec<String> = attribs(m).into_iter().map(|(t, _)| t).collect();
    assert_eq!(tags, TITLE_FIELDS);
    assert!(!tags.iter().any(|t| t == "QA" || t == "MFG"), "{tags:?}");
    // The block defines the same tags, in the same order.
    let defs: Vec<String> = m
        .paper
        .block("TCI_TITLE_ANSI")
        .unwrap()
        .entities
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::AttDef(a) => Some(a.tag.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(defs, TITLE_FIELDS);
    // Without a model: computed fields and the creation defaults only.
    let a = attrib_map(m);
    assert_eq!((a["SIZE"].as_str(), a["SHEET"].as_str(), a["SCALE"].as_str()), ("D", "1 OF 1", ""));
    assert_eq!((a["TITLE"].as_str(), a["DWG_NO"].as_str(), a["MATERIAL"].as_str()), ("", "", ""));
    assert_eq!(a["DRAWN_DATE"], drawing::template::today());
}

#[test]
fn new_from_a_model_fills_the_title_block() {
    let dir = temp_dir("fills-title");
    let block = block_part(&dir);
    let mut s = Session::default();
    run(&mut s, "drawing.new", json!({"model": block, "properties": {"company": "TCI"}}));
    let m = model(&s);
    let a = attrib_map(m);
    assert_eq!(a["TITLE"], "block");
    assert_eq!(a["DWG_NO"], "block");
    assert_eq!(a["SIZE"], "D");
    assert_eq!(a["SHEET"], "1 OF 1");
    assert_eq!(a["SCALE"], "");
    assert_eq!(a["COMPANY"], "TCI");
    assert_eq!(a["DRAWN_DATE"], drawing::template::today());
    assert_eq!(a["MATERIAL"], "");
    assert_eq!(a["REV"], "");
    let user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default();
    assert_eq!(a["DRAWN_BY"], user);
    // The properties hold the same values (the computed fields are not properties).
    assert_eq!((m.properties.title.as_str(), m.properties.dwg_no.as_str(), m.properties.company.as_str()), ("block", "block", "TCI"));
    // `today()` is a date: YYYY-MM-DD.
    let today = drawing::template::today();
    assert_eq!(today.len(), 10, "{today}");
    assert!(today.chars().enumerate().all(|(i, c)| if i == 4 || i == 7 { c == '-' } else { c.is_ascii_digit() }), "{today}");
    // A field the title block doesn't have is refused, and so is a value that isn't text.
    let mut t = Session::default();
    assert!(err(&mut t, "drawing.new", json!({"properties": {"qa": "x"}})).contains("unknown title block field qa"));
    assert!(err(&mut t, "drawing.new", json!({"properties": {"size": "E"}})).contains("SIZE is computed (drawing.sheet.set_size)"));
    assert!(err(&mut t, "drawing.new", json!({"properties": {"rev": 2}})).contains("`properties.rev` must be text"));
    assert_eq!(t.doc.kind, DocumentKind::Assembly, "a refused drawing.new leaves the design as it was");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn ansi_inch_dimension_style_is_the_capture() {
    let mut s = Session::default();
    run(&mut s, "drawing.new", json!({}));
    let p = &model(&s).paper;
    let current = p.header.str("DIMSTYLE", "");
    assert_eq!(current, DIM_STYLE);
    assert_eq!(current, "ANSI_INCH");
    let ds = p.dim_style(&current).unwrap();
    assert_eq!(ds.name, "ANSI_INCH");
    assert_eq!(ds.decimals, 2);
    assert_eq!(ds.zero_suppression, 4);
    assert_eq!(ds.text_height, 0.12);
    assert_eq!(ds.arrow_size, 0.125);
    assert_eq!(ds.arrow_block, "");
    assert_eq!(ds.text_above, 0);
    assert_eq!(ds.ext_offset, 0.0625);
    assert_eq!(ds.ext_extend, 0.125);
    assert_eq!(ds.text_gap, 0.0625);
    assert_eq!(*ds, drawing::template::ansi_inch_dim_style());
    // Everything else is CADCraft's default style.
    let rest = cadcraft_doc::DimStyle { name: "x".into(), text_style: "x".into(), ..ds.clone() };
    let base = cadcraft_doc::DimStyle {
        name: "x".into(),
        text_style: "x".into(),
        decimals: 2,
        zero_suppression: 4,
        text_height: 0.12,
        arrow_size: 0.125,
        ext_extend: 0.125,
        text_gap: 0.0625,
        ..cadcraft_doc::DimStyle::default()
    };
    assert_eq!(rest, base);
    // Its text style is the built-in single-stroke font, never a TTF.
    let style = p.text_style(&ds.text_style).unwrap();
    assert_eq!(style.font, cadcraft_fonts::BUILTIN_FONT);
    assert_eq!(style.font, "CADCraft Stroke");
    assert_eq!(p.header.str("TEXTSTYLE", ""), ds.text_style, "new text uses it too");
    // No dimension variable overrides the style.
    assert!(p.dim_overrides().is_empty(), "{:?}", p.dim_overrides());
}

#[test]
fn layers_follow_the_template() {
    let mut s = Session::default();
    run(&mut s, "drawing.new", json!({}));
    let p = &model(&s).paper;
    let names: Vec<&str> = LAYERS.iter().map(|(n, _, _)| *n).collect();
    assert_eq!(names, ["VISIBLE", "HIDDEN", "CENTER", "DIMENSION", "NOTE", "BORDER"]);
    for (name, linetype, weight) in LAYERS {
        let layer = p.layer(name).unwrap_or_else(|| panic!("no layer {name}"));
        assert_eq!(layer.name, name);
        assert_eq!(layer.linetype, linetype, "{name}");
        assert_eq!(layer.lineweight, Lineweight::Mm100(weight as u16), "{name}");
        assert!(p.linetype(linetype).is_some(), "{name}'s linetype {linetype} is loaded");
    }
    assert_eq!(p.layer("HIDDEN").unwrap().lineweight, Lineweight::Mm100(35));
    assert_eq!(p.linetype("HIDDEN").unwrap().pattern.iter().map(|d| d.length).collect::<Vec<_>>(), [0.25, -0.125]);
    assert_eq!(p.linetype("CENTER").unwrap().pattern.iter().map(|d| d.length).collect::<Vec<_>>(), [1.25, -0.25, 0.25, -0.25]);
}

#[test]
fn drawing_commands_refuse_other_documents() {
    let ids: Vec<&str> = crate::command_specs().iter().map(|c| c.id).filter(|id| id.starts_with("drawing.") && *id != "drawing.new").collect();
    for id in &ids {
        let mut s = Session::default();
        let e = err(&mut s, id, json!({}));
        assert!(e.contains("is not a drawing"), "{id}: {e}");
    }
    // The helper every drawing command goes through, on a part and on an assembly.
    for kind in ["document.new_part", "document.new_assembly"] {
        let mut s = Session::default();
        run(&mut s, kind, json!({}));
        let e = drawing::require(&s, "drawing.test").unwrap_err();
        assert_eq!(e.to_string(), "drawing.test: the active document is not a drawing (drawing.new makes one)");
        let before = s.doc.clone();
        let e = drawing::edit(&mut s, "drawing.test", |_, _| Ok(())).unwrap_err();
        assert!(e.to_string().contains("is not a drawing"), "{e}");
        assert!(std::sync::Arc::ptr_eq(&before, &s.doc), "nothing changed");
    }
    // On a drawing, `edit` changes a copy and installs it.
    let mut s = Session::default();
    run(&mut s, "drawing.new", json!({}));
    let before = s.doc.clone();
    let n = drawing::edit(&mut s, "drawing.test", |m, _| {
        m.properties.rev = "B".into();
        Ok(7)
    })
    .unwrap();
    assert_eq!(n, 7);
    assert_eq!(model(&s).properties.rev, "B");
    assert_eq!(before.drawing.as_deref().unwrap().properties.rev, "", "the old document is untouched");
}

#[test]
fn a_drawing_refuses_model_geometry() {
    let dir = temp_dir("refuses-geometry");
    let mut s = Session::default();
    run(&mut s, "drawing.new", json!({}));
    let e = s.execute("solid.box", &json!({"length": 1, "width": 1, "height": 1})).unwrap_err();
    assert!(matches!(e, EngineError::Other(_)), "{e:?}");
    assert_eq!(e.to_string(), "solid.box: a drawing holds no model geometry: open the part or assembly");
    assert!(s.doc.features.is_empty() && s.undo.is_empty());
    assert!(err(&mut s, "sketch.create", json!({"plane": "XY"})).contains("a drawing holds no model geometry"));
    // Inserting a STEP or mesh file adds model geometry too (R47).
    assert_eq!(
        err(&mut s, "file.insert_step", json!({"path": dir.join("x.step")})),
        "file.insert_step: a drawing holds no model geometry: open the part or assembly"
    );
    assert!(err(&mut s, "file.insert_mesh", json!({"path": dir.join("x.stl")})).contains("a drawing holds no model geometry"));
    assert!(s.undo.is_empty());
    // Files, documents and the drawing's own commands still run.
    run(&mut s, "file.save", json!({"path": dir.join("sheet")}));
    assert!(dir.join("sheet.tcidrw").is_file());
    assert_eq!(run(&mut s, "document.kind", json!({}))["kind"], "drawing");
    // A part takes geometry as before.
    run(&mut s, "document.new_part", json!({}));
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1}));
    assert_eq!(s.doc.features.len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_drawing_saves_and_opens() {
    let (mut s, block) = block_drawing("save-open");
    let dir = std::path::Path::new(&block).parent().unwrap().to_path_buf();
    let saved = attribs(model(&s));
    let before = model(&s).clone();
    let x = dir.join("x");
    run(&mut s, "file.save", json!({"path": x}));
    let file = dir.join("x.tcidrw");
    assert!(file.is_file());
    // Showing another sheet is view state: opening the file starts on the first.
    s.drawing_sheet = 3;
    run(&mut s, "doc.open", json!({"path": file}));
    assert_eq!(s.doc.kind, DocumentKind::Drawing);
    assert_eq!(s.drawing_sheet, 0, "the shown sheet is view state, not saved");
    let m = model(&s);
    assert_eq!(m.sheets.len(), 1);
    assert_eq!(m.sheets[0], before.sheets[0]);
    assert_eq!(attribs(m), saved);
    assert_eq!(attrib_map(m)["TITLE"], "block");
    assert_eq!(*m, before, "the whole drawing reads back");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn default_drawing_path_is_next_to_the_model() {
    assert_eq!(default_drawing_path("C:/w/block.tcipart"), "C:/w/block.tcidrw");
    assert_eq!(default_drawing_path("parts/arm.tciasm"), "parts/arm.tcidrw");
    assert_eq!(default_drawing_path("C:\\w\\v1.2\\block"), "C:\\w\\v1.2\\block.tcidrw");
    assert_eq!(default_drawing_path("old.design.solidcraft"), "old.design.tcidrw");
}

#[test]
fn drawing_new_checks_its_model() {
    let dir = temp_dir("checks-model");
    let mut s = Session::default();
    let drw = dir.join("x.tcidrw").to_string_lossy().into_owned();
    assert!(err(&mut s, "drawing.new", json!({"model": drw})).contains("a drawing view shows a part or an assembly"));
    let missing = dir.join("missing.tcipart").to_string_lossy().into_owned();
    assert!(err(&mut s, "drawing.new", json!({"model": missing})).contains("model not found"));
    assert!(err(&mut s, "drawing.new", json!({"size": "F"})).contains("size must be A, B, C, D or E"));
    assert!(err(&mut s, "drawing.new", json!({"template": "iso_mm"})).contains("`template` must be \"ansi_inch\""));
    assert!(err(&mut s, "drawing.new", json!({"landscape": "yes"})).contains("`landscape` must be true or false"));
    // A saved drawing given as the model is refused too, whatever its extension says.
    run(&mut s, "drawing.new", json!({}));
    run(&mut s, "file.save", json!({"path": dir.join("y.tcidrw")}));
    let y = dir.join("y.tcidrw").to_string_lossy().into_owned();
    assert!(err(&mut s, "drawing.new", json!({"model": y})).contains("a drawing view shows a part or an assembly"));
    // Other sizes and portrait.
    let mut t = Session::default();
    assert_eq!(run(&mut t, "drawing.new", json!({"size": " b ", "landscape": false}))["size"], "B");
    let m = model(&t);
    assert_eq!((m.sheets[0].size, m.sheets[0].landscape), (AnsiSize::B, false));
    let page = &m.paper.layout("Sheet:1").unwrap().page;
    assert!(close(page.width_mm, 279.4) && close(page.height_mm, 431.8) && !page.landscape, "{page:?}");
    assert_eq!(
        m.paper.entity(m.sheets[0].border).map(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block == "TCI_BORDER_B_PORTRAIT")),
        Some(true)
    );
    assert_eq!(attrib_map(m)["SIZE"], "B");
    let _ = std::fs::remove_dir_all(dir);
}
