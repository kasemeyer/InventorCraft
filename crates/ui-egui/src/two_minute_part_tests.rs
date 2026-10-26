#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The two-minute part (Plan B-2 Task 11): the Inventor workflow driven end to end through the
//! UI paths an Inventor user takes (aliases, clicks, the marking menu, the panel's "+", the
//! browser), one named assertion per owner answer; and the same part under SolidCraft.

use serde_json::{Value, json};
use solidcraft_engine::doc::DocumentKind::Part;
use solidcraft_engine::doc::FeatureKind;
use solidcraft_engine::geom::Vec3;

use crate::dialogs::Kind;
use crate::scenario::{Harness, handle_at, handles};
use crate::test_support::inventor;

/// Type an alias as a user does: each letter pressed and released, then wait out the
/// profile's prefix wait.
fn alias(h: &mut Harness, keys: &str) {
    h.app.alias.fired = None;
    for c in keys.chars() {
        h.step(&json!({"key": c.to_string()})).unwrap();
    }
    // A prefix waits the profile's alias_wait_ms for the next letter (an exact match fired at once).
    if h.app.alias.fired.is_none() {
        h.frames(frames_for(f64::from(crate::profile::active(&h.app).alias_wait_ms) / 1000.0));
    }
    assert!(h.app.alias.fired.is_some(), "alias {keys} ran nothing");
    h.frames(3);
}

/// Harness frames (60 Hz) that cover `seconds`, plus one.
fn frames_for(seconds: f64) -> usize {
    (seconds * 60.0).ceil() as usize + 1
}

/// Let the last clicks expire (egui's double-click delay), so the next click is a new one.
fn clicks_expire(h: &mut Harness) {
    let delay = h.ctx.options(|o| o.input_options.max_double_click_delay);
    h.frames(frames_for(delay));
}

/// A right-click at `at`: under Inventor the marking menu opens there.
fn right_click(h: &mut Harness, at: Value) {
    h.step(&json!({"click": at, "button": "right"})).unwrap();
    h.frames(2);
}

/// Click the drawn menu entry `id`: a ring pill (`marking:`) or a list row (`menu:`).
fn menu_pick(h: &mut Harness, prefix: &str, id: &str) {
    let handle = format!("{prefix}:{id}");
    assert!(handle_at(&handle).is_some(), "{handle} is not drawn: {:?} {}", handles(prefix), crate::context_menu::describe(&h.app));
    h.step(&json!({"click": {"handle": handle}})).unwrap();
    h.frames(3);
}

fn ring_ids(h: &Harness) -> Vec<String> {
    crate::context_menu::describe(&h.app)["radial"].as_array().into_iter().flatten().map(|i| i["id"].as_str().unwrap().to_string()).collect()
}

fn count(h: &Harness, f: impl Fn(&FeatureKind) -> bool) -> usize {
    h.app.session.doc.features.iter().filter(|x| f(&x.kind)).count()
}

fn volume(h: &mut Harness) -> f64 {
    h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().expect("a measured volume")
}

fn dof(h: &Harness) -> usize {
    let st = h.app.session.world_state();
    st.sketch(h.app.session.active_sketch.expect("a sketch is open")).unwrap().report.dof
}

/// The one body's extent (low and high corners).
fn extent(h: &Harness) -> (Vec3, Vec3) {
    let st = h.app.session.world_state();
    let body = &st.bodies.first().expect("a body").body;
    let mut lo = Vec3::new(f64::MAX, f64::MAX, f64::MAX);
    let mut hi = Vec3::new(f64::MIN, f64::MIN, f64::MIN);
    for e in body.edges(0.001).unwrap() {
        for p in e.points {
            lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
            hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        }
    }
    (lo, hi)
}

/// A screen point below the part where the view shows nothing.
fn empty_space(h: &mut Harness) -> Value {
    let [x, y] = h.at(&json!({"world": [0, 0, 0]})).unwrap();
    json!([x, y + 160.0])
}

