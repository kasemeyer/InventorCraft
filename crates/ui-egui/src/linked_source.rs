//! Explicitly choose a linked component's source; all its instances share that source. Extract
//! makes a component that is not linked the source of its own design file.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use serde_json::json;

use crate::SolveApp;

/// Whether `path` is the saved source file of a linked component in `session`.
/// Relative link paths are resolved beside the assembly, just as Update Links resolves them.
pub(crate) fn is_source_of(session: &solidcraft_engine::Session, path: &str) -> bool {
    let target = Path::new(path);
    let target = target.canonicalize().unwrap_or_else(|_| target.to_path_buf());
    session.doc.links.values().any(|link| {
        let source = PathBuf::from(&link.path);
        let source = if source.is_absolute() {
            source
        } else {
            session.path.as_deref().and_then(|p| Path::new(p).parent()).unwrap_or_else(|| Path::new(".")).join(source)
        };
        source.canonicalize().unwrap_or(source) == target
    })
}

/// Open a linked component's source in its own design tab. Closing that tab switches back to the
/// assembly tab, where Save writes any assembly changes as usual.
pub(crate) fn open(app: &mut SolveApp, component: u64) {
    if app.file_picker_busy() || app.file_load.busy() {
        return;
    }
    if let Ok(status) = app.session.execute("component.source_status", &json!({}))
        && status["sources"].as_array().into_iter().flatten().any(|r| r["component"] == component && r["dirty"] == true)
    {
        crate::source_save::request(app);
        app.set_status("Save the edited part in the file list, then choose Open again.", false);
        return;
    }
    let Some(link) = app.session.doc.links.get(&component) else {
        app.set_status("This component has no linked source design", true);
        return;
    };
    let source = PathBuf::from(&link.path);
    let path = if source.is_absolute() {
        source
    } else {
        app.session.path.as_deref().and_then(|p| Path::new(p).parent()).unwrap_or_else(|| Path::new(".")).join(source)
    };
    let path = path.canonicalize().unwrap_or(path);
    if !path.is_file() {
        app.set_status(format!("Linked source file not found: {}", path.display()), true);
        return;
    }
    let same_file = |candidate: &str| {
        let candidate = Path::new(candidate);
        candidate.canonicalize().unwrap_or_else(|_| candidate.to_path_buf()) == path
    };
    if let Some(tab) = crate::documents::tab_paths(app).iter().position(|p| p.as_deref().is_some_and(same_file)) {
        crate::documents::switch(app, tab);
    } else {
        app.open_path_async(&path.to_string_lossy());
    }
}

pub(crate) fn start_selected(app: &mut SolveApp) {
    let mut components = app.tree.picked_components.clone();
    for selection in &app.session.selection {
        if let solidcraft_engine::Sel::Occurrence { path } = selection
            && let Ok(placement) = app.session.occurrence_placement(path)
        {
            components.push(placement.component);
        }
    }
    components.sort_unstable();
    components.dedup();
    if let [component] = components.as_slice()
        && app.session.doc.links.contains_key(component)
    {
        start(app, *component);
    } else {
        app.set_status(app.tr("ui.relink_select_one").to_string(), true);
    }
}

