#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use serde_json::json;

const FIXTURE: &str = r#"{
  "format": "tci-ui-profile/1", "id": "fixture", "name": "profile.solidcraft",
  "panels": {
    "make": {"id": "CREATE", "label": "ui.create", "promoted": 1, "commands": ["solid.extrude", "solid.revolve"]},
    "pick": {"id": "SELECT", "promoted": 0, "commands": []}
  },
  "tabs": {
    "model": {"id": "MODEL", "label": "ui.design", "panels": ["make", "pick"]},
    "render": {"id": "RENDER", "shown": "render", "inherit": "solidcraft"}
  },
  "ribbons": {"part": ["model", "render"], "assembly": ["model"], "drawing": ["model"]}
}"#;

fn with(edit: impl FnOnce(&mut serde_json::Value)) -> String {
    let mut v: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    edit(&mut v);
    v.to_string()
}

fn fixture() -> Profile {
    parse(FIXTURE, &solidcraft()).unwrap()
}

fn panel_ids(tab: &RibbonTab) -> Vec<&str> {
    tab.panels.iter().map(|p| p.id.as_str()).collect()
}

fn has(problems: &[String], text: &str) -> bool {
    problems.iter().any(|p| p.contains(text))
}

/// Updated by design (R70, R67): Part and Assembly from `SECTIONS`, Drawing from its own
/// `DRAWING_SECTIONS` through `drawing_layout`.
#[test]
fn solidcraft_profile_reproduces_the_workspace_table() {
    let p = solidcraft();
    let specs = command_specs();
    for s in crate::workspace::DRAWING_SECTIONS {
        let panel = p.ribbon(DocumentKind::Drawing).tab(s.tab).unwrap().panel(s.panel).unwrap();
        let (cmds, promoted) = crate::workspace::drawing_layout(s.panel, &specs).unwrap();
        let want: Vec<&str> = cmds.iter().map(|c| c.id).collect();
        let got: Vec<&str> = panel.commands.iter().map(String::as_str).collect();
        assert_eq!(got, want, "Drawing {}/{}", s.tab, s.panel);
        assert_eq!(want, s.ids, "Drawing {}/{}: listed ids only", s.tab, s.panel);
        assert_eq!((panel.promoted, promoted), (s.promoted, s.promoted), "Drawing {}/{}", s.tab, s.panel);
    }
    for kind in [DocumentKind::Part, DocumentKind::Assembly] {
        for s in crate::workspace::SECTIONS {
            let panel = p.ribbon(kind).tab(s.tab).unwrap().panel(s.panel).unwrap();
            let (cmds, _) = crate::workspace::layout(s.tab, s.panel, &specs).unwrap();
            let want: Vec<&str> = cmds.iter().map(|c| c.id).collect();
            let got: Vec<&str> = panel.commands.iter().map(String::as_str).collect();
            assert_eq!(got, want, "{kind:?} {}/{}", s.tab, s.panel);
            assert_eq!(panel.promoted, s.promoted, "{kind:?} {}/{}", s.tab, s.panel);
        }
    }
    let modify = p.ribbon(DocumentKind::Part).tab("MODEL").unwrap().panel("MODIFY").unwrap();
    let labels: Vec<&str> = modify.groups.iter().map(|g| g.label.as_str()).collect();
    assert_eq!(labels, ["Edges", "Faces", "Bodies", "Specialise"]);
}

#[test]
fn solidcraft_profile_tab_order_is_upstream() {
    let p = solidcraft();
    let ribbon = p.ribbon(DocumentKind::Part);
    let got: Vec<(&str, Shown)> = ribbon.tabs.iter().map(|t| (t.id.as_str(), t.shown)).collect();
    let mut want: Vec<(&str, Shown)> = crate::workspace::TABS.iter().map(|t| (*t, Shown::Always)).collect();
    want.extend([("SKETCH", Shown::Sketch), ("SHEET METAL", Shown::Always), ("RENDER", Shown::Render)]);
    assert_eq!(got, want);
    for tab in &ribbon.tabs {
        assert!(tab.label.is_none(), "tab {} has a label", tab.id);
        for panel in &tab.panels {
            assert!(panel.label.is_none(), "panel {}/{} has a label", tab.id, panel.id);
        }
    }
    assert_eq!(ribbon.home(), "MODEL");
}

