//! Headless test fixtures. Callers retain frame counts, input and command parameters.
use crate::{Services, SolveApp, scenario::Harness};
use egui::{Event, FullOutput, RawInput, Shape, Ui};
use serde_json::{Value, json};
use solidcraft_engine::{Sel, Session};

pub(crate) fn app_with_script(script: Value) -> SolveApp {
    let mut session = Session::default();
    session.run_script(&script).unwrap();
    SolveApp::new(session, Services::default())
}

/// A harness on the Inventor profile with the active document of `kind`.
pub(crate) fn inventor(kind: solidcraft_engine::doc::DocumentKind) -> Harness {
    let mut h = Harness::new();
    assert_eq!(h.call("ui.prefs", json!({"ui_profile": "inventor"}))["ok"], json!(true));
    h.app.session.doc_mut().kind = kind;
    // The kind is how the design was created, not an edit: a blank design stays blank.
    h.app.session.mark_saved();
    h.frames(2);
    h
}

/// The drawing tests' block (D-1's fixture, `engine/src/cmd/drawing_fixture.rs`): 4 x 3 x 1 in
/// with a drilled hole and four rounded corners, saved as `<dir>/block.tcipart`.
pub(crate) fn block_part(dir: &std::path::Path) -> String {
    let path = dir.join("block.tcipart").to_string_lossy().into_owned();
    let mut s = Session::default();
    let mut run = |id: &str, p: Value| {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    };
    run("document.new_part", json!({}));
    run("document.units", json!({"units": "in"}));
    run("solid.box", json!({"length": "4 in", "width": "1 in", "height": "3 in", "corner": [-50.8, 0, 0], "name": "Block", "body_name": "Block"}));
    run("solid.hole", json!({"position": [0, 0, 38.1], "direction": [0, 1, 0], "diameter": "0.75 in", "name": "Drill"}));
    run("solid.fillet", json!({"edges": [[-50.8, 12.7, 0], [50.8, 12.7, 0], [-50.8, 12.7, 76.2], [50.8, 12.7, 76.2]], "radius": "0.5 in"}));
    run("file.save", json!({"path": path}));
    path
}

