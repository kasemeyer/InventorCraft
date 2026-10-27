//! What a `.tcidrw` saves: a [`DrawingModel`] in `Document::drawing` (a `Document` of kind
//! `DocumentKind::Drawing`, which holds no bodies, features or components). The model wraps
//! one vendored `cadcraft_doc::Drawing` (the paper: every sheet's layout, block and entity)
//! with our sheet and view metadata and the title block's field values.
//!
//! Units: paper is inches (an ANSI inch drawing); the model a view shows is in mm, and a view's
//! `scale` is paper / model.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Vec2, Vec3};

use crate::Document;

/// A view's identity within its drawing.
pub type ViewId = u64;

/// A drawing: its paper (all sheets, entities and styles), sheets, views and title block fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrawingModel {
    /// The vendored drawing database: one CADCraft layout per sheet.
    #[serde(with = "crate::drawing_paper")]
    pub paper: cadcraft_doc::Drawing,
    /// The sheets in order, one per layout of `paper`.
    pub sheets: Vec<SheetMeta>,
    pub views: BTreeMap<ViewId, DrawingView>,
    /// The title block's field values.
    pub properties: DrawingProps,
    pub template: TemplateId,
    /// The number the next view's identifier takes (`VIEW<n>`).
    pub next_view_number: u32,
}

impl DrawingModel {
    /// A drawing with no sheets (the paper has no layouts yet) from `template`.
    pub fn new(template: TemplateId) -> DrawingModel {
        let mut paper = match template {
            TemplateId::AnsiInch => cadcraft_doc::Drawing::new_imperial(),
        };
        paper.layouts.clear();
        DrawingModel { paper, sheets: Vec::new(), views: BTreeMap::new(), properties: DrawingProps::default(), template, next_view_number: 1 }
    }
}

/// One sheet: a CADCraft layout with our border and title block placed on it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetMeta {
    /// The CADCraft layout's name.
    pub layout: String,
    pub size: AnsiSize,
    pub landscape: bool,
    /// The `Insert` of the border block.
    pub border: cadcraft_doc::Handle,
    /// The `Insert` of the title block, whose attributes are the fields.
    pub title_block: cadcraft_doc::Handle,
}

/// A view of a part or assembly on a sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrawingView {
    pub id: ViewId,
    /// `VIEW1`, `VIEW2`…
    pub identifier: String,
    /// The sheet's index in `DrawingModel::sheets`.
    pub sheet: usize,
    /// The view this one is projected from.
    pub parent: Option<ViewId>,
    pub model: ModelRef,
    /// `None`: the model's only state.
    pub model_state: Option<String>,
    pub orientation: Orientation,
    /// Paper / model.
    pub scale: f64,
    pub scale_from_base: bool,
    pub lines: LineStyle,
    pub shaded: bool,
    pub style_from_base: bool,
    pub tangent_edges: bool,
    /// The view's centre on paper, inches.
    pub position: Vec2,
    pub label_visible: bool,
    /// The CADCraft group holding this view's entities.
    pub group: cadcraft_doc::Handle,
    /// The model's revision when the view was last projected.
    pub revision: ModelRevision,
    /// The shaded image.
    pub raster: Option<RasterRef>,
}

/// The direction a view looks along (`dir`, model space, unit) and the model direction shown
/// up on paper (`up`, unit, perpendicular to `dir`), and the cube target it was set from.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Orientation {
    pub dir: Vec3,
    pub up: Vec3,
    pub preset: Option<CubeTarget>,
}

/// A face, edge or corner of the view cube: the side the eye is on, each of `x`, `y`, `z` in
/// -1..=1 and not all 0. Saved by name (`front`, `iso_top_right`, `back_top_left`…).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct CubeTarget {
    pub x: i8,
    pub y: i8,
    pub z: i8,
}

impl CubeTarget {
    /// The 26 targets.
    pub const ALL: [CubeTarget; 26] = {
        let mut out = [CubeTarget { x: 0, y: 0, z: 0 }; 26];
        let mut i = 0;
        let mut x = -1;
        while x <= 1 {
            let mut y = -1;
            while y <= 1 {
                let mut z = -1;
                while z <= 1 {
                    if x != 0 || y != 0 || z != 0 {
                        out[i] = CubeTarget { x, y, z };
                        i += 1;
                    }
                    z += 1;
                }
                y += 1;
            }
            x += 1;
        }
        out
    };

