//! The ANSI inch template: our own zoned border and title block, laid out from ASME Y14.1
//! conventions (no Autodesk template, border or title block artwork is read or traced), the
//! `ANSI_INCH` dimension style, the text style on CADCraft's built-in single-stroke font, and
//! the drawing layers. Paper units are inches.

use std::sync::Arc;

use cadcraft_doc::{
    Attrib, AttribModes, AttribProps, Block, Common, DimStyle, Drawing, Entity, EntityKind, HAlign, Handle, Insert, Layer, Layout, Line, Lineweight,
    LwPolyline, PageSetup, Text, TextStyle, VAlign, XrefStatus,
};
use cadcraft_geom::{PolyVertex, Vec2, Vec3};
use solidcraft_doc::{AnsiSize, DrawingModel, TemplateId};

/// The title block's fields (attribute tags), in the block's order (owner default; §8.2): the
/// capture's visible fields plus COMPANY and MATERIAL; no QA or MFG rows.
pub const TITLE_FIELDS: [&str; 14] = [
    "TITLE",
    "COMPANY",
    "DWG_NO",
    "REV",
    "SIZE",
    "SCALE",
    "SHEET",
    "DRAWN_BY",
    "DRAWN_DATE",
    "CHECKED_BY",
    "CHECKED_DATE",
    "APPROVED_BY",
    "APPROVED_DATE",
    "MATERIAL",
]; // (owner default; §8.2)

/// Tags computed from the sheet and views, never set by `drawing.properties.set`; every other tag's
/// `DrawingProps` field is the tag in lower case (R8).
pub const COMPUTED_FIELDS: [&str; 3] = ["SIZE", "SCALE", "SHEET"];

/// The border frame's inset from the sheet's edges, in inches. [A] ASME practice.
pub const BORDER_INSET_IN: f64 = 0.5;

/// The drawing's dimension style.
pub const DIM_STYLE: &str = "ANSI_INCH";

/// The drawing layers: (name, linetype, lineweight in 1/100 mm).
pub const LAYERS: [(&str, &str, i16); 6] = [
    ("VISIBLE", "CONTINUOUS", 50),
    ("HIDDEN", "HIDDEN", 35),
    ("CENTER", "CENTER", 25),
    ("DIMENSION", "CONTINUOUS", 25),
    ("NOTE", "CONTINUOUS", 25),
    ("BORDER", "CONTINUOUS", 50),
];

/// The text style of everything the template writes and of `ANSI_INCH`: CADCraft's built-in
/// single-stroke font, never a TTF (owner default; §8.7).
pub const TEXT_STYLE: &str = "TCI_STROKE";

/// The title block's block name (one title block for every size).
pub const TITLE_BLOCK: &str = "TCI_TITLE_ANSI";

/// Layer of the border and title block.
const BORDER_LAYER: &str = "BORDER";

/// Zone marks: ticks from the frame out into the margin, and the zone text's height.
const ZONE_TICK_IN: f64 = 0.25;
const ZONE_TEXT_IN: f64 = 0.15;
/// Centring marks run from the sheet's edge across the frame, this far inside it.
const CENTRING_PAST_FRAME_IN: f64 = 0.125;

/// The title block: 7 x 2.5 in, drawn left and up from its base point, its lower-right corner.
const TITLE_W: f64 = 7.0;
const TITLE_H: f64 = 2.5;
/// A title block cell's caption height and inset from the cell's top-left corner.
const CAPTION_IN: f64 = 0.07;
const CAPTION_INSET_IN: f64 = 0.04;

/// One title block cell: its tag, caption, corners (block coordinates, lower left and upper right)
/// and the value's text height.
struct Cell {
    tag: &'static str,
    caption: &'static str,
    min: [f64; 2],
    max: [f64; 2],
    height: f64,
}

const fn cell(tag: &'static str, caption: &'static str, min: [f64; 2], max: [f64; 2], height: f64) -> Cell {
    Cell { tag, caption, min, max, height }
}

