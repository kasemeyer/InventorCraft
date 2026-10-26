#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Extrude's panel under a profile that sections it (Inventor's): Behavior (Direction, Extents,
//! distances), Output (the boolean, once there is a body to join) and Advanced.

use super::*;
use crate::scenario::Harness;
use solidcraft_engine::doc::DocumentKind;

/// The Inventor profile, optionally with a box away from the sketch, then a 60 x 40 rectangle
/// on XY and the Extrude panel open.
fn extrude(with_box: bool) -> Harness {
    let mut h = crate::test_support::inventor(DocumentKind::Part);
    h.app.preferences.look_at_anim = false;
    if with_box {
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [200, 0, 0]})).unwrap();
    }
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[60,40]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.start("solid.extrude");
    h.frames(2);
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Extrude { .. })), "the Extrude panel is open");
    h
}

fn direction(h: &Harness) -> usize {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Extrude { direction, .. }) => *direction,
        _ => panic!("no Extrude panel"),
    }
}

fn operation(h: &Harness) -> (usize, bool) {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Extrude { operation, auto_op, .. }) => (*operation, *auto_op),
        _ => panic!("no Extrude panel"),
    }
}

fn click(h: &mut Harness, text: &str) {
    h.step(&json!({"widget": text})).unwrap();
    h.frames(2);
}

#[test]
fn inventor_extrude_shows_behavior_output_and_advanced() {
    let h = extrude(true);
    for text in ["Flipped", "Asymmetric", "Through All", "New Solid", "Behavior", "Output", "Distance A", "Advanced Properties"] {
        assert!(h.widget(text).is_some(), "{text} is drawn");
    }
}

#[test]
fn direction_buttons_map_to_directions() {
    let mut h = extrude(true);
    click(&mut h, "Asymmetric");
    assert_eq!(direction(&h), TWO_SIDES);
    assert!(h.widget("Distance B").is_some(), "Asymmetric asks for the second distance");
    click(&mut h, "Flipped");
    assert_eq!(direction(&h), 1);
    assert!(h.widget("Distance B").is_none());
    click(&mut h, "Default");
    assert_eq!(direction(&h), 0);
}

#[test]
fn output_buttons_map_to_operations() {
    let mut h = extrude(true);
    click(&mut h, "Cut");
    assert_eq!(operation(&h), (2, false));
    click(&mut h, "New Solid");
    assert_eq!(operation(&h).0, 0);
}

#[test]
fn no_output_row_on_the_first_feature() {
    let h = extrude(false);
    assert!(h.widget("Behavior").is_some(), "the sectioned panel is drawn");
    assert!(h.widget("New Solid").is_none());
}

#[test]
fn solidcraft_extrude_keeps_its_rows() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[60,40]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.start("solid.extrude");
    h.frames(2);
    assert!(h.widget("Asymmetric").is_none());
    assert!(h.widget("Extent Type").is_some());
}

