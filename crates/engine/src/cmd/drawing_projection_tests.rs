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

/// The visible lines, each named and with its ends in order (by x, then y), sorted.
fn visible_lines(g: &ViewGeometry) -> Vec<(String, [f64; 4])> {
    let mut out: Vec<(String, [f64; 4])> = g
        .curves
        .iter()
        .filter(|c| !c.hidden)
        .map(|c| match c.curve {
            PaperCurve::Line { a, b } => {
                let (p, q) = if (a.x, a.y) <= (b.x, b.y) { (a, b) } else { (b, a) };
                (c.edge.clone(), [p.x, p.y, q.x, q.y])
            }
            ref other => panic!("a visible curve that is not a line: {other:?}"),
        })
        .collect();
    out.sort_by(|(e, p), (f, q)| e.cmp(f).then(p[0].total_cmp(&q[0])).then(p[1].total_cmp(&q[1])));
    out
}

fn assert_lines(g: &ViewGeometry, expected: &[(&str, [f64; 4])]) {
    let got = visible_lines(g);
    let same = got.len() == expected.len()
        && got.iter().zip(expected).all(|((e, p), (f, q))| e == &format!("0/Block/e/{f}") && p.iter().zip(q).all(|(a, b)| (a - b).abs() <= 1e-4));
    assert!(same, "{got:#?}");
}

/// Spec §7.3: the top view's 10 visible lines with tangent edges off (Inventor's default; R64
/// hides the top face's joins with the rounds): the sides (2), the front (3: the rounds' edges
/// and the top face's) and the back (5). The back's left end is split where the front and back
/// rounds' edges, which project onto each other, alternate in naming the line by chord, leaving
/// a 0.1954 mm (0.0077 in) piece named after the bottom round; the line is drawn whole.
#[test]
fn top_view_visible_lines_are_pinned() {
    let g = view(&block(), "top", LineStyle::Hidden, false);
    assert_eq!(counts(&g), (12, 0, 0, 0, 2));
    assert_lines(
        &g,
        &[
            ("F1:+x|F3:blend:F1:+x|F1:-z", [50.8, -12.7, 50.8, 12.7]),
            ("F1:+y|F1:+z", [-38.1, 12.7, 38.1, 12.7]),
            ("F1:+y|F3:blend:F1:+x|F1:+z", [38.1, 12.7, 50.8, 12.7]),
            ("F1:+y|F3:blend:F1:+z|F1:-x", [-50.8, 12.7, -50.4126, 12.7]),
            ("F1:+y|F3:blend:F1:+z|F1:-x", [-50.2172, 12.7, -38.1, 12.7]),
            ("F1:+y|F3:blend:F1:-x|F1:-z", [-50.4126, 12.7, -50.2172, 12.7]),
            ("F1:+z|F1:-y", [-38.1, -12.7, 38.1, -12.7]),
            ("F1:-x|F3:blend:F1:+z|F1:-x", [-50.8, -12.7, -50.8, 12.7]),
            ("F1:-y|F3:blend:F1:+x|F1:+z", [38.1, -12.7, 50.8, -12.7]),
            ("F1:-y|F3:blend:F1:+z|F1:-x", [-50.8, -12.7, -38.1, -12.7]),
        ],
    );
    // With tangent edges on, the joins at x = -38.1 and 38.1 are added (12 visible).
    let on = view(&block(), "top", LineStyle::Hidden, true);
    assert_eq!(visible_lines(&on).len(), 12);
}

