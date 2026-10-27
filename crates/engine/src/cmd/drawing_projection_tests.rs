#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Drawing views as exact curves (Task 6): lines, arcs and circles from the mesh HLR, in the
//! Hidden Line and Hidden Line Removed styles, with and without tangent edges. Projection
//! goldens on the drawing tests' block (spec §7.3).

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use serde_json::{Value, json};
use solidcraft_doc::{CubeTarget, LineStyle};
use solidcraft_geom::{Vec2, Vec3};

use crate::Session;
use crate::drawing::project::{PaperCurve, ProjectOptions, ViewCurve, ViewGeometry, camera_for, project_view};

const EPS: f64 = 1e-6;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// The drawing tests' block (`drawing_fixture::block_part`'s commands), kept in the session.
fn block() -> Session {
    let mut s = Session::default();
    run(&mut s, "document.new_part", json!({}));
    run(&mut s, "document.units", json!({"units": "in"}));
    run(
        &mut s,
        "solid.box",
        json!({"length": "4 in", "width": "1 in", "height": "3 in", "corner": [-50.8, 0, 0], "name": "Block", "body_name": "Block"}),
    );
    run(&mut s, "solid.hole", json!({"position": [0, 0, 38.1], "direction": [0, 1, 0], "diameter": "0.75 in", "name": "Drill"}));
    run(&mut s, "solid.fillet", json!({"edges": [[-50.8, 12.7, 0], [50.8, 12.7, 0], [-50.8, 12.7, 76.2], [50.8, 12.7, 76.2]], "radius": "0.5 in"}));
    s
}

