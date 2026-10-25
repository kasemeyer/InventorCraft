#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use serde_json::json;
use solidcraft_engine::doc::DocumentKind::*;
use solidcraft_engine::{CommandSpec, Session};

use crate::prefs::NavigatorStyle;
use crate::scenario::Harness;
use crate::test_support::inventor;
use crate::toolbar_custom::{PanelLists, panel_lists};
use crate::workspace::{TABS, visible_tabs};
use crate::{Services, SolveApp};

fn panel_ids(lists: &PanelLists) -> Vec<&'static str> {
    lists.iter().map(|(p, ..)| *p).collect()
}

fn command_ids(lists: &PanelLists) -> Vec<(&'static str, Vec<&'static str>)> {
    lists.iter().map(|(p, cmds, _)| (*p, cmds.iter().map(|c| c.id).collect())).collect()
}

/// The commands of a tab's panel, as the toolbar lists them.
fn panel_commands(app: &SolveApp, tab: &str, panel: &str) -> Vec<&'static CommandSpec> {
    panel_lists(app, tab).into_iter().find(|(p, ..)| *p == panel).map(|(_, cmds, _)| cmds).unwrap()
}

fn headings(grouped: &[(Option<&'static str>, &CommandSpec)]) -> Vec<Option<&'static str>> {
    grouped.iter().map(|(h, _)| *h).collect()
}

/// A sheet body made from a sketched rectangle, as `workspace`'s contextual-tab test makes it.
fn make_sheet_body(h: &mut Harness) {
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.frame();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.frame();
    h.app.run("sheet.flange", json!({"name":"Sheet", "body_name":"Sheet"})).unwrap();
    h.frames(2);
}

#[test]
fn the_solidcraft_profile_keeps_the_upstream_ribbon() {
    let h = Harness::new();
    let mut expected = TABS.to_vec();
    expected.push("SHEET METAL");
    assert_eq!(visible_tabs(&h.app), expected);
    assert_eq!(h.app.ui.tab, "MODEL");
    assert_eq!(custom_scope(&h.app), "DESIGN");
    assert_eq!(tab_label(&h.app, "MODEL"), "Model");
}

#[test]
fn inventor_part_document_shows_the_part_ribbon() {
    let h = inventor(Part);
    assert_eq!(visible_tabs(&h.app), ["SHEET METAL", "MODEL", "SKETCH", "INSPECT", "TOOLS", "MANAGE", "VIEW", "ENVIRONMENTS"]);
    assert_eq!(h.app.ui.tab, "MODEL");
    assert_eq!(tab_label(&h.app, "MODEL"), "3D Model");
    assert_eq!(panel_label(&h.app, "MODEL", "WORK FEATURES"), "Work Features");
    let shown: Vec<&str> = ribbon(&h.app).tab("MODEL").unwrap().panels.iter().filter(|p| !p.hidden).map(|p| p.id.as_str()).collect();
    assert_eq!(panel_ids(&panel_lists(&h.app, "MODEL")), shown);
}

#[test]
fn switching_document_kind_resets_the_ribbon_to_its_home_tab() {
    let mut h = inventor(Part);
    h.app.ui.tab = "INSPECT".into();
    h.frame();
    assert_eq!(h.app.ui.tab, "INSPECT");
    h.app.session.doc_mut().kind = Assembly;
    h.frame();
    assert_eq!(h.app.ui.tab, ribbon(&h.app).home());
    assert_eq!(h.app.ui.tab, "TOOLS");
}

#[test]
fn the_exit_panel_shows_only_while_sketching() {
    let mut h = inventor(Part);
    assert!(!panel_ids(&panel_lists(&h.app, "SKETCH")).contains(&"FINISH SKETCH"));
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.frame();
    assert!(panel_ids(&panel_lists(&h.app, "SKETCH")).contains(&"FINISH SKETCH"));
}

#[test]
fn ribbon_labels_and_button_labels_come_from_the_profile() {
    let plane = solidcraft_engine::find_command("construct.plane.offset").unwrap();
    let h = inventor(Part);
    assert_eq!(button_label(&h.app, plane), "Plane");
    let h = Harness::new();
    assert_eq!(button_label(&h.app, plane), "Offset Plane");
}

#[test]
fn customised_buttons_are_kept_per_profile_and_kind() {
    use crate::toolbar_custom as tc;
    let mut h = Harness::new();
    tc::pin(&mut h.app, "MODEL", "solid.box").unwrap();
    let design: Vec<(String, Vec<String>)> =
        h.app.toolbar_custom.panels.iter().filter(|(k, _)| k.starts_with("DESIGN/MODEL/")).map(|(k, v)| (k.clone(), v.clone())).collect();
    assert!(!design.is_empty(), "{:?}", h.app.toolbar_custom.panels);

    assert_eq!(h.call("ui.prefs", json!({"ui_profile": "inventor"}))["ok"], json!(true));
    h.app.session.doc_mut().kind = Part;
    h.frames(2);
    assert_eq!(custom_scope(&h.app), "inventor.part");
    let create = panel_commands(&h.app, "MODEL", "CREATE");
    let panel = ribbon(&h.app).tab("MODEL").unwrap().panel("CREATE").unwrap();
    let shown: Vec<&str> = tc::buttons(&h.app, "MODEL", "CREATE", &create, panel.promoted).iter().map(|c| c.id).collect();
    let face: Vec<&str> = panel.commands.iter().take(panel.promoted).map(String::as_str).collect();
    assert_eq!(shown, face, "Inventor's own face, not SolidCraft's customisation");
    assert_eq!(shown[..2], ["solid.extrude", "solid.revolve"]);
    assert!(!shown.contains(&"solid.box"));

    tc::pin(&mut h.app, "MODEL", "solid.box").unwrap();
    assert!(h.app.toolbar_custom.panels.keys().any(|k| k.starts_with("inventor.part/MODEL/")), "{:?}", h.app.toolbar_custom.panels);
    for (k, v) in &design {
        assert_eq!(h.app.toolbar_custom.panels.get(k), Some(v), "{k}");
    }

    choose(&mut h.app, SOLIDCRAFT).unwrap();
    h.frames(2);
    assert_eq!(custom_scope(&h.app), "DESIGN");
    assert!(tc::pinned_on(&h.app, "MODEL", "solid.box").is_some());
}

#[test]
fn an_unknown_stored_profile_falls_back_to_inventor() {
    let mut h = Harness::new();
    h.app.preferences.ui_profile = "solidworks".into();
    assert_eq!(active(&h.app).id, "inventor");
    h.frame();
    assert!(!visible_tabs(&h.app).is_empty());
    assert!(visible_tabs(&h.app).contains(&h.app.ui.tab.as_str()), "{}", h.app.ui.tab);
}

#[test]
fn render_workspace_keeps_its_ribbon_under_inventor() {
    let mut h = inventor(Part);
    crate::render_workspace::enter(&mut h.app, true).unwrap();
    h.frame();
    assert_eq!(visible_tabs(&h.app), ["RENDER"]);
    let upstream = get(SOLIDCRAFT).unwrap().ribbon(Part).tab("RENDER").unwrap();
    let expected: Vec<(&str, Vec<&str>)> = upstream
        .panels
        .iter()
        .map(|p| (p.id.as_str(), p.commands.iter().map(String::as_str).collect::<Vec<_>>()))
        .filter(|(p, c)| !c.is_empty() || *p == "SELECT")
        .collect();
    let lists = panel_lists(&h.app, "RENDER");
    assert!(!lists.is_empty());
    assert_eq!(command_ids(&lists), expected);
}

#[test]
fn a_sheet_body_in_an_assembly_keeps_a_visible_tab() {
    let mut h = inventor(Assembly);
    make_sheet_body(&mut h);
    assert!(visible_tabs(&h.app).contains(&h.app.ui.tab.as_str()), "{}", h.app.ui.tab);
    assert_ne!(h.app.ui.tab, "SHEET METAL");

    let mut h = inventor(Part);
    make_sheet_body(&mut h);
    assert_eq!(h.app.ui.tab, "SHEET METAL");
}

#[test]
fn a_drawing_shows_the_placeholder_ribbon() {
    let h = inventor(Drawing);
    assert_eq!(visible_tabs(&h.app), ["TOOLS", "VIEW"]);
    assert_eq!(h.app.ui.tab, "TOOLS");
}

#[test]
fn first_run_adopts_inventor_and_its_defaults() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    assert_eq!(app.preferences.ui_profile, "");
    first_run(&mut app);
    assert_eq!(app.preferences.ui_profile, "inventor");
    assert_eq!(app.preferences.nav, "inventor");
    assert_eq!(app.preferences.navigator_style, NavigatorStyle::Cube);
    assert!(app.preferences.timeline_left && app.preferences.timeline_under_browser);
}

#[test]
fn first_run_keeps_a_chosen_profile() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    app.preferences.ui_profile = "solidcraft".into();
    first_run(&mut app);
    assert_eq!(app.preferences.ui_profile, "solidcraft");
    assert_eq!(app.preferences.nav, "fusion");
}

#[test]
fn choosing_solidcraft_restores_upstream_defaults() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    choose(&mut app, INVENTOR).unwrap();
    assert_eq!(app.preferences.navigator_style, NavigatorStyle::Cube);
    choose(&mut app, SOLIDCRAFT).unwrap();
    assert_eq!(app.preferences.ui_profile, "solidcraft");
    assert_eq!(app.preferences.nav, "fusion");
    assert_eq!(app.preferences.navigator_style, NavigatorStyle::Axes);
    assert!(!app.preferences.timeline_left);
    assert_eq!(app.preferences.theme, "dark");
    let err = choose(&mut app, "nope").unwrap_err();
    assert!(err.contains("nope"), "{err}");
    assert_eq!(app.preferences.ui_profile, "solidcraft", "a refused choice changes nothing");
}