pub(crate) fn start(app: &mut SolveApp, component: u64) {
    if app.file_picker_busy() || app.file_load.busy() {
        return;
    }
    if !app.session.doc.links.contains_key(&component) {
        app.set_status(app.tr_label("ui.relink_not_linked").to_string(), true);
        return;
    }
    let document = app.session.doc.clone();
    app.pick_open_for(&solidcraft_engine::io::DESIGN_EXTS, move |app, path| {
        let Some(path) = path else { return };
        // A host or programmatic caller can replace/edit a design while a native picker is
        // pending. Never apply that old component id to a different design or newer revision.
        if !Arc::ptr_eq(&document, &app.session.doc) {
            app.set_status(app.tr_label("ui.relink_design_changed").to_string(), true);
            return;
        }
        if let Ok(result) = app.run("component.relink", json!({"component": component, "path": path})) {
            let warnings: Vec<&str> = result["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str()).collect();
            if warnings.is_empty() {
                app.set_status(app.tr_label("ui.relink_done").to_string(), false);
            } else {
                let text = format!("{} {}", app.tr_label("ui.relink_done"), warnings.join("; "));
                app.session.echo(text.clone());
                app.set_status(text, true);
            }
        }
    });
}

/// Ask where the component's design goes, then move its history there and link it.
pub(crate) fn extract(app: &mut SolveApp, component: u64) {
    if app.file_picker_busy() || app.file_load.busy() {
        return;
    }
    let Some(name) = app.session.doc.components.iter().find(|c| c.id == component).map(|c| c.name.clone()) else { return };
    let document = app.session.doc.clone();
    // The design extensions a component is extracted to: not a drawing's, nor the legacy one.
    let extensions: Vec<&str> = solidcraft_engine::io::DESIGN_EXTS
        .into_iter()
        .filter(|e| {
            *e != solidcraft_engine::io::LEGACY_DESIGN_EXT
                && solidcraft_engine::doc::DocumentKind::from_extension(e) != Some(solidcraft_engine::doc::DocumentKind::Drawing)
        })
        .collect();
    app.pick_save(&format!("{name}.solidcraft"), &extensions, move |app, path| {
        let Some(mut path) = path else { return };
        if !Arc::ptr_eq(&document, &app.session.doc) {
            app.set_status(app.tr_label("ui.extract_design_changed").to_string(), true);
            return;
        }
        if std::path::Path::new(&path).extension().is_none() {
            path.push_str(".solidcraft");
        }
        if let Ok(result) = app.run("component.extract", json!({"component": component, "path": path})) {
            report(app, &result, "ui.extract_done");
            let copies: Vec<(u64, String)> = result["copies"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| Some((c["component"].as_u64()?, c["name"].as_str().unwrap_or_default().to_string())))
                .collect();
            if !copies.is_empty() {
                app.menu.link_copies = Some(Copies { linked: component, document: app.session.doc.clone(), copies });
            }
        }
    });
}

/// The status line after a command: `done`, then its warnings and notices.
fn report(app: &mut SolveApp, result: &serde_json::Value, done: &str) {
    let notes: Vec<&str> =
        ["warnings", "notices"].iter().flat_map(|k| result[*k].as_array().into_iter().flatten()).filter_map(|w| w.as_str()).collect();
    if notes.is_empty() {
        app.set_status(app.tr_label(done).to_string(), false);
    } else {
        let text = format!("{} {}", app.tr_label(done), notes.join("; "));
        app.session.echo(text.clone());
        app.set_status(text, true);
    }
}

/// Components Extract to File found to build the same bodies as the one it linked.
#[derive(Clone, Debug)]
pub struct Copies {
    linked: u64,
    document: Arc<solidcraft_engine::doc::Document>,
    copies: Vec<(u64, String)>,
}

/// Link the copies too, or leave them as they are (also the card's buttons).
pub fn link_copies(app: &mut SolveApp, accept: bool) -> bool {
    let Some(c) = app.menu.link_copies.take() else { return false };
    // The design changed since they were found: the ids may name something else now
    if accept && Arc::ptr_eq(&c.document, &app.session.doc) {
        let ids: Vec<u64> = c.copies.iter().map(|(id, _)| *id).collect();
        if let Ok(result) = app.run("component.replace_with_link", json!({"components": ids, "with": c.linked})) {
            report(app, &result, "ui.replace_with_link_done");
        }
    }
    true
}

/// The card offering to link the copies Extract to File found.
pub fn show_copies(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(c) = app.menu.link_copies.clone() else { return };
    let name = app.session.doc.components.iter().find(|x| x.id == c.linked).map(|x| x.name.clone()).unwrap_or_default();
    let (closed, action) = crate::ui_kit::Confirm::new(egui::Id::new("sc_link_copies"), "help", crate::i18n::tr_label("Link Identical Copies"))
        .actions("Link", "Keep", None)
        .show(ctx, true, |ui| {
            let text = crate::i18n::format("ui.link_copies_text", &[("name", &name)]);
            crate::ui_kit::text(ui, &text, crate::ui_kit::Text::Report("copies", false));
            for (id, copy) in &c.copies {
                crate::ui_kit::text(ui, &format!("  • {copy}"), crate::ui_kit::Text::Report(&format!("copy:{id}"), false));
            }
        });
    match action {
        crate::ui_kit::Action::Ok => drop(link_copies(app, true)),
        crate::ui_kit::Action::Cancel => drop(link_copies(app, false)),
        _ if closed => drop(link_copies(app, false)),
        _ => {}
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::context_menu::{self, Target};
    use solidcraft_engine::Session;
    use std::path::PathBuf;

    #[test]
    fn source_save_list_selects_files_and_save_all_finishes_the_round_trip() {
        let mut h = crate::test_support::inventor(solidcraft_engine::doc::DocumentKind::Assembly);
        let mut files = Vec::new();
        let mut components = Vec::new();
        for n in 0..2 {
            let path = h.dir.join(format!("Source{n}.tcipart"));
            let mut source = Session::default();
            source.execute("document.new_part", &json!({})).unwrap();
            source.execute("solid.box", &json!({"length":20,"width":20,"height":10})).unwrap();
            source.execute("file.save", &json!({"path":path})).unwrap();
            let made = h.app.run("component.insert_design", json!({"path":path})).unwrap();
            let c = made["component"].as_u64().unwrap();
            h.app.run("component.activate", json!({"component":c})).unwrap();
            h.app.run("solid.box", json!({"length":3,"width":3,"height":3,"corner":[30,0,0]})).unwrap();
            h.app.run("component.finish_edit", json!({})).unwrap();
            files.push(path);
            components.push(c);
        }
        h.app.run("file.save", json!({"path":h.dir.join("Assembly.tciasm")})).unwrap();
        crate::toolbar::save(&mut h.app);
        let mut panel = h.app.docs.source_save.take().unwrap();
        assert_eq!(panel.rows.len(), 2);
        panel.rows.iter_mut().find(|r| r.component == components[0]).unwrap().save = false;
        crate::source_save::apply(&mut h.app, panel, false);
        h.settle();
        let count = |path: &std::path::Path| {
            let (doc, _) =
                solidcraft_engine::io::read_design_file(&path.to_string_lossy(), None::<fn(solidcraft_engine::io::DesignPreview)>).unwrap();
            doc.features.len()
        };
        assert_eq!(count(&files[0]), 1);
        assert_eq!(count(&files[1]), 2);
        assert!(h.app.session.is_dirty());
        crate::toolbar::save(&mut h.app);
        let panel = h.app.docs.source_save.take().unwrap();
        assert_eq!(panel.rows.len(), 1);
        crate::source_save::apply(&mut h.app, panel, true);
        h.settle();
        assert!(!h.app.session.is_dirty());
        open(&mut h.app, components[0]);
        h.settle();
        assert_eq!(h.app.session.doc.features.len(), 2);
        assert_eq!(std::path::Path::new(h.app.session.path.as_deref().unwrap()).canonicalize().unwrap(), files[0].canonicalize().unwrap());
    }

    #[test]
    fn source_save_cancel_and_missing_source_leave_edits_pending() {
        let mut h = crate::test_support::inventor(solidcraft_engine::doc::DocumentKind::Assembly);
        let path = h.dir.join("Part.tcipart");
        let mut part = Session::default();
        part.execute("document.new_part", &json!({})).unwrap();
        part.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
        part.execute("file.save", &json!({"path":path})).unwrap();
        let c = h.app.run("component.insert_design", json!({"path":path})).unwrap()["component"].as_u64().unwrap();
        h.app.run("component.activate", json!({"component":c})).unwrap();
        h.app.run("solid.box", json!({"length":2,"width":2,"height":2,"corner":[20,0,0]})).unwrap();
        h.app.run("component.finish_edit", json!({})).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        crate::toolbar::save(&mut h.app);
        h.frames(3);
        h.step(&json!({"widget":"Cancel"})).unwrap();
        assert!(h.app.docs.source_save.is_none());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(h.app.session.is_dirty());
        std::fs::remove_file(&path).unwrap();
        crate::toolbar::save(&mut h.app);
        let panel = h.app.docs.source_save.take().unwrap();
        crate::source_save::apply(&mut h.app, panel, true);
        assert!(h.app.docs.source_save.as_ref().is_some_and(|p| p.error.is_some()));
        assert!(h.app.session.path.is_none());
        assert!(h.app.session.is_dirty());
    }

    #[test]
    fn assembly_save_includes_a_dirty_linked_part_open_in_another_tab() {
        let mut h = crate::test_support::inventor(solidcraft_engine::doc::DocumentKind::Assembly);
        let file = h.dir.join("OpenPart.tcipart");
        let mut part = Session::default();
        part.execute("document.new_part", &json!({})).unwrap();
        part.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
        part.execute("file.save", &json!({"path":file})).unwrap();
        h.app.run("component.insert_design", json!({"path":file})).unwrap();
        h.app.run("file.save", json!({"path":h.dir.join("Assembly.tciasm")})).unwrap();
        crate::documents::open_tab(&mut h.app, part).unwrap();
        h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"corner":[20,0,0]})).unwrap();
        crate::documents::switch(&mut h.app, 0);
        crate::toolbar::save(&mut h.app);
        let panel = h.app.docs.source_save.take().unwrap();
        assert_eq!(panel.rows.len(), 1);
        assert_eq!(panel.rows[0].tab, Some(1));
        crate::source_save::apply(&mut h.app, panel, true);
        h.settle();
        assert_eq!(h.app.session.world_state().bodies.len(), 2);
        assert!(!h.app.session.is_dirty());
        crate::documents::switch(&mut h.app, 1);
        assert!(!h.app.session.is_dirty());
        assert_eq!(h.app.session.doc.features.len(), 2);
    }

    fn fixture(name: &str) -> (SolveApp, u64, u64, PathBuf, PathBuf) {
        let folder = std::env::temp_dir().join(format!("solidcraft-relink-ui-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let source = folder.join("source.solidcraft");
        let moved = folder.join("moved.solidcraft");
        let mut part = Session::default();
        part.execute("solid.box", &json!({"length": 10, "width": 20, "height": 30})).unwrap();
        part.execute("file.save", &json!({"path": source})).unwrap();
        let mut app = SolveApp::new(Session::default(), Default::default());
        let inserted = app.run("component.insert_design", json!({"path": source})).unwrap();
        let component = inserted["component"].as_u64().unwrap();
        let occurrence = inserted["occurrence"].as_u64().unwrap();
        std::fs::rename(&source, &moved).unwrap();
        (app, component, occurrence, folder, moved)
    }

    #[test]
    fn switching_back_to_assembly_refreshes_a_saved_linked_source() {
        let folder = std::env::temp_dir().join(format!("solidcraft-linked-source-return-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let source = folder.join("pipe.tcipart");
        let mut original = Session::default();
        original.execute("document.new_part", &json!({})).unwrap();
        original.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
        original.execute("file.save", &json!({"path":source})).unwrap();

        let mut h = crate::test_support::inventor(solidcraft_engine::doc::DocumentKind::Assembly);
        h.app.run("component.insert_design", json!({"path":source})).unwrap();
        h.app.run("component.insert_design", json!({"path":source})).unwrap();
        assert_eq!(h.app.session.world_state().bodies.len(), 2);
        h.app.session.mark_saved();

        let mut edited = Session::default();
        edited.execute("document.new_part", &json!({})).unwrap();
        edited.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
        edited.execute("solid.box", &json!({"length":5,"width":5,"height":5,"corner":[20,0,0]})).unwrap();
        edited.execute("file.save", &json!({"path":source})).unwrap();
        crate::documents::open_tab(&mut h.app, edited).unwrap();
        crate::documents::switch(&mut h.app, 0);

        assert_eq!(h.app.session.world_state().bodies.len(), 4, "both occurrences use the updated source");
        let _ = std::fs::remove_dir_all(folder);
    }

    fn menu(app: &SolveApp, target: Target) -> Option<context_menu::Item> {
        context_menu::items(app, &target).into_iter().find(|i| i.id == "ui.relinkSource")
    }

    fn answer(app: &mut SolveApp, path: Option<String>) {
        app.services.pick_open = Some(Box::new(move |extensions| {
            assert_eq!(extensions, &["tcipart", "tciasm", "tcidrw", "solidcraft", "solvecraft"]);
            let result = path.clone();
            Box::pin(async move { result })
        }));
    }

    #[test]
    fn relink_menu_uses_the_selected_link_from_components_and_occurrences() {
        let (mut app, component, occurrence, folder, moved) = fixture("menu");
        let second = app.run("occurrence.copy", json!({"occurrence": occurrence, "translate": [90,0,0]})).unwrap()["occurrence"].as_u64().unwrap();
        let occurrences = app.session.doc.occurrences.clone();
        let item = menu(&app, Target::Component { id: component }).unwrap();
        assert!(item.label.contains("All Instances"));
        assert_eq!(menu(&app, Target::Occurrence { path: vec![second] }).unwrap().params, Some(json!({"component": component})));
        assert!(menu(&app, Target::Component { id: 0 }).is_none());
        answer(&mut app, Some(moved.to_string_lossy().into_owned()));
        let before = app.session.doc.clone();
        context_menu::run_item(&mut app, &item, egui::Pos2::ZERO);
        assert!(app.file_picker_busy());
        assert!(Arc::ptr_eq(&before, &app.session.doc), "choosing is not an edit");
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert!(!app.file_picker_busy());
        assert!(app.session.doc.links[&component].path.ends_with("moved.solidcraft"));
        assert_eq!(app.session.doc.occurrences, occurrences);
        assert!(app.status.as_ref().is_some_and(|(s, _, error)| s.contains("all instances") && !error));
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(serde_json::to_value(&*app.session.doc).unwrap(), serde_json::to_value(&*before).unwrap());
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn relink_picker_cancel_bad_file_and_changed_document_keep_the_link() {
        let (mut app, component, _, folder, moved) = fixture("cancel");
        let before = serde_json::to_value(&*app.session.doc).unwrap();
        let undo = app.session.undo.len();
        answer(&mut app, None);
        start(&mut app, component);
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert_eq!(serde_json::to_value(&*app.session.doc).unwrap(), before);
        assert_eq!(app.session.undo.len(), undo);
        answer(&mut app, Some(folder.join("missing.solidcraft").to_string_lossy().into_owned()));
        start(&mut app, component);
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert_eq!(serde_json::to_value(&*app.session.doc).unwrap(), before);
        assert_eq!(app.session.undo.len(), undo);
        assert!(app.status.as_ref().is_some_and(|(_, _, error)| *error));
        answer(&mut app, Some(moved.to_string_lossy().into_owned()));
        start(&mut app, component);
        // The ordinary UI blocks edits while picking. Simulate a host replacing/editing the
        // session directly to exercise the continuation's independent stale-target guard.
        app.session.execute("component.rename", &json!({"component": component, "name": "Changed"})).unwrap();
        let changed = serde_json::to_value(&*app.session.doc).unwrap();
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert_eq!(serde_json::to_value(&*app.session.doc).unwrap(), changed);
        assert!(app.status.as_ref().is_some_and(|(s, _, error)| s.contains("assembly changed") && *error));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn relink_start_requires_one_linked_selection_and_opens_its_picker() {
        let (mut app, component, occurrence, folder, moved) = fixture("start");
        answer(&mut app, Some(moved.to_string_lossy().into_owned()));
        app.start("component.relink");
        assert!(!app.file_picker_busy());
        assert!(app.status.as_ref().is_some_and(|(s, _, error)| s.contains("Select one linked") && *error));
        app.session.selection = vec![solidcraft_engine::Sel::Occurrence { path: vec![occurrence] }];
        app.start("component.relink");
        assert!(app.file_picker_busy());
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert!(app.session.doc.links[&component].path.ends_with("moved.solidcraft"));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn extract_menu_asks_where_to_save_and_links_the_component_there() {
        let folder = std::env::temp_dir().join(format!("solidcraft-extract-ui-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let mut app = SolveApp::new(Session::default(), Default::default());
        let made = app.run("component.create", json!({"name": "Bracket", "grounded": false})).unwrap();
        let (component, occurrence) = (made["component"].as_u64().unwrap(), made["occurrence"].as_u64().unwrap());
        app.run("solid.box", json!({"length": 10, "width": 20, "height": 30})).unwrap();
        let find = |app: &SolveApp, id: u64| context_menu::items(app, &Target::Component { id }).into_iter().find(|i| i.id == "ui.extractComponent");
        assert!(find(&app, 0).is_none());
        // Not offered while the component can't go: here, moved without Capture Position
        app.run("occurrence.move", json!({"occurrence": occurrence, "translate": [5, 0, 0], "capture": false})).unwrap();
        assert!(find(&app, component).is_none());
        app.run("component.revert_position", json!({})).unwrap();
        // In Relink Source's place, right after Properties
        let ids: Vec<String> = context_menu::items(&app, &Target::Component { id: component }).into_iter().map(|i| i.id).collect();
        let properties = ids.iter().position(|i| i == "ui.properties").unwrap();
        assert_eq!(ids.get(properties + 1).map(String::as_str), Some("ui.extractComponent"));
        let item = find(&app, component).unwrap();
        let chosen = folder.join("bracket").to_string_lossy().into_owned();
        app.services.pick_save = Some(Box::new(move |name, extensions| {
            assert_eq!((name, extensions), ("Bracket.solidcraft", &["tcipart", "tciasm", "solidcraft"][..]));
            let result = Some(chosen.clone());
            Box::pin(async move { result })
        }));
        context_menu::run_item(&mut app, &item, egui::Pos2::ZERO);
        assert!(app.file_picker_busy());
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert!(folder.join("bracket.solidcraft").is_file());
        assert!(app.session.doc.links[&component].path.ends_with("bracket.solidcraft"));
        assert!(app.status.as_ref().is_some_and(|(s, _, error)| s.contains("its own design") && !error));
        assert!(find(&app, component).is_none(), "a linked component is not extracted again");
        assert!(menu(&app, Target::Component { id: component }).is_some());
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn extract_offers_to_link_identical_copies() {
        let folder = std::env::temp_dir().join(format!("solidcraft-extract-copies-ui-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let mut app = SolveApp::new(Session::default(), Default::default());
        let made = app.run("component.create", json!({"name": "Bracket"})).unwrap();
        let (component, occurrence) = (made["component"].as_u64().unwrap(), made["occurrence"].as_u64().unwrap());
        app.run("solid.box", json!({"length": 10, "width": 20, "height": 30})).unwrap();
        app.run("component.activate", json!({"component": 0})).unwrap();
        let copy = app.run("component.paste_new", json!({"component": component, "occurrence": occurrence, "translate": [50, 0, 0]})).unwrap();
        let (copy, copy_occurrence) = (copy["component"].as_u64().unwrap(), copy["occurrence"].as_u64().unwrap());
        let chosen = folder.join("bracket.solidcraft").to_string_lossy().into_owned();
        app.services.pick_save = Some(Box::new(move |_, _| {
            let result = Some(chosen.clone());
            Box::pin(async move { result })
        }));
        extract(&mut app, component);
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert!(app.session.doc.links.contains_key(&component));
        assert_eq!(app.menu.link_copies.as_ref().map(|c| c.copies.iter().map(|(id, _)| *id).collect::<Vec<_>>()), Some(vec![copy]));
        // Keep leaves the copy alone; Link makes its placement one of the linked component's
        let mut kept = app.menu.link_copies.clone();
        assert!(link_copies(&mut app, false));
        assert!(app.session.doc.components.iter().any(|c| c.id == copy));
        app.menu.link_copies = kept.take();
        assert!(link_copies(&mut app, true));
        assert!(app.session.doc.components.iter().all(|c| c.id != copy));
        assert_eq!(app.session.doc.occurrences.iter().find(|o| o.id == copy_occurrence).map(|o| o.component), Some(component));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn relink_displays_the_warning_when_changed_geometry_has_joints() {
        let (mut app, component, occurrence, folder, moved) = fixture("warning");
        app.run(
            "joint.create",
            json!({"name": "Seat", "type": "rigid", "a": {"occurrence": 0, "point": [0,0,0]}, "b": {"occurrence": occurrence, "point": [0,0,0]}}),
        )
        .unwrap();
        let mut changed = Session::default();
        changed.execute("solid.box", &json!({"length": 10, "width": 20, "height": 45})).unwrap();
        changed.execute("file.save", &json!({"path": moved})).unwrap();
        answer(&mut app, Some(moved.to_string_lossy().into_owned()));
        start(&mut app, component);
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert!(app.status.as_ref().is_some_and(|(s, _, error)| s.contains("check this component's joints") && *error));
        assert!(app.session.log.iter().any(|s| s.contains("check this component's joints")));
        let _ = std::fs::remove_dir_all(folder);
    }
}