/// Our title block (ours, not traced): the right half carries what the drawing is (company,
/// title, size, drawing number, revision, scale, sheet), the left half who made it (material,
/// then drawn, checked and approved, each with its date). In `TITLE_FIELDS` order.
const CELLS: [Cell; 14] = [
    cell("TITLE", "TITLE", [-3.5, 1.0], [0.0, 1.75], 0.18),
    cell("COMPANY", "COMPANY", [-3.5, 1.75], [0.0, 2.5], 0.18),
    cell("DWG_NO", "DWG NO", [-2.75, 0.5], [-0.75, 1.0], 0.15),
    cell("REV", "REV", [-0.75, 0.5], [0.0, 1.0], 0.15),
    cell("SIZE", "SIZE", [-3.5, 0.5], [-2.75, 1.0], 0.15),
    cell("SCALE", "SCALE", [-3.5, 0.0], [-1.75, 0.5], 0.12),
    cell("SHEET", "SHEET", [-1.75, 0.0], [0.0, 0.5], 0.12),
    cell("DRAWN_BY", "DRAWN", [-7.0, 1.25], [-4.75, 1.875], 0.12),
    cell("DRAWN_DATE", "DATE", [-4.75, 1.25], [-3.5, 1.875], 0.12),
    cell("CHECKED_BY", "CHECKED", [-7.0, 0.625], [-4.75, 1.25], 0.12),
    cell("CHECKED_DATE", "DATE", [-4.75, 0.625], [-3.5, 1.25], 0.12),
    cell("APPROVED_BY", "APPROVED", [-7.0, 0.0], [-4.75, 0.625], 0.12),
    cell("APPROVED_DATE", "DATE", [-4.75, 0.0], [-3.5, 0.625], 0.12),
    cell("MATERIAL", "MATERIAL", [-7.0, 1.875], [-3.5, 2.5], 0.12),
];

/// `DimStyle::default()` with the capture's ANSI inch values (spec §3.1).
pub(crate) fn ansi_inch_dim_style() -> DimStyle {
    DimStyle {
        name: DIM_STYLE.into(),
        decimals: 2,
        // Leading zeros suppressed (.50), trailing kept (DIMZIN 4).
        zero_suppression: 4,
        text_height: 0.12,
        arrow_size: 0.125,
        // Closed filled.
        arrow_block: String::new(),
        text_above: 0,
        ext_offset: 0.0625,
        ext_extend: 0.125,
        text_gap: 0.0625,
        text_style: TEXT_STYLE.into(),
        ..DimStyle::default()
    }
}

/// Today's local date, "YYYY-MM-DD". [A: Inventor showed m/d/yyyy]
pub(crate) fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// The signed-in user's name, for DRAWN BY: `USERNAME`, else `USER`, else empty.
pub(crate) fn os_user() -> String {
    std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default()
}

/// A drawing from the ANSI inch template, with no sheets yet: its text style, `ANSI_INCH` as the
/// current dimension style, the linetypes the layers use, and the layers.
pub(crate) fn new_model() -> DrawingModel {
    // Inch paper only: no metric template in Plan D (owner default; §8.3).
    let mut m = DrawingModel::new(TemplateId::AnsiInch);
    let p = &mut m.paper;
    p.text_styles.push(TextStyle { name: TEXT_STYLE.into(), font: cadcraft_fonts::BUILTIN_FONT.into(), ..TextStyle::default() });
    p.header.set_str("TEXTSTYLE", TEXT_STYLE);
    p.dim_styles.push(ansi_inch_dim_style());
    p.header.set_str("DIMSTYLE", DIM_STYLE);
    p.sync_dim_vars();
    let library = cadcraft_doc::library::standard_linetypes_for(p);
    for (_, linetype, _) in LAYERS {
        if p.linetype(linetype).is_none()
            && let Some(lt) = library.iter().find(|l| l.name.eq_ignore_ascii_case(linetype))
        {
            p.linetypes.push(lt.clone());
        }
    }
    for (name, linetype, weight) in LAYERS {
        p.layers.push(Layer { linetype: linetype.into(), lineweight: Lineweight::from_dxf(weight), ..Layer::new(name) });
    }
    m
}

