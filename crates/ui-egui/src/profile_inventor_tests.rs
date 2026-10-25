#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use serde_json::{Value, json};
use solidcraft_engine::doc::DocumentKind::{self, *};

use crate::prefs::NavigatorStyle;
use crate::scenario::Harness;
use crate::workspace::CONSTRAINT_GALLERY;

fn inventor() -> &'static Profile {
    get(INVENTOR).expect("the Inventor profile is built in")
}

fn tabs(kind: DocumentKind) -> Vec<(&'static str, Shown)> {
    inventor().ribbon(kind).tabs.iter().map(|t| (t.id.as_str(), t.shown)).collect()
}

fn panels(kind: DocumentKind, tab: &str) -> Vec<&'static str> {
    inventor().ribbon(kind).tab(tab).unwrap().panels.iter().map(|p| p.id.as_str()).collect()
}

/// The panel's face as the ribbon draws it: (command, small?), in order.
fn face(kind: DocumentKind, tab: &str, panel: &str) -> Vec<(&'static str, bool)> {
    let p = inventor().ribbon(kind).tab(tab).unwrap().panel(panel).unwrap();
    p.commands.iter().take(p.promoted).map(|c| (c.as_str(), p.is_small(c))).collect()
}

fn large(kind: DocumentKind, tab: &str, panel: &str) -> Vec<&'static str> {
    face(kind, tab, panel).into_iter().filter(|(_, small)| !small).map(|(c, _)| c).collect()
}

fn hidden(kind: DocumentKind, tab: &str) -> Vec<&'static str> {
    inventor().ribbon(kind).tab(tab).unwrap().panels.iter().filter(|p| p.hidden).map(|p| p.id.as_str()).collect()
}

fn commands(kind: DocumentKind, tab: &str, panel: &str) -> Vec<&'static str> {
    inventor().ribbon(kind).tab(tab).unwrap().panel(panel).unwrap().commands.iter().map(String::as_str).collect()
}

const L: bool = false;
const S: bool = true;

