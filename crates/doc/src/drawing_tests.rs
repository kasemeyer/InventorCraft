#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use cadcraft_doc::geom::Vec3 as Vec3Cc;
use cadcraft_doc::{Block, Circle, Common, Entity, EntityKind, Group, Insert, Layout, Line, RawTag, Space};
use solidcraft_geom::Vec3;

use super::*;
use crate::{Document, DocumentKind};

fn target(name: &str) -> CubeTarget {
    CubeTarget::from_name(name).unwrap()
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a.x - b.x).abs() <= 1e-12 && (a.y - b.y).abs() <= 1e-12 && (a.z - b.z).abs() <= 1e-12
}

#[test]
fn cube_targets_are_the_26_cube_directions() {
    assert_eq!(CubeTarget::ALL.len(), 26);
    let names: std::collections::BTreeSet<String> = CubeTarget::ALL.iter().map(|t| t.name()).collect();
    assert_eq!(names.len(), 26);
    let pinned: [(&str, (i8, i8, i8)); 12] = [
        ("front", (0, -1, 0)),
        ("back", (0, 1, 0)),
        ("left", (-1, 0, 0)),
        ("right", (1, 0, 0)),
        ("top", (0, 0, 1)),
        ("bottom", (0, 0, -1)),
        ("iso_top_right", (1, -1, 1)),
        ("iso_top_left", (-1, -1, 1)),
        ("iso_bottom_right", (1, -1, -1)),
        ("iso_bottom_left", (-1, -1, -1)),
        ("front_top", (0, -1, 1)),
        ("back_top_left", (-1, 1, 1)),
    ];
    for (name, (x, y, z)) in pinned {
        assert_eq!(CubeTarget::from_name(name), Some(CubeTarget { x, y, z }), "{name}");
        assert_eq!(CubeTarget { x, y, z }.name(), name);
    }
    assert_eq!(CubeTarget::from_name("sideways"), None);
    // Every name reads back as its own target.
    for t in CubeTarget::ALL {
        assert_eq!(CubeTarget::from_name(&t.name()), Some(t));
    }
}

#[test]
fn cube_target_orientations() {
    let front = target("front").orientation();
    assert!(close(front.dir, Vec3::new(0.0, 1.0, 0.0)), "{front:?}");
    assert!(close(front.up, Vec3::new(0.0, 0.0, 1.0)), "{front:?}");
    assert_eq!(front.preset, Some(target("front")));
    let top = target("top").orientation();
    assert!(close(top.dir, Vec3::new(0.0, 0.0, -1.0)), "{top:?}");
    assert!(close(top.up, Vec3::new(0.0, 1.0, 0.0)), "{top:?}");
    let bottom = target("bottom").orientation();
    assert!(close(bottom.dir, Vec3::new(0.0, 0.0, 1.0)), "{bottom:?}");
    assert!(close(bottom.up, Vec3::new(0.0, -1.0, 0.0)), "{bottom:?}");
    let right = target("right").orientation();
    assert!(close(right.dir, Vec3::new(-1.0, 0.0, 0.0)), "{right:?}");
    assert!(close(right.up, Vec3::new(0.0, 0.0, 1.0)), "{right:?}");
    let iso = target("iso_top_right").orientation();
    let s = 1.0 / 3f64.sqrt();
    assert!(close(iso.dir, Vec3::new(-s, s, -s)), "{iso:?}");
    assert!(iso.up.dot(iso.dir).abs() <= 1e-12, "{iso:?}");
    assert!(iso.up.z > 0.0, "{iso:?}");
    assert!((iso.up.len() - 1.0).abs() <= 1e-12, "{iso:?}");
    let back = target("back");
    assert_eq!(CubeTarget::matching(&back.orientation()), Some(back));
    let upside_down = Orientation { dir: Vec3::new(0.0, -1.0, 0.0), up: Vec3::new(0.0, 0.0, -1.0), preset: None };
    assert_eq!(CubeTarget::matching(&upside_down), None);
}