/// Adds a sheet of `size` to the drawing: layout `Sheet:<n>` with the ANSI page, our border and
/// the title block (its fields filled by [`refresh_title_blocks`]). Returns the sheet's index.
pub(crate) fn add_sheet(m: &mut DrawingModel, size: AnsiSize, landscape: bool) -> usize {
    let border = ensure_border(&mut m.paper, size, landscape);
    ensure_title_block(&mut m.paper);
    let index = m.sheets.len();
    let name = format!("Sheet:{}", index + 1);
    let (w, _) = size.inches(landscape);
    let mut layout = Layout::new(&name, u32::try_from(index + 1).unwrap_or(u32::MAX));
    layout.page = page(size, landscape);
    let border_insert = m.paper.new_handle();
    layout.entities.push(Entity { handle: border_insert, common: on_border(), kind: EntityKind::Insert(insert(&border, Vec3::ZERO, Vec::new())) });
    let at = Vec3::new(w - BORDER_INSET_IN, BORDER_INSET_IN, 0.0);
    let attribs = title_attribs(&m.paper, at);
    let title_insert = m.paper.new_handle();
    layout.entities.push(Entity { handle: title_insert, common: on_border(), kind: EntityKind::Insert(insert(TITLE_BLOCK, at, attribs)) });
    m.paper.layouts.push(layout);
    m.sheets.push(solidcraft_doc::SheetMeta { layout: name, size, landscape, border: border_insert, title_block: title_insert });
    index
}

/// Every sheet's title block attributes from `properties`, the sheet and the views: SIZE the
/// sheet's letter, SHEET "<n> OF <m>", SCALE the first base view's scale (empty until one
/// exists), the others the `DrawingProps` field named by the tag in lower case.
pub(crate) fn refresh_title_blocks(m: &mut DrawingModel) {
    let props = serde_json::to_value(&m.properties).unwrap_or_default();
    let count = m.sheets.len();
    let first_base = |sheet: Option<usize>| {
        m.views.values().find(|v| v.parent.is_none() && sheet.is_none_or(|s| v.sheet == s)).map(|v| solidcraft_doc::format_scale(v.scale))
    };
    let mut updates: Vec<(Handle, Vec<(String, String)>)> = Vec::new();
    for (i, sheet) in m.sheets.iter().enumerate() {
        // The sheet's own first base view; a sheet with none shows the drawing's.
        let scale = first_base(Some(i)).or_else(|| first_base(None)).unwrap_or_default();
        let values = TITLE_FIELDS
            .iter()
            .map(|tag| {
                let value = match *tag {
                    "SIZE" => sheet.size.letter().to_string(),
                    "SHEET" => format!("{} OF {count}", i + 1),
                    "SCALE" => scale.clone(),
                    _ => props.get(tag.to_ascii_lowercase()).and_then(serde_json::Value::as_str).unwrap_or_default().to_string(),
                };
                (tag.to_string(), value)
            })
            .collect();
        updates.push((sheet.title_block, values));
    }
    for (handle, values) in updates {
        let result = m.paper.modify_entity(handle, |e| {
            if let EntityKind::Insert(ins) = &mut e.kind {
                for a in &mut ins.attribs {
                    if let Some((_, v)) = values.iter().find(|(t, _)| *t == a.tag) {
                        a.text.value = v.clone();
                    }
                }
            }
        });
        if let Err(e) = result {
            log::warn!("a title block was not refreshed: {e}");
        }
    }
}

fn on_border() -> Common {
    Common { layer: BORDER_LAYER.into(), ..Common::default() }
}

fn insert(block: &str, at: Vec3, attribs: Vec<Attrib>) -> Insert {
    Insert {
        block: block.into(),
        insert: at,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs,
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    }
}

