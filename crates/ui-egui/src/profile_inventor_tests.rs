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

/// `(tab, panel, face)` of the Part ribbon; `S` marks a small button.
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
    // The capture's order: Dimension large and alone, then a small column standing for Auto
    // Dimension / Show Constraints, then Constraint Settings itself (R107), then the grid. `horizontal_vertical`
    // stands in for both Horizontal and Vertical, at Horizontal's place; midpoint and polygon
    // have no capture slot and close the grid.
    (
        "SKETCH",
        "CONSTRAIN",
        &[
            ("sketch.dimension", L),
            ("sketch.auto_constrain", S),
            ("sketch.auto_constrain_from_datum", S),
            ("sketch.constrainer", S),
            ("sketch.constraint_settings", S),
            ("sketch.constraint.coincident", S),
            ("sketch.constraint.parallel", S),
            ("sketch.constraint.tangent", S),
            ("sketch.constraint.collinear", S),
            ("sketch.constraint.perpendicular", S),
            ("sketch.constraint.curvature", S),
            ("sketch.constraint.concentric", S),
            ("sketch.constraint.horizontal_vertical", S),
            ("sketch.constraint.symmetry", S),
            ("sketch.constraint.fix", S),
            ("sketch.constraint.equal", S),
            ("sketch.constraint.midpoint", S),
            ("sketch.constraint.polygon", S),
        ],
    ),
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
    ("ENVIRONMENTS", "BEGIN", &[("render.start", L)]),
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
        ("ENVIRONMENTS", &["BEGIN", "CONVERT"]),
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
    // Every constraint command the registry has is on the CONSTRAIN face; none waits in overflow.
    let constrain = face(Part, "SKETCH", "CONSTRAIN");
    for c in command_specs().iter().map(|c| c.id).filter(|id| id.starts_with("sketch.constraint.")) {
        assert!(constrain.contains(&(c, S)), "{c} is not a small button on SKETCH/CONSTRAIN");
    }
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
            let listed = PART_FACES.iter().any(|(t, q, f)| *q == p.id && (*t == tab.id || got == *f));
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

/// Updated by design (R70): the drawing ribbon follows the capture's tabs, with no RENDER tab.
#[test]
fn the_drawing_ribbon_data_follows_the_capture() {
    use Shown::*;
    assert_eq!(tabs(Drawing), [("PLACE VIEWS", Always), ("ANNOTATE", Always), ("TOOLS", Always), ("MANAGE", Always), ("VIEW", Always)]);
}

#[test]
fn inventor_defaults_follow_inventor() {
    let d = &inventor().defaults;
    assert_eq!(d.nav.as_deref(), Some("inventor"));
    assert_eq!(d.navigator, Some(NavigatorStyle::Cube));
    assert_eq!(d.history, Some(History::Browser));
    assert_eq!(d.units.as_deref(), Some("in"));
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

/// `(tab, panel, face)` of the Assembly ribbon; `S` marks a small button.
const ASSEMBLY_FACES: &[(&str, &str, &[(&str, bool)])] = &[
    ("ASSEMBLE", "COMPONENT", &[("component.insert_design", L), ("component.create", L)]),
    ("ASSEMBLE", "POSITION", &[("solid.move", L)]),
    ("ASSEMBLE", "RELATIONSHIPS", &[("joint.create", L), ("joint.as_built", S)]),
    ("ASSEMBLE", "MANAGE", &[("assembly.bom", L), ("parameters.change", L)]),
    ("DESIGN", "FASTEN", &[("parts.fastener", L)]),
    ("DESIGN", "FRAME", &[("frame.member", L)]),
    // The capture's Slot has no command; its Move Face is `solid.press_pull`.
    (
        "MODEL",
        "MODIFY ASSEMBLY",
        &[
            ("solid.extrude", L),
            ("solid.revolve", L),
            ("solid.hole", L),
            ("solid.fillet", L),
            ("solid.chamfer", S),
            ("solid.sweep", S),
            ("solid.press_pull", S),
            ("solid.thread", S),
        ],
    ),
    ("SKETCH", "MEASURE", &[("inspect.measure", L)]),
    ("INSPECT", "INTERFERENCE", &[("inspect.interference", L), ("contact.enable_sets", L)]),
    ("INSPECT", "MEASURE", &[("inspect.measure", L), ("inspect.center_of_mass", L)]),
    ("MANAGE", "UPDATE", &[("timeline.compute_all", L)]),
    ("ENVIRONMENTS", "BEGIN", &[("render.start", L), ("motion.study", L)]),
    ("ENVIRONMENTS", "3D PRINT", &[("file.save_mesh", L)]),
];

/// Every command the Inventor Part, Assembly and Drawing ribbons place.
fn inventor_commands() -> BTreeSet<&'static str> {
    let mut inv = inventor().ribbon(Part).commands();
    inv.extend(inventor().ribbon(Assembly).commands());
    inv.extend(inventor().ribbon(Drawing).commands());
    inv
}

