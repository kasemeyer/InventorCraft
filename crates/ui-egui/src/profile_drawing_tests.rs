//! The Inventor profile's Drawing ribbon, aliases and browser root (Plan D-2 Task 1), as data
//! following the capture (RIB Drawing; spec §5.1).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use serde_json::json;
use solidcraft_engine::Session;
use solidcraft_engine::doc::DocumentKind::*;

use crate::alias_keys::{Context, Resolution, resolve};

fn inventor() -> &'static Profile {
    get(INVENTOR).expect("the Inventor profile is built in")
}

/// (panel id, hidden, commands) per tab, in the ribbon's order.
type Layout = [(&'static str, &'static [(&'static str, bool, &'static [&'static str])])];

const DRAWING_RIBBON: &Layout = &[
    ("PLACE VIEWS", &[("CREATE", false, &["drawing.view.base", "drawing.view.projected"]), ("SHEETS", false, &["drawing.sheet.add"])]),
    (
        "ANNOTATE",
        &[
            ("DIMENSION", false, &["drawing.dim.general"]),
            ("FEATURE NOTES", false, &["drawing.note.hole"]),
            ("TEXT", false, &["drawing.note.text", "drawing.note.leader"]),
            ("SYMBOLS", false, &["drawing.centermark", "drawing.centerline.bisector"]),
        ],
    ),
    (
        "TOOLS",
        &[
            ("MEASURE", true, &["inspect.measure"]),
            ("OPTIONS", false, &["document.units", "document.metadata"]),
            ("CLIPBOARD", true, &["timeline.copy", "timeline.paste"]),
        ],
    ),
    (
        "MANAGE",
        &[
            ("UPDATE", false, &["drawing.update"]),
            ("PARAMETERS", true, &["parameters.change", "parameters.graph", "parameters.import", "parameters.export", "parameters.move"]),
        ],
    ),
    ("VIEW", &[("WINDOWS", false, &["view.layout", "view.history_layout"])]),
];

#[test]
fn inventor_drawing_ribbon_follows_the_capture() {
    let ribbon = inventor().ribbon(Drawing);
    let tabs: Vec<&str> = ribbon.tabs.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(tabs, ["PLACE VIEWS", "ANNOTATE", "TOOLS", "MANAGE", "VIEW"]);
    for (tab, want) in DRAWING_RIBBON {
        let got: Vec<(&str, bool, Vec<&str>)> =
            ribbon.tab(tab).unwrap().panels.iter().map(|p| (p.id.as_str(), p.hidden, p.commands.iter().map(String::as_str).collect())).collect();
        let want: Vec<(&str, bool, Vec<&str>)> = want.iter().map(|(p, h, c)| (*p, *h, c.to_vec())).collect();
        assert_eq!(got, want, "{tab}");
    }
    // Size tiers (R95): every command on its panel's face; Symbols' two draw small, the rest large.
    for tab in &ribbon.tabs {
        for panel in tab.panels.iter().filter(|p| p.commands.iter().any(|c| c.starts_with("drawing."))) {
            assert_eq!(panel.promoted, panel.commands.len(), "{}/{}", tab.id, panel.id);
            let small: Vec<&str> = panel.small.iter().map(String::as_str).collect();
            let want: &[&str] = if panel.id == "SYMBOLS" { &["drawing.centermark", "drawing.centerline.bisector"] } else { &[] };
            assert_eq!(small, want, "{}/{}", tab.id, panel.id);
            assert!(panel.grid.is_empty() && panel.dropdowns.is_empty(), "{}/{}", tab.id, panel.id);
        }
    }
    assert_eq!(ribbon.home(), "PLACE VIEWS");
    assert_eq!(ribbon.home.as_deref(), Some("PLACE VIEWS"));
}

/// The captured panels with no registry command stay undeclared (R19): the validator's
/// "panel is empty" rule stays, and `omitted` is keyed by command id.
#[test]
fn uncaptured_panels_stay_undeclared() {
    let ribbon = inventor().ribbon(Drawing);
    for tab in &ribbon.tabs {
        for id in [
            "MODIFY",
            "SKETCH",
            "RETRIEVE",
            "TABLE",
            "REVISION",
            "FORMAT",
            "NAVIGATE",
            "TEAM WEB",
            "TUTORIALS",
            "STYLES AND STANDARDS",
            "INSERT",
            "DEFINE",
            "ILOGIC",
            "CONTENT CENTER",
            "WEB",
        ] {
            assert!(tab.panel(id).is_none(), "{}/{id} is declared", tab.id);
        }
    }
    assert!(ribbon.tab("SKETCH").is_none() && ribbon.tab("RENDER").is_none());
}

/// R96: the drawing buttons' compact labels are the profile's own (catalog texts), not
/// SolidCraft's `RIBBON_LABELS`.
#[test]
fn drawing_buttons_carry_the_profile_labels() {
    let english: BTreeSet<String> =
        serde_json::from_str::<BTreeMap<String, String>>(include_str!("../../../assets/locales/en.json")).unwrap().into_values().collect();
    for (id, label) in [
        ("drawing.view.base", "Base"),
        ("drawing.view.projected", "Projected"),
        ("drawing.sheet.add", "New Sheet"),
        ("drawing.dim.general", "Dimension"),
        ("drawing.note.hole", "Hole and Thread"),
        ("drawing.note.text", "Text"),
        ("drawing.note.leader", "Leader Text"),
        ("drawing.centermark", "Center Mark"),
        ("drawing.centerline.bisector", "Centerline Bisector"),
        ("drawing.update", "Update"),
    ] {
        assert_eq!(inventor().button_labels.get(id).map(String::as_str), Some(label), "{id}");
        assert!(english.contains(label), "{label}");
    }
    assert_eq!(inventor().button_labels.keys().filter(|id| id.starts_with("drawing.")).count(), 10);
}

/// Review fix 5: drawing nodes stay in the drawing root, design nodes out of it.
#[test]
fn validate_rejects_cross_kind_browser_nodes() {
    let mut p = inventor().clone();
    p.browser.part_root.push(RootNode::Sheets);
    p.browser.assembly_root.push(RootNode::DrawingResources);
    p.browser.drawing_root.push(RootNode::Bodies);
    let problems = validate(&p);
    for needle in [
        "browser.part_root: Sheets is a drawing's node",
        "browser.assembly_root: DrawingResources is a drawing's node",
        "browser.drawing_root: Bodies is not a drawing's node",
    ] {
        assert!(problems.iter().any(|l| l == needle), "{needle}: {problems:?}");
    }
    assert!(validate(inventor()).is_empty() && validate(&solidcraft()).is_empty());
}

#[test]
fn drawing_aliases_follow_the_tsv() {
    let table: Vec<(String, String, AliasScope)> = inventor().aliases.iter().map(|a| (a.keys.clone(), a.command.clone(), a.scope)).collect();
    for (keys, id) in [
        ("BV", "drawing.view.base"),
        ("PV", "drawing.view.projected"),
        ("D", "drawing.dim.general"),
        ("T", "drawing.note.text"),
        ("LE", "drawing.note.leader"),
        ("CM", "drawing.centermark"),
        ("CB", "drawing.centerline.bisector"),
        ("HN", "drawing.note.hole"),
    ] {
        assert_eq!(resolve(&table, Context::Drawing, keys), Resolution::Run(id.into()), "{keys}");
        let alias = inventor().aliases.iter().find(|a| a.command == id).unwrap();
        assert_eq!(alias.scope, AliasScope::Drawing, "{keys}");
    }
}

/// Review Focus: the drawing's D does not reach a part or a sketch.
#[test]
fn d_in_a_part_still_dimensions_the_sketch() {
    let table: Vec<(String, String, AliasScope)> = inventor().aliases.iter().map(|a| (a.keys.clone(), a.command.clone(), a.scope)).collect();
    for ctx in [Context::Part, Context::Sketch] {
        assert_eq!(resolve(&table, ctx, "D"), Resolution::Run("sketch.dimension".into()), "{ctx:?}");
    }
    assert!(validate(inventor()).iter().all(|line| !line.contains("conflict")), "{:?}", validate(inventor()));
    let drawing = AliasScope::Drawing;
    for x in [AliasScope::Model, AliasScope::Part, AliasScope::Assembly, AliasScope::Sketch, AliasScope::SketchWrapper] {
        assert!(!drawing.overlaps(x) && !x.overlaps(drawing), "{x:?}");
    }
    assert!(drawing.overlaps(drawing) && drawing.overlaps(AliasScope::Everywhere) && AliasScope::Everywhere.overlaps(drawing));
}

#[test]
fn drawing_browser_root_is_resources_then_sheets() {
    use RootNode::*;
    assert_eq!(inventor().browser.drawing_root, [DrawingResources, Sheets]);
    assert_eq!(solidcraft().browser.drawing_root, [DrawingResources, Sheets]);
    assert_eq!(BrowserLayout::default().drawing_root, [DrawingResources, Sheets]);
    let english: BTreeMap<String, String> = serde_json::from_str(include_str!("../../../assets/locales/en.json")).unwrap();
    for (folder, id) in [
        ("drawing_resources", "profile.inventor.browser.drawing_resources"),
        ("sheet", "profile.inventor.browser.sheet"),
        ("border", "profile.inventor.browser.border"),
        ("title_block", "profile.inventor.browser.title_block"),
    ] {
        assert_eq!(inventor().browser.folder_labels.get(folder).map(String::as_str), Some(id), "{folder}");
        assert!(english.contains_key(id), "{id}");
    }
}

/// R85: a hidden Clipboard panel shown on a drawing meets the engine's kind guard.
#[test]
fn a_hidden_clipboard_button_meets_the_guard() {
    let mut s = Session::default();
    s.execute("drawing.new", &json!({})).unwrap();
    let steps = s.undo.len();
    let err = s.execute("timeline.paste", &json!({})).unwrap_err();
    assert_eq!(err.to_string(), "timeline.paste: a drawing holds no model geometry: open the part or assembly");
    assert_eq!(s.undo.len(), steps);
}
