#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;

use cadcraft_doc::{EntityKind, Handle};
use serde_json::{Value, json};
use solidcraft_doc::{AnsiSize, DrawingModel};

use crate::Session;

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

/// A new drawing without a model: one D landscape sheet.
fn new_drawing() -> Session {
    let mut s = Session::default();
    run(&mut s, "drawing.new", json!({}));
    s
}

/// Sheet `i`'s title block value for `tag`.
fn field(m: &DrawingModel, i: usize, tag: &str) -> String {
    let e = m.paper.entity(m.sheets[i].title_block).expect("the title block insert");
    let EntityKind::Insert(ins) = &e.kind else { panic!("the title block is an Insert: {:?}", e.kind) };
    ins.attribs.iter().find(|a| a.tag == tag).map(|a| a.text.value.clone()).unwrap_or_else(|| panic!("no attribute {tag}"))
}

/// The block name and insertion point of the Insert `h`.
fn insert_of(m: &DrawingModel, h: Handle) -> (String, [f64; 2]) {
    let e = m.paper.entity(h).expect("an insert");
    let EntityKind::Insert(ins) = &e.kind else { panic!("not an Insert: {:?}", e.kind) };
    (ins.block.clone(), [ins.insert.x, ins.insert.y])
}

fn layouts(m: &DrawingModel) -> Vec<String> {
    m.paper.layouts.iter().map(|l| l.name.clone()).collect()
}

#[test]
fn sheet_add_appends_activates_and_renumbers() {
    let mut s = new_drawing();
    let undo = s.undo.len();
    assert_eq!(run(&mut s, "drawing.sheet.add", json!({})), json!({"sheet": 1}));
    assert_eq!(s.drawing_sheet, 1, "the new sheet is active (R7)");
    assert_eq!(s.undo.len(), undo + 1, "one undo step");
    let m = model(&s);
    assert_eq!(layouts(m), ["Sheet:1", "Sheet:2"]);
    assert_eq!(m.sheets.len(), 2);
    assert_eq!((m.sheets[1].layout.as_str(), m.sheets[1].size, m.sheets[1].landscape), ("Sheet:2", AnsiSize::D, true));
    assert_eq!(field(m, 0, "SHEET"), "1 OF 2");
    assert_eq!(field(m, 1, "SHEET"), "2 OF 2");
    assert_eq!(field(m, 1, "SIZE"), "D");
    run(&mut s, "edit.undo", json!({}));
    let m = model(&s);
    assert_eq!(m.sheets.len(), 1);
    assert_eq!(layouts(m), ["Sheet:1"]);
    assert_eq!(field(m, 0, "SHEET"), "1 OF 1");
    assert_eq!(s.drawing_sheet, 0, "undo clamps the active sheet to the last one (R7)");
}

#[test]
fn sheet_add_takes_the_active_size() {
    let mut s = new_drawing();
    run(&mut s, "drawing.sheet.set_size", json!({"sheet": 0, "size": "C", "landscape": true}));
    assert_eq!(run(&mut s, "drawing.sheet.add", json!({})), json!({"sheet": 1}));
    let m = model(&s);
    assert_eq!((m.sheets[0].size, m.sheets[1].size, m.sheets[1].landscape), (AnsiSize::C, AnsiSize::C, true));
    let page = &m.paper.layout("Sheet:2").unwrap().page;
    // Portrait media plus the landscape flag (R46).
    assert!(close(page.width_mm, 431.8) && close(page.height_mm, 558.8) && page.landscape, "{page:?}");
    assert_eq!(insert_of(m, m.sheets[1].border).0, "TCI_BORDER_C");
    assert!(m.paper.block("TCI_BORDER_D").is_none(), "no sheet is D any more");
    let at = insert_of(m, m.sheets[1].title_block).1;
    assert!(close(at[0], 21.5) && close(at[1], 0.5), "{at:?}");
    assert_eq!(field(m, 1, "SIZE"), "C");
    // From the active sheet: a portrait A sheet added to, then active, gives a portrait A sheet.
    run(&mut s, "drawing.sheet.set_size", json!({"sheet": 1, "size": "A", "landscape": false}));
    assert_eq!(run(&mut s, "drawing.sheet.add", json!({})), json!({"sheet": 2}));
    let m = model(&s);
    assert_eq!((m.sheets[2].size, m.sheets[2].landscape), (AnsiSize::A, false));
    assert_eq!(insert_of(m, m.sheets[2].border).0, "TCI_BORDER_A_PORTRAIT");
    // Sheet 0 still uses the C border, so resizing sheet 1 away from it kept it.
    assert!(m.paper.block("TCI_BORDER_C").is_some());
    let borders: Vec<&String> = m.paper.blocks.keys().filter(|k| k.starts_with("TCI_BORDER_")).collect();
    assert_eq!(borders, ["TCI_BORDER_A_PORTRAIT", "TCI_BORDER_C"]);
    assert_eq!(field(m, 2, "SHEET"), "3 OF 3");
}

