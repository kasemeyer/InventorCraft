#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The ribbon's own menu lists the tab's panels to show or hide (Inventor's Show Panels), and
//! the Inventor profile leaves Q and M as the owner set them in a Part.

use super::*;
use crate::profile::AliasScope;
use crate::scenario::Harness;
use solidcraft_engine::doc::DocumentKind;

fn sketch_tab() -> Harness {
    let mut h = crate::test_support::inventor(DocumentKind::Part);
    h.app.ui.tab = "SKETCH".into();
    h
}

fn panels_item(app: &SolveApp) -> Item {
    toolbar_items(app).into_iter().find(|i| i.id == "ui.toolbarPanels").expect("the ribbon menu offers Show Panels")
}

fn child(app: &SolveApp, panel: &str) -> Item {
    panels_item(app)
        .children
        .into_iter()
        .find(|c| c.params.as_ref().is_some_and(|p| p["panel"] == panel))
        .unwrap_or_else(|| panic!("{panel} is listed"))
}

fn shown(app: &SolveApp, tab: &str, panel: &str) -> bool {
    panel_lists(app, tab).iter().any(|(p, ..)| *p == panel)
}

#[test]
fn the_ribbon_menu_lists_this_tabs_panels() {
    let h = sketch_tab();
    let create = child(&h.app, "CREATE");
    assert_eq!(create.id, "ui.toolbarPanelShown");
    assert_eq!(create.shortcut, "current", "a shown panel is checked");
    assert_eq!(create.params.as_ref().unwrap()["shown"], json!(false), "choosing it hides it");
    let parameters = child(&h.app, "PARAMETERS");
    assert_eq!(parameters.shortcut, "");
    assert_eq!(parameters.params.as_ref().unwrap()["shown"], json!(true), "choosing it shows it");
    assert_eq!(parameters.params.as_ref().unwrap()["tab"], json!("SKETCH"));
}

#[test]
fn showing_a_hidden_panel_draws_it() {
    let mut h = sketch_tab();
    assert!(!shown(&h.app, "SKETCH", "PARAMETERS"));
    let item = child(&h.app, "PARAMETERS");
    crate::context_menu::run_item(&mut h.app, &item, egui::Pos2::ZERO);
    assert!(shown(&h.app, "SKETCH", "PARAMETERS"));
    let again = child(&h.app, "PARAMETERS");
    assert_eq!(again.params.as_ref().unwrap()["shown"], json!(false));
    crate::context_menu::run_item(&mut h.app, &again, egui::Pos2::ZERO);
    assert!(!shown(&h.app, "SKETCH", "PARAMETERS"));
}

#[test]
fn q_and_m_are_not_solidcrafts_in_a_part() {
    let inventor = crate::profile::get("inventor").expect("the Inventor profile");
    let in_a_part: Vec<_> = inventor
        .aliases
        .iter()
        .filter(|a| matches!(a.scope, AliasScope::Everywhere | AliasScope::Model | AliasScope::Part | AliasScope::Sketch | AliasScope::SketchWrapper))
        .collect();
    assert!(!in_a_part.is_empty());
    assert!(in_a_part.iter().all(|a| a.keys != "Q"), "Q is unbound in a Part");
    assert!(in_a_part.iter().all(|a| a.command != "solid.press_pull" && a.command != "solid.move"), "no key reaches press pull or move in a Part");
    let m: Vec<_> = in_a_part.iter().filter(|a| a.keys == "M").collect();
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].command, "inspect.measure");
}