#[test]
fn nearest_snaps_a_camera_direction() {
    assert_eq!(CubeTarget::nearest(Vec3::new(-0.55, 0.62, -0.56)), target("iso_top_right"));
    assert_eq!(CubeTarget::nearest(Vec3::new(0.1, 0.99, 0.05)), target("front"));
}

#[test]
fn ansi_sizes_follow_the_standard() {
    let expected = [
        (AnsiSize::A, (11.0, 8.5), (4, 2), "A"),
        (AnsiSize::B, (17.0, 11.0), (4, 4), "B"),
        (AnsiSize::C, (22.0, 17.0), (4, 4), "C"),
        (AnsiSize::D, (34.0, 22.0), (8, 4), "D"),
        (AnsiSize::E, (44.0, 34.0), (8, 4), "E"),
    ];
    for (size, (w, h), zones, letter) in expected {
        assert_eq!(size.inches(true), (w, h), "{letter}");
        assert_eq!(size.inches(false), (h, w), "{letter}");
        assert_eq!(size.zones(), zones, "{letter}");
        assert_eq!(size.letter(), letter);
        assert_eq!(AnsiSize::parse(letter), Some(size));
    }
    assert_eq!(AnsiSize::D.inches(false), (22.0, 34.0));
    assert_eq!(AnsiSize::parse("d"), Some(AnsiSize::D));
    assert_eq!(AnsiSize::parse("F"), None);
    assert_eq!(AnsiSize::E.letter(), "E");
}

#[test]
fn scales_parse_and_format() {
    let names: Vec<&str> = SCALE_PRESETS.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, ["1:4", "1:2", "1:1", "2:1", "3:1", "4:1", "5:1", "10:1"]);
    let values: Vec<f64> = SCALE_PRESETS.iter().map(|(_, v)| *v).collect();
    assert_eq!(values, [0.25, 0.5, 1.0, 2.0, 3.0, 4.0, 5.0, 10.0]);
    for (name, value) in SCALE_PRESETS {
        assert_eq!(parse_scale(name), Some(value), "{name}");
        assert_eq!(format_scale(value), name);
    }
    assert_eq!(parse_scale("3:8"), Some(0.375));
    assert_eq!(parse_scale("1/4"), Some(0.25));
    assert_eq!(parse_scale("0.5"), Some(0.5));
    assert_eq!(parse_scale("2"), Some(2.0));
    for bad in ["0:1", "-1", "1:0", "abc", "NaN", "", "inf"] {
        assert_eq!(parse_scale(bad), None, "{bad}");
    }
    assert_eq!(format_scale(0.375), "3:8");
    assert_eq!(format_scale(0.25), "1:4");
    assert_eq!(format_scale(10.0), "10:1");
    assert_eq!(format_scale(0.3), "3:10");
    assert_eq!(format_scale(100.0), "100");
    assert_eq!(format_scale(0.1234567), "0.123457");
}

#[test]
fn a_drawing_document_round_trips() {
    let mut doc = Document::new_with_kind("D1", DocumentKind::Drawing);
    assert_eq!(doc.drawing, None);
    doc.drawing = Some(Arc::new(DrawingModel::new(TemplateId::AnsiInch)));
    let model = doc.drawing.as_deref().unwrap();
    assert!(model.sheets.is_empty());
    assert!(model.views.is_empty());
    assert!(model.paper.layouts.is_empty());
    assert_eq!(model.template, TemplateId::AnsiInch);
    assert_eq!(model.next_view_number, 1);
    assert_eq!(model.properties, DrawingProps::default());
    let text = serde_json::to_string(&doc).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["drawing"]["template"], "ansi_inch");
    let back: Document = serde_json::from_str(&text).unwrap();
    assert_eq!(back, doc);
}

