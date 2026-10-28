#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! CPU-side checks of the sheet pass: the draw order, the stroke and fill geometry, the uniform,
//! and the shader as naga reads it. Nothing here opens a device (the GPU crate's device tests
//! crash under some software rasterizers).

use std::sync::Arc;

use super::*;

const INK: [u8; 4] = [10, 20, 30, 255];

fn scene_of(strokes: Vec<SheetStroke>, fills: Vec<SheetFill>, images: Vec<SheetImage>) -> SheetScene {
    SheetScene { paper: [0.0, 0.0, 34.0, 22.0], paper_color: [235, 234, 219, 255], strokes, fills, images }
}

#[test]
fn the_pass_orders_images_fills_strokes() {
    let stroke = SheetStroke { points: vec![[1.0, 1.0], [2.0, 1.0]], color: INK, width_px: 1.0 };
    let fill = SheetFill { triangles: vec![[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]], color: INK };
    let image = SheetImage { rect: [8.0, 6.5, 12.0, 9.5], rgba: Arc::new(vec![255; 4]), size: [1, 1] };
    let s = scene_of(vec![stroke], vec![fill], vec![image]);
    assert_eq!(SheetPass::draw_list(&s), [SheetDraw::Image, SheetDraw::Fill, SheetDraw::Stroke]);
    // Several of each keep the kinds apart, each kind in scene order.
    let two = scene_of([s.strokes.clone(), s.strokes.clone()].concat(), s.fills.clone(), [s.images.clone(), s.images.clone()].concat());
    assert_eq!(SheetPass::draw_list(&two), [SheetDraw::Image, SheetDraw::Image, SheetDraw::Fill, SheetDraw::Stroke, SheetDraw::Stroke]);
    // An empty scene draws nothing but its paper.
    assert!(SheetPass::draw_list(&scene_of(vec![], vec![], vec![])).is_empty());
}

#[test]
fn the_paper_fills_come_before_the_strokes_in_one_buffer() {
    let s = scene_of(
        vec![SheetStroke { points: vec![[1.0, 1.0], [3.0, 1.0], [3.0, 2.0]], color: INK, width_px: 2.0 }],
        vec![SheetFill { triangles: vec![[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]], color: [1, 2, 3, 255] }],
        vec![],
    );
    let v = vertices(&s);
    // The paper (2 triangles), the fill's triangle, then 2 segments of 2 triangles each.
    assert_eq!(v.len(), 6 + 3 + 12);
    assert!(v[..6].iter().all(|x| x.color == [235, 234, 219, 255] && x.extrude == [0.0, 0.0]));
    assert_eq!(v[0].pos, [0.0, 0.0]);
    assert!(v[..6].iter().any(|x| x.pos == [34.0, 22.0]));
    assert!(v[6..9].iter().all(|x| x.color == [1, 2, 3, 255]));
    // Fills are solid: their coverage never fades.
    assert!(v[..9].iter().all(|x| x.half >= SOLID_HALF));
    let first = &v[9..15];
    // A horizontal segment: its corners sit on its ends, pushed up and down by half the width
    // plus the one-pixel fringe, and out past the ends by the same (square caps).
    assert!(first.iter().all(|x| x.color == INK && x.half == 1.0));
    let mut ys: Vec<f32> = first.iter().map(|x| x.extrude[1]).collect();
    ys.sort_by(f32::total_cmp);
    assert_eq!((ys[0], ys[5]), (-2.0, 2.0));
    assert!(first.iter().all(|x| x.dist.abs() == 2.0 && x.dist == x.extrude[1]));
    assert!(first.iter().all(|x| (x.pos == [1.0, 1.0] && x.extrude[0] == -2.0) || (x.pos == [3.0, 1.0] && x.extrude[0] == 2.0)));
    // The vertical one is pushed sideways.
    assert!(v[15..].iter().all(|x| x.extrude[0].abs() == 2.0 && x.dist.abs() == 2.0));
}

#[test]
fn degenerate_strokes_and_bad_points_draw_nothing() {
    let s = scene_of(
        vec![
            SheetStroke { points: vec![[1.0, 1.0]], color: INK, width_px: 1.0 },
            SheetStroke { points: vec![[1.0, 1.0], [1.0, 1.0]], color: INK, width_px: 1.0 },
            SheetStroke { points: vec![[f32::NAN, 1.0], [2.0, 1.0]], color: INK, width_px: 1.0 },
        ],
        vec![],
        vec![],
    );
    assert_eq!(vertices(&s).len(), 6, "only the paper");
    // No paper (a drawing with no sheet): nothing at all.
    let mut bare = scene_of(vec![], vec![], vec![]);
    bare.paper = [0.0; 4];
    assert!(vertices(&bare).is_empty());
}

#[test]
fn the_view_maps_paper_inches_to_clip_space() {
    // 100 px per inch, an 800 x 600 px canvas centred on paper (17, 11).
    let view = SheetView { center: [17.0, 11.0], px_per_in: 100.0, size_px: [800.0, 600.0] };
    let u = uniform(&view, false);
    assert_eq!(u, [17.0, 11.0, 400.0, 300.0, 100.0, 0.0, 0.0, 0.0]);
    // The shader's map, on the CPU: paper (21, 11) is 400 px right of the centre, the right edge.
    let clip = |p: [f32; 2]| [(p[0] - u[0]) * u[4] / u[2], (p[1] - u[1]) * u[4] / u[3]];
    assert_eq!(clip([21.0, 11.0]), [1.0, 0.0]);
    assert_eq!(clip([17.0, 14.0]), [0.0, 1.0]);
    assert_eq!(uniform(&view, true)[5], 1.0);
}

#[test]
fn image_quads_put_the_first_row_at_the_top() {
    let q = image_quad([8.0, 6.5, 12.0, 9.5]);
    assert_eq!(q.len(), 6);
    // uv (0, 0), the picture's first pixel, is the rectangle's top-left corner.
    assert!(q.iter().any(|v| v.pos == [8.0, 9.5] && v.uv == [0.0, 0.0]));
    assert!(q.iter().any(|v| v.pos == [12.0, 6.5] && v.uv == [1.0, 1.0]));
}

#[test]
fn the_sheet_shader_is_valid_wgsl() {
    let module = naga::front::wgsl::parse_str(SHADER).unwrap();
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap();
}