/// Spec §7.3: the right view's 10 visible lines with tangent edges off: the outline's sides
/// between the rounds (2), its top and bottom (1 each), the rounds' edges on the -y face (1 above,
/// 1 below) and on the +y face (1 above, 3 below: split as the top view's back is, leaving a
/// 0.2554 mm piece named after the left bottom round).
#[test]
fn right_view_visible_lines_are_pinned() {
    let g = view(&block(), "right", LineStyle::Hidden, false);
    assert_eq!(counts(&g), (12, 0, 0, 0, 2));
    assert_lines(
        &g,
        &[
            ("F1:+x|F1:+y", [12.7, -25.4, 12.7, 25.4]),
            ("F1:+x|F1:-y", [-12.7, -25.4, -12.7, 25.4]),
            ("F1:+y|F3:blend:F1:+x|F1:+z", [12.7, 25.4, 12.7, 38.1]),
            ("F1:+y|F3:blend:F1:+x|F1:-z", [12.7, -38.1, 12.7, -34.9752]),
            ("F1:+y|F3:blend:F1:+x|F1:-z", [12.7, -34.7197, 12.7, -25.4]),
            ("F1:+y|F3:blend:F1:-x|F1:-z", [12.7, -34.9752, 12.7, -34.7197]),
            ("F1:+z|F3:blend:F1:+x|F1:+z", [-12.7, 38.1, 12.7, 38.1]),
            ("F1:-y|F3:blend:F1:+x|F1:+z", [-12.7, 25.4, -12.7, 38.1]),
            ("F1:-y|F3:blend:F1:+x|F1:-z", [-12.7, -38.1, -12.7, -25.4]),
            ("F1:-z|F3:blend:F1:+x|F1:-z", [-12.7, -38.1, 12.7, -38.1]),
        ],
    );
    let on = view(&block(), "right", LineStyle::Hidden, true);
    assert_eq!(visible_lines(&on).len(), 12);
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
    // R64: the block's fillet joins are tangent by the dihedral rule (the kernel flags none,
    // R50). From the top, the top face's joins with the rounds, x = -38.1 and 38.1 front to back,
    // are drawn only with tangent edges on; from the right, the right face's, y = -25.4 and 25.4
    // across. The joins with the faces square to the view lie on the outline, drawn in both.
    let s = block();
    for (target, joins) in [
        (
            "top",
            [
                ("0/Block/e/F1:+z|F3:blend:F1:+z|F1:-x", [-38.1, -12.7], [-38.1, 12.7]),
                ("0/Block/e/F1:+z|F3:blend:F1:+x|F1:+z", [38.1, -12.7], [38.1, 12.7]),
            ],
        ),
        (
            "right",
            [
                ("0/Block/e/F1:+x|F3:blend:F1:+x|F1:-z", [-12.7, -25.4], [12.7, -25.4]),
                ("0/Block/e/F1:+x|F3:blend:F1:+x|F1:+z", [-12.7, 25.4], [12.7, 25.4]),
            ],
        ),
    ] {
        let off = view(&s, target, LineStyle::Hidden, false);
        let on = view(&s, target, LineStyle::Hidden, true);
        assert_eq!((counts(&off), counts(&on)), ((12, 0, 0, 0, 2), (14, 0, 0, 0, 2)), "{target}");
        let added: Vec<&ViewCurve> = on.curves.iter().filter(|c| !off.curves.contains(c)).collect();
        assert_eq!(added.len(), 2, "{target}: {added:#?}");
        // R94: off, the joins are kept aside as suppressed curves (placed invisible), exactly as
        // they are drawn when on.
        assert_eq!(off.suppressed.iter().collect::<Vec<_>>(), added, "{target}");
        assert!(on.suppressed.is_empty(), "{target}");
        assert!(off.curves.iter().all(|c| on.curves.contains(c)), "{target}");
        for (edge, a, b) in joins {
            let c = added.iter().find(|c| c.edge == edge).unwrap_or_else(|| panic!("{target}: {edge} in {added:#?}"));
            let PaperCurve::Line { a: p, b: q } = c.curve else { panic!("{c:?}") };
            let (a, b) = (Vec2::new(a[0], a[1]), Vec2::new(b[0], b[1]));
            assert!(!c.hidden && ((near2(p, a) && near2(q, b)) || (near2(p, b) && near2(q, a))), "{target}: {c:?}");
        }
        assert_eq!((off.min, off.max), (on.min, on.max));
        assert_no_visible_duplicates(&on);
        assert_no_visible_duplicates(&off);
    }
    // The tube's two certified tangent joins, seen from the iso corner, where they are not its
    // outline: drawn only with tangent edges on.
    let s = tube();
    let off = view(&s, "iso_top_right", LineStyle::Hidden, false);
    let on = view(&s, "iso_top_right", LineStyle::Hidden, true);
    let joins =
        |g: &ViewGeometry| -> Vec<ViewCurve> { g.curves.iter().filter(|c| c.edge.starts_with("0/Tube/e/F2:side:s1|F2:side:s2~")).cloned().collect() };
    assert_eq!(joins(&on).len(), 2, "{:#?}", on.curves);
    assert!(joins(&off).is_empty(), "{:#?}", off.curves);
    let kept: Vec<&ViewCurve> = off.suppressed.iter().collect();
    assert_eq!(kept, joins(&on).iter().collect::<Vec<_>>(), "R94");
    assert_eq!(
        on.curves.len(),
        off.curves.len() + 2,
        "{:#?}
{:#?}",
        on.curves,
        off.curves
    );
    assert_no_visible_duplicates(&on);
    assert_no_visible_duplicates(&off);
}

/// The tube with a 6 mm bore.
fn bored_tube() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.spline.control_point", json!({"points": [[10, 0], [10, 13.333], [-10, 13.333], [-10, 0]]}));
    run(&mut s, "sketch.spline.control_point", json!({"points": [[-10, 0], [-10, -6], [10, -6], [10, 0]]}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 2], "radius": 3}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 10, "body_name": "Tube"}));
    s
}