/// With Dimension running: pick the line at `on`, place the value at `at`, type `value`, Enter.
fn dimension(h: &mut Harness, on: [f64; 2], at: [f64; 2], value: &str) {
    h.step(&json!({"click": {"world": [on[0], on[1], 0]}})).unwrap();
    h.step(&json!({"click": {"world": [at[0], at[1], 0]}})).unwrap();
    assert!(crate::dim_view::editing(), "placing a dimension opens its value field: {}", h.call("ui.inspect", json!({})));
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": value})).unwrap();
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(2);
    assert!(!crate::dim_view::editing(), "Enter accepts {value}");
    // Let the placement clicks expire before the next pick.
    clicks_expire(h);
}

/// The open sketch's lines' extent: (low x, low y, high x, high y).
fn sketch_box(h: &Harness) -> [f64; 4] {
    let st = h.app.session.world_state();
    let sk = &st.sketch(h.app.session.active_sketch.expect("a sketch is open")).unwrap().sketch;
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for c in &sk.curves {
        if let solidcraft_engine::sketch::CurveKind::Line { a, b: e } = c.kind {
            for p in [sk.point(a).unwrap(), sk.point(e).unwrap()] {
                b = [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)];
            }
        }
    }
    b
}

/// Empty sketch space below and left of the sketch's lines.
fn beside_sketch(h: &Harness) -> Value {
    let [x0, y0, ..] = sketch_box(h);
    json!({"world": [x0 - 20.0, y0 - 20.0, 0]})
}

/// The top face's two edges along X (front and back), as world midpoints. A width change grows
/// the rectangle evenly about its middle, so these midpoints stay on their edges.
fn top_x_edges(lo: Vec3, hi: Vec3) -> [[f64; 3]; 2] {
    let x = (lo.x + hi.x) / 2.0;
    [[x, lo.y, hi.z], [x, hi.y, hi.z]]
}