/// `(tab, panel, face)` of the Part ribbon; `S` marks a small button. CONSTRAIN is built in code.
const PART_FACES: &[(&str, &str, &[(&str, bool)])] = &[
    ("MODEL", "SKETCH", &[("sketch.create", L)]),
    ("MODEL", "PRIMITIVES", &[("solid.box", L)]),
    (
        "MODEL",
        "CREATE",
        &[
            ("solid.extrude", L),
            ("solid.revolve", L),
            ("solid.sweep", S),
            ("solid.loft", S),
            ("solid.coil", S),
            ("solid.emboss", S),
            ("solid.rib", S),
            ("canvas.decal", S),
        ],
    ),
    (
        "MODEL",
        "MODIFY",
        &[
            ("solid.hole", L),
            ("solid.fillet", L),
            ("solid.chamfer", S),
            ("solid.shell", S),
            ("solid.draft", S),
            ("solid.thread", S),
            ("solid.combine", S),
            ("surface.thicken", S),
            ("solid.split_body", S),
            ("solid.press_pull", S),
            ("face.delete", S),
        ],
    ),
    ("MODEL", "WORK FEATURES", &[("construct.plane.offset", L), ("construct.axis.edge", S), ("construct.point.vertex", S)]),
    ("MODEL", "PATTERN", &[("solid.pattern.rectangular", S), ("solid.pattern.circular", S), ("solid.mirror", S)]),
    (
        "MODEL",
        "SURFACE",
        &[
            ("surface.stitch", S),
            ("surface.patch", S),
            ("solid.boundary_fill", S),
            ("surface.ruled", S),
            ("surface.trim", S),
            ("surface.extend", S),
            ("solid.replace_face", S),
            ("mesh.convert", S),
        ],
    ),
    ("MODEL", "PARAMETERS", &[("parameters.change", L)]),
    ("MODEL", "PLASTIC PART", &[("plastic.boss", S), ("plastic.rest", S), ("plastic.snap_fit", S), ("plastic.lip", S)]),
    ("MODEL", "MEASURE", &[("inspect.measure", L)]),
    ("MODEL", "INSERT", &[("file.insert_step", S)]),
    ("MODEL", "CONVERT", &[("sheet.convert", L)]),
    ("SKETCH", "SKETCH", &[("sketch.create", L)]),
    (
        "SKETCH",
        "CREATE",
        &[
            ("sketch.line", L),
            ("sketch.circle.center", L),
            ("sketch.arc.three_point", L),
            ("sketch.rectangle.two_point", L),
            ("sketch.fillet", S),
            ("sketch.text", S),
            ("sketch.point", S),
            ("sketch.project", L),
        ],
    ),
    (
        "SKETCH",
        "MODIFY",
        &[
            ("sketch.move", S),
            ("sketch.copy", S),
            ("sketch.trim", S),
            ("sketch.extend", S),
            ("sketch.break", S),
            ("sketch.scale", S),
            ("sketch.offset", S),
        ],
    ),
    ("SKETCH", "PATTERN", &[("sketch.pattern.rectangular", S), ("sketch.pattern.circular", S), ("sketch.mirror", S)]),
    ("SKETCH", "INSERT", &[("canvas.insert", S), ("sketch.insert_dxf", S)]),
    ("SKETCH", "FORMAT", &[("sketch.construction", S), ("sketch.centerline", S)]),
    ("SKETCH", "FINISH SKETCH", &[("sketch.finish", L)]),
    ("SHEET METAL", "CREATE", &[("sheet.flange", L), ("sheet.hem", S), ("sheet.fold", S)]),
    ("SHEET METAL", "MODIFY", &[("sheet.perforate", L), ("sheet.unfold", S), ("sheet.refold", S), ("solid.hole", S)]),
    ("SHEET METAL", "SETUP", &[("sheet.manage_rules", L)]),
    ("SHEET METAL", "FLAT PATTERN", &[("sheet.flat_pattern", L)]),
    ("INSPECT", "MEASURE", &[("inspect.measure", L), ("inspect.center_of_mass", L)]),
    (
        "INSPECT",
        "ANALYZE",
        &[("inspect.zebra", L), ("inspect.draft", L), ("inspect.curvature_map", L), ("inspect.section", L), ("inspect.curvature_comb", L)],
    ),
    ("TOOLS", "MEASURE", &[("inspect.measure", L)]),
    ("TOOLS", "MATERIAL AND APPEARANCE", &[("material.assign", L), ("appearance.assign", L)]),
    ("TOOLS", "OPTIONS", &[("document.units", L)]),
    ("TOOLS", "CLIPBOARD", &[("timeline.copy", L), ("timeline.paste", L)]),
    ("TOOLS", "FIND", &[("timeline.dependents", S), ("document.inspect", S)]),
    ("MANAGE", "UPDATE", &[("timeline.compute_all", L)]),
    ("MANAGE", "PARAMETERS", &[("parameters.change", L)]),
    ("MANAGE", "STYLES AND STANDARDS", &[("sheet.manage_rules", L)]),
    ("MANAGE", "INSERT", &[("file.insert_step", S)]),
    ("MANAGE", "AUTHOR", &[("config.table", L), ("config.configure", S)]),
    ("VIEW", "VISIBILITY", &[("inspect.section", S)]),
    ("VIEW", "APPEARANCE", &[("view.graphics_preset", S), ("view.ambient_occlusion", S)]),
    ("VIEW", "WINDOWS", &[("view.layout", S), ("view.history_layout", S)]),
    ("VIEW", "NAVIGATE", &[("view.normal_to", S), ("view.isometric", S)]),
    ("ENVIRONMENTS", "CONVERT", &[("sheet.convert", L)]),
];

#[test]
fn inventor_part_ribbon_follows_the_capture() {
    use Shown::*;
    assert_eq!(
        tabs(Part),
        [
            ("SHEET METAL", Always),
            ("MODEL", Always),
            ("SKETCH", Always),
            ("INSPECT", Always),
            ("TOOLS", Always),
            ("MANAGE", Always),
            ("VIEW", Always),
            ("ENVIRONMENTS", Always),
            ("RENDER", Render),
        ]
    );
    assert_eq!(inventor().ribbon(Part).home(), "MODEL");
}