#[test]
fn solidcraft_profile_destinations_match_workspace_destination() {
    let p = solidcraft();
    let ribbon = p.ribbon(DocumentKind::Assembly);
    for c in command_specs() {
        assert_eq!(ribbon.destination(c.id), crate::workspace::destination(c), "{}", c.id);
    }
}

#[test]
fn solidcraft_profile_keeps_upstream_keys_and_browser() {
    use RootNode::*;
    let p = solidcraft();
    assert_eq!(p.name, "profile.solidcraft");
    assert_eq!(p.alias_wait_ms, DEFAULT_ALIAS_WAIT_MS);
    assert_eq!(p.letter_keys, LetterKeys::Registry);
    assert!(p.aliases.is_empty());
    assert_eq!(p.view_cube, ViewCubeStyle::default());
    assert_eq!(
        p.browser.part_root,
        [DocSettings, NamedViews, Origin, Canvases, Analyses, Bodies, Sketches, Construction, Features, Joints, Components]
    );
    assert_eq!(p.browser.assembly_root, p.browser.part_root);
    let keys: Vec<&str> = p.browser.origin.iter().map(|o| o.key.as_str()).collect();
    assert_eq!(keys, ["O", "X", "Y", "Z", "XY", "XZ", "YZ"]);
    assert!(p.browser.origin.iter().all(|o| o.label.is_none()));
    assert_eq!(p.defaults.nav.as_deref(), Some("fusion"));
    assert_eq!(p.defaults.navigator, Some(crate::prefs::NavigatorStyle::Axes));
    assert_eq!(p.defaults.theme.as_deref(), Some("dark"));
    assert_eq!(p.defaults.history, Some(super::History::Bottom));
    assert!(p.defaults.new_document.is_none());
}

#[test]
fn parse_resolves_keys_in_order() {
    let p = fixture();
    let part = p.ribbon(DocumentKind::Part);
    let tabs: Vec<&str> = part.tabs.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(tabs, ["MODEL", "RENDER"]);
    let model = part.tab("MODEL").unwrap();
    assert_eq!(panel_ids(model), ["CREATE", "SELECT"]);
    assert_eq!(model.label.as_deref(), Some("ui.design"));
    let create = model.panel("CREATE").unwrap();
    assert_eq!(create.commands, ["solid.extrude", "solid.revolve"]);
    assert_eq!(create.promoted, 1);
    assert_eq!(create.label.as_deref(), Some("ui.create"));
    assert!(model.panel("SELECT").unwrap().label.is_none());
    assert_eq!(p.browser, BrowserLayout::default());
    assert_eq!(p.alias_wait_ms, DEFAULT_ALIAS_WAIT_MS);
    assert_eq!(part.home(), "MODEL");
}

#[test]
fn inherit_copies_the_base_profiles_tab() {
    let p = fixture();
    let render = p.ribbon(DocumentKind::Part).tab("RENDER").unwrap();
    assert_eq!(render.shown, Shown::Render);
    let base = solidcraft();
    let want = base.ribbon(DocumentKind::Part).tab("RENDER").unwrap();
    let shape = |t: &RibbonTab| t.panels.iter().map(|p| (p.id.clone(), p.commands.clone(), p.promoted)).collect::<Vec<_>>();
    assert!(!want.panels.is_empty());
    assert_eq!(shape(render), shape(want));
}

#[test]
fn parse_reads_small_and_hidden_panels() {
    let text = with(|v| {
        v["panels"]["make"]["promoted"] = 2.into();
        v["panels"]["make"]["small"] = json!(["solid.revolve"]);
        v["tabs"]["model"]["panels"] = json!(["make", {"panel": "pick", "hidden": true}]);
    });
    let p = parse(&text, &solidcraft()).unwrap();
    let model = p.ribbon(DocumentKind::Part).tab("MODEL").unwrap();
    let create = model.panel("CREATE").unwrap();
    assert_eq!(create.small, ["solid.revolve"]);
    assert!(create.is_small("solid.revolve"));
    assert!(!create.is_small("solid.extrude"));
    assert!(!create.hidden);
    assert!(model.panel("SELECT").unwrap().hidden);

    let plain = fixture();
    let model = plain.ribbon(DocumentKind::Part).tab("MODEL").unwrap();
    assert!(model.panel("CREATE").unwrap().small.is_empty());
    assert!(model.panels.iter().all(|p| !p.hidden));
    assert!(!plain.quick_access);

    let off_face = with(|v| v["panels"]["make"]["small"] = json!(["solid.revolve"]));
    let problems = validate(&parse(&off_face, &solidcraft()).unwrap());
    assert!(has(&problems, "small"), "{problems:?}");
}

