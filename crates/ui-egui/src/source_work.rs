//! Linked-source transitions compute transactionally on workers, independent of agent settings.
//! The live tabs keep their sessions, cameras and undo stacks until every result is ready.

use std::sync::{Arc, mpsc};

use serde_json::Value;
use solidcraft_engine::{Session, doc::Document};

use crate::{SolveApp, documents, source_sessions};

#[derive(Clone)]
pub(crate) enum Action {
    Edit(Value),
    Command(String, Value),
    Open(u64),
    Switch(usize),
    Place(String),
}

pub(crate) enum Followup {
    Feature(u64),
    Sketch(u64),
    Pattern(u64, String),
}

pub(crate) fn after(app: &mut SolveApp, next: Followup) -> bool {
    if let Some(work) = &mut app.source_work {
        work.followup = Some(next);
        true
    } else {
        false
    }
}

struct Base {
    doc: Arc<Document>,
    saved: Arc<Document>,
    revision: u64,
    path: Option<String>,
}

struct Made {
    sessions: Vec<Session>,
    active: usize,
    shared: std::collections::BTreeMap<std::path::PathBuf, Document>,
    checked: Vec<source_sessions::Checkpoint>,
    files: std::collections::BTreeMap<std::path::PathBuf, Vec<source_sessions::FileCheck>>,
    place: Option<crate::place_component::Place>,
    action: Action,
}

pub(crate) struct Work {
    rx: mpsc::Receiver<Result<Made, String>>,
    bases: Vec<Base>,
    active: usize,
    progress: Arc<solidcraft_engine::doc::progress::Progress>,
    followup: Option<Followup>,
}

impl Drop for Work {
    fn drop(&mut self) {
        self.progress.cancel();
    }
}

pub(crate) fn begin(app: &mut SolveApp, action: Action) -> bool {
    if !app.source_background || cfg!(target_arch = "wasm32") {
        return false;
    }
    if app.source_work.is_some() {
        return true;
    }
    if app.file_load.busy() || app.file_picker_busy() || app.deferred.is_some() {
        return false;
    }
    match &action {
        Action::Edit(p) => {
            let root = p.get("component").is_some_and(|c| c == 0 || c == "root");
            if root || !crate::profile::active(app).workflow.component_edit_environment {
                return false;
            }
            if (0..app.docs.count()).all(|i| documents::session_at(app, i).is_none_or(|s| s.doc.links.is_empty())) {
                return false;
            }
            if p["component"].as_u64().is_some_and(|c| app.session.doc.source_edits.contains_key(&c)) && source_sessions::unchanged(app) {
                return false;
            }
        }
        Action::Switch(i) => {
            let related = (0..app.docs.count()).any(|n| documents::session_at(app, n).is_some_and(|s| !s.doc.links.is_empty()));
            if !related {
                return false;
            }
            let external = app
                .session
                .path
                .clone()
                .filter(|p| documents::session_at(app, *i).is_some_and(|s| crate::linked_source::is_source_of(s, p)))
                .is_some_and(|p| source_sessions::source_file_changed(app, &p, *i));
            if source_sessions::unchanged(app) && !external {
                return false;
            }
        }
        Action::Command(_, _) if app.session.active_sketch.is_some() => return false,
        _ => {}
    }
    if app.docs.slots.is_empty() {
        app.docs.slots.push(None);
    }
    let mut bases = Vec::new();
    let mut sessions = Vec::new();
    for i in 0..app.docs.count() {
        let Some(s) = documents::session_at(app, i) else { return false };
        bases.push(Base { doc: s.doc.clone(), saved: s.saved_document_arc().clone(), revision: s.revision, path: s.path.clone() });
        let mut copy = s.scratch();
        copy.path.clone_from(&s.path);
        sessions.push(copy);
    }
    let active = app.docs.active;
    let profile = app.preferences.ui_profile.clone();
    let shared = app.docs.source_shared.clone();
    let checked = app.docs.source_checked.clone();
    let files = app.docs.source_files.clone();
    let (tx, rx) = mpsc::channel();
    let progress = Arc::new(solidcraft_engine::doc::progress::Progress::default());
    let worker_progress = progress.clone();
    let spawned = std::thread::Builder::new().name("source-workflow".into()).spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            solidcraft_engine::doc::progress::watch(worker_progress, || compute(sessions, active, profile, shared, checked, files, action))
        }))
        .unwrap_or_else(|_| Err("Source operation failed; the open documents are unchanged.".into()));
        let _ = tx.send(result);
    });
    match spawned {
        Ok(_) => app.source_work = Some(Work { rx, bases, active, progress, followup: None }),
        Err(e) => app.set_status(format!("Could not start source operation: {e}"), true),
    }
    true
}