    /// The target's name: its words joined by `_` in the order front/back, top/bottom,
    /// left/right; the four front corners are `iso_<top|bottom>_<left|right>`.
    pub fn name(self) -> String {
        let front_back = match self.y {
            -1 => Some("front"),
            1 => Some("back"),
            _ => None,
        };
        let top_bottom = match self.z {
            1 => Some("top"),
            -1 => Some("bottom"),
            _ => None,
        };
        let left_right = match self.x {
            -1 => Some("left"),
            1 => Some("right"),
            _ => None,
        };
        if let (Some("front"), Some(tb), Some(lr)) = (front_back, top_bottom, left_right) {
            return format!("iso_{tb}_{lr}");
        }
        [front_back, top_bottom, left_right].into_iter().flatten().collect::<Vec<_>>().join("_")
    }

    /// The target named `s`.
    pub fn from_name(s: &str) -> Option<CubeTarget> {
        CubeTarget::ALL.into_iter().find(|t| t.name() == s)
    }

    /// The view from this side: `dir` toward the model's centre, `up` world +Z made
    /// perpendicular to `dir`, except `top` (+Y) and `bottom` (−Y), whose side toward the front
    /// view faces it (third angle).
    pub fn orientation(self) -> Orientation {
        let eye = Vec3::new(f64::from(self.x), f64::from(self.y), f64::from(self.z));
        let dir = eye.normalized().map_or(Vec3::Y, |e| -e);
        let up = match (self.x, self.y, self.z) {
            (0, 0, 1) => Vec3::Y,
            (0, 0, -1) => -Vec3::Y,
            _ => (Vec3::Z - dir * dir.z).normalized().unwrap_or(Vec3::Z),
        };
        Orientation { dir, up, preset: Some(self) }
    }

    /// The target whose view direction is closest to the look direction `dir`.
    pub fn nearest(dir: Vec3) -> CubeTarget {
        let mut best = CubeTarget::ALL[0];
        let mut best_dot = f64::NEG_INFINITY;
        for t in CubeTarget::ALL {
            let d = t.orientation().dir.dot(dir);
            if d > best_dot {
                best = t;
                best_dot = d;
            }
        }
        best
    }

    /// The target whose orientation equals `o` (dir and up) within 1e-9, else None (R6).
    pub fn matching(o: &Orientation) -> Option<CubeTarget> {
        let near = |a: Vec3, b: Vec3| (a.x - b.x).abs() <= 1e-9 && (a.y - b.y).abs() <= 1e-9 && (a.z - b.z).abs() <= 1e-9;
        CubeTarget::ALL.into_iter().find(|t| {
            let r = t.orientation();
            near(r.dir, o.dir) && near(r.up, o.up)
        })
    }
}

impl From<CubeTarget> for String {
    fn from(t: CubeTarget) -> String {
        t.name()
    }
}

impl TryFrom<String> for CubeTarget {
    type Error = String;
    fn try_from(s: String) -> Result<CubeTarget, String> {
        CubeTarget::from_name(&s).ok_or_else(|| format!("unknown view direction `{s}`"))
    }
}

/// Visible lines only, or visible and hidden lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineStyle {
    Hidden,
    Removed,
}

/// The part or assembly a view shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelRef {
    /// The model's file.
    pub path: String,
    /// The project the model was found through, if any.
    pub project_path: Option<String>,
    pub kind: ModelKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    Part,
    Assembly,
}

/// A model's content hash: lowercase hex SHA-256; empty when the view was never projected.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRevision(pub String);

/// SHA-256 of `serde_json::to_vec(doc)`, lowercase hex (R34). A document that does not
/// serialise has the empty revision, which no projected view has.
pub fn revision_of(doc: &Document) -> ModelRevision {
    use sha2::{Digest, Sha256};
    let Ok(bytes) = serde_json::to_vec(doc) else { return ModelRevision::default() };
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(&bytes) {
        let _ = write!(hex, "{byte:02x}");
    }
    ModelRevision(hex)
}

/// A view's shaded image: a PNG of `width` × `height` pixels, saved as base64.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RasterRef {
    pub width: u32,
    pub height: u32,
    #[serde(with = "png_base64")]
    pub png: Vec<u8>,
}