#[test]
fn parse_reads_home_and_alias_wait() {
    let p = parse(&with(|v| v["alias_wait_ms"] = 500.into()), &solidcraft()).unwrap();
    assert_eq!(p.alias_wait_ms, 500);

    let p = parse(&with(|v| v["home"] = json!({"assembly": "MODEL"})), &solidcraft()).unwrap();
    assert_eq!(p.ribbon(DocumentKind::Assembly).home(), "MODEL");

    let p = parse(&with(|v| v["home"] = json!({"part": "RENDER"})), &solidcraft()).unwrap();
    let problems = validate(&p);
    assert!(has(&problems, "home"), "{problems:?}");
}

#[test]
fn parse_rejects_bad_input() {
    let cases: Vec<(String, &str)> = vec![
        (with(|v| v["format"] = "tci-ui-profile/9".into()), "format"),
        (with(|v| v["tabs"]["model"]["panels"] = json!(["make", "pick", "nope"])), "nope"),
        (with(|v| v["ribbons"]["drawing"] = json!(["model", "ghost"])), "ghost"),
        (with(|v| v["colour"] = 1.into()), "colour"),
        (with(|v| v["tabs"]["render"]["inherit"] = "nobody".into()), "nobody"),
        (with(|v| v["panels"]["spare"] = json!({"id": "X", "promoted": 0, "commands": []})), "spare"),
        (with(|v| v["home"] = json!({"part": "PHANTOM"})), "PHANTOM"),
    ];
    for (text, needle) in cases {
        let err = parse(&text, &solidcraft()).unwrap_err();
        assert!(err.contains(needle), "{needle}: {err}");
    }
}

#[test]
fn validate_reports_bad_placements() {
    let clean = fixture();
    assert_eq!(validate(&clean), Vec::<String>::new());

    let mut p = clean.clone();
    let part = p.ribbon_mut(DocumentKind::Part);
    let model = &mut part.tabs[0];
    model.panels[0].commands.push("no.such.command".into());
    model.panels[0].commands.push("solid.extrude".into());
    model.panels[0].commands.push("sketch.solve".into());
    let mut second = model.panels[0].clone();
    second.id = "OTHER".into();
    second.commands = vec!["solid.extrude".into()];
    model.panels.push(second);
    let mut twin = model.panels[0].clone();
    twin.commands = vec!["solid.chamfer".into()];
    model.panels.push(twin);
    let mut empty = model.panels[0].clone();
    empty.id = "BARE".into();
    empty.commands.clear();
    empty.promoted = 0;
    model.panels.push(empty);
    let drawing = p.ribbon_mut(DocumentKind::Drawing);
    drawing.tabs.truncate(1);
    drawing.tabs[0].shown = Shown::Sketch;

    let problems = validate(&p);
    for needle in ["no.such.command", "twice in panel", "two panels", "panel id", "empty", "sketch.solve", "no tab shown always"] {
        assert!(has(&problems, needle), "{needle}: {problems:?}");
    }
}

