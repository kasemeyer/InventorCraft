//! Constraint, assembly snap, hole placement and dimension marker artwork.
use crate::dialogs_assembly::SnapKind;
use crate::theme::Tokens;
use egui::{Color32, Pos2, Stroke, vec2};

pub(crate) fn snap(painter: &egui::Painter, c: Pos2, tip: Option<Pos2>, kind: SnapKind, col: Color32, tag: Option<&str>) {
    // A dark halo under every stroke keeps the glyph readable on light and dark faces alike.
    let ic = Tokens::get().icons;
    let halo = Stroke::new(4.5, ic.marker_halo);
    let white = Stroke::new(1.5, ic.highlight);
    if let Some(tip) = tip {
        let d = (tip - c).normalized();
        let n = vec2(-d.y, d.x);
        let head = vec![tip + d * 2.0, tip - d * 9.0 + n * 5.0, tip - d * 9.0 - n * 5.0];
        painter.line_segment([c, tip], halo);
        painter.add(egui::Shape::convex_polygon(head.clone(), Color32::TRANSPARENT, halo));
        painter.line_segment([c, tip], Stroke::new(2.0, col));
        painter.add(egui::Shape::convex_polygon(head, col, Stroke::NONE));
    }
    let shape = |fill: Color32, stroke: Stroke, grow: f32| -> egui::Shape {
        match kind {
            SnapKind::FaceCenter => egui::Shape::rect_filled(egui::Rect::from_center_size(c, vec2(12.0 + grow, 12.0 + grow)), 1.5, fill),
            SnapKind::CircleCenter => egui::Shape::circle_filled(c, 7.0 + grow / 2.0, fill),
            SnapKind::EdgeMid => {
                let r = 8.0 + grow / 2.0;
                egui::Shape::convex_polygon(vec![c + vec2(0.0, -r), c + vec2(r * 0.93, r * 0.7), c + vec2(-r * 0.93, r * 0.7)], fill, stroke)
            }
            SnapKind::Vertex => {
                let r = 7.5 + grow / 2.0;
                egui::Shape::convex_polygon(vec![c + vec2(0.0, -r), c + vec2(r, 0.0), c + vec2(0.0, r), c + vec2(-r, 0.0)], fill, stroke)
            }
        }
    };
    painter.add(shape(ic.marker_halo, Stroke::NONE, 4.0));
    painter.add(shape(col, Stroke::NONE, 0.0));
    match kind {
        SnapKind::FaceCenter => {
            painter.rect_stroke(egui::Rect::from_center_size(c, vec2(12.0, 12.0)), 1.5, white, egui::StrokeKind::Middle);
        }
        SnapKind::CircleCenter => {
            painter.circle_stroke(c, 7.0, white);
            painter.line_segment([c - vec2(4.5, 0.0), c + vec2(4.5, 0.0)], Stroke::new(1.2, ic.highlight));
            painter.line_segment([c - vec2(0.0, 4.5), c + vec2(0.0, 4.5)], Stroke::new(1.2, ic.highlight));
        }
        SnapKind::EdgeMid | SnapKind::Vertex => {
            painter.add(shape(Color32::TRANSPARENT, white, 0.0));
        }
    }
    // What it snaps to (hovered), or which component it is (picked): a small label.
    if let Some(tag) = tag {
        let t = Tokens::get();
        let font = egui::FontId::proportional(crate::theme::Tokens::get().font_caption);
        let g = painter.layout_no_wrap(tag.to_string(), font, t.text);
        let at = c + vec2(12.0, 10.0);
        let r = egui::Rect::from_min_size(at, g.size()).expand2(vec2(5.0, 2.0));
        painter.rect(r, 3.0, t.panel.gamma_multiply(0.92), Stroke::new(1.0, col), egui::StrokeKind::Inside);
        painter.galley(at, g, t.text);
    }
}
/// A place a hole can snap to, before the cursor reaches it: a small hollow mark of its kind
/// (a square for a face's centre, a ring for a round edge's centre).
pub(crate) fn snap_hint(painter: &egui::Painter, c: Pos2, kind: SnapKind, col: Color32) {
    let ic = Tokens::get().icons;
    for stroke in [Stroke::new(3.5, ic.marker_halo), Stroke::new(1.4, col)] {
        match kind {
            SnapKind::FaceCenter => {
                painter.rect_stroke(egui::Rect::from_center_size(c, vec2(8.0, 8.0)), 1.0, stroke, egui::StrokeKind::Middle);
            }
            SnapKind::CircleCenter => {
                painter.circle_stroke(c, 4.5, stroke);
            }
            SnapKind::EdgeMid | SnapKind::Vertex => {
                let r = 5.0;
                let diamond = vec![c + vec2(0.0, -r), c + vec2(r, 0.0), c + vec2(0.0, r), c + vec2(-r, 0.0)];
                painter.add(egui::Shape::convex_polygon(diamond, Color32::TRANSPARENT, stroke));
            }
        }
    }
    painter.circle_filled(c, 1.3, col);
}