/// The PNG bytes of a [`RasterRef`] as canonical padded base64 (at most 64 MiB of PNG).
mod png_base64 {
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _, ser::Error as _};

    const MAX_PNG_BYTES: usize = 64 * 1024 * 1024;

    pub(super) fn serialize<S: Serializer>(png: &[u8], s: S) -> Result<S::Ok, S::Error> {
        if png.len() > MAX_PNG_BYTES {
            return Err(S::Error::custom("a view's image must be at most 64 MiB"));
        }
        if png.is_empty() {
            return s.serialize_str("");
        }
        s.serialize_str(&crate::render_resources::encode_base64(png).map_err(S::Error::custom)?)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        if text.is_empty() {
            return Ok(Vec::new());
        }
        crate::render_resources::decode_base64(&text, MAX_PNG_BYTES).map_err(|e| D::Error::custom(format!("a view's image: {e}")))
    }
}

/// An ANSI sheet size (ASME Y14.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnsiSize {
    A,
    B,
    C,
    D,
    E,
}

impl AnsiSize {
    /// Width and height in inches, landscape or portrait.
    pub fn inches(self, landscape: bool) -> (f64, f64) {
        let (w, h) = match self {
            AnsiSize::A => (11.0, 8.5),
            AnsiSize::B => (17.0, 11.0),
            AnsiSize::C => (22.0, 17.0),
            AnsiSize::D => (34.0, 22.0),
            AnsiSize::E => (44.0, 34.0),
        };
        if landscape { (w, h) } else { (h, w) }
    }

    /// The border's zones: (columns, rows).
    pub fn zones(self) -> (u32, u32) {
        match self {
            AnsiSize::A => (4, 2),
            AnsiSize::B => (4, 4),
            AnsiSize::C => (4, 4),
            AnsiSize::D => (8, 4),
            AnsiSize::E => (8, 4),
        }
    }

    pub fn letter(self) -> &'static str {
        match self {
            AnsiSize::A => "A",
            AnsiSize::B => "B",
            AnsiSize::C => "C",
            AnsiSize::D => "D",
            AnsiSize::E => "E",
        }
    }

    /// The size whose letter is `s` (any case).
    pub fn parse(s: &str) -> Option<AnsiSize> {
        [AnsiSize::A, AnsiSize::B, AnsiSize::C, AnsiSize::D, AnsiSize::E].into_iter().find(|z| z.letter().eq_ignore_ascii_case(s.trim()))
    }
}

/// The title block's field values.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DrawingProps {
    pub title: String,
    pub company: String,
    pub dwg_no: String,
    pub rev: String,
    pub drawn_by: String,
    pub drawn_date: String,
    pub checked_by: String,
    pub checked_date: String,
    pub approved_by: String,
    pub approved_date: String,
    pub material: String,
}

/// The template a drawing was made from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateId {
    /// ANSI sheets in inches.
    #[default]
    AnsiInch,
}

/// The scales offered for a view, in order: (name, paper / model).
pub const SCALE_PRESETS: [(&str, f64); 8] =
    [("1:4", 0.25), ("1:2", 0.5), ("1:1", 1.0), ("2:1", 2.0), ("3:1", 3.0), ("4:1", 4.0), ("5:1", 5.0), ("10:1", 10.0)];

/// A typed scale: `a:b`, `a/b` or a decimal, finite and greater than 0.
pub fn parse_scale(s: &str) -> Option<f64> {
    fn positive(s: &str) -> Option<f64> {
        let v: f64 = s.trim().parse().ok()?;
        (v.is_finite() && v > 0.0).then_some(v)
    }
    let s = s.trim();
    let v = match s.split_once(':').or_else(|| s.split_once('/')) {
        Some((a, b)) => positive(a)? / positive(b)?,
        None => positive(s)?,
    };
    (v.is_finite() && v > 0.0).then_some(v)
}

/// `a:b` in smallest integers when `scale` is a ratio of integers up to 64 (within 1e-9), else
/// the decimal with at most 6 places.
pub fn format_scale(scale: f64) -> String {
    if scale.is_finite() && scale > 0.0 {
        for q in 1..=64u32 {
            let p = (scale * f64::from(q)).round();
            if (1.0..=64.0).contains(&p) && (p / f64::from(q) - scale).abs() <= 1e-9 {
                return format!("{p}:{q}");
            }
        }
    }
    let text = format!("{scale:.6}");
    if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_string() } else { text }
}

#[cfg(test)]
#[path = "drawing_tests.rs"]
mod drawing_tests;