/// The middle of the part's top face.
fn top_middle(h: &Harness) -> [f64; 3] {
    let (lo, hi) = extent(h);
    [(lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, hi.z]
}

fn name_of(h: &Harness, f: impl Fn(&FeatureKind) -> bool) -> String {
    h.app.session.doc.features.iter().find(|x| f(&x.kind)).expect("the feature").name.clone()
}

fn labels() -> Vec<String> {
    crate::browser::drawn_rows().into_iter().map(|(l, ..)| l).collect()
}

const IN: f64 = 25.4;

#[test]
fn a_two_minute_part_under_inventor() {
    let mut h = inventor(Part);
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;

    // 1. A new inch part.
    crate::documents::new_document(&mut h.app, Part, Some("in"));
    h.frames(3);
    assert_eq!(h.app.session.doc.units, "in", "the Standard (in) template");
    assert_eq!(crate::prefs::document_length_decimals(&h.app.session.doc), Some(3), "an inch document shows three decimals");
    assert!(!crate::timeline::shown(&h.app), "no timeline panel: the browser is the history");
    let iso = h.app.cam;

    // Plan B's final wave, as the walkthrough meets it: versioned defaults are recorded, the ribbon
    // collapses whole panels (checked at the end, narrowed), and the QAT's Return waits for a sketch.
    assert_eq!(
        h.app.preferences.profile_defaults.get("inventor"),
        Some(&crate::profile::get("inventor").unwrap().defaults.version()),
        "the Inventor defaults version is recorded"
    );
    assert!(crate::quick_access::entries(&h.app).contains(&("return", "finish", false)), "Return is off with no sketch open");

    // 2. S, then the XY plane: the sketch opens facing the viewer.
    alias(&mut h, "S");
    h.step(&json!({"click": {"plane": "XY"}})).unwrap();
    h.frames(3);
    let sketch = h.app.session.active_sketch.expect("S + XY opens a sketch");
    assert!(h.app.cam.back().z > 0.999, "the sketch is looked at: {:?}", h.app.cam.back());
    {
        let st = h.app.session.world_state();
        let ss = st.sketch(sketch).unwrap();
        // Owner answer 2, Autoproject (part origin) off: the new sketch holds only its own fixed
        // origin point, nothing linked to or projected from the part.
        let points: Vec<(&str, bool)> = ss.sketch.points.iter().map(|p| (p.id.as_str(), p.link.is_none())).collect();
        assert_eq!(points, [("origin", true)], "owner answer 2, Autoproject off: only the sketch's own origin point");
        assert!(ss.sketch.links.is_empty(), "owner answer 2, Autoproject off: nothing linked into a new sketch");
        assert!(ss.sketch.curves.is_empty(), "owner answer 2, Autoproject off: no projected curves");
    }
    assert!(h.app.viewport.palette_rect.is_none(), "no Sketch Palette docks");
    assert!(crate::quick_access::entries(&h.app).contains(&("return", "finish", true)), "the QAT's Return finishes the open sketch");

    // 3. REC, two clicks, D, the width typed bare and the height with its unit.
    alias(&mut h, "REC");
    h.step(&json!({"click": {"world": [10, 10, 0]}})).unwrap();
    h.step(&json!({"click": {"world": [40, 30, 0]}})).unwrap();
    clicks_expire(&mut h);
    assert_eq!(dof(&h), 4, "a two-point rectangle leaves its width, height and position free");
    alias(&mut h, "D");
    assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.dimension"));
    let width = h.app.session.doc.next_d_name();
    // The bottom line, its value placed below it.
    let [x0, y0, x1, _] = sketch_box(&h);
    dimension(&mut h, [(x0 + x1) / 2.0, y0], [(x0 + x1) / 2.0, y0 - 10.0], "2");
    // The right line (wherever the width moved it), its value placed right of it.
    let height = h.app.session.doc.next_d_name();
    let [_, y0, x1, y1] = sketch_box(&h);
    dimension(&mut h, [x1, (y0 + y1) / 2.0], [x1 + 10.0, (y0 + y1) / 2.0], "1.5 in");
    assert_eq!(dof(&h), 2, "Ruling 20: two dimensions leave only the rectangle's position free");
    assert_eq!(crate::status_bar::texts(&h.app).sketch.as_deref(), Some("2 dimensions needed"));
    assert_eq!(h.app.session.doc.param(&width).unwrap().expr.as_str(), "2 in", "a bare 2 is stored in the design unit");
    assert_eq!(h.app.session.doc.param(&height).unwrap().expr.as_str(), "1.5 in");

    // 4. Right-click: the tool's ring, OK; right-click again: Finish Sketch.
    let beside = beside_sketch(&h);
    right_click(&mut h, beside);
    // Owner answer 3, no flick gestures: a right-click opens the ring (hold and flick are off
    // under a marking menu: marking_menu::tests::marking_menus_have_no_hold_or_flick).
    assert!(h.app.viewport.context_menu.is_some(), "owner answer 3: a right-click opens the ring");
    assert_eq!(ring_ids(&h).len(), 8, "a ring of eight");
    menu_pick(&mut h, "marking", "ui.done");
    assert!(h.app.tool.is_none(), "OK ends Dimension");
    assert_eq!(h.app.session.active_sketch, Some(sketch), "and stays in the sketch");
    let beside = beside_sketch(&h);
    right_click(&mut h, beside);
    menu_pick(&mut h, "marking", "ui.finishSketch");
    assert!(h.app.session.active_sketch.is_none());
    assert_eq!(h.app.cam, iso, "owner answer 1, Finish Sketch returns to the previous (iso) view");

    // 5. E opens at 1 in; Enter is OK.
    alias(&mut h, "E");
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Extrude { distance, .. }) => assert_eq!(distance, "1 in", "owner answer 7, the inch template's extrusion"),
        k => panic!("E opened {k:?}"),
    }
    h.step(&json!({"key": "Enter"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "owner answer 6, Enter = OK in the panel");
    assert_eq!(count(&h, |k| matches!(k, FeatureKind::Extrude { .. })), 1);
    let (lo, hi) = extent(&h);
    assert!(
        ((hi.x - lo.x) - 2.0 * IN).abs() < 1e-6 && ((hi.y - lo.y) - 1.5 * IN).abs() < 1e-6 && ((hi.z - lo.z) - IN).abs() < 1e-6,
        "a 2 × 1.5 × 1 in block: {lo:?} {hi:?}"
    );
    let extrusion = name_of(&h, |k| matches!(k, FeatureKind::Extrude { .. }));
    let sketch_name = name_of(&h, |k| matches!(k, FeatureKind::Sketch { .. }));
    h.frames(2);
    let rows = labels();
    assert!(rows.contains(&extrusion), "the browser shows {extrusion}: {rows:?}");
    assert!(!rows.contains(&sketch_name), "the consumed {sketch_name} is folded inside it: {rows:?}");
    h.step(&json!({"click": {"chevron": extrusion.as_str()}})).unwrap();
    h.frames(2);
    let rows = labels();
    let at = rows.iter().position(|l| *l == extrusion).unwrap();
    assert_eq!(rows.get(at + 1), Some(&sketch_name), "expanded, {sketch_name} follows {extrusion}: {rows:?}");
    assert_eq!(rows.get(at + 2).map(String::as_str), Some("End of Part"), "and End of Part is below: {rows:?}");

    // 6. H, the top face, OK through the marking menu.
    alias(&mut h, "H");
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Hole { .. })), "H opens Hole");
    let top = top_middle(&h);
    h.step(&json!({"click": {"world": top}})).unwrap();
    h.frames(3);
    let empty = empty_space(&mut h);
    right_click(&mut h, empty.clone());
    menu_pick(&mut h, "menu", "ui.ok");
    h.settle();
    assert!(h.app.dialog.is_none(), "the marking menu's OK closes Hole");
    let holes: Vec<String> = h
        .app
        .session
        .doc
        .features
        .iter()
        .filter_map(|f| match &f.kind {
            FeatureKind::Hole { diameter, .. } => Some(diameter.as_str().to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(holes, ["0.25 in"], "owner answer 7, one hole at the inch template's 0.25 in");

    // 7. F, an edge, "+", another edge, OK.
    alias(&mut h, "F");
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Fillet { .. })), "F opens Fillet");
    let (lo, hi) = extent(&h);
    let [e1, e2] = top_x_edges(lo, hi);
    h.step(&json!({"click": {"world": e1}})).unwrap();
    h.frames(3);
    assert!(handle_at("dialog:apply").is_some(), "the Fillet panel offers +");
    h.step(&json!({"click": {"handle": "dialog:apply"}})).unwrap();
    h.settle();
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Fillet { .. })), "+ keeps the panel for the next one");
    h.step(&json!({"click": {"world": e2}})).unwrap();
    h.frames(3);
    right_click(&mut h, empty.clone());
    menu_pick(&mut h, "menu", "ui.ok");
    h.settle();
    assert!(h.app.dialog.is_none(), "the marking menu's OK closes Fillet");
    let fillets: Vec<String> = h
        .app
        .session
        .doc
        .features
        .iter()
        .filter_map(|f| match &f.kind {
            FeatureKind::Fillet { radius, .. } => Some(radius.as_str().to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(fillets, ["0.125 in", "0.125 in"], "owner answer 7, two fillets at the inch template's 0.125 in");
    let before_edit = h.app.cam;

    // 8. Double-click the consumed sketch: the extrusion hides and the view faces the sketch.
    // Let the last clicks expire so the double-click is one.
    clicks_expire(&mut h);
    h.step(&json!({"click": {"browser": sketch_name.as_str()}, "double": true})).unwrap();
    h.settle();
    assert_eq!(h.app.session.active_sketch, Some(sketch), "the double-click edits {sketch_name}");
    assert!(h.app.session.world_state().bodies.is_empty(), "owner answer 4, editing a consumed sketch hides the consuming feature");
    assert!(h.app.cam.back().z > 0.999, "the view faces the sketch: {:?}", h.app.cam.back());
    clicks_expire(&mut h);
    h.step(&json!({"click": {"dimension": width.as_str()}, "double": true})).unwrap();
    assert!(crate::dim_view::editing(), "a double-click opens the width");
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": "3"})).unwrap();
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.doc.param(&width).unwrap().expr.as_str(), "3 in");
    let beside = beside_sketch(&h);
    right_click(&mut h, beside);
    // The edited dimension is still selected: its menu lists Finish Sketch below the ring.
    menu_pick(&mut h, "menu", "ui.finishSketch");
    h.settle();
    assert!(h.app.session.active_sketch.is_none());
    assert_eq!(h.app.session.world_state().bodies.len(), 1, "the part rebuilds");
    let (lo, hi) = extent(&h);
    assert!(((hi.x - lo.x) - 3.0 * IN).abs() < 1e-6, "3 in wide now: {lo:?} {hi:?}");
    assert_eq!(count(&h, |k| matches!(k, FeatureKind::Fillet { .. })), 2);
    assert_eq!(h.app.cam, before_edit, "the view returns");
    the_finished_part(&mut h);

    // F6 is Home View (Plan B final wave): from Top, reached from Front, so neither Previous
    // View (Front) nor the bare iso orientation is what Home gives; Home is iso, fitted.
    for view in ["front", "top"] {
        h.step(&json!({"call": "ui.view", "params": {"view": view}})).unwrap();
        h.settle();
    }
    let mut home = h.app.cam;
    home.set_view(solidcraft_engine::render::StandardView::Iso);
    let home = h.app.fitted(home);
    h.step(&json!({"key": "F6"})).unwrap();
    h.settle();
    let c = h.app.cam;
    assert!(
        (c.target - home.target).len() < 1e-6
            && (c.distance - home.distance).abs() < 1e-6
            && (c.yaw - home.yaw).abs() < 1e-9
            && (c.pitch - home.pitch).abs() < 1e-9
            && (c.roll - home.roll).abs() < 1e-9,
        "F6 goes to the Home camera: {c:?} vs {home:?}"
    );

    // 9. Save.
    assert!(h.app.session.is_dirty(), "the part has unsaved changes");
    let path = std::env::temp_dir().join(format!("tci-two-minute-part-{}.solidcraft", std::process::id()));
    let saved = h.app.run("file.save", json!({"path": path.to_string_lossy()}));
    let _ = std::fs::remove_file(&path);
    assert!(saved.is_ok(), "file.save: {saved:?}");

    // 10. Right-click › Repeat starts a third fillet.
    right_click(&mut h, empty);
    menu_pick(&mut h, "menu", "ui.repeat");
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Fillet { .. })), "owner answer 5, right-click Repeat starts Fillet again");

    // Plan B final wave: narrowed, the ribbon collapses whole panels and trims no label.
    h.app.ui.tab = "MODEL".into();
    h.call("ui.resize", json!({"width": 1100, "height": 1000}));
    h.frames(2);
    assert!(crate::scenario::count_of("toolbar_collapsed").is_some_and(|n| n > 0.0), "at 1100 px whole panels collapse");
    assert_eq!(crate::scenario::count_of("toolbar_trimmed"), Some(0.0), "the ribbon trims no panel");
    assert_eq!(crate::scenario::count_of("toolbar_truncated_labels"), Some(0.0), "the ribbon truncates no label");
}