#[test]
fn preferences_offer_each_profile() {
    let mut h = Harness::new();
    h.call("ui.prefs", json!({"open": "Interface"}));
    h.frames(2);
    assert!(crate::scenario::rect_of("prefs:ui_profile").is_some());
}

#[test]
fn hidden_panels_are_not_drawn() {
    let mut h = inventor(Part);
    h.call("ui.resize", json!({"width": 3200, "height": 1200}));
    h.frames(2);
    let lists = panel_lists(&h.app, "MODEL");
    assert_eq!(panel_ids(&lists), ["SKETCH", "CREATE", "MODIFY", "WORK FEATURES", "PATTERN", "SURFACE", "CONVERT"]);
    assert!(crate::scenario::rect_of("panel:MODEL:PRIMITIVES").is_none());
    assert!(crate::scenario::rect_of("panel:MODEL:CREATE").is_some());

    let mut h = Harness::new();
    h.call("ui.resize", json!({"width": 3200, "height": 1200}));
    h.frames(2);
    for (panel, ..) in panel_lists(&h.app, "MODEL") {
        assert!(crate::scenario::rect_of(&format!("panel:MODEL:{panel}")).is_some(), "{panel}");
    }
}

#[test]
fn both_profiles_group_through_one_function() {
    let h = Harness::new();
    let cmds = panel_commands(&h.app, "MODEL", "MODIFY");
    let ours = headings(&grouped_commands(&h.app, "MODEL", "MODIFY", &cmds));
    assert_eq!(ours, headings(&crate::workspace::grouped_commands("MODEL", "MODIFY", &cmds)));
    assert!(ours.contains(&Some("Edges")));

    let h = inventor(Part);
    let cmds = panel_commands(&h.app, "MODEL", "MODIFY");
    let grouped = grouped_commands(&h.app, "MODEL", "MODIFY", &cmds);
    assert_eq!(grouped.len(), cmds.len());
    assert!(headings(&grouped).iter().all(Option::is_none));
}