fn sample_paper() -> (cadcraft_doc::Drawing, cadcraft_doc::Handle) {
    let mut paper = cadcraft_doc::Drawing::new_imperial();
    paper.layouts = vec![Layout::new("Sheet:1", 1)];
    let sheet = Space::Paper("Sheet:1".into());
    let xdata = vec![RawTag { code: 1001, value: "TCICAD_VIEW".into() }, RawTag { code: 1000, value: "7".into() }];
    let line = paper
        .add(&sheet, Common { xdata, ..Common::default() }, EntityKind::Line(Line { a: Vec3Cc::new(0.0, 0.0, 0.0), b: Vec3Cc::new(2.0, 1.0, 0.0) }))
        .unwrap();
    let circle = paper.new_handle();
    let mut block = Block::new("B");
    block.entities.push(Entity::new(circle, EntityKind::Circle(Circle { center: Vec3Cc::new(0.5, 0.5, 0.0), radius: 0.25 })));
    paper.blocks.insert("B".into(), Arc::new(block));
    let insert = Insert {
        block: "B".into(),
        insert: Vec3Cc::new(3.0, 4.0, 0.0),
        scale: Vec3Cc::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    paper.add(&sheet, Common::default(), EntityKind::Insert(insert)).unwrap();
    // One model-space entity and one group, so the round trip covers both.
    paper.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3Cc::new(-1.0, 0.0, 0.0), b: Vec3Cc::new(-1.0, 3.0, 0.0) })).unwrap();
    paper.groups.push(Group { name: "VIEW7".into(), description: "a view".into(), selectable: true, members: vec![line] });
    (paper, line)
}

#[test]
fn the_paper_keeps_entities_handles_and_xdata() {
    let (paper, line) = sample_paper();
    let mut model = DrawingModel::new(TemplateId::AnsiInch);
    model.paper = paper.clone();
    let text = serde_json::to_string(&model).unwrap();
    let back: DrawingModel = serde_json::from_str(&text).unwrap();
    assert_eq!(back.paper, paper);
    assert_eq!(back, model);
    assert_eq!(back.paper.handseed, paper.handseed);
    assert_eq!(back.paper.handseed, 0x104);
    let model_lines: Vec<&Entity> = back.paper.model.iter().map(|e| &**e).collect();
    assert_eq!(model_lines.len(), 1);
    assert!(matches!(&model_lines[0].kind, EntityKind::Line(l) if l.b.y == 3.0), "{model_lines:?}");
    assert_eq!(back.paper.groups.len(), 1);
    let g = &back.paper.groups[0];
    assert_eq!((g.name.as_str(), g.description.as_str(), g.selectable, g.members.as_slice()), ("VIEW7", "a view", true, &[line][..]));
    let layout = back.paper.layouts.iter().find(|l| l.name == "Sheet:1").unwrap();
    assert_eq!(layout.entities.len(), 2);
    let kept = layout.entities.get(line).unwrap();
    assert_eq!(kept.handle, line);
    assert_eq!(kept.common.xdata, vec![RawTag { code: 1001, value: "TCICAD_VIEW".into() }, RawTag { code: 1000, value: "7".into() }]);
    let block = back.paper.blocks.get("B").unwrap();
    assert_eq!(block.entities.len(), 1);
    assert!(matches!(&block.entities.iter().next().unwrap().kind, EntityKind::Circle(c) if c.radius == 0.25));
    let inserts = layout.entities.iter().filter(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block == "B")).count();
    assert_eq!(inserts, 1);
}

#[test]
fn an_older_design_has_no_drawing() {
    let part = Document::new_with_kind("P1", DocumentKind::Part);
    let mut value = serde_json::to_value(&part).unwrap();
    assert!(value.get("drawing").is_none(), "{value}");
    value.as_object_mut().unwrap().remove("drawing");
    let back: Document = serde_json::from_value(value).unwrap();
    assert_eq!(back.drawing, None);
    assert_eq!(back, part);
}