/// A placed hole's drag handle: a ring round the hole's centre with a cross through it, filled
/// while it is hovered or dragged.
pub(crate) fn hole_handle(painter: &egui::Painter, c: Pos2, col: Color32, hot: bool) {
    let ic = Tokens::get().icons;
    let r = 6.0;
    painter.circle_filled(c, r + 3.5, ic.marker_halo);
    painter.circle(c, r, if hot { col } else { col.gamma_multiply(0.35) }, Stroke::new(1.6, col));
    let ink = Stroke::new(1.3, if hot { ic.highlight } else { col });
    painter.line_segment([c - vec2(r + 3.0, 0.0), c + vec2(r + 3.0, 0.0)], ink);
    painter.line_segment([c - vec2(0.0, r + 3.0), c + vec2(0.0, r + 3.0)], ink);
}
pub(crate) fn glyph_icon(name: &str) -> Option<&'static str> {
    Some(match name {
        "Coincident" => "c_coincident",
        "Parallel" => "c_parallel",
        "Perpendicular" => "c_perpendicular",
        "Collinear" => "c_collinear",
        "Tangent" => "c_tangent",
        "Smooth" => "c_smooth",
        "Equal" => "c_equal",
        "Concentric" => "c_concentric",
        "MidPoint" => "c_midpoint",
        "Symmetry" => "c_symmetry",
        "Fix" => "c_fix",
        _ => return None,
    })
}
pub(crate) fn glyph_letter(name: &str) -> &'static str {
    match name {
        "Horizontal" => "-",
        "Vertical" => "|",
        _ => "?",
    }
}
pub(crate) fn dimension_arrow(painter: &egui::Painter, t: Pos2, back: Pos2, col: Color32) {
    let d = (t - back).normalized();
    let n = vec2(-d.y, d.x);
    painter.add(egui::Shape::convex_polygon(vec![t, t - d * 9.0 + n * 3.0, t - d * 9.0 - n * 3.0], col, Stroke::NONE));
}

/// A component's constrained status, drawn before its browser label (spec §6.4, R139): a filled
/// disc when it cannot move (grounded or fully constrained), a half-filled one while it is partly
/// held, a ring when nothing holds it. Our own mark, not a vendor character.
pub(crate) fn status(painter: &egui::Painter, r: egui::Rect, name: &str, col: Color32) {
    let (c, radius) = (r.center(), r.width().min(r.height()) / 2.0 - 1.0);
    let ring = Stroke::new(1.2, col);
    match name {
        "status_full" => {
            painter.circle_filled(c, radius, col);
        }
        "status_half" => {
            // The left half filled: a convex half-disc under the ring.
            let half: Vec<Pos2> = (0..=16)
                .map(|k| {
                    let a = std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * (k as f32 / 16.0);
                    c + vec2(radius * a.cos(), -radius * a.sin())
                })
                .collect();
            painter.add(egui::Shape::convex_polygon(half, col, Stroke::NONE));
            painter.circle_stroke(c, radius, ring);
        }
        _ => {
            painter.circle_stroke(c, radius, ring);
        }
    }
}

/// An assembly constraint's on-model glyph (spec §6.8): a 16-point plate with the type's own
/// symbol, outlined and drawn in `col` (the relationship colour, or the error colour for a
/// problem). Our own marks, not a vendor's: two facing bars (Mate), an opened corner with its arc
/// (Angle), a circle on a line (Tangent), a ring on its axis (Insert), mirrored wedges about a
/// centre line (Symmetry).
pub(crate) fn constraint_glyph(painter: &egui::Painter, at: Pos2, kind: solidcraft_engine::doc::constraints::ConstraintKind, col: Color32) {
    use solidcraft_engine::doc::constraints::ConstraintKind as K;
    let t = Tokens::get();
    let ic = t.icons;
    let half = crate::constraint_overlay::GLYPH_SIZE / 2.0;
    let plate = egui::Rect::from_center_size(at, vec2(half * 2.0, half * 2.0));
    painter.rect_filled(plate.expand(1.5), 3.5, ic.marker_halo);
    painter.rect(plate, 3.0, t.panel, Stroke::new(1.2, col), egui::StrokeKind::Inside);
    let ink = Stroke::new(1.6, col);
    let p = |x: f32, y: f32| at + vec2(x, y);
    match kind {
        K::Mate => {
            painter.line_segment([p(-4.5, -2.0), p(4.5, -2.0)], ink);
            painter.line_segment([p(-4.5, 2.0), p(4.5, 2.0)], ink);
        }
        K::Angle => {
            painter.line_segment([p(-4.5, 4.0), p(4.5, 4.0)], ink);
            painter.line_segment([p(-4.5, 4.0), p(3.0, -4.5)], ink);
            let arc: Vec<Pos2> = (0..=8)
                .map(|k| {
                    let a = -(k as f32 / 8.0) * 0.85;
                    p(-4.5 + 5.5 * a.cos(), 4.0 + 5.5 * a.sin())
                })
                .collect();
            painter.add(egui::Shape::line(arc, Stroke::new(1.1, col)));
        }
        K::Tangent => {
            painter.line_segment([p(-5.0, 4.5), p(5.0, 4.5)], ink);
            painter.circle_stroke(p(0.0, 0.5), 3.6, ink);
        }
        K::Insert => {
            painter.circle_stroke(at, 3.8, ink);
            painter.line_segment([p(0.0, -5.5), p(0.0, 5.5)], Stroke::new(1.1, col));
        }
        K::Symmetry => {
            for y in [-5.0, -1.5, 2.0] {
                painter.line_segment([p(0.0, y), p(0.0, y + 2.0)], Stroke::new(1.1, col));
            }
            painter.add(egui::Shape::convex_polygon(vec![p(-1.8, -3.0), p(-1.8, 3.0), p(-5.0, 0.0)], col, Stroke::NONE));
            painter.add(egui::Shape::convex_polygon(vec![p(1.8, -3.0), p(5.0, 0.0), p(1.8, 3.0)], col, Stroke::NONE));
        }
    }
}

/// Where a picked constraint's pick sits on the model: a ring with a dot, over a halo.
pub(crate) fn constraint_anchor(painter: &egui::Painter, c: Pos2, col: Color32) {
    let ic = Tokens::get().icons;
    painter.circle_stroke(c, 5.5, Stroke::new(4.0, ic.marker_halo));
    painter.circle_stroke(c, 5.5, Stroke::new(1.8, col));
    painter.circle_filled(c, 2.0, col);
}
