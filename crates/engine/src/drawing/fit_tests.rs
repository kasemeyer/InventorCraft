#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The default scale of a new base view (wave 3, R100 amended by R108, owner default §8.8). A D
//! sheet is 34 × 22 in; inside its 0.5 in border it prints 33 × 21 in, half of that 16.5 × 10.5.
//! A B sheet is 17 × 11 in: 16 × 10 in, half 8 × 5. The three views need (x + y)·s + 1 across
//! and (z + y)·s + 1 up (front x × z, top x × y above it, right y × z beside it, 1 in gaps), within
//! the half area.

use solidcraft_doc::AnsiSize;

use super::*;

const IN: f64 = 25.4;

fn mm(x: f64, y: f64, z: f64) -> [f64; 3] {
    [x * IN, y * IN, z * IN]
}

#[test]
fn printable_areas_are_inside_the_border() {
    assert_eq!(printable_in(AnsiSize::D, true), [33.0, 21.0]);
    assert_eq!(printable_in(AnsiSize::B, true), [16.0, 10.0]);
    assert_eq!(printable_in(AnsiSize::B, false), [10.0, 16.0]);
}

#[test]
fn the_block_is_full_size_on_d() {
    // 4 × 1 × 3 in at 1:1: across (4 + 1)·1 + 1 = 6 ≤ 16.5, up (3 + 1)·1 + 1 = 5 ≤ 10.5.
    assert_eq!(fit_scale(mm(4.0, 1.0, 3.0), printable_in(AnsiSize::D, true)), 1.0);
}

#[test]
fn a_40_by_20_by_10_box_is_quarter_size_on_d_as_inventor_draws_it() {
    // x 40, y 20 (depth), z 10 (height). On D, half area 16.5 × 10.5:
    //   1:1  across (40 + 20)·1 + 1 = 61 > 16.5: no.
    //   1:2  across (40 + 20)·0.5 + 1 = 31 > 16.5: no (R100 chose this; Inventor shows 1:4).
    //   1:4  across 60·0.25 + 1 = 16 ≤ 16.5, up (10 + 20)·0.25 + 1 = 8.5 ≤ 10.5: fits → 1:4.
    assert_eq!(fit_scale(mm(40.0, 20.0, 10.0), printable_in(AnsiSize::D, true)), 0.25);
    // On B, half area 8 × 5: 1:4 across 16 > 8, and no preset is smaller (there is no 1:8):
    // the smallest preset, 1:4.
    assert_eq!(fit_scale(mm(40.0, 20.0, 10.0), printable_in(AnsiSize::B, true)), 0.25);
}

#[test]
fn half_the_sheet_decides_between_presets() {
    // 20 × 4 × 6 in on D: 1:1 across 24 + 1 = 25 > 16.5: no; 1:2 across 12 + 1 = 13 ≤ 16.5, up
    // (6 + 4)·0.5 + 1 = 6 ≤ 10.5 → 1:2 (the whole sheet, 33 × 21, would have taken 1:1).
    assert_eq!(fit_scale(mm(20.0, 4.0, 6.0), printable_in(AnsiSize::D, true)), 0.5);
    // The height decides too: 6 × 2 × 9 in on D: 1:1 up (9 + 2) + 1 = 12 > 10.5 → 1:2.
    assert_eq!(fit_scale(mm(6.0, 2.0, 9.0), printable_in(AnsiSize::D, true)), 0.5);
}

#[test]
fn a_100_inch_part_gets_the_smallest_preset() {
    // 100 × 100 × 100 in: at 1:4 across (100 + 100)·0.25 + 1 = 51 > 16.5, and no preset is
    // smaller: 1:4, the smallest (it never fits, so the smallest is the best there is).
    assert_eq!(fit_scale(mm(100.0, 100.0, 100.0), printable_in(AnsiSize::D, true)), 0.25);
    // 100 × 10 × 10 in: 1:4 across 110·0.25 + 1 = 28.5 > 16.5 → still the smallest, 1:4.
    assert_eq!(fit_scale(mm(100.0, 10.0, 10.0), printable_in(AnsiSize::D, true)), 0.25);
}

#[test]
fn never_enlarged_and_empty_models_are_full_size() {
    // A 0.1 in part would fit at 10:1, but the default only goes down from 1:1.
    assert_eq!(fit_scale(mm(0.1, 0.1, 0.1), printable_in(AnsiSize::D, true)), 1.0);
    assert_eq!(fit_scale([0.0; 3], printable_in(AnsiSize::A, true)), 1.0);
    assert_eq!(fit_scale([f64::NAN, f64::INFINITY, -1.0], printable_in(AnsiSize::D, true)), 1.0);
}
