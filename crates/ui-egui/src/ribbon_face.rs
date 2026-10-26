//! A ribbon panel's face: its large buttons and, as Inventor draws them, its small ones (a 16 px
//! icon and a one-line label) stacked three high in columns between them.

use egui::FontId;
use solidcraft_engine::CommandSpec;

/// Small buttons in a column.
pub(crate) const SMALL_ROWS: usize = 3;

/// One slot of a panel's face, in order.
#[derive(Clone)]
pub(crate) enum FaceItem<'a> {
    Large(&'a CommandSpec),
    /// Up to [`SMALL_ROWS`] small buttons, top to bottom.
    Column(Vec<&'a CommandSpec>),
}

/// A panel's shown buttons in face order; each run of small buttons fills columns of up to
/// `SMALL_ROWS`, top to bottom, then left to right.
pub(crate) fn face_items<'a>(shown: &[&'a CommandSpec], small: impl Fn(&str) -> bool) -> Vec<FaceItem<'a>> {
    let mut items: Vec<FaceItem<'a>> = Vec::new();
    for &c in shown {
        if !small(c.id) {
            items.push(FaceItem::Large(c));
            continue;
        }
        match items.last_mut() {
            Some(FaceItem::Column(column)) if column.len() < SMALL_ROWS => column.push(c),
            _ => items.push(FaceItem::Column(vec![c])),
        }
    }
    items
}

/// A small column's width: 6 + 16 (icon) + 4 + its widest label + 8, at least 54.
pub(crate) fn column_width(painter: &egui::Painter, labels: &[&str]) -> f32 {
    let t = crate::theme::Tokens::get();
    let widest = labels
        .iter()
        .map(|label| painter.layout_no_wrap((*label).to_owned(), FontId::proportional(t.font_button), t.text).size().x)
        .fold(0.0_f32, f32::max);
    (6.0 + 16.0 + 4.0 + widest + 8.0).max(54.0)
}

#[cfg(test)]
#[path = "ribbon_face_tests.rs"]
mod tests;
