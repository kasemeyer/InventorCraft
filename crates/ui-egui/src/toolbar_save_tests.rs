#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Save and Save As name the document kind's own extension (wave 3 item 6, R102; Plan A Q1):
//! a part saves as `.tcipart`, an assembly as `.tciasm` (a drawing already saved as `.tcidrw`).
//! The kind-less `.solidcraft` is offered only as a second choice, and only for a file that
//! already has it; a plain Save of such a file keeps it.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::json;
use solidcraft_engine::doc::DocumentKind::{self, Assembly, Part};

use super::*;
use crate::scenario::Harness;
use crate::test_support::inventor;

/// What each Save As asked of the host's save dialog: (suggested name, filter extensions).
type Asked = Rc<RefCell<Vec<(String, Vec<String>)>>>;

/// The host's save dialog, cancelled each time, recording what it was asked.
fn record(h: &mut Harness) -> Asked {
    let asked: Asked = Rc::default();
    let log = asked.clone();
    h.app.services.pick_save = Some(Box::new(move |name: &str, exts: &[&str]| {
        log.borrow_mut().push((name.to_string(), exts.iter().map(|e| e.to_string()).collect()));
        Box::pin(std::future::ready(None))
    }));
    asked
}

fn last(asked: &Asked) -> (String, Vec<String>) {
    asked.borrow().last().cloned().expect("Save As asked nothing")
}

/// A new design of `kind` under Inventor's profile or SolidCraft's (whose blank design is an
/// assembly-capable design until made a part, so the kind is set as `inventor` sets it).
fn harness(inventor_profile: bool, kind: DocumentKind) -> Harness {
    let mut h = if inventor_profile { inventor(kind) } else { Harness::new() };
    if !inventor_profile {
        h.app.session.doc_mut().kind = kind;
        h.app.session.mark_saved();
    }
    h.app.home.open = false;
    h.frames(1);
    h
}

#[test]
fn a_new_part_saves_as_tcipart() {
    for profile in [true, false] {
        let mut h = harness(profile, Part);
        assert_eq!(h.app.session.doc.kind, Part);
        let asked = record(&mut h);
        save_as(&mut h.app);
        h.frames(1);
        let (name, exts) = last(&asked);
        assert_eq!(name, format!("{}.tcipart", h.app.session.doc.name), "inventor {profile}");
        assert_eq!(exts, ["tcipart"], "inventor {profile}");
        // A design never saved: Save is Save As.
        save(&mut h.app);
        h.frames(1);
        assert_eq!(last(&asked).1, ["tcipart"]);
    }
    // A new design's name.
    let h = harness(true, Part);
    assert_eq!(format!("{}.tcipart", h.app.session.doc.name), "Untitled.tcipart");
}

#[test]
fn an_assembly_saves_as_tciasm() {
    for profile in [true, false] {
        let mut h = harness(profile, Assembly);
        assert_eq!(h.app.session.doc.kind, Assembly, "inventor {profile}");
        let asked = record(&mut h);
        save_as(&mut h.app);
        h.frames(1);
        let (name, exts) = last(&asked);
        assert!(name.ends_with(".tciasm"), "{name}");
        assert_eq!(exts, ["tciasm"]);
    }
}

#[test]
fn a_legacy_part_offers_solidcraft_second_and_save_keeps_it() {
    for profile in [true, false] {
        let mut h = harness(profile, Part);
        let legacy = h.dir.join("legacy.solidcraft").to_string_lossy().into_owned();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        h.app.run("file.save_as", json!({"path": legacy})).unwrap();
        assert!(h.app.session.path.as_deref().is_some_and(|p| p.ends_with(".solidcraft")));
        let asked = record(&mut h);
        save_as(&mut h.app);
        h.frames(1);
        let (name, exts) = last(&asked);
        assert_eq!(name, format!("{}.tcipart", h.app.session.doc.name));
        assert_eq!(exts, ["tcipart", "solidcraft"], "inventor {profile}");
        // A plain Save writes the same legacy file (Plan A Q1).
        h.app.run("solid.sphere", json!({"diameter": 4})).unwrap();
        save(&mut h.app);
        crate::save_job::tick(&mut h.app, true);
        h.frames(1);
        assert_eq!(h.app.session.path.as_deref(), Some(legacy.as_str()));
        assert!(!h.app.session.is_dirty(), "saved");
        assert_eq!(asked.borrow().len(), 1, "Save asked for no file");
        assert!(std::path::Path::new(&legacy).is_file());
    }
}