#[test]
fn inventor_part_panels_follow_the_capture() {
    let want: &[(&str, &[&str])] = &[
        (
            "MODEL",
            &[
                "SKETCH",
                "PRIMITIVES",
                "CREATE",
                "MODIFY",
                "WORK FEATURES",
                "PATTERN",
                "SURFACE",
                "PARAMETERS",
                "PLASTIC PART",
                "MEASURE",
                "INSERT",
                "CONVERT",
            ],
        ),
        ("SKETCH", &["SKETCH", "CREATE", "MODIFY", "PATTERN", "CONSTRAIN", "INSERT", "FORMAT", "PARAMETERS", "FINISH SKETCH"]),
        ("SHEET METAL", &["SKETCH", "CREATE", "MODIFY", "WORK FEATURES", "PATTERN", "SETUP", "FLAT PATTERN", "PARAMETERS", "MEASURE"]),
        ("INSPECT", &["MEASURE", "ANALYZE", "PARAMETERS"]),
        ("TOOLS", &["MEASURE", "MATERIAL AND APPEARANCE", "OPTIONS", "CLIPBOARD", "FIND"]),
        ("MANAGE", &["UPDATE", "PARAMETERS", "STYLES AND STANDARDS", "INSERT", "AUTHOR"]),
        ("VIEW", &["VISIBILITY", "APPEARANCE", "WINDOWS", "NAVIGATE"]),
        ("ENVIRONMENTS", &["CONVERT"]),
    ];
    for (tab, ids) in want {
        assert_eq!(panels(Part, tab), *ids, "{tab}");
    }
}

#[test]
fn inventor_part_faces_follow_the_capture() {
    for (tab, panel, want) in PART_FACES {
        assert_eq!(face(Part, tab, panel), *want, "{tab}/{panel}");
    }
    let constrain: Vec<(&str, bool)> = std::iter::once("sketch.dimension").chain(CONSTRAINT_GALLERY.iter().copied()).map(|c| (c, false)).collect();
    assert_eq!(face(Part, "SKETCH", "CONSTRAIN"), constrain);
    assert!(inventor().ribbon(Part).tab("SKETCH").unwrap().panel("CONSTRAIN").unwrap().small.is_empty());
    assert_eq!(large(Part, "MODEL", "MODIFY"), ["solid.hole", "solid.fillet"]);
    // No panel of any Inventor ribbon draws a blank face, and every Part panel outside the
    // inherited RENDER tab is pinned by the table.
    for (kind, ribbon) in inventor().ribbons() {
        for tab in &ribbon.tabs {
            for p in &tab.panels {
                assert!(!face(kind, &tab.id, &p.id).is_empty(), "{kind:?} {}/{} draws a blank face", tab.id, p.id);
            }
        }
    }
    for tab in inventor().ribbon(Part).tabs.iter().filter(|t| t.id != "RENDER") {
        for p in &tab.panels {
            // A panel shared between tabs (WORK FEATURES on SHEET METAL, PARAMETERS on INSPECT…)
            // has a row under another tab with the same face.
            let got = face(Part, &tab.id, &p.id);
            let listed = PART_FACES.iter().any(|(t, q, f)| *q == p.id && (*t == tab.id || got == *f)) || (tab.id == "SKETCH" && p.id == "CONSTRAIN");
            assert!(listed, "Part {}/{} has no face row", tab.id, p.id);
        }
    }
}

#[test]
fn surface_commands_live_on_the_3d_model_surface_panel() {
    let surface_ids: Vec<&str> = command_specs().iter().map(|c| c.id).filter(|id| id.starts_with("surface.")).collect();
    assert_eq!(surface_ids.len(), 19);
    let panel = commands(Part, "MODEL", "SURFACE");
    let modify = commands(Part, "MODEL", "MODIFY");
    let in_surface: Vec<&str> = surface_ids.iter().copied().filter(|id| panel.contains(id)).collect();
    assert_eq!(in_surface.len(), 18, "{in_surface:?}");
    for id in &surface_ids {
        if *id == "surface.thicken" {
            assert!(modify.contains(id) && !panel.contains(id));
        } else {
            assert!(panel.contains(id), "{id} is not on MODEL/SURFACE");
        }
    }
    for id in ["solid.boundary_fill", "solid.replace_face", "mesh.convert"] {
        assert!(panel.contains(&id), "{id}");
    }
    assert_eq!(panel.len(), 21);
    for (kind, ribbon) in inventor().ribbons() {
        for tab in &ribbon.tabs {
            assert!(tab.id != "SURFACE" && tab.id != "INSERT", "{kind:?} has a {} tab", tab.id);
        }
    }
}

