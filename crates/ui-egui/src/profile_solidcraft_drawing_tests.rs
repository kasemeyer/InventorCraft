//! SolidCraft's minimal DRAWING tab (Plan D-2 Task 2; spec §5.2), read from its own
//! `workspace::DRAWING_SECTIONS` table so the Part and Assembly ribbons stay as they were (R67).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use solidcraft_engine::doc::DocumentKind::*;

use crate::workspace;

fn ids(panel: &RibbonPanel) -> Vec<&str> {
    panel.commands.iter().map(String::as_str).collect()
}

#[test]
fn solidcraft_drawing_ribbon_is_minimal() {
    let p = solidcraft();
    let ribbon = p.ribbon(Drawing);
    let tabs: Vec<&str> = ribbon.tabs.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(tabs, ["DRAWING", "VIEW"]);
    assert!(ribbon.tabs.iter().all(|t| t.shown == Shown::Always && t.label.is_none()));
    let drawing = ribbon.tab("DRAWING").unwrap();
    let got: Vec<(&str, Vec<&str>)> = drawing.panels.iter().map(|panel| (panel.id.as_str(), ids(panel))).collect();
    assert_eq!(
        got,
        [
            ("VIEWS", vec!["drawing.view.base", "drawing.view.projected"]),
            (
                "ANNOTATE",
                vec![
                    "drawing.dim.general",
                    "drawing.note.text",
                    "drawing.note.leader",
                    "drawing.centermark",
                    "drawing.centerline.bisector",
                    "drawing.note.hole",
                ]
            ),
            ("SHEET", vec!["drawing.sheet.add", "drawing.update"]),
            ("EXPORT", vec!["drawing.export.pdf", "drawing.export.dxf"]),
        ]
    );
    // Every button is on the face, and a long engine label has a compact ribbon name.
    for panel in &drawing.panels {
        assert_eq!(panel.promoted, panel.commands.len(), "{}", panel.id);
        assert!(panel.small.is_empty() && panel.grid.is_empty() && panel.dropdowns.is_empty() && !panel.hidden, "{}", panel.id);
        for id in &panel.commands {
            let label = find_command(id).unwrap().label;
            if label.chars().count() > 12 || label.split_whitespace().any(|w| w.chars().count() > 9) {
                assert!(workspace::RIBBON_LABELS.iter().any(|(c, _)| c == id), "missing compact label for {id}");
            }
        }
    }
    // SolidCraft's own VIEW tab, as on a part.
    assert_eq!(ribbon.tab("VIEW"), p.ribbon(Part).tab("VIEW"));
    assert_eq!(ribbon.home(), "DRAWING");
    assert_eq!(ribbon.destination("drawing.dim.general"), Some(("DRAWING", "ANNOTATE")));
    assert_eq!(validate(&p), Vec::<String>::new());
}

/// Review Focus (R67): the Part and Assembly ribbons, their homes and search are as before.
#[test]
fn solidcraft_part_and_assembly_ribbons_are_unchanged() {
    let p = solidcraft();
    let specs = command_specs();
    let mut want_tabs: Vec<&str> = workspace::TABS.to_vec();
    want_tabs.extend(["SKETCH", "SHEET METAL", "RENDER"]);
    for kind in [Part, Assembly] {
        let ribbon = p.ribbon(kind);
        let tabs: Vec<&str> = ribbon.tabs.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(tabs, want_tabs, "{kind:?}");
        for tab in &ribbon.tabs {
            for panel in &tab.panels {
                let (cmds, promoted) = workspace::layout(&tab.id, &panel.id, &specs).unwrap();
                let want: Vec<&str> = cmds.iter().map(|c| c.id).collect();
                assert_eq!(ids(panel), want, "{kind:?} {}/{}", tab.id, panel.id);
                assert_eq!(panel.promoted, promoted, "{kind:?} {}/{}", tab.id, panel.id);
            }
        }
        assert!(ribbon.commands().iter().all(|c| !c.starts_with("drawing.")), "{kind:?}");
        for c in &specs {
            assert_eq!(ribbon.destination(c.id), workspace::destination(c), "{kind:?} {}", c.id);
        }
    }
    for c in specs.iter().filter(|c| c.id.starts_with("drawing.")) {
        assert_eq!(workspace::destination(c), None, "{}", c.id);
    }
    assert_eq!(p.ribbon_identity(Part), p.ribbon_identity(Assembly));
    assert_ne!(p.ribbon_identity(Part), p.ribbon_identity(Drawing));
}