#[test]
fn rasters_save_as_base64() {
    let raster = RasterRef { width: 2, height: 1, png: vec![137, 80, 78, 71] };
    let value = serde_json::to_value(&raster).unwrap();
    assert_eq!(value["png"], "iVBORw==");
    assert_eq!(value["width"], 2);
    assert_eq!(value["height"], 1);
    let back: RasterRef = serde_json::from_value(value).unwrap();
    assert_eq!(back, raster);
    let bad = serde_json::json!({"width": 2, "height": 1, "png": "iVBORw="});
    let e = serde_json::from_value::<RasterRef>(bad).unwrap_err().to_string();
    // The shared decoder's messages name no HDR (final wave 1).
    assert_eq!(e, "a view's image: base64 exceeds the resource budget or is not canonical");
    let bad = serde_json::json!({"width": 2, "height": 1, "png": "iVB!Rw=="});
    let e = serde_json::from_value::<RasterRef>(bad).unwrap_err().to_string();
    assert_eq!(e, "a view's image: noncanonical base64");
}

#[test]
fn revision_is_the_documents_sha256() {
    let a = Document::new_with_kind("P1", DocumentKind::Part);
    let b = a.clone();
    let ra = revision_of(&a);
    assert_eq!(ra.0.len(), 64);
    assert!(ra.0.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)), "{}", ra.0);
    assert_eq!(revision_of(&b), ra);
    let mut renamed = b;
    renamed.name = "P2".into();
    assert_ne!(revision_of(&renamed), ra);
    assert_eq!(ModelRevision::default().0, "");
}

#[test]
fn format_pending_lists_the_drawing_model() {
    assert!(crate::format::pending_names().iter().any(|n| n == "drawing-model"), "{:?}", crate::format::pending_names());
}

#[test]
fn a_projected_view_saves_its_projection() {
    use crate::drawing::Projection;
    let text = |p: Projection| serde_json::to_value(p).unwrap();
    assert_eq!(text(Projection::Top), serde_json::json!("top"));
    assert_eq!(text(Projection::Left), serde_json::json!("left"));
    assert_eq!(text(Projection::Iso { sx: 1, sy: -1 }), serde_json::json!({"iso": {"sx": 1, "sy": -1}}));
    for p in [Projection::Top, Projection::Bottom, Projection::Left, Projection::Right, Projection::Iso { sx: -1, sy: 1 }] {
        assert_eq!(serde_json::from_value::<Projection>(text(p)).unwrap(), p);
    }
}

/// Final wave 4: only the 26 canonical targets have names (each loads back as itself).
#[test]
fn every_cube_target_name_loads_back() {
    for t in CubeTarget::ALL {
        assert_eq!(CubeTarget::from_name(&t.name()), Some(t));
    }
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a non-canonical view cube target")]
fn a_non_canonical_cube_target_has_no_name() {
    let _ = CubeTarget { x: 2, y: 0, z: 0 }.name();
}

/// Final wave 14: a view saved before R55 (no `projection`) loads with none.
#[test]
fn a_view_without_a_projection_loads_with_none() {
    use crate::drawing::Projection;
    let view = DrawingView {
        id: 2,
        identifier: "VIEW2".into(),
        sheet: 0,
        parent: Some(1),
        projection: Some(Projection::Top),
        model: ModelRef { path: "block.tcipart".into(), project_path: None, kind: ModelKind::Part },
        model_state: None,
        orientation: CubeTarget::from_name("top").unwrap().orientation(),
        scale: 1.0,
        scale_from_base: true,
        lines: LineStyle::Hidden,
        shaded: false,
        style_from_base: true,
        tangent_edges: false,
        position: solidcraft_geom::Vec2::new(10.0, 16.0),
        label_visible: false,
        group: cadcraft_doc::Handle(0x2a),
        revision: ModelRevision("ab".into()),
        raster: None,
    };
    let mut value = serde_json::to_value(&view).unwrap();
    assert_eq!(value["projection"], "top");
    value.as_object_mut().unwrap().remove("projection");
    let back: DrawingView = serde_json::from_value(value).unwrap();
    assert_eq!(back.projection, None);
    assert_eq!(back, DrawingView { projection: None, ..view });
}