#[test]
fn tangent_silhouettes_are_always_drawn() {
    // R54: from the front the tube's tangent joins at x = -10 and 10 are its outline, so they are
    // drawn with tangent edges off and on, named after the joins (B-rep edges); the
    // bore's silhouettes are the hidden lines at x = -3 and 3.
    let s = bored_tube();
    for tangent_edges in [false, true] {
        let g = view(&s, "front", LineStyle::Hidden, tangent_edges);
        let at = |x: f64, hidden: bool| -> Vec<ViewCurve> {
            g.curves
                .iter()
                .filter(|c| {
                    c.hidden == hidden && matches!(c.curve, PaperCurve::Line { a, b } if near(a.x, x) && near(b.x, x) && near((a.y - b.y).abs(), 10.))
                })
                .cloned()
                .collect()
        };
        for x in [-10., 10.] {
            let outline = at(x, false);
            assert_eq!(outline.len(), 1, "{tangent_edges} {x}: {:#?}", g.curves);
            assert!(outline[0].edge.starts_with("0/Tube/e/") && outline[0].edge.contains("F2:side:s2~"), "{outline:?}");
        }
        for x in [-3., 3.] {
            assert_eq!(at(x, true).len(), 1, "{tangent_edges} {x}: {:#?}", g.curves);
        }
        assert!(near2(g.min, Vec2::new(-10., -5.)) && near2(g.max, Vec2::new(10., 5.)), "{:?} {:?}", g.min, g.max);
        assert_no_visible_duplicates(&g);
    }
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

/// Final wave 8: a hidden polyline is dropped only where a visible chord draws over each of its
/// chords; a hidden run whose two ends lie on two separate visible outlines stays.
#[test]
fn a_hidden_polyline_between_two_outlines_stays() {
    use crate::drawing::project::visible_wins;
    let curve = |hidden: bool, curve: PaperCurve| ViewCurve { edge: "e".into(), sources: vec!["e".into()], segment: 0, hidden, curve };
    let line = |a: [f64; 2], b: [f64; 2]| PaperCurve::Line { a: Vec2::new(a[0], a[1]), b: Vec2::new(b[0], b[1]) };
    let poly = |p: &[[f64; 2]]| PaperCurve::Poly { points: p.iter().map(|q| Vec2::new(q[0], q[1])).collect() };
    // Two visible outlines at x = 0 and 10; a hidden 2-point run from one to the other.
    let across = curve(true, poly(&[[0.0, 0.0], [10.0, 0.0]]));
    let kept = visible_wins(vec![curve(false, line([0.0, -5.0], [0.0, 5.0])), curve(false, line([10.0, -5.0], [10.0, 5.0])), across.clone()]);
    assert_eq!(kept.len(), 3);
    assert!(kept.contains(&across));
    // Under one visible line, or a visible polyline it crosses a vertex of: dropped.
    let under = curve(true, poly(&[[1.0, 0.0], [4.0, 0.0], [9.0, 0.0]]));
    assert_eq!(visible_wins(vec![curve(false, line([0.0, 0.0], [10.0, 0.0])), under.clone()]).len(), 1);
    let bent = curve(false, poly(&[[0.0, 0.0], [5.0, 0.0], [10.0, 0.0]]));
    assert_eq!(visible_wins(vec![bent, curve(true, poly(&[[2.0, 0.0], [8.0, 0.0]]))]).len(), 1);
    // A hidden run with one chord off every visible chord stays.
    let partly = curve(true, poly(&[[1.0, 0.0], [4.0, 0.0], [4.0, 3.0]]));
    assert_eq!(visible_wins(vec![curve(false, line([0.0, 0.0], [10.0, 0.0])), partly]).len(), 2);
}