/// The arrowheads among a glyph's shapes (filled triangles): their tips' heights relative to
/// the square's centre, the furthest point from the head's base.
fn arrow_tips(shapes: &[egui::Shape], centre: egui::Pos2) -> Vec<f32> {
    shapes
        .iter()
        .filter_map(|s| match s {
            egui::Shape::Path(p) if p.closed && p.points.len() == 3 => {
                p.points.iter().map(|q| q.y - centre.y).max_by(|a, b| a.abs().total_cmp(&b.abs()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn direction_and_extent_glyphs_draw_arrows_not_words() {
    let square = egui::Rect::from_center_size(egui::pos2(50.0, 50.0), egui::vec2(20.0, 20.0));
    let ink = egui::Color32::WHITE;
    let direction = dense::direction_glyphs();
    let keys: Vec<_> = direction.iter().map(|g| g.key).collect();
    assert_eq!(keys, ["Default", "Flipped", "Symmetric", "Asymmetric"]);
    let tips: Vec<_> = direction.iter().map(|g| arrow_tips(&(g.glyph)(square, ink), square.center())).collect();
    assert!(tips[0].len() == 1 && tips[0][0] < 0.0, "Default: one arrow up {:?}", tips[0]);
    assert!(tips[1].len() == 1 && tips[1][0] > 0.0, "Flipped: one arrow down {:?}", tips[1]);
    assert!(tips[2].len() == 2 && (tips[2][0] + tips[2][1]).abs() < 1e-3, "Symmetric: equal halves {:?}", tips[2]);
    assert!(tips[3].len() == 2 && tips[3][0] < 0.0 && tips[3][1] > 0.0, "Asymmetric: both ways {:?}", tips[3]);
    assert!(tips[3][1] < -tips[3][0] * 0.8, "Asymmetric: unequal halves {:?}", tips[3]);
    let extents = dense::extent_glyphs();
    assert_eq!(extents.iter().map(|g| g.key).collect::<Vec<_>>(), ["Distance", "To", "Through All"]);
    for g in direction.iter().chain(extents.iter()) {
        let shapes = (g.glyph)(square, ink);
        assert!(!shapes.iter().any(|s| matches!(s, egui::Shape::Text(_))), "{}: no text", g.key);
        assert!(!arrow_tips(&shapes, square.center()).is_empty(), "{}: an arrow", g.key);
        assert!(shapes.iter().all(|s| square.expand(1.0).contains_rect(s.visual_bounding_rect())), "{}: inside its square", g.key);
        assert_eq!(g.tip, g.key, "{}: the word is the hover text (English)", g.key);
    }
    let blocks = |g: &crate::ui_kit::GlyphSegment| (g.glyph)(square, ink).iter().filter(|s| matches!(s, egui::Shape::Rect(_))).count();
    assert_eq!(extents.iter().map(blocks).collect::<Vec<_>>(), [0, 1, 1], "To and Through All meet a block");
}

#[test]
fn direction_and_extents_rows_fit_the_panel() {
    let h = extrude(true);
    for (row, keys) in [("Direction", &["Default", "Flipped", "Symmetric", "Asymmetric"][..]), ("Extents", &["Distance", "To", "Through All"][..])] {
        let rect = crate::scenario::rect_of(&format!("dialog:segments:{row}")).unwrap_or_else(|| panic!("{row} row drawn"));
        let boolean = crate::scenario::rect_of("dialog:segments:Boolean").expect("Boolean row drawn");
        // Direction fills the field column; the extents end it (at the end of the distance row).
        if row == "Direction" {
            assert!((rect.left() - boolean.left()).abs() < 0.5, "{row} starts the field column");
        }
        assert!(rect.left() >= boolean.left() - 0.5 && (rect.right() - boolean.right()).abs() < 0.5, "{row} sits in the field column");
        let width = rect.width() / keys.len() as f32;
        assert!(width >= rect.height(), "{row}: each segment holds its glyph square ({width} for {})", rect.height());
        for key in keys {
            let at = crate::scenario::handle_at(&format!("dialog:{row}:{key}")).unwrap_or_else(|| panic!("{row}:{key} drawn"));
            assert!(rect.contains(at), "{row}:{key} inside its row");
            assert!(h.widget(key).is_some(), "{key} is the segment's accessible name");
        }
    }
}

/// A glyph's filled rectangles and its outlined (stroke-only) rectangles.
fn boxes(shapes: &[egui::Shape]) -> (Vec<egui::Rect>, Vec<egui::Rect>) {
    let (mut filled, mut outlined) = (Vec::new(), Vec::new());
    for s in shapes {
        if let egui::Shape::Rect(r) = s {
            if r.fill == egui::Color32::TRANSPARENT { outlined.push(r.rect) } else { filled.push(r.rect) }
        }
    }
    (filled, outlined)
}

#[test]
fn boolean_glyphs_draw_bodies_not_words() {
    let square = egui::Rect::from_center_size(egui::pos2(50.0, 50.0), egui::vec2(20.0, 20.0));
    let ink = egui::Color32::WHITE;
    let glyphs = dense::boolean_glyphs();
    assert_eq!(glyphs.iter().map(|g| g.key).collect::<Vec<_>>(), ["Join", "Cut", "Intersect", "New Solid"]);
    for g in &glyphs {
        let shapes = (g.glyph)(square, ink);
        assert!(!shapes.iter().any(|s| matches!(s, egui::Shape::Text(_))), "{}: no text", g.key);
        assert!(shapes.iter().all(|s| square.expand(1.0).contains_rect(s.visual_bounding_rect())), "{}: inside its square", g.key);
        assert_eq!(g.tip, g.key, "{}: the word is the hover text (English)", g.key);
    }
    let shapes: Vec<_> = glyphs.iter().map(|g| (g.glyph)(square, ink)).collect();
    // Join: two filled bodies that overlap (merged).
    let (filled, outlined) = boxes(&shapes[0]);
    assert!(filled.len() == 2 && outlined.is_empty(), "Join: two filled bodies {filled:?}");
    let overlap = filled[0].intersect(filled[1]);
    assert!(overlap.is_positive(), "Join: the bodies overlap");
    // Cut: the body less the overlap is filled; the tool is an outline; the bite is empty.
    let (filled, outlined) = boxes(&shapes[1]);
    assert!(filled.len() == 2 && outlined.len() == 1, "Cut: the body in two parts and the tool {filled:?} {outlined:?}");
    assert!(!filled.iter().any(|f| f.contains(overlap.center())), "Cut: the bite is removed");
    assert!(outlined[0].contains(overlap.center()), "Cut: the tool's outline runs over the bite");
    // Intersect: only the overlap filled, both bodies outlines.
    let (filled, outlined) = boxes(&shapes[2]);
    assert!(filled.len() == 1 && outlined.len() == 2, "Intersect: the overlap and two outlines");
    assert!((filled[0].area() - overlap.area()).abs() < 0.01 && filled[0].contains(overlap.center()), "Intersect: the overlap only");
    // New Solid: one lone body and a plus (two crossing segments) outside it.
    let (filled, outlined) = boxes(&shapes[3]);
    let plus: Vec<_> = shapes[3].iter().filter(|s| matches!(s, egui::Shape::LineSegment { .. })).map(|s| s.visual_bounding_rect()).collect();
    assert!(filled.len() == 1 && outlined.is_empty() && plus.len() == 2, "New Solid: a body and a plus");
    assert!(plus[0].intersects(plus[1]) && !filled[0].intersects(plus[0]), "New Solid: the plus crosses beside the body");
}

#[test]
fn boolean_row_fits_the_panel() {
    let h = extrude(true);
    let row = crate::scenario::rect_of("dialog:segments:Boolean").expect("Boolean row drawn");
    let direction = crate::scenario::rect_of("dialog:segments:Direction").expect("Direction row drawn");
    assert!((row.left() - direction.left()).abs() < 0.5 && (row.right() - direction.right()).abs() < 0.5, "Boolean sits in the field column");
    assert!(row.width() / 4.0 >= row.height(), "each segment holds its glyph square ({} for {})", row.width() / 4.0, row.height());
    for key in ["Join", "Cut", "Intersect", "New Solid"] {
        let at = crate::scenario::handle_at(&format!("dialog:Boolean:{key}")).unwrap_or_else(|| panic!("Boolean:{key} drawn"));
        assert!(row.contains(at), "Boolean:{key} inside its row");
        assert!(h.widget(key).is_some(), "{key} is the segment's accessible name");
    }
}

/// The extent buttons sit at the end of the distance row (the capture's row), after the field,
/// and stay there with the field disabled when the extent is not a distance.
#[test]
fn extents_sit_at_the_end_of_the_distance_row() {
    let mut h = extrude(true);
    let place = |h: &Harness| {
        let row = crate::scenario::rect_of("dialog:row:Distance A").expect("Distance A row drawn");
        let strip = crate::scenario::rect_of("dialog:segments:Extents").expect("extent buttons drawn");
        let field = crate::scenario::handle_at("dialog:Distance A").expect("Distance A field drawn");
        assert!(row.contains_rect(strip), "the extent buttons are in the distance row: {strip:?} in {row:?}");
        assert!((strip.right() - row.right()).abs() < 0.5, "the extent buttons end the row");
        assert!(field.x < strip.left() && row.contains(field), "the field comes first, then the buttons");
        assert!(crate::scenario::rect_of("dialog:row:Extents").is_none(), "no separate Extents row");
        assert!(h.widget("Distance A").is_some(), "the field keeps its accessible name");
        strip
    };
    let before = place(&h);
    // Whether the Distance A field (the text input so labelled) takes typing.
    let enabled = |h: &Harness| {
        h.output.accesskit_update.as_ref().and_then(|tree| {
            tree.nodes
                .iter()
                .find_map(|(_, n)| (n.role() == egui::accesskit::Role::TextInput && n.label() == Some("Distance A")).then(|| !n.is_disabled()))
        })
    };
    assert_eq!(enabled(&h), Some(true), "a distance extent types the distance");
    click(&mut h, "Through All");
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Extrude { all: true, .. })), "Through All picked");
    let after = place(&h);
    assert_eq!(before, after, "the buttons do not move");
    assert_eq!(enabled(&h), Some(false), "through all has no distance to type");
    // By handle, as the extent and its word are named alike.
    h.step(&json!({"click": {"handle": "dialog:Extents:Distance"}})).unwrap();
    h.frames(2);
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Extrude { all: false, .. })));
    assert_eq!(enabled(&h), Some(true));
}

