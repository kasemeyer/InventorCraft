//! Centre marks and centrelines (spec §2.8): plain `Line`s on the CENTER layer. A centre mark is
//! two lines crossing at a circle's (or an arc's) centre, each across the diameter plus an
//! overshoot at both ends; a centreline bisector is one line midway between two straight edges.
//!
//! Logic ported from CADCraft `crates/engine/src/cmd/draw2/centermark.rs` at
//! d864a08b013353143086bd8bb568c26cb6256796 (MIT OR Apache-2.0; ATTRIBUTION.md): `center_mark`
//! and `center_line`. Not ported: `add_center_line` (CADCraft loads the CENTER linetype and sets
//! it on the entity; our lines are on the template's CENTER layer, whose linetype is CENTER),
//! `center_ext` (CENTEREXE; ours is [`MARK_OVERSHOOT_IN`]), the `run_*` commands and the
//! `CenterM` interactive tool (our commands are `cmd::drawing_note`'s, and the mapper
//! regenerates the lines with their edges). Adaptations are marked `TCI-CAD:`.

use cadcraft_geom::Vec2;

/// How far a centre mark runs past its circle, and a centreline past the edges it bisects,
/// inches. [A]
pub const MARK_OVERSHOOT_IN: f64 = 0.125;

/// The two lines of a centre mark on the circle (`c`, `r`): across, then up.
// TCI-CAD: returns the lines' ends instead of adding them to a session; the overshoot is
// `MARK_OVERSHOOT_IN`, not CENTEREXE.
pub(crate) fn center_mark(c: Vec2, r: f64) -> [(Vec2, Vec2); 2] {
    let ext = r + MARK_OVERSHOOT_IN;
    [(c - Vec2::X * ext, c + Vec2::X * ext), (c - Vec2::Y * ext, c + Vec2::Y * ext)]
}

/// The centreline midway between the straight edges `(a1, b1)` and `(a2, b2)`, None when they
/// define none.
// TCI-CAD: returns the line's ends instead of adding it; it spans the edges' common length
// along the centreline (their overlap) plus `MARK_OVERSHOOT_IN` at each end, where CADCraft
// runs from the middle of their starts to the middle of their ends plus CENTEREXE (the same
// line for two equal edges side by side); edges that do not overlap give None.
pub(crate) fn center_line(first: (Vec2, Vec2), second: (Vec2, Vec2)) -> Option<(Vec2, Vec2)> {
    let (a1, b1) = first;
    let (mut a2, mut b2) = second;
    if (b1 - a1).dot(b2 - a2) < 0.0 {
        std::mem::swap(&mut a2, &mut b2);
    }
    let (st, en) = (a1.mid(a2), b1.mid(b2));
    let dir = (en - st).normalized();
    // TCI-CAD: `|| !dir.is_finite()` added: a non-finite direction (from non-finite ends) gives none.
    if dir == Vec2::ZERO || !dir.is_finite() {
        return None;
    }
    // TCI-CAD: the common length of the two edges, measured along the centreline from `st`.
    let along = |p: Vec2| (p - st).dot(dir);
    let span = |a: Vec2, b: Vec2| (along(a).min(along(b)), along(a).max(along(b)));
    let ((lo1, hi1), (lo2, hi2)) = (span(a1, b1), span(a2, b2));
    let (lo, hi) = (lo1.max(lo2), hi1.min(hi2));
    if hi - lo <= 1e-9 {
        return None;
    }
    let ext = MARK_OVERSHOOT_IN;
    Some((st + dir * (lo - ext), st + dir * (hi + ext)))
}