/// The page setup of an ANSI sheet: the paper in millimetres as CADCraft keeps them, the portrait
/// media size (width the short side) plus the `landscape` flag (R46): the DXF writer emits the
/// media and the rotation, and the reader takes `landscape = rotated != (w > h)`.
fn page(size: AnsiSize, landscape: bool) -> PageSetup {
    let (a, b) = size.inches(landscape);
    let (w, h) = (a.min(b), a.max(b));
    let paper = cadcraft_render::paper_size(&format!("ANSI {}", size.letter())).map_or_else(String::new, |p| p.name.to_string());
    PageSetup { paper, width_mm: w * 25.4, height_mm: h * 25.4, landscape, ..PageSetup::default() }
}

/// The border block's name: `TCI_BORDER_<letter>`, with `_PORTRAIT` for a portrait sheet.
fn border_name(size: AnsiSize, landscape: bool) -> String {
    if landscape { format!("TCI_BORDER_{}", size.letter()) } else { format!("TCI_BORDER_{}_PORTRAIT", size.letter()) }
}

fn define_block(p: &mut Drawing, name: &str, description: &str, entities: Vec<(Common, EntityKind)>) {
    let mut block = Block {
        name: name.into(),
        annotative: false,
        base: Vec3::ZERO,
        entities: cadcraft_doc::EntityStore::new(),
        description: description.into(),
        xref_overlay: false,
        xref_status: XrefStatus::default(),
        anonymous: false,
        xref_path: None,
        explodable: true,
        // Inches (INSUNITS 1).
        units: 1,
        array: None,
    };
    for (common, kind) in entities {
        let handle = p.new_handle();
        block.entities.push(Entity { handle, common, kind });
    }
    p.blocks.insert(name.into(), Arc::new(block));
}

fn line(a: [f64; 2], b: [f64; 2]) -> (Common, EntityKind) {
    (Common::default(), EntityKind::Line(Line { a: Vec3::new(a[0], a[1], 0.0), b: Vec3::new(b[0], b[1], 0.0) }))
}

fn rectangle(min: [f64; 2], max: [f64; 2]) -> (Common, EntityKind) {
    let corners = [[min[0], min[1]], [max[0], min[1]], [max[0], max[1]], [min[0], max[1]]];
    let vertices = corners.iter().map(|c| PolyVertex::new(Vec2::new(c[0], c[1]))).collect();
    (Common::default(), EntityKind::LwPolyline(LwPolyline { vertices, closed: true, const_width: 0.0, elevation: 0.0, plinegen: false }))
}

/// Text placed by `at` with the given justification.
fn text(value: &str, at: [f64; 2], height: f64, halign: HAlign, valign: VAlign) -> Text {
    let p = Vec3::new(at[0], at[1], 0.0);
    let aligned = !(halign == HAlign::Left && valign == VAlign::Baseline);
    Text {
        insert: p,
        align_pt: aligned.then_some(p),
        height,
        value: value.into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: TEXT_STYLE.into(),
        halign,
        valign,
    }
}