#[test]
fn validate_checks_aliases() {
    let base = |extra: Option<serde_json::Value>| {
        with(|v| {
            v["letter_keys"] = "aliases".into();
            let mut aliases = json!([
                {"keys": "E", "command": "solid.extrude", "scope": "model"},
                {"keys": "E", "command": "sketch.line", "scope": "sketch"}
            ]);
            if let Some(extra) = extra {
                aliases.as_array_mut().unwrap().push(extra);
            }
            v["aliases"] = aliases;
        })
    };
    let check = |extra: Option<serde_json::Value>| validate(&parse(&base(extra), &solidcraft()).unwrap());

    let clean = parse(&base(None), &solidcraft()).unwrap();
    assert_eq!(clean.letter_keys, LetterKeys::Aliases);
    assert_eq!(clean.aliases.len(), 2);
    assert_eq!(validate(&clean), Vec::<String>::new());

    let problems = check(Some(json!({"keys": "E", "command": "solid.revolve", "scope": "part"})));
    assert!(has(&problems, "E") && has(&problems, "conflict"), "{problems:?}");
    let problems = check(Some(json!({"keys": "e", "command": "solid.extrude", "scope": "part"})));
    assert!(has(&problems, "keys"), "{problems:?}");
    let problems = check(Some(json!({"keys": "Q", "command": "no.such", "scope": "part"})));
    assert!(has(&problems, "no.such"), "{problems:?}");
    let problems = check(Some(json!({"keys": "OP", "command": "ui.preferences", "scope": "everywhere"})));
    assert_eq!(problems, Vec::<String>::new());

    use AliasScope::*;
    assert!(SketchWrapper.overlaps(Part));
    assert!(SketchWrapper.overlaps(Sketch));
    assert!(!Sketch.overlaps(Model));
    for x in [Everywhere, Model, Part, Assembly, Sketch, SketchWrapper] {
        assert!(Everywhere.overlaps(x), "{x:?}");
    }
}

#[test]
fn every_built_in_profile_parses_and_validates() {
    let all = all();
    assert_eq!(all.len(), 1 + BUILTIN.len());
    assert_eq!(all[0].id, SOLIDCRAFT);
    let ids: BTreeSet<&str> = all.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids.len(), all.len());
    for p in all {
        let problems = validate(p);
        assert!(problems.is_empty(), "{}: {problems:#?}", p.id);
        assert!(get(&p.id).is_some_and(|g| g.id == p.id));
    }
}

#[test]
fn validate_checks_group_commands() {
    let grouped = |groups: serde_json::Value| {
        with(|v| {
            v["panels"]["make"]["groups"] = groups;
        })
    };
    let clean = parse(&grouped(json!([{"label": "Solids", "commands": ["solid.extrude", "solid.revolve"]}])), &solidcraft()).unwrap();
    assert_eq!(validate(&clean), Vec::<String>::new());

    let problems = validate(&parse(&grouped(json!([{"label": "Solids", "commands": ["no.such.command"]}])), &solidcraft()).unwrap());
    assert!(has(&problems, "group Solids: no.such.command is not a registry command"), "{problems:?}");

    let problems = validate(&parse(&grouped(json!([{"label": "Solids", "commands": ["solid.chamfer"]}])), &solidcraft()).unwrap());
    assert!(has(&problems, "group Solids: solid.chamfer is not in the panel"), "{problems:?}");
    assert!(!has(&problems, "registry"), "{problems:?}");
}

/// Ruling B as data: function keys map to UI actions per profile. SolidCraft's F6 fits; a data
/// profile that names none inherits the base's; bad keys and targets are reported.
#[test]
fn function_keys_are_profile_data() {
    assert_eq!(solidcraft().function_keys.get("F6").map(String::as_str), Some("ui.view_fit"));
    assert_eq!(fixture().function_keys, solidcraft().function_keys);
    let p = parse(&with(|v| v["function_keys"] = json!({"F6": "ui.view_home"})), &solidcraft()).unwrap();
    assert_eq!(p.function_keys.get("F6").map(String::as_str), Some("ui.view_home"));
    assert_eq!(validate(&p), Vec::<String>::new());
    let check = |keys: serde_json::Value| validate(&parse(&with(|v| v["function_keys"] = keys), &solidcraft()).unwrap());
    assert!(has(&check(json!({"F13": "ui.view_fit"})), "F13"));
    assert!(has(&check(json!({"G6": "ui.view_fit"})), "G6"));
    assert!(has(&check(json!({"F2": "ui.view_fit"})), "F2"));
    assert!(has(&check(json!({"F5": "solid.extrude"})), "solid.extrude"));
}

/// New (Ctrl+N) makes a part or an assembly: a profile whose default is a drawing is refused,
/// rather than leaving New doing nothing.
#[test]
fn a_drawing_is_not_a_new_document_default() {
    let check = |kind: &str| validate(&parse(&with(|v| v["defaults"] = json!({"new_document": kind})), &solidcraft()).unwrap());
    assert_eq!(check("drawing"), ["defaults.new_document: drawing is not offered; New makes a part or an assembly"]);
    assert_eq!(check("part"), Vec::<String>::new());
    assert_eq!(check("assembly"), Vec::<String>::new());
}