/// Two tangent cubic arcs extruded 10 mm: the kernel certifies their two straight joins
/// tangent (`Mesh::smooth_edges`; Task 5's tube).
fn tube() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.spline.control_point", json!({"points": [[10, 0], [10, 13.333], [-10, 13.333], [-10, 0]]}));
    run(&mut s, "sketch.spline.control_point", json!({"points": [[-10, 0], [-10, -6], [10, -6], [10, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10, "body_name": "Tube"}));
    s
}

fn view(s: &Session, target: &str, lines: LineStyle, tangent_edges: bool) -> ViewGeometry {
    let o = CubeTarget::from_name(target).unwrap().orientation();
    project_view(s, &o, &ProjectOptions { lines, tangent_edges, tolerance_mm: 0.01 }).unwrap()
}

/// (lines, circles, arcs, polys, hidden)
fn counts(g: &ViewGeometry) -> (usize, usize, usize, usize, usize) {
    let n = |f: fn(&PaperCurve) -> bool| g.curves.iter().filter(|c| f(&c.curve)).count();
    (
        n(|c| matches!(c, PaperCurve::Line { .. })),
        n(|c| matches!(c, PaperCurve::Circle { .. })),
        n(|c| matches!(c, PaperCurve::Arc { .. })),
        n(|c| matches!(c, PaperCurve::Poly { .. })),
        g.curves.iter().filter(|c| c.hidden).count(),
    )
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS
}
fn near2(a: Vec2, b: Vec2) -> bool {
    near(a.x, b.x) && near(a.y, b.y)
}

/// The hidden curves, each a line, in order of their midpoints' x, then y.
fn hidden_lines(g: &ViewGeometry) -> Vec<(Vec2, Vec2)> {
    let mut out: Vec<(Vec2, Vec2)> = g
        .curves
        .iter()
        .filter(|c| c.hidden)
        .map(|c| match c.curve {
            PaperCurve::Line { a, b } => (a, b),
            ref other => panic!("a hidden curve that is not a line: {other:?}"),
        })
        .collect();
    out.sort_by(|p, q| (p.0.x + p.1.x).total_cmp(&(q.0.x + q.1.x)).then((p.0.y + p.1.y).total_cmp(&(q.0.y + q.1.y))));
    out
}

/// No two visible curves draw the same thing.
fn assert_no_visible_duplicates(g: &ViewGeometry) {
    let visible: Vec<&ViewCurve> = g.curves.iter().filter(|c| !c.hidden).collect();
    for (i, p) in visible.iter().enumerate() {
        for q in visible.iter().skip(i + 1) {
            let same = match (&p.curve, &q.curve) {
                (PaperCurve::Line { a, b }, PaperCurve::Line { a: c, b: d }) => (near2(*a, *c) && near2(*b, *d)) || (near2(*a, *d) && near2(*b, *c)),
                (PaperCurve::Circle { c, r }, PaperCurve::Circle { c: d, r: s }) => near2(*c, *d) && near(*r, *s),
                (PaperCurve::Arc { c, r, start, end }, PaperCurve::Arc { c: d, r: s, start: t, end: u }) => {
                    near2(*c, *d) && near(*r, *s) && near(*start, *t) && near(*end, *u)
                }
                _ => false,
            };
            assert!(!same, "{p:?} and {q:?} coincide");
        }
    }
}

#[test]
fn front_view_of_the_block_is_4_lines_1_circle_4_arcs() {
    let g = view(&block(), "front", LineStyle::Hidden, false);
    assert_eq!(counts(&g), (4, 1, 4, 0, 0), "{:#?}", g.curves);
    // Model x to the right and z up, centred on the 101.6 x 76.2 outline.
    assert!(near2(g.min, Vec2::new(-50.8, -38.1)) && near2(g.max, Vec2::new(50.8, 38.1)), "{:?} {:?}", g.min, g.max);
    let circle = g.curves.iter().find(|c| matches!(c.curve, PaperCurve::Circle { .. })).unwrap();
    let PaperCurve::Circle { c, r } = circle.curve else { panic!("{circle:?}") };
    assert!(near2(c, Vec2::ZERO) && near(r, 9.525), "{circle:?}");
    // The hole's rim is two B-rep edges (the kernel halves the bore): one circle naming both,
    // the one that sorts first as its edge (R51).
    assert_eq!(circle.edge, "0/Block/e/F1:-y|F2:bore:side:0");
    assert_eq!(circle.sources, ["0/Block/e/F1:-y|F2:bore:side:0", "0/Block/e/F1:-y|F2:bore:side:1"]);
    assert_eq!(circle.segment, 0);
    // The four rounds, each a quarter turn counter-clockwise about its centre.
    let mut arcs: Vec<(Vec2, f64, f64, f64)> =
        g.curves.iter().filter_map(|c| if let PaperCurve::Arc { c, r, start, end } = c.curve { Some((c, r, start, end)) } else { None }).collect();
    arcs.sort_by(|a, b| a.2.total_cmp(&b.2));
    let expected = [
        (Vec2::new(38.1, 25.4), 0.0, FRAC_PI_2),
        (Vec2::new(-38.1, 25.4), FRAC_PI_2, PI),
        (Vec2::new(-38.1, -25.4), PI, 3.0 * FRAC_PI_2),
        (Vec2::new(38.1, -25.4), 3.0 * FRAC_PI_2, TAU),
    ];
    assert_eq!(arcs.len(), 4);
    for ((c, r, start, end), (ec, es, ee)) in arcs.iter().zip(expected) {
        assert!(near2(*c, ec) && near(*r, 12.7) && near(*start, es) && near(*end, ee), "{c:?} {r} {start} {end}");
    }
    // The four straight sides, each between two rounds.
    let mut lines: Vec<(Vec2, Vec2)> = g
        .curves
        .iter()
        .filter_map(|c| if let PaperCurve::Line { a, b } = c.curve { Some(if (a.x, a.y) <= (b.x, b.y) { (a, b) } else { (b, a) }) } else { None })
        .collect();
    lines.sort_by(|p, q| (p.0.x, p.0.y).partial_cmp(&(q.0.x, q.0.y)).unwrap());
    let sides = [
        (Vec2::new(-50.8, -25.4), Vec2::new(-50.8, 25.4)),
        (Vec2::new(-38.1, -38.1), Vec2::new(38.1, -38.1)),
        (Vec2::new(-38.1, 38.1), Vec2::new(38.1, 38.1)),
        (Vec2::new(50.8, -25.4), Vec2::new(50.8, 25.4)),
    ];
    for (l, e) in lines.iter().zip(sides) {
        assert!(near2(l.0, e.0) && near2(l.1, e.1), "{l:?} {e:?}");
    }
    // All of them are edges of the front face.
    assert!(g.curves.iter().all(|c| c.edge.starts_with("0/Block/e/") && c.edge.contains("F1:-y")), "{:#?}", g.curves);
    assert_no_visible_duplicates(&g);
}

#[test]
fn top_view_has_two_hidden_lines_for_the_hole() {
    let g = view(&block(), "top", LineStyle::Hidden, false);
    let (_, circles, _, _, hidden) = counts(&g);
    assert_eq!((circles, hidden), (0, 2), "{:#?}", g.curves);
    // The bore's silhouettes, snapped to its exact radius (R52), running front to back.
    let h = hidden_lines(&g);
    for ((a, b), x) in h.iter().zip([-9.525, 9.525]) {
        assert!(near(a.x, x) && near(b.x, x) && near((a.y - b.y).abs(), 25.4), "{h:?}");
    }
    let mut named: Vec<&str> = g.curves.iter().filter(|c| c.hidden).map(|c| c.edge.as_str()).collect();
    named.sort_unstable();
    assert_eq!(named, ["0/Block/s/F2:bore:side:0/0", "0/Block/s/F2:bore:side:0/1"]);
    let size = g.max - g.min;
    assert!(near(size.x, 101.6) && near(size.y, 25.4), "{size:?}");
    assert!(near2(g.min + g.max, Vec2::ZERO));
    assert_no_visible_duplicates(&g);
}

#[test]
fn right_view_has_two_hidden_lines_for_the_hole() {
    let g = view(&block(), "right", LineStyle::Hidden, false);
    let (_, circles, _, _, hidden) = counts(&g);
    assert_eq!((circles, hidden), (0, 2), "{:#?}", g.curves);
    // Model y to the right and z up: the bore runs across, 9.525 above and below its axis.
    let mut h = hidden_lines(&g);
    h.sort_by(|p, q| p.0.y.total_cmp(&q.0.y));
    for ((a, b), y) in h.iter().zip([-9.525, 9.525]) {
        assert!(near(a.y, y) && near(b.y, y) && near((a.x - b.x).abs(), 25.4), "{h:?}");
    }
    let size = g.max - g.min;
    assert!(near(size.x, 25.4) && near(size.y, 76.2), "{size:?}");
    assert_no_visible_duplicates(&g);
}

#[test]
fn hidden_line_removed_drops_the_hidden_curves() {
    let s = block();
    let hidden = view(&s, "top", LineStyle::Hidden, false);
    let removed = view(&s, "top", LineStyle::Removed, false);
    assert_eq!(counts(&hidden).4, 2);
    assert_eq!(counts(&removed).4, 0);
    // The visible curves are the same in both styles, and so is the outline's box.
    let visible: Vec<&ViewCurve> = hidden.curves.iter().filter(|c| !c.hidden).collect();
    assert_eq!(removed.curves.iter().collect::<Vec<_>>(), visible);
    assert_eq!((removed.min, removed.max), (hidden.min, hidden.max));
}

#[test]
fn tangent_edges_appear_only_when_asked() {
    // R50: the kernel flags none of the block's fillet joins tangent, so the toggle adds nothing.
    let s = block();
    let off = view(&s, "top", LineStyle::Hidden, false);
    let on = view(&s, "top", LineStyle::Hidden, true);
    assert_eq!(on, off);
    assert_no_visible_duplicates(&on);
    // The tube's two certified tangent joins, seen from the front at x = -10 and 10, 10 mm
    // tall: drawn only with tangent edges on.
    let s = tube();
    let off = view(&s, "front", LineStyle::Hidden, false);
    let on = view(&s, "front", LineStyle::Hidden, true);
    let at = |g: &ViewGeometry, x: f64| -> Vec<ViewCurve> {
        g.curves
            .iter()
            .filter(|c| matches!(c.curve, PaperCurve::Line { a, b } if near(a.x, x) && near(b.x, x) && near((a.y - b.y).abs(), 10.)))
            .cloned()
            .collect()
    };
    for x in [-10., 10.] {
        let joins = at(&on, x);
        assert_eq!(joins.len(), 1, "{x}: {:#?}", on.curves);
        assert!(!joins[0].hidden && joins[0].edge.starts_with("0/Tube/e/F2:side:s1|F2:side:s2~"), "{joins:?}");
        assert!(at(&off, x).is_empty(), "{x}: {:#?}", off.curves);
    }
    assert_eq!(on.curves.len(), off.curves.len() + 2, "{:#?}\n{:#?}", on.curves, off.curves);
    assert_no_visible_duplicates(&on);
    assert_no_visible_duplicates(&off);
}

#[test]
fn iso_views_draw_the_hole_as_curves_not_circles() {
    let g = view(&block(), "iso_top_right", LineStyle::Hidden, false);
    let (_, circles, arcs, _, _) = counts(&g);
    assert_eq!((circles, arcs), (0, 0), "{:#?}", g.curves);
    // The hole's rims (front visible, back hidden), each of two halves: polylines only.
    let rim = |face: &str, half: u32| format!("0/Block/e/{face}|F2:bore:side:{half}");
    for (face, hidden) in [("F1:-y", false), ("F1:+y", true)] {
        for half in [0, 1] {
            let curves: Vec<&ViewCurve> = g.curves.iter().filter(|c| c.edge == rim(face, half)).collect();
            assert!(!curves.is_empty(), "{}", rim(face, half));
            assert!(curves.iter().all(|c| matches!(c.curve, PaperCurve::Poly { .. }) && c.hidden == hidden), "{curves:#?}");
        }
    }
    // The bore's silhouettes are straight lines (its generators), not rim pieces.
    let bore: Vec<&ViewCurve> = g.curves.iter().filter(|c| c.edge.starts_with("0/Block/s/F2:bore:side:0/")).collect();
    assert!(!bore.is_empty() && bore.iter().all(|c| matches!(c.curve, PaperCurve::Line { .. })), "{bore:#?}");
    assert_no_visible_duplicates(&g);
}

#[test]
fn camera_for_matches_the_orientation() {
    for t in CubeTarget::ALL {
        let o = t.orientation();
        let cam = camera_for(&o, Vec3::new(1., 2., 3.), 150.);
        let (r, u, b) = cam.basis();
        let want = (o.dir.cross(o.up), o.up, -o.dir);
        for (got, want) in [(r, want.0), (u, want.1), (b, want.2)] {
            assert!(got.dist(want) <= 1e-9, "{}: {got:?} vs {want:?}", t.name());
        }
        assert_eq!(cam.fov, 0.);
        assert_eq!(cam.target, Vec3::new(1., 2., 3.));
        assert!(cam.is_valid());
    }
}