/// Defines `TCI_BORDER_<letter>` once: the frame inset `BORDER_INSET_IN`; zone numbers along the
/// top and bottom and zone letters down both sides, from `zones()`, numbered from the right and
/// lettered from the bottom (ASME Y14.1), between ticks into the margin; centring marks mid-side.
/// Drawn in sheet coordinates (base point the sheet's lower-left corner). Returns its name.
fn ensure_border(p: &mut Drawing, size: AnsiSize, landscape: bool) -> String {
    let name = border_name(size, landscape);
    if p.block(&name).is_some() {
        return name;
    }
    let (w, h) = size.inches(landscape);
    let i = BORDER_INSET_IN;
    let (columns, rows) = size.zones();
    let mut e = vec![rectangle([i, i], [w - i, h - i])];
    let column = (w - 2.0 * i) / f64::from(columns);
    let row = (h - 2.0 * i) / f64::from(rows);
    for k in 1..columns {
        let x = i + f64::from(k) * column;
        e.push(line([x, i], [x, i - ZONE_TICK_IN]));
        e.push(line([x, h - i], [x, h - i + ZONE_TICK_IN]));
    }
    for k in 1..rows {
        let y = i + f64::from(k) * row;
        e.push(line([i, y], [i - ZONE_TICK_IN, y]));
        e.push(line([w - i, y], [w - i + ZONE_TICK_IN, y]));
    }
    let zone = |value: String, at: [f64; 2]| (Common::default(), EntityKind::Text(text(&value, at, ZONE_TEXT_IN, HAlign::Center, VAlign::Middle)));
    for k in 0..columns {
        let x = i + (f64::from(k) + 0.5) * column;
        let number = (columns - k).to_string();
        e.push(zone(number.clone(), [x, i / 2.0]));
        e.push(zone(number, [x, h - i / 2.0]));
    }
    for k in 0..rows {
        let y = i + (f64::from(k) + 0.5) * row;
        let letter = char::from_u32(u32::from('A') + k).map(String::from).unwrap_or_default();
        e.push(zone(letter.clone(), [i / 2.0, y]));
        e.push(zone(letter, [w - i / 2.0, y]));
    }
    let past = i + CENTRING_PAST_FRAME_IN;
    e.push(line([w / 2.0, 0.0], [w / 2.0, past]));
    e.push(line([w / 2.0, h], [w / 2.0, h - past]));
    e.push(line([0.0, h / 2.0], [past, h / 2.0]));
    e.push(line([w, h / 2.0], [w - past, h / 2.0]));
    let description = format!("TCI-CAD ANSI {} border, {}", size.letter(), if landscape { "landscape" } else { "portrait" });
    define_block(p, &name, &description, e);
    name
}

/// Defines `TCI_TITLE_ANSI` once: every cell's outline and caption, and an attribute
/// definition per `TITLE_FIELDS` tag, in that order.
fn ensure_title_block(p: &mut Drawing) {
    if p.block(TITLE_BLOCK).is_some() {
        return;
    }
    let mut e = vec![rectangle([-TITLE_W, 0.0], [0.0, TITLE_H])];
    for c in &CELLS {
        e.push(rectangle(c.min, c.max));
        let caption = text(c.caption, [c.min[0] + CAPTION_INSET_IN, c.max[1] - CAPTION_INSET_IN], CAPTION_IN, HAlign::Left, VAlign::Top);
        e.push((Common::default(), EntityKind::Text(caption)));
    }
    for c in &CELLS {
        let value =
            text("", [c.min[0] + 0.08, c.min[1] + (c.max[1] - c.min[1] - CAPTION_IN - c.height) / 2.0], c.height, HAlign::Left, VAlign::Baseline);
        let def = Attrib {
            tag: c.tag.into(),
            text: value,
            invisible: false,
            constant: false,
            prompt: c.caption.into(),
            props: AttribProps::default(),
            multiline: None,
            modes: AttribModes::default(),
        };
        e.push((Common::default(), EntityKind::AttDef(def)));
    }
    define_block(p, TITLE_BLOCK, "TCI-CAD ANSI title block", e);
}

/// The title block reference's attributes, made from the block's definitions and placed with the
/// reference at `at` (its values empty until [`refresh_title_blocks`]).
fn title_attribs(p: &Drawing, at: Vec3) -> Vec<Attrib> {
    let Some(block) = p.block(TITLE_BLOCK) else { return Vec::new() };
    block
        .entities
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::AttDef(def) => {
                let mut a = def.clone();
                a.text.insert = a.text.insert + at;
                a.text.align_pt = a.text.align_pt.map(|q| q + at);
                a.text.value = String::new();
                a.prompt = String::new();
                a.modes = def.modes.for_reference();
                Some(a)
            }
            _ => None,
        })
        .collect()
}