#[test]
fn sheet_metal_comes_first_on_every_part() {
    assert_eq!(tabs(Part)[0], ("SHEET METAL", Shown::Always));
    let ribbon = inventor().ribbon(Part);
    assert_eq!(ribbon.home(), "MODEL");
    let flat = commands(Part, "SHEET METAL", "FLAT PATTERN");
    for id in ["sheet.flat_pattern", "sheet.export_dxf", "sheet.export_bend_table"] {
        assert!(flat.contains(&id), "{id}: {flat:?}");
    }
    assert!(commands(Part, "MODEL", "CONVERT").contains(&"sheet.convert"));
    assert!(commands(Part, "ENVIRONMENTS", "CONVERT").contains(&"sheet.convert"));
    assert_eq!(ribbon.destination("solid.hole"), Some(("MODEL", "MODIFY")));
}

#[test]
fn inventor_keeps_the_shell_conventions() {
    let sketch = inventor().ribbon(Part).tab("SKETCH").unwrap();
    let finish = sketch.panel("FINISH SKETCH").unwrap();
    assert_eq!(finish.shown, Shown::Sketch);
    assert_eq!(finish.label, None);
    assert!(finish.commands.iter().any(|c| c == "sketch.finish"));
    let constrain = commands(Part, "SKETCH", "CONSTRAIN");
    for id in CONSTRAINT_GALLERY {
        assert!(constrain.contains(id), "{id}");
    }
    assert!(!panels(Part, "VIEW").contains(&"SELECT"));
}

#[test]
fn a_drawing_shows_the_placeholder_ribbon_data() {
    assert_eq!(tabs(Drawing), [("TOOLS", Shown::Always), ("VIEW", Shown::Always)]);
}

#[test]
fn inventor_defaults_follow_inventor() {
    let d = &inventor().defaults;
    assert_eq!(d.nav.as_deref(), Some("inventor"));
    assert_eq!(d.navigator, Some(NavigatorStyle::Cube));
    assert_eq!(d.history, Some(History::UnderBrowser));
}

#[test]
fn every_profile_label_is_in_the_english_catalog() {
    let en: BTreeMap<String, String> = serde_json::from_str(include_str!("../../../assets/locales/en.json")).unwrap();
    let values: BTreeSet<&str> = en.values().map(String::as_str).collect();
    let mut missing = Vec::new();
    for p in all() {
        let mut key = |what: String, id: &str| {
            if !en.contains_key(id) {
                missing.push(format!("{}: {what} {id}", p.id));
            }
        };
        key("name".into(), &p.name);
        for (kind, ribbon) in p.ribbons() {
            for tab in &ribbon.tabs {
                if let Some(id) = &tab.label {
                    key(format!("{kind:?} tab {}", tab.id), id);
                }
                for panel in &tab.panels {
                    if let Some(id) = &panel.label {
                        key(format!("{kind:?} panel {}/{}", tab.id, panel.id), id);
                    }
                }
            }
        }
        for (folder, id) in &p.browser.folder_labels {
            key(format!("folder {folder}"), id);
        }
        for o in &p.browser.origin {
            if let Some(id) = &o.label {
                key(format!("origin {}", o.key), id);
            }
        }
        for (command, text) in &p.button_labels {
            if !values.contains(text.as_str()) {
                missing.push(format!("{}: button label for {command} \"{text}\" is no English value", p.id));
            }
        }
    }
    assert!(missing.is_empty(), "{missing:#?}");
}

#[test]
fn hidden_panels_follow_the_capture() {
    assert_eq!(hidden(Part, "MODEL"), ["PRIMITIVES", "PARAMETERS", "PLASTIC PART", "MEASURE", "INSERT"]);
    assert_eq!(hidden(Part, "SKETCH"), ["PARAMETERS"]);
    assert_eq!(hidden(Part, "SHEET METAL"), ["PARAMETERS", "MEASURE"]);
    assert_eq!(hidden(Part, "INSPECT"), ["PARAMETERS"]);
    for tab in ["TOOLS", "MANAGE", "VIEW", "ENVIRONMENTS"] {
        assert!(hidden(Part, tab).is_empty(), "{tab}");
    }
    assert!(inventor().quick_access);
}

#[test]
fn clipboard_buttons_use_the_edit_clipboard() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    let rect = h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]})).unwrap();
    let ids: Vec<&str> = rect["curves"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(ids.len(), 4);
    let items: Vec<Value> = ids.iter().map(|id| json!({"type": "sketch_curve", "id": id})).collect();
    h.app.run("select.set", json!({ "items": items })).unwrap();
    let count = |h: &Harness| h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap().curves.len();
    assert_eq!(count(&h), 4);

    h.app.start("timeline.copy");
    assert!(h.app.session.sketch_clipboard.is_some());
    h.app.start("timeline.paste");
    assert_eq!(count(&h), 8);
}
