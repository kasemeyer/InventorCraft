//! The default scale of a new base view (wave 3, R100; owner default, plan §8.8): the largest
//! preset scale up to 1:1 at which the model's front, top and right views fit the sheet inside
//! its border. Only ever down from 1:1: a small part is drawn full size, never enlarged. Every
//! profile (SolidCraft's drawings too) gets the same default from this pure function.

use solidcraft_doc::{AnsiSize, ModelKind, ModelRef, SCALE_PRESETS};

use super::template::BORDER_INSET_IN;

/// Millimetres per inch.
const MM_PER_IN: f64 = 25.4;
/// The paper gap between the front view and the top view above it, and between the front view
/// and the right view beside it, in inches.
pub const FIT_GAP_IN: f64 = 1.0;

/// The largest `SCALE_PRESETS` value at most 1.0 at which a model of `model_extents_mm` (its
/// x, y, z extents: width, depth, height; the front view shows x by z, the top x by y above it,
/// the right y by z beside it) fits `sheet_printable_in` ([width, height] in inches) as front,
/// top and right views with `FIT_GAP_IN` between them: `(x + y)·s + gap ≤ width` and
/// `(z + y)·s + gap ≤ height`. The smallest preset when none fits; 1.0 for an empty or unusable
/// model. (owner default; §8.8)
pub fn fit_scale(model_extents_mm: [f64; 3], sheet_printable_in: [f64; 2]) -> f64 {
    let [x, y, z] = model_extents_mm.map(|v| if v.is_finite() { v.max(0.0) / MM_PER_IN } else { 0.0 });
    let [w, h] = sheet_printable_in;
    let fits = |s: f64| (x + y) * s + FIT_GAP_IN <= w + 1e-9 && (z + y) * s + FIT_GAP_IN <= h + 1e-9;
    let mut presets: Vec<f64> = SCALE_PRESETS.iter().map(|(_, v)| *v).filter(|v| *v <= 1.0).collect();
    presets.sort_by(|a, b| b.total_cmp(a));
    presets.iter().copied().find(|s| fits(*s)).or_else(|| presets.last().copied()).unwrap_or(1.0)
}

/// The printable area of an ANSI sheet: the sheet inside its border frame, [width, height] in
/// inches.
pub fn printable_in(size: AnsiSize, landscape: bool) -> [f64; 2] {
    let (w, h) = size.inches(landscape);
    [w - 2.0 * BORDER_INSET_IN, h - 2.0 * BORDER_INSET_IN]
}

/// The extents (x, y, z, millimetres) of the part or assembly at `model` (a path, as a view's
/// `model`), loaded as a view loads it; None when it cannot be loaded or shows nothing.
pub fn model_extents_mm(s: &crate::Session, model: &str) -> Option<[f64; 3]> {
    let r = ModelRef { path: model.to_string(), project_path: None, kind: ModelKind::Part };
    let loaded = super::model_ref::load(s, &r).ok()?;
    let b = crate::view::bounds(&loaded.session);
    let d = b.max - b.min;
    [d.x, d.y, d.z].iter().all(|v| v.is_finite() && *v >= 0.0).then_some([d.x, d.y, d.z])
}

/// The default scale of a new base view of `model` on the active sheet ([`fit_scale`]); None
/// with no drawing or no model to measure.
pub fn default_scale(s: &crate::Session, model: &str) -> Option<f64> {
    let m = s.doc.drawing.as_deref()?;
    let sheet = m.sheets.get(s.drawing_sheet.min(m.sheets.len().saturating_sub(1)))?;
    Some(fit_scale(model_extents_mm(s, model)?, printable_in(sheet.size, sheet.landscape)))
}

#[cfg(test)]
#[path = "fit_tests.rs"]
mod tests;