/// Type an alias as a user does: each letter pressed and released, then wait out the
/// profile's prefix wait.
pub(crate) fn alias(h: &mut Harness, keys: &str) {
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
pub(crate) fn frames_for(seconds: f64) -> usize {
    (seconds * 60.0).ceil() as usize + 1
}

/// E-1's `blocks()` built through `h.app.run`, in millimetres, laid out for picking from the default view:
///   document.units {"units": "mm"};
///   "A" grounded (the first placement, R103/R123): solid.box {100, 50, 20, corner [0, 0, 0]}
///        (top z = 20, pick [50, 25, 20]; right face x = 100, pick [100, 25, 10]);
///   "B": solid.box {100, 50, 20, corner [0, 70, 7]}  (top z = 27, 7 mm above A's, pick [50, 95, 27];
///        top corner vertex [100, 120, 27]; top front edge along X at [50, 70, 27]);
///   "Plate": solid.box {80, 80, 10, corner [150, -40, 0]} + cut solid.cylinder {diameter 20, height 12, base [190, 0, -1]}
///        (top pick [160, 30, 10]; hole top rim pick [200, 0, 10]; hole wall pick [190, -10, 5]);
///   "Pin": solid.cylinder {diameter 20, height 60, base [300, 0, 50]}
///        (top face pick [300, 5, 110]; top rim pick [310, 0, 110]; wall pick [310, 0, 80]);
///   then the `ui.home` view and `h.settle()`.
pub(crate) fn constraint_blocks(h: &mut Harness) {
    let mut run = |id: &str, p: Value| {
        if let Err(e) = h.app.run(id, p.clone()) {
            panic!("{id} {p}: {e}");
        }
    };
    run("document.units", json!({"units": "mm"}));
    let parts: [(&str, bool, Vec<(&str, Value)>); 4] = [
        ("A", true, vec![("solid.box", json!({"length": 100, "width": 50, "height": 20, "corner": [0, 0, 0]}))]),
        ("B", false, vec![("solid.box", json!({"length": 100, "width": 50, "height": 20, "corner": [0, 70, 7]}))]),
        (
            "Plate",
            false,
            vec![
                ("solid.box", json!({"length": 80, "width": 80, "height": 10, "corner": [150, -40, 0], "name": "Plate"})),
                ("solid.cylinder", json!({"diameter": 20, "height": 12, "base": [190, 0, -1], "operation": "cut", "name": "Hole"})),
            ],
        ),
        ("Pin", false, vec![("solid.cylinder", json!({"diameter": 20, "height": 60, "base": [300, 0, 50], "name": "PinBody"}))]),
    ];
    for (name, grounded, steps) in parts {
        run("component.create", json!({"name": name, "grounded": grounded}));
        for (id, p) in steps {
            run(id, p);
        }
        run("component.activate", json!({"component": "root"}));
    }
    assert_eq!(h.call("ui.view", json!({"view": "home"}))["ok"], json!(true));
    h.settle();
}

/// Click a world point (`{"click": {"world": [x, y, z]}}`), then 2 frames.
pub(crate) fn click_world(h: &mut Harness, p: [f64; 3]) {
    h.step(&json!({"click": {"world": p}})).unwrap();
    h.frames(2);
}

/// The constraint's id by name (`h.app.session.doc.assembly.constraint(name)`, `doc/src/constraints.rs:405`).
pub(crate) fn constraint_id(h: &Harness, name: &str) -> u64 {
    h.app.session.doc.assembly.constraint(name).map(|c| c.id).unwrap_or_else(|| panic!("no constraint {name}"))
}

/// Let the last clicks expire (egui's double-click delay), so the next click is a new one.
pub(crate) fn clicks_expire(h: &mut Harness) {
    let delay = h.ctx.options(|o| o.input_options.max_double_click_delay);
    h.frames(frames_for(delay));
}

/// A right-click at `at`: under Inventor the marking menu opens there.
pub(crate) fn right_click(h: &mut Harness, at: Value) {
    h.step(&json!({"click": at, "button": "right"})).unwrap();
    h.frames(2);
}

/// Click the drawn menu entry `id`: a ring pill (`marking:`) or a list row (`menu:`).
pub(crate) fn menu_pick(h: &mut Harness, prefix: &str, id: &str) {
    let handle = format!("{prefix}:{id}");
    assert!(
        crate::scenario::handle_at(&handle).is_some(),
        "{handle} is not drawn: {:?} {}",
        crate::scenario::handles(prefix),
        crate::context_menu::describe(&h.app)
    );
    h.step(&json!({"click": {"handle": handle}})).unwrap();
    h.frames(3);
}

pub(crate) fn sample_app() -> SolveApp {
    app_with_script(solidcraft_engine::sample::script())
}

pub(crate) fn run_all(app: &mut SolveApp, commands: Vec<(String, Value)>) {
    for (id, params) in commands {
        app.run(&id, params).unwrap();
    }
}

pub(crate) fn play(h: &mut Harness) {
    while !h.app.synthetic.is_empty() {
        h.frame();
    }
    h.frames(2);
}

pub(crate) fn click(h: &mut Harness, handle: &str) {
    assert!(crate::scenario::handle_at(handle).is_some(), "{handle} is not drawn: {:?}", crate::scenario::handles(""));
    h.step(&json!({"click": {"handle": handle}})).unwrap();
    h.frames(3);
}

/// One component named Block, with its original occurrence free and the root active.
pub(crate) fn component_box() -> (Harness, u64, u64) {
    let mut h = Harness::new();
    let part = h.app.run("component.create", json!({"name":"Block","grounded":false})).unwrap();
    let component = part["component"].as_u64().unwrap();
    let occurrence = part["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    (h, component, occurrence)
}

/// Named 20 × 20 × 10 boxes with A grounded; preserve the caller's command entry point.
pub(crate) fn grounded_boxes(parts: &[(&str, i32)], mut run: impl FnMut(&str, Value) -> Value) {
    for (name, x) in parts {
        run("component.activate", json!({"component":"root"}));
        run("component.create", json!({"name":name}));
        run("solid.box", json!({"length":20,"width":20,"height":10,"corner":[x,0,0]}));
    }
    run("component.activate", json!({"component":"root"}));
    run("occurrence.ground", json!({"occurrence":"A:1","grounded":true}));
}

/// Text with its first outline selected; callers decide when to start tools and draw.
pub(crate) fn selected_text(text: &str, at: [i32; 2]) -> (Harness, String, String) {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let result = h.app.run("sketch.text", json!({"text":text,"at":at,"height":10})).unwrap();
    let link = result["link"].as_str().unwrap().to_owned();
    let curve = result["curves"][0].as_str().unwrap().to_owned();
    h.app.session.selection = vec![Sel::SketchCurve { id: curve.clone() }];
    (h, link, curve)
}

/// A frame without a renderer. Preserve all caller-supplied timing and viewport options.
pub(crate) fn frame(ctx: &egui::Context, input: RawInput, add: impl FnMut(&mut Ui)) -> FullOutput {
    let mut output = ctx.run_ui(input, add);
    output.textures_delta.clear();
    output
}

pub(crate) fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<Event> {
    vec![Event::PointerMoved(pos), Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }]
}

pub(crate) fn key(key: egui::Key) -> Vec<Event> {
    vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }]
}

/// Top-level text shapes only, in their original paint order.
pub(crate) fn text_shapes(output: &FullOutput) -> impl Iterator<Item = &egui::epaint::TextShape> {
    output.shapes.iter().filter_map(|clipped| match &clipped.shape {
        Shape::Text(text) => Some(text),
        _ => None,
    })
}

/// Every leaf of a shape list, in paint order.
pub(crate) fn each_shape(shape: &Shape, visit: &mut impl FnMut(&Shape)) {
    match shape {
        Shape::Vec(shapes) => shapes.iter().for_each(|shape| each_shape(shape, visit)),
        shape => visit(shape),
    }
}