/// The finished part, whichever way it was made: a 3 × 1.5 × 1 in block with a 0.25 in hole in
/// the middle of its top face and 0.125 in rounds on the top face's two long edges, the same
/// volume as that part built from commands.
fn the_finished_part(h: &mut Harness) {
    let (lo, hi) = extent(h);
    let size = hi - lo;
    assert!((size.x - 3.0 * IN).abs() < 1e-6 && (size.y - 1.5 * IN).abs() < 1e-6 && (size.z - IN).abs() < 1e-6, "a 3 × 1.5 × 1 in block: {size:?}");
    let doc = &h.app.session.doc;
    let length = |e: &str| doc.eval(e, solidcraft_engine::doc::expr::Kind::Length).unwrap();
    let (position, diameter, depth, tip) = doc
        .features
        .iter()
        .find_map(|f| match &f.kind {
            FeatureKind::Hole { position, diameter, depth, hole, .. } => Some((
                position.0,
                length(diameter.as_str()),
                depth.as_ref().map(|d| length(d.as_str())),
                match hole {
                    solidcraft_engine::doc::HoleKind::Drilled { tip_angle } => Some(tip_angle.as_str().to_string()),
                    _ => None,
                },
            )),
            _ => None,
        })
        .expect("a hole");
    let middle = Vec3::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, hi.z);
    // The click is a screen point (f32), so the picked point is exact to about 1e-5 mm.
    assert!((position - middle).len() < 1e-4, "the hole is in the middle of the top face: {position:?} vs {middle:?}");
    assert!((diameter - 0.25 * IN).abs() < 1e-9, "a 0.25 in hole: {diameter}");
    assert!(depth.is_some_and(|d| (d - 0.5 * IN).abs() < 1e-9), "0.5 in deep: {depth:?}");
    let made = volume(h);

    let mut r = Harness::new();
    r.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    r.app.run("sketch.rectangle.two_point", json!({"p0": [lo.x, lo.y], "p1": [hi.x, hi.y]})).unwrap();
    r.app.run("sketch.finish", json!({})).unwrap();
    r.app.run("solid.extrude", json!({"distance": IN})).unwrap();
    let mut hole = json!({"position": [(lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, hi.z], "diameter": 0.25 * IN, "depth": 0.5 * IN});
    if let Some(tip) = tip {
        hole["type"] = json!("drilled");
        hole["tip_angle"] = json!(tip);
    }
    r.app.run("solid.hole", hole).unwrap();
    for e in top_x_edges(lo, hi) {
        r.app.run("solid.fillet", json!({"edges": [e], "radius": 0.125 * IN})).unwrap();
    }
    let want = volume(&mut r);
    assert!((made - want).abs() < 1e-3, "the part's volume {made} mm³ is the commanded part's {want} mm³");
}