/// Carried from Task 2's review: a command placed on a visible panel and on a hidden one goes to
/// the visible one; only a command placed nowhere visible falls back to a hidden panel.
#[test]
fn destination_prefers_a_visible_panel() {
    let ribbon = get(INVENTOR).unwrap().ribbon(Part);
    let visible = |tab: &str, panel: &str| ribbon.tab(tab).and_then(|t| t.panel(panel)).is_some_and(|p| !p.hidden);
    let mut checked = 0;
    for id in ribbon.commands() {
        let anywhere_visible = ribbon.tabs.iter().any(|t| t.panels.iter().any(|p| !p.hidden && p.commands.iter().any(|c| c == id)));
        let (tab, panel) = ribbon.destination(id).unwrap();
        if anywhere_visible {
            assert!(visible(tab, panel), "{id} goes to hidden {tab}/{panel}");
            checked += 1;
        } else {
            assert!(!visible(tab, panel), "{id}: {tab}/{panel}");
        }
    }
    assert!(checked > 0);
    for id in ["parameters.change", "inspect.measure", "file.insert_step", "file.insert_mesh", "canvas.insert"] {
        let (tab, panel) = ribbon.destination(id).unwrap();
        assert!(visible(tab, panel), "{id} goes to hidden {tab}/{panel}");
    }
    // PRIMITIVES is hidden and Box is placed nowhere else: it still has a home.
    assert_eq!(ribbon.destination("solid.box"), Some(("MODEL", "PRIMITIVES")));
}