#[test]
fn set_size_regenerates_border_and_title_block() {
    let mut s = new_drawing();
    let (border, title) = (model(&s).sheets[0].border, model(&s).sheets[0].title_block);
    let undo = s.undo.len();
    let r = run(&mut s, "drawing.sheet.set_size", json!({"sheet": 0, "size": "B", "landscape": true}));
    assert_eq!(r, json!({"sheet": 0, "size": "B", "landscape": true}));
    assert_eq!(s.undo.len(), undo + 1, "one undo step");
    let m = model(&s);
    let page = &m.paper.layout("Sheet:1").unwrap().page;
    // Portrait media plus the landscape flag (R46); the renderer lays it out 17 x 11 in.
    assert!(close(page.width_mm, 279.4) && close(page.height_mm, 431.8), "{page:?}");
    assert!(page.landscape);
    let rendered = cadcraft_render::Sheet::from_page(page, 25.4);
    assert!(close(rendered.size.x, 17.0) && close(rendered.size.y, 11.0), "{:?}", rendered.size);
    assert_eq!(page.paper, "ANSI B (11.00 x 17.00 Inches)");
    assert_eq!((m.sheets[0].size, m.sheets[0].landscape), (AnsiSize::B, true));
    // The same inserts, regenerated in place.
    assert_eq!((m.sheets[0].border, m.sheets[0].title_block), (border, title));
    assert_eq!(insert_of(m, border), ("TCI_BORDER_B".to_string(), [0.0, 0.0]));
    // The D border no sheet uses any more is gone (final wave 7).
    assert!(m.paper.block("TCI_BORDER_D").is_none());
    let block = m.paper.block("TCI_BORDER_B").expect("the B border block");
    let zones: BTreeSet<String> = block
        .entities
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Text(t) => Some(t.value.clone()),
            _ => None,
        })
        .collect();
    let expected: BTreeSet<String> = ["1", "2", "3", "4", "A", "B", "C", "D"].iter().map(ToString::to_string).collect();
    assert_eq!(zones, expected);
    let (name, at) = insert_of(m, title);
    assert_eq!(name, "TCI_TITLE_ANSI");
    assert!(close(at[0], 16.5) && close(at[1], 0.5), "{at:?}");
    // The attributes moved with the insert: TITLE's value starts 0.08 in right of its cell's left
    // edge, 3.5 in left of the corner.
    let e = m.paper.entity(title).unwrap();
    let EntityKind::Insert(ins) = &e.kind else { panic!("not an Insert") };
    let t = ins.attribs.iter().find(|a| a.tag == "TITLE").unwrap();
    assert!(close(t.text.insert.x, 16.5 - 3.5 + 0.08), "{:?}", t.text.insert);
    assert_eq!(field(m, 0, "SIZE"), "B");
    assert_eq!(field(m, 0, "SHEET"), "1 OF 1");
    // Refusals add no step.
    assert!(err(&mut s, "drawing.sheet.set_size", json!({"sheet": 3, "size": "B"})).contains("sheet 3 does not exist (1 sheets)"));
    assert!(err(&mut s, "drawing.sheet.set_size", json!({"size": "F"})).contains("size must be A, B, C, D or E"));
    assert_eq!(s.undo.len(), undo + 1);
    run(&mut s, "edit.undo", json!({}));
    let m = model(&s);
    assert_eq!(m.sheets[0].size, AnsiSize::D);
    assert_eq!(insert_of(m, border).0, "TCI_BORDER_D");
    assert!(close(insert_of(m, title).1[0], 33.5));
    assert_eq!(field(m, 0, "SIZE"), "D");
}

#[test]
fn activate_switches_without_an_undo_step() {
    let mut s = new_drawing();
    run(&mut s, "drawing.sheet.add", json!({}));
    assert_eq!(s.drawing_sheet, 1);
    let undo = s.undo.len();
    assert_eq!(run(&mut s, "drawing.sheet.activate", json!({"sheet": 0})), json!({"sheet": 0}));
    assert_eq!(s.drawing_sheet, 0);
    assert_eq!(s.undo.len(), undo, "activating a sheet is view state, not an undo step");
    assert!(err(&mut s, "drawing.sheet.activate", json!({"sheet": 5})).contains("sheet 5 does not exist (2 sheets)"));
    assert_eq!(s.drawing_sheet, 0);
    // Not a drawing: every sheet and property command fails cleanly.
    let mut part = Session::default();
    for (id, p) in [
        ("drawing.sheet.add", json!({})),
        ("drawing.sheet.activate", json!({"sheet": 0})),
        ("drawing.sheet.set_size", json!({"size": "B"})),
        ("drawing.properties.set", json!({"title": "X"})),
    ] {
        assert!(err(&mut part, id, p).contains(crate::drawing::NOT_A_DRAWING), "{id}");
    }
    assert!(part.doc.drawing.is_none());
}