/// A reversed asymmetric pair shows as Asymmetric; clicking that shown segment sets the plain
/// pair rather than doing nothing.
#[test]
fn asymmetric_on_a_reversed_pair_sets_the_plain_pair() {
    let mut h = extrude(true);
    if let Some(Kind::Extrude { direction, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *direction = TWO_SIDES_REVERSED;
    }
    h.frames(2);
    assert_eq!(direction(&h), TWO_SIDES_REVERSED);
    click(&mut h, "Asymmetric");
    assert_eq!(direction(&h), TWO_SIDES, "Asymmetric sets the plain pair");
    assert!(h.widget("Distance B").is_some(), "still two-sided");
}

/// The capture's words: Distance A and Advanced Properties, translated (ja here); SolidCraft's
/// Extrude keeps Distance and Advanced.
#[test]
fn distance_a_and_advanced_properties_are_the_capture_words() {
    let mut h = extrude(true);
    h.app.preferences.language = "ja".into();
    h.frames(2);
    let ja = crate::i18n::Locale::for_language("ja");
    for word in ["Distance A", "Advanced Properties"] {
        let shown = ja.text(word);
        assert_ne!(shown, word, "{word} is translated in ja");
        assert!(h.widget(shown).is_some(), "{word} shows as {shown}");
    }
    assert!(crate::scenario::rect_of("dialog:row:Distance A").is_some(), "the handle keeps the English key");
}