fn compute(
    sessions: Vec<Session>,
    active: usize,
    profile: String,
    shared: std::collections::BTreeMap<std::path::PathBuf, Document>,
    checked: Vec<source_sessions::Checkpoint>,
    files: std::collections::BTreeMap<std::path::PathBuf, Vec<source_sessions::FileCheck>>,
    action: Action,
) -> Result<Made, String> {
    // This app is created and destroyed on the worker; no UI service or live view is moved.
    let mut worker = SolveApp::new(Session::default(), Default::default());
    worker.source_background = false;
    worker.preferences.ui_profile = profile;
    worker.docs.active = active;
    for (i, s) in sessions.into_iter().enumerate() {
        if i == active {
            worker.session = s;
            worker.docs.slots.push(None);
        } else {
            worker.docs.slots.push(Some(documents::DocSlot::new(s)));
        }
    }
    worker.docs.source_shared = shared;
    worker.docs.source_checked = checked;
    worker.docs.source_files = files;
    match &action {
        Action::Edit(p) => {
            worker.run("component.activate", p.clone())?;
        }
        Action::Command(id, p) => {
            worker.run(id, p.clone())?;
        }
        Action::Open(c) => crate::linked_source::open(&mut worker, *c),
        Action::Switch(i) => documents::switch(&mut worker, *i),
        Action::Place(path) => crate::place_component::begin(&mut worker, path.clone()),
    }
    if let Some((error, _, true)) = &worker.status {
        return Err(error.clone());
    }
    if worker.file_load.busy() {
        return Err("Finish the source edit before opening this document.".into());
    }
    let mut sessions = Vec::new();
    for i in 0..worker.docs.count() {
        let session = if i == worker.docs.active {
            std::mem::take(&mut worker.session)
        } else {
            worker.docs.slots.get_mut(i).and_then(Option::take).map(|s| s.session).ok_or("Source tab disappeared")?
        };
        session.world_state().warm_display();
        sessions.push(session);
    }
    Ok(Made {
        sessions,
        active: worker.docs.active,
        shared: worker.docs.source_shared,
        checked: worker.docs.source_checked,
        files: worker.docs.source_files,
        place: worker.viewport.place.take(),
        action,
    })
}

pub(crate) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(work) = &app.source_work else { return };
    let result = match work.rx.try_recv() {
        Ok(result) => result,
        Err(mpsc::TryRecvError::Empty) => {
            ctx.request_repaint_after(std::time::Duration::from_millis(25));
            return;
        }
        Err(mpsc::TryRecvError::Disconnected) => Err("Source worker stopped; documents are unchanged.".into()),
    };
    let Some(mut work) = app.source_work.take() else { return };
    if app.docs.active != work.active
        || app.docs.count() != work.bases.len()
        || work.bases.iter().enumerate().any(|(i, b)| {
            documents::session_at(app, i).is_none_or(|s| {
                !Arc::ptr_eq(&s.doc, &b.doc) || !Arc::ptr_eq(s.saved_document_arc(), &b.saved) || s.revision != b.revision || s.path != b.path
            })
        })
    {
        app.set_status("Source operation discarded because the open documents changed. Try again.", true);
        return;
    }
    let made = match result {
        Ok(made) => made,
        Err(e) => {
            app.set_status(e, true);
            return;
        }
    };
    let added = made.sessions.len() > work.bases.len();
    for (i, session) in made.sessions.into_iter().enumerate() {
        // A new source tab is inserted immediately after the active assembly.
        if added && i == made.active {
            app.docs.slots.insert(i, Some(documents::DocSlot::new(session)));
            if app.docs.active >= i {
                app.docs.active += 1;
            }
        } else if let Some(target) = documents::session_at_mut(app, i) {
            target.accept_source_computed(session);
        }
    }
    app.docs.source_shared = made.shared;
    app.docs.source_checked = made.checked;
    app.docs.source_files = made.files;
    let background = app.source_background;
    app.source_background = false;
    if app.docs.active != made.active {
        documents::switch(app, made.active);
    }
    app.source_background = background;
    match made.action {
        Action::Edit(_) => {
            app.finish_command("component.activate", &Ok(Value::Null));
            crate::workspace::sync_context(app);
        }
        Action::Open(_) => app.fit_view(),
        Action::Command(id, _) => app.finish_command(&id, &Ok(Value::Null)),
        Action::Place(_) => {
            app.viewport.place = made.place;
            app.fit_view();
        }
        Action::Switch(_) => {}
    }
    match work.followup.take() {
        Some(Followup::Feature(id)) => app.edit_feature(id),
        Some(Followup::Sketch(id)) => app.edit_sketch(id),
        Some(Followup::Pattern(sketch, pattern)) => {
            app.edit_sketch(sketch);
            if app.session.active_sketch == Some(sketch) {
                crate::sketch_modify_ui::edit_pattern(app, &pattern);
            }
        }
        None => {}
    }
}

pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if app.source_work.is_none() {
        return;
    }
    use crate::ui_kit::{Container, Surface};
    let screen = ctx.content_rect();
    let cover = egui::Rect::from_min_max(egui::pos2(screen.left(), screen.top() + crate::toolbar::APP_BAR_HEIGHT), screen.max);
    ctx.memory_mut(|m| {
        if let Some(id) = m.focused() {
            m.surrender_focus(id);
        }
    });
    for id in ["source_work_shield", "source_work_progress"] {
        ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, egui::Id::new(id)));
    }
    Container::new(ctx, Surface::InputShield).area(egui::Id::new("source_work_shield")).order(egui::Order::Middle).fixed_pos(cover.min).show(
        ctx,
        |ui| {
            ui.set_min_size(cover.size());
            ui.interact(cover, ui.id().with("block"), egui::Sense::click_and_drag());
            ui.painter().rect_filled(cover, 0.0, egui::Color32::from_black_alpha(70));
        },
    );
    let closed = crate::ui_kit::Window::new(egui::Id::new("source_work_progress"), "component", "Updating linked documents").show_body(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Preparing geometry…");
        });
        if ui.button("Cancel").clicked() {
            app.source_work = None;
        }
    });
    if closed {
        app.source_work = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(label: &str) -> SolveApp {
        let dir = std::env::temp_dir().join(format!("source-worker-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut source = Session::default();
        source.execute("document.new_part", &json!({})).unwrap();
        source.execute("solid.box", &json!({"length":10,"width":20,"height":30})).unwrap();
        let path = dir.join("Source.tcipart");
        source.execute("file.save", &json!({"path":path})).unwrap();
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.preferences.ui_profile = "inventor".into();
        app.run("document.new_assembly", json!({})).unwrap();
        app.run("component.insert_design", json!({"path":path})).unwrap();
        app.source_background = true;
        app
    }

    fn settle(app: &mut SolveApp) {
        let ctx = crate::fonts::test_context();
        let start = std::time::Instant::now();
        while app.source_work.is_some() && start.elapsed().as_secs() < 20 {
            tick(app, &ctx);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(app.source_work.is_none());
        assert!(!app.status.as_ref().is_some_and(|s| s.2), "{:?}", app.status);
    }

    #[test]
    fn source_worker_keeps_undo_tabs_and_saved_baselines() {
        let mut app = fixture("transitions");
        let component = app.session.doc.components[0].id;
        let old = app.session.doc.clone();
        app.run("component.activate", json!({"component":component})).unwrap();
        assert!(app.file_picker_busy());
        assert!(Arc::ptr_eq(&old, &app.session.doc), "live session stays unchanged until commit");
        settle(&mut app);
        assert_eq!(app.session.active_component, component);
        app.run("solid.box", json!({"length":2,"width":2,"height":2,"corner":[40,0,0]})).unwrap();
        app.run("component.finish_edit", json!({})).unwrap();
        crate::linked_source::open(&mut app, component);
        settle(&mut app);
        assert_eq!(app.docs.count(), 2);
        assert_eq!(app.docs.active, 1);
        assert!(app.session.is_dirty());
        assert_eq!(app.session.model.state().bodies.len(), 2);
        app.run("solid.box", json!({"length":3,"width":3,"height":3,"corner":[60,0,0]})).unwrap();
        documents::switch(&mut app, 0);
        settle(&mut app);
        assert_eq!(app.session.model.state().bodies.len(), 3);
        documents::switch(&mut app, 1);
        settle(&mut app);
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.session.model.state().bodies.len(), 2, "original undo history survives worker transitions");
        app.run("file.save", json!({})).unwrap();
        documents::switch(&mut app, 0);
        settle(&mut app);
        assert!(!app.session.doc.source_edits.get(&component).unwrap().baseline_features.is_empty());
        documents::switch(&mut app, 1);
        settle(&mut app);
        assert!(!app.session.is_dirty(), "source save acknowledgement survives commit");
    }

    #[test]
    fn source_worker_cancel_and_stale_result_preserve_live_documents() {
        for cancel in [false, true] {
            let mut app = fixture(if cancel { "cancel" } else { "stale" });
            let component = app.session.doc.components[0].id;
            let old = app.session.doc.clone();
            app.run("component.activate", json!({"component":component})).unwrap();
            if cancel {
                let progress = app.source_work.as_ref().unwrap().progress.clone();
                app.source_work = None;
                assert!(progress.is_cancelled());
                assert!(Arc::ptr_eq(&old, &app.session.doc));
            } else {
                app.session.doc_mut().name = "Newer live edit".into();
                let start = std::time::Instant::now();
                while app.source_work.is_some() && start.elapsed().as_secs() < 20 {
                    tick(&mut app, &crate::fonts::test_context());
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                assert!(app.source_work.is_none());
                assert_eq!(app.session.doc.name, "Newer live edit");
                assert_eq!(app.session.active_component, 0);
                assert!(app.status.as_ref().is_some_and(|s| s.0.contains("discarded")));
            }
        }
    }

    #[test]
    fn source_worker_opens_between_existing_tabs_and_places_a_component() {
        let mut app = fixture("tab-order");
        let component = app.session.doc.components[0].id;
        app.source_background = false;
        documents::open_tab(&mut app, Session::new(Document::new("Other A"))).unwrap();
        documents::open_tab(&mut app, Session::new(Document::new("Other B"))).unwrap();
        documents::switch(&mut app, 0);
        app.source_background = true;
        app.run("component.activate", json!({"component":component})).unwrap();
        settle(&mut app);
        crate::linked_source::open(&mut app, component);
        settle(&mut app);
        assert_eq!(app.docs.count(), 4);
        assert_eq!(app.docs.active, 1);
        assert_eq!(documents::session_at(&app, 2).unwrap().doc.name, "Other A");
        assert_eq!(documents::session_at(&app, 3).unwrap().doc.name, "Other B");

        let path = app.session.path.clone().unwrap();
        app.source_background = false;
        documents::open_tab(&mut app, Session::default()).unwrap();
        app.run("document.new_assembly", json!({})).unwrap();
        app.source_background = true;
        crate::place_component::begin(&mut app, path);
        assert!(app.file_picker_busy());
        settle(&mut app);
        assert_eq!(app.session.doc.occurrences.len(), 1);
        assert!(crate::place_component::live(&app));
        assert_eq!(app.session.model.state().bodies.len(), 1);
        app.run("edit.undo", json!({})).unwrap();
        assert!(app.session.doc.components.is_empty());
    }

    #[test]
    fn source_worker_feature_edit_computes_new_geometry() {
        let mut app = fixture("feature-edit");
        let component = app.session.doc.components[0].id;
        app.run("component.activate", json!({"component":component})).unwrap();
        settle(&mut app);
        let feature = app.session.doc.features[0].id;
        let before = app.session.doc.clone();
        app.run("timeline.edit", json!({"feature":feature,"set":{"length":"25 mm"}})).unwrap();
        assert!(app.source_work.is_some());
        assert!(Arc::ptr_eq(&before, &app.session.doc));
        settle(&mut app);
        assert!((app.session.model.state().bodies[0].outline_bounds().max.x - 25.0).abs() < 1e-6);
        app.docs.source_checked.clear();
        app.run("component.activate", json!({"component":component})).unwrap();
        app.edit_feature(feature);
        assert!(app.dialog.is_none());
        settle(&mut app);
        assert!(app.dialog.is_some(), "double-click continues into its feature editor after activation");
    }

    #[test]
    fn source_worker_continues_into_a_sketch_after_activation() {
        let mut app = fixture("sketch-continuation");
        let component = app.session.doc.components[0].id;
        app.source_background = false;
        app.run("component.activate", json!({"component":component})).unwrap();
        let sketch = app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
        app.run("sketch.finish", json!({})).unwrap();
        app.run("component.finish_edit", json!({})).unwrap();
        app.source_background = true;
        app.run("component.activate", json!({"component":component})).unwrap();
        app.edit_sketch(sketch);
        assert!(app.session.active_sketch.is_none());
        settle(&mut app);
        assert_eq!(app.session.active_sketch, Some(sketch));
        assert_eq!(app.session.active_component, component);
    }
}