/// Registry commands no Inventor ribbon places, from `docs/features.md` at
/// 9bb4d408 (466 commands, 305 placed). The count may only fall, except by a plan's new
/// commands: Plan D-1's 29 drawing commands (161 to 190) and Plan D-2 Task 0's
/// `drawing.note.edit` (191; R74), which the Drawing ribbon does not place. Plan D-2 Task 1's
/// Drawing ribbon places ten drawing commands (191 - 10 = 181). Plan E-1 Task 0 measured the
/// base N = 182 (R121). Plan E-1 Task 4's six constraint commands (+6): 182 + 6 = 188 (E-2 Task 1
/// places them). Plan E-1 Task 5's three show commands (+3): 188 + 3 = 191 (N + 9). Plan E-1
/// Task 8's `constraint.drive` (+1): 191 + 1 = 192 (N + 10).
const INVENTOR_UNPLACED: usize = 192;

#[test]
fn inventor_assembly_ribbon_follows_the_capture() {
    use Shown::*;
    assert_eq!(
        tabs(Assembly),
        [
            ("ASSEMBLE", Always),
            ("DESIGN", Always),
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
    assert_eq!(inventor().ribbon(Assembly).home(), "ASSEMBLE");
    assert!(inventor().ribbon(Assembly).tab("SHEET METAL").is_none());
}

#[test]
fn inventor_assembly_panels_follow_the_capture() {
    let want: &[(&str, &[&str])] = &[
        ("ASSEMBLE", &["COMPONENT", "POSITION", "RELATIONSHIPS", "MANAGE", "WORK FEATURES", "MEASURE"]),
        ("DESIGN", &["FASTEN", "FRAME", "MEASURE"]),
        ("MODEL", &["SKETCH", "MODIFY ASSEMBLY", "WORK FEATURES", "PATTERN", "PARAMETERS", "MEASURE"]),
        ("SKETCH", &["SKETCH", "CREATE", "MODIFY", "PATTERN", "CONSTRAIN", "INSERT", "FORMAT", "MEASURE", "FINISH SKETCH"]),
        ("INSPECT", &["INTERFERENCE", "ANALYZE", "MEASURE"]),
        ("TOOLS", &["MEASURE", "MATERIAL AND APPEARANCE", "OPTIONS", "CLIPBOARD", "FIND"]),
        ("MANAGE", &["UPDATE", "MANAGE", "STYLES AND STANDARDS", "AUTHOR"]),
        ("VIEW", &["VISIBILITY", "APPEARANCE", "WINDOWS", "NAVIGATE"]),
        ("ENVIRONMENTS", &["BEGIN", "3D PRINT"]),
    ];
    for (tab, ids) in want {
        assert_eq!(panels(Assembly, tab), *ids, "{tab}");
    }
}

#[test]
fn inventor_assembly_faces_follow_the_capture() {
    for (tab, panel, want) in ASSEMBLY_FACES {
        assert_eq!(face(Assembly, tab, panel), *want, "{tab}/{panel}");
    }
    // Joint is the only large button on RELATIONSHIPS, and no stand-in Constrain is placed.
    assert_eq!(large(Assembly, "ASSEMBLE", "RELATIONSHIPS"), ["joint.create"]);
    let relationships = commands(Assembly, "ASSEMBLE", "RELATIONSHIPS");
    assert!(relationships.iter().all(|c| !c.contains("constrain")), "{relationships:?}");
    assert_eq!(large(Assembly, "MODEL", "MODIFY ASSEMBLY"), ["solid.extrude", "solid.revolve", "solid.hole", "solid.fillet"]);
    // Every Assembly panel outside the inherited RENDER tab has a row here, or shares the face
    // of a panel with the same id pinned by either table.
    for tab in inventor().ribbon(Assembly).tabs.iter().filter(|t| t.id != "RENDER") {
        for p in &tab.panels {
            let got = face(Assembly, &tab.id, &p.id);
            let listed = PART_FACES.iter().chain(ASSEMBLY_FACES).any(|(_, q, f)| *q == p.id && got == *f);
            assert!(listed, "Assembly {}/{} has no face row", tab.id, p.id);
        }
    }
    // C3: hidden by default where the capture hides them.
    assert_eq!(hidden(Assembly, "ASSEMBLE"), ["MEASURE"]);
    assert_eq!(hidden(Assembly, "DESIGN"), ["MEASURE"]);
    assert_eq!(hidden(Assembly, "MODEL"), ["MEASURE"]);
    assert!(hidden(Assembly, "SKETCH").is_empty());
    for tab in ["INSPECT", "TOOLS", "MANAGE", "VIEW", "ENVIRONMENTS"] {
        assert!(hidden(Assembly, tab).is_empty(), "{tab}");
    }
}

#[test]
fn frame_member_lives_on_design_frame() {
    assert_eq!(inventor().ribbon(Assembly).destination("frame.member"), Some(("DESIGN", "FRAME")));
    assert!(!inventor().ribbon(Part).commands().contains("frame.member"));
}

#[test]
fn link_commands_live_only_on_the_assembly_ribbon() {
    let update = commands(Assembly, "MANAGE", "UPDATE");
    let part = inventor().ribbon(Part).commands();
    for id in ["component.update_links", "component.break_link"] {
        assert!(update.contains(&id), "{id}: {update:?}");
        assert!(!part.contains(id), "{id} is on a Part tab");
    }
}

#[test]
fn inventor_reaches_everything_the_solidcraft_ribbon_reached() {
    let sc = get(SOLIDCRAFT).unwrap().ribbon(Assembly).commands();
    let inv = inventor_commands();
    let lost: BTreeSet<&str> = sc.difference(&inv).copied().collect();
    let omitted: BTreeSet<&str> = inventor().omitted.keys().map(String::as_str).collect();
    assert_eq!(lost, omitted);
    assert_eq!(omitted, BTreeSet::from(["timeline.delete", "view.list", "view.navigate"]));
}

#[test]
fn inventor_unplaced_commands_do_not_grow() {
    let inv = inventor_commands();
    let unplaced = command_specs().iter().filter(|c| !inv.contains(c.id)).count();
    println!("Inventor leaves {unplaced} registry commands unplaced (ratchet {INVENTOR_UNPLACED})");
    assert!(unplaced <= INVENTOR_UNPLACED, "{unplaced} unplaced commands; the ratchet is {INVENTOR_UNPLACED}");
}

#[test]
fn only_the_assembly_offers_component_placement() {
    assert!(commands(Assembly, "ASSEMBLE", "COMPONENT").contains(&"component.insert_design"));
    assert!(!inventor().ribbon(Part).commands().contains("component.insert_design"));
}

/// Carried from Task 4's review: every Inventor modelling ribbon carries SolidCraft's contextual
/// RENDER tab, so activating Render in a part or assembly never finds an empty panel list.
/// Updated by design (R70): a drawing has no render workspace, so its ribbon has no RENDER tab.
#[test]
fn every_inventor_ribbon_carries_the_render_tab() {
    assert!(inventor().ribbon(Drawing).tab("RENDER").is_none());
    for kind in [Part, Assembly] {
        let render = inventor().ribbon(kind).tab("RENDER").unwrap_or_else(|| panic!("{kind:?} has no RENDER tab"));
        assert_eq!(render.shown, Shown::Render, "{kind:?}");
        let upstream = get(SOLIDCRAFT).unwrap().ribbon(kind).tab("RENDER").unwrap();
        assert_eq!(render.panels, upstream.panels, "{kind:?} inherits SolidCraft's RENDER");
        assert!(render.panels.iter().any(|p| !p.commands.is_empty()), "{kind:?}");
    }
}

/// Task 6 (D5): the 46 Inventor aliases with a SolidCraft target, as data, with the one-key wait;
/// updated by design (R70) for Plan D-2 Task 1's eight drawing aliases.
#[test]
fn inventor_maps_54_aliases_without_conflicts() {
    assert_eq!(inventor().letter_keys, LetterKeys::Aliases);
    assert_eq!(inventor().aliases.len(), 54);
    assert_eq!(inventor().alias_wait_ms, 750);
    assert_eq!(validate(inventor()), Vec::<String>::new());
}

/// Ruling B: Inventor's F6 is Home View, carried by the profile's data.
#[test]
fn inventor_f6_is_home_view() {
    assert_eq!(inventor().function_keys.get("F6").map(String::as_str), Some("ui.view_home"));
}

/// Inventor Studio's slot: ENVIRONMENTS › BEGIN carries the Render entry on Part and Assembly
/// (the workspace switcher that used to provide it is not drawn under Inventor). Starting it from
/// the design enters the Render workspace.
#[test]
fn environments_begin_enters_the_render_workspace() {
    for kind in [Part, Assembly] {
        let begin = inventor().ribbon(kind).tab("ENVIRONMENTS").unwrap().panel("BEGIN").unwrap();
        assert!(!begin.hidden, "{kind:?}: BEGIN is on the tab's face");
        assert!(face(kind, "ENVIRONMENTS", "BEGIN").contains(&("render.start", L)), "{kind:?}");
        let mut h = crate::test_support::inventor(kind);
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        assert!(!crate::render_workspace::active(&h.app));
        h.app.start("render.start");
        assert!(crate::render_workspace::active(&h.app), "{kind:?}: {:?}", h.app.status);
        assert_eq!(h.app.ui.tab, "RENDER");
    }
}

/// The captured split buttons (inventor-ribbon-capture1.json): Plane, Axis and Point are
/// most-recently-used split buttons (control type 45060), so their faces follow the last pick;
/// Box (Primitives), sketch Fillet and Place are plain split buttons (45059) whose face is fixed.
/// Their lists hold the captured variants TCI-CAD has a command for, in the capture's order.
#[test]
fn split_buttons_follow_the_capture() {
    assert!(inventor().workflow.sticky_split_buttons);
    let dropdowns = |kind: DocumentKind, tab: &str, panel: &str| -> Vec<(&'static str, &'static str, bool, Vec<&'static str>)> {
        let p = inventor().ribbon(kind).tab(tab).unwrap().panel(panel).unwrap();
        p.dropdowns.iter().map(|d| (d.id.as_str(), d.face.as_str(), d.fixed_face, d.commands.iter().map(String::as_str).collect())).collect()
    };
    let work = vec![
        (
            "work_plane",
            "construct.plane.offset",
            false,
            vec![
                "construct.plane.offset",
                "construct.plane.midplane",
                "construct.plane.at_angle",
                "construct.plane.three_points",
                "construct.plane.two_edges",
                "construct.plane.tangent",
                "construct.plane.perpendicular",
                "construct.plane.along_path",
            ],
        ),
        (
            "work_axis",
            "construct.axis.edge",
            false,
            vec![
                "construct.axis.edge",
                "construct.axis.two_points",
                "construct.axis.two_planes",
                "construct.axis.normal_to_face",
                "construct.axis.through_cylinder",
            ],
        ),
        (
            "work_point",
            "construct.point.vertex",
            false,
            vec![
                "construct.point.vertex",
                "construct.point.three_planes",
                "construct.point.two_edges",
                "construct.point.edge_plane",
                "construct.point.center",
            ],
        ),
    ];
    for (kind, tab) in [(Part, "MODEL"), (Part, "SHEET METAL"), (Assembly, "ASSEMBLE"), (Assembly, "MODEL")] {
        assert_eq!(dropdowns(kind, tab, "WORK FEATURES"), work, "{kind:?} {tab}");
    }
    assert_eq!(
        dropdowns(Part, "MODEL", "PRIMITIVES"),
        [("primitive", "solid.box", true, vec!["solid.box", "solid.cylinder", "solid.sphere", "solid.torus"])]
    );
    assert_eq!(
        dropdowns(Part, "SKETCH", "CREATE"),
        [(
            "sketch_fillet",
            "sketch.fillet",
            true,
            vec!["sketch.fillet", "sketch.chamfer.equal_distance", "sketch.chamfer.two_distance", "sketch.chamfer.distance_angle"]
        )]
    );
    assert_eq!(
        dropdowns(Assembly, "ASSEMBLE", "COMPONENT"),
        [("place", "component.insert_design", true, vec!["component.insert_design", "parts.insert", "file.insert_step"])]
    );
    // No other panel has a split button.
    let count: usize =
        DocumentKind::ALL.into_iter().flat_map(|k| inventor().ribbon(k).tabs.iter()).flat_map(|t| &t.panels).map(|p| p.dropdowns.len()).sum();
    assert_eq!(count, 3 * 4 + 1 + 1 + 1 + 1, "Work Features on four tabs, Primitives, sketch Fillet on two sketch tabs, Place");
    assert!(validate(inventor()).is_empty(), "{:?}", validate(inventor()));
}

/// The Inventor profile's JSON with `edit` applied, parsed.
fn inventor_with(edit: impl FnOnce(&mut Value)) -> Result<Profile, String> {
    let mut v: Value = serde_json::from_str(include_str!("../../../assets/profiles/inventor.json")).unwrap();
    edit(&mut v);
    parse(&v.to_string(), &solidcraft())
}

#[test]
fn validate_rejects_fixed_face_outside_a_drop_down_and_a_face_off_its_list() {
    // A one-command "drop-down" is a plain button: fixed_face on it is refused.
    let p = inventor_with(|v| {
        v["panels"]["work_features"]["dropdowns"] =
            json!([{"id": "work_plane", "face": "construct.plane.offset", "commands": ["construct.plane.offset"], "fixed_face": true}]);
    })
    .unwrap();
    let problems = validate(&p);
    assert!(
        problems.iter().any(|m| m.contains("drop-down work_plane") && m.contains("fixed_face on a button that is not a drop-down")),
        "{problems:?}"
    );
    // fixed_face anywhere but a drop-down entry does not parse.
    let e = inventor_with(|v| v["panels"]["work_features"]["fixed_face"] = json!(true)).unwrap_err();
    assert!(e.contains("fixed_face"), "{e}");
    // The face must be one of its commands.
    let p = inventor_with(|v| v["panels"]["work_features"]["dropdowns"][0]["face"] = json!("construct.axis.edge")).unwrap();
    let problems = validate(&p);
    assert!(problems.iter().any(|m| m.contains("drop-down work_plane: face construct.axis.edge is not in its commands")), "{problems:?}");
}

#[test]
fn validate_rejects_a_grid_command_that_is_not_small() {
    assert!(validate(inventor()).is_empty(), "{:?}", validate(inventor()));
    // Dimension is large: it cannot be a grid cell.
    let p = inventor_with(|v| v["panels"]["constrain"]["grid"].as_array_mut().unwrap().push(json!("sketch.dimension"))).unwrap();
    let problems = validate(&p);
    assert!(problems.iter().any(|m| m.ends_with("SKETCH/CONSTRAIN: grid sketch.dimension is not a small command")), "{problems:?}");
}
