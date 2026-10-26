//! The status bar along the bottom of the window, under a profile whose workflow asks for one
//! (Inventor's): the latest message or "Ready" on the left; the active sketch's constraint status
//! and the number of open documents on the right.

use crate::SolveApp;
use crate::sketch_palette::{dof_text, has_geometry, is_blank};
use crate::theme::Tokens;
use crate::ui_kit::{Container, Edge, Surface};
use egui::{Align2, FontId, Rect, Stroke, pos2};

/// The bar's height: 44 capture px at the capture's 1.75 scaling (`statusbar.png`).
const HEIGHT: f32 = 44.0 / 1.75;

/// What the bar says.
pub(crate) struct StatusTexts {
    /// The status message while it is fresh, else "Ready".
    pub left: String,
    /// The active sketch's constraint status; none outside a sketch or in a blank one.
    pub sketch: Option<String>,
    /// The number of open documents.
    pub documents: usize,
}

pub(crate) fn texts(app: &SolveApp) -> StatusTexts {
    let fresh = app.status.as_ref().filter(|(_, at, _)| crate::now_ms() - at < crate::viewport::STATUS_TOAST_MS);
    let left = fresh.map_or_else(|| crate::i18n::tr("ui.status_ready").to_string(), |(text, _, _)| crate::i18n::tr_dyn(text));
    let sketch = app.session.active_sketch.and_then(|id| {
        let st = app.session.world_state();
        let ss = st.sketch(id)?;
        if is_blank(&ss.sketch) {
            return None;
        }
        let (dof, ok) = (ss.report.dof, ss.report.ok());
        Some(if ok && dof == 0 {
            crate::i18n::tr("ui.fully_constrained").to_string()
        } else if ok {
            crate::i18n::format("status.dimensions_needed", &[("count", &dof.to_string())])
        } else {
            crate::i18n::tr_dyn(&dof_text(dof, ok, has_geometry(&ss.sketch)))
        })
    });
    StatusTexts { left, sketch, documents: app.docs.count() }
}

pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let s = texts(app);
    let response = Container::new(ui.ctx(), Surface::Panel)
        .edge("sc_status_bar", Edge::Bottom)
        .exact_size(HEIGHT)
        .frame(Container::new(ui.ctx(), Surface::Panel).edge_frame(t.panel))
        .show(ui, |ui| {
            let r = ui.max_rect();
            let p = ui.painter();
            let font = FontId::proportional(t.font_label);
            let y = r.center().y;
            p.text(pos2(r.left() + t.space_8, y), Align2::LEFT_CENTER, &s.left, font.clone(), t.text);
            // Right-aligned cells, each set off by a rule on its left: the sketch status, then
            // the document count.
            let mut x = r.right() - t.space_8;
            let rule = Stroke::new(1.0, t.dialog_rule);
            for cell in [Some(s.documents.to_string()), s.sketch.clone()].into_iter().flatten() {
                let at = p.text(pos2(x, y), Align2::RIGHT_CENTER, &cell, font.clone(), t.text);
                x = at.left() - t.space_8;
                p.vline(x, r.y_range().shrink(t.space_2), rule);
                x -= t.space_8;
            }
        });
    let rect: Rect = response.response.rect;
    crate::scenario::publish_rect("status_bar", rect);
}

#[cfg(test)]
#[path = "status_bar_tests.rs"]
mod tests;