#[test]
fn properties_set_fills_every_title_block() {
    let mut s = new_drawing();
    run(&mut s, "drawing.sheet.add", json!({}));
    let undo = s.undo.len();
    run(&mut s, "drawing.properties.set", json!({"title": "BRACKET", "rev": "A", "checked_by": "KP"}));
    assert_eq!(s.undo.len(), undo + 1, "one undo step");
    let m = model(&s);
    for i in 0..2 {
        assert_eq!(field(m, i, "TITLE"), "BRACKET");
        assert_eq!(field(m, i, "REV"), "A");
        assert_eq!(field(m, i, "CHECKED_BY"), "KP");
    }
    assert_eq!((m.properties.title.as_str(), m.properties.rev.as_str(), m.properties.checked_by.as_str()), ("BRACKET", "A", "KP"));
    assert!(err(&mut s, "drawing.properties.set", json!({"qa": "x"})).contains("unknown title block field qa"));
    assert!(err(&mut s, "drawing.properties.set", json!({"size": "B"})).contains("SIZE is computed (drawing.sheet.set_size)"));
    assert!(err(&mut s, "drawing.properties.set", json!({"rev": 2})).contains("`rev` must be text"));
    assert_eq!(s.undo.len(), undo + 1, "a refused call adds no step");
    assert_eq!(field(model(&s), 0, "REV"), "A");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(field(model(&s), 1, "TITLE"), "");
    assert_eq!(model(&s).properties.rev, "");
}

/// R79: going back to a checkpoint clamps the active sheet, as undo does (R7).
#[test]
fn restoring_a_checkpoint_clamps_the_sheet() {
    let mut s = new_drawing();
    let id = s.checkpoint();
    run(&mut s, "drawing.sheet.add", json!({}));
    assert_eq!(s.drawing_sheet, 1);
    s.restore_checkpoint(id).unwrap();
    assert_eq!((model(&s).sheets.len(), s.drawing_sheet), (1, 0));
}

/// R104: a sheet is named "Sheet" until `drawing.sheet.set_name` names it: one undo step, an
/// empty name refused with no step, undo puts the old name back.
#[test]
fn set_name_names_a_sheet_in_one_step() {
    let mut s = new_drawing();
    assert_eq!(model(&s).sheets[0].name, "Sheet");
    let undo = s.undo.len();
    let r = run(&mut s, "drawing.sheet.set_name", json!({"sheet": 0, "name": "  Details "}));
    assert_eq!(r, json!({"sheet": 0, "name": "Details"}));
    assert_eq!(model(&s).sheets[0].name, "Details");
    assert_eq!(s.undo.len(), undo + 1);
    assert_eq!(
        err(&mut s, "drawing.sheet.set_name", json!({"sheet": 0, "name": "  "})),
        "invalid parameters for `drawing.sheet.set_name`: a sheet needs a name"
    );
    assert!(err(&mut s, "drawing.sheet.set_name", json!({"sheet": 3, "name": "X"})).contains("sheet 3 does not exist"));
    assert_eq!(s.undo.len(), undo + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(model(&s).sheets[0].name, "Sheet");
    // A new sheet is "Sheet" too; with no `sheet` the active one is named.
    run(&mut s, "drawing.sheet.add", json!({}));
    run(&mut s, "drawing.sheet.set_name", json!({"name": "Assembly"}));
    let names: Vec<&str> = model(&s).sheets.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["Sheet", "Assembly"]);
}

/// R104: a drawing saved before sheets had names (no `name` in its sheets) opens with "Sheet".
#[test]
fn a_drawing_saved_before_sheet_names_opens_named_sheet() {
    let s = new_drawing();
    let mut v: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let sheets = v["drawing"]["sheets"].as_array_mut().expect("the drawing's sheets in the file");
    for sheet in sheets.iter_mut() {
        assert!(sheet.as_object_mut().unwrap().remove("name").is_some());
    }
    let dir = crate::cmd::drawing::drawing_fixture::temp_dir("a_drawing_saved_before_sheet_names");
    let path = dir.join("old.tcidrw");
    std::fs::write(&path, v.to_string()).unwrap();
    let mut t = Session::default();
    run(&mut t, "doc.open", json!({"path": path.to_string_lossy()}));
    assert_eq!(model(&t).sheets[0].name, "Sheet");
}