/// Type `value` into the open panel's text field labelled `label`, which shows `current`: click
/// it, select all, type.
fn type_field(h: &mut Harness, label: &str, current: &str, value: &str) {
    let at = h.output.accesskit_update.as_ref().and_then(|tree| {
        tree.nodes.iter().find_map(|(_, n)| {
            let b = n.bounds()?;
            (n.role() == egui::accesskit::Role::TextInput && n.label() == Some(label) && b.x1 > b.x0)
                .then(|| (n.value().map(str::to_string), [(b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0]))
        })
    });
    let Some((shown, [x, y])) = at else { panic!("no {label} field in the panel") };
    assert_eq!(shown.as_deref(), Some(current), "the {label} field");
    h.step(&json!({"click": [x, y]})).unwrap();
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": value})).unwrap();
    h.frames(3);
}

fn panel(h: &Harness) -> &Kind {
    &h.app.dialog.as_ref().expect("a panel is open").kind
}

/// The same part under SolidCraft, through its own paths (toolbar starts, typed panel fields, the
/// panels' OK, no aliases or marking menu), with SolidCraft's own defaults and views unchanged:
/// nothing in the Inventor workflow changed what SolidCraft makes.
#[test]
fn the_same_part_under_solidcraft() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;
    crate::documents::new_document(&mut h.app, Part, Some("in"));
    h.frames(3);
    assert_eq!(h.app.session.doc.units, "in");

    h.step(&json!({"start": "sketch.create"})).unwrap();
    h.step(&json!({"click": {"plane": "XY"}})).unwrap();
    h.frames(3);
    let sketch = h.app.session.active_sketch.expect("a sketch on XY");
    h.step(&json!({"start": "sketch.rectangle.two_point"})).unwrap();
    h.step(&json!({"click": {"world": [10, 10, 0]}})).unwrap();
    h.step(&json!({"click": {"world": [40, 30, 0]}})).unwrap();
    clicks_expire(&mut h);
    h.step(&json!({"start": "sketch.dimension"})).unwrap();
    let width = h.app.session.doc.next_d_name();
    let [x0, y0, x1, _] = sketch_box(&h);
    dimension(&mut h, [(x0 + x1) / 2.0, y0], [(x0 + x1) / 2.0, y0 - 10.0], "2");
    let [_, y0, x1, y1] = sketch_box(&h);
    dimension(&mut h, [x1, (y0 + y1) / 2.0], [x1 + 10.0, (y0 + y1) / 2.0], "1.5 in");
    assert_eq!(dof(&h), 2);
    // Esc ends Dimension; Finish Sketch from the toolbar.
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(3);
    assert!(h.app.tool.is_none(), "Esc ends Dimension");
    assert_eq!(h.app.session.active_sketch, Some(sketch), "one Esc keeps the sketch");
    h.step(&json!({"start": "sketch.finish"})).unwrap();
    h.settle();
    assert!(h.app.session.active_sketch.is_none());
    assert!(h.app.cam.back().z > 0.999, "SolidCraft keeps facing the sketch after Finish: {:?}", h.app.cam.back());
    // Turn to the iso view to see the part.
    h.step(&json!({"call": "ui.view", "params": {"view": "iso"}})).unwrap();
    h.settle();

    h.step(&json!({"start": "solid.extrude"})).unwrap();
    h.frames(3);
    let Kind::Extrude { distance, .. } = panel(&h) else { panic!("Extrude opened {:?}", panel(&h)) };
    assert_eq!(distance, "10 mm", "SolidCraft's Extrude keeps its mm default in an inch design");
    type_field(&mut h, "Distance", "10 mm", "1 in");
    h.step(&json!({"widget": "OK"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "OK closes Extrude");

    let (lo, hi) = extent(&h);
    h.step(&json!({"start": "solid.hole"})).unwrap();
    h.step(&json!({"click": {"world": [(lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, hi.z]}})).unwrap();
    h.frames(3);
    let Kind::Hole { diameter, depth, .. } = panel(&h) else { panic!("Hole opened {:?}", panel(&h)) };
    assert_eq!((diameter.as_str(), depth.as_str()), ("5 mm", "10 mm"), "SolidCraft's Hole keeps its mm defaults");
    type_field(&mut h, "Diameter", "5 mm", "0.25 in");
    type_field(&mut h, "Depth", "10 mm", "0.5 in");
    h.step(&json!({"widget": "OK"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "OK closes Hole");

    // One fillet per edge, as the Inventor walkthrough makes them: pick the edge, then Fillet.
    // (With SolidCraft's Fillet panel open, a click on the far edge picks nothing in this view.)
    for e in top_x_edges(lo, hi) {
        h.step(&json!({"click": {"world": e}})).unwrap();
        h.frames(3);
        assert!(
            matches!(h.app.session.selection.as_slice(), [solidcraft_engine::Sel::Edge { .. }]),
            "{e:?} picks an edge: {:?}",
            h.app.session.selection
        );
        h.step(&json!({"start": "solid.fillet"})).unwrap();
        h.frames(3);
        let Kind::Fillet { radius, .. } = panel(&h) else { panic!("Fillet opened {:?}", panel(&h)) };
        assert_eq!(radius, "2 mm", "SolidCraft's Fillet keeps its mm default");
        type_field(&mut h, "Radius", "2 mm", "0.125 in");
        h.step(&json!({"widget": "OK"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "OK closes Fillet on {e:?}");
        clicks_expire(&mut h);
    }
    assert_eq!(count(&h, |k| matches!(k, FeatureKind::Fillet { .. })), 2);

    let sketch_name = name_of(&h, |k| matches!(k, FeatureKind::Sketch { .. }));
    clicks_expire(&mut h);
    h.step(&json!({"click": {"browser": sketch_name.as_str()}, "double": true})).unwrap();
    h.settle();
    assert_eq!(h.app.session.active_sketch, Some(sketch), "the double-click edits {sketch_name}");
    clicks_expire(&mut h);
    h.step(&json!({"click": {"dimension": width.as_str()}, "double": true})).unwrap();
    assert!(crate::dim_view::editing(), "a double-click opens the width");
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": "3"})).unwrap();
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.doc.param(&width).unwrap().expr.as_str(), "3 in");
    h.step(&json!({"start": "sketch.finish"})).unwrap();
    h.settle();
    assert!(h.app.session.active_sketch.is_none());
    assert_eq!(h.app.session.world_state().bodies.len(), 1);
    the_finished_part(&mut h);
}
