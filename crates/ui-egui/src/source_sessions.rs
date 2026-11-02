//! Reconcile unsaved source revisions across tabs and assembly projections. A common
//! revision distinguishes a new edit from divergent drafts; synchronization never writes files.
use crate::{SolveApp, documents};
use serde_json::json;
use solidcraft_engine::{Session, doc::Document};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(crate) fn key(path: &str) -> PathBuf {
    Path::new(path).canonicalize().unwrap_or_else(|_| PathBuf::from(path))
}
fn linked_file(session: &Session, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    let path = if path.is_absolute() {
        path
    } else {
        session.path.as_deref().and_then(|p| Path::new(p).parent()).unwrap_or_else(|| Path::new(".")).join(path)
    };
    key(&path.to_string_lossy())
}
fn comparable(doc: &Document) -> Document {
    let mut out = doc.clone();
    out.next_id = 0;
    if out.marker.is_some_and(|m| m >= out.features.len()) {
        out.marker = None;
    }
    out
}
struct Revision {
    tab: usize,
    component: Option<u64>,
    doc: Document,
    baseline: Document,
    comparable: Document,
    baseline_comparable: Document,
    dirty: bool,
    label: String,
}

#[derive(Clone)]
pub(crate) struct Checkpoint {
    doc: std::sync::Weak<Document>,
    saved: std::sync::Weak<Document>,
    path: Option<String>,
    sketch: Option<u64>,
    visibility: solidcraft_engine::Visibility,
}
impl Checkpoint {
    fn new(s: &Session) -> Self {
        Self {
            doc: std::sync::Arc::downgrade(&s.doc),
            saved: std::sync::Arc::downgrade(s.saved_document_arc()),
            path: s.path.clone(),
            sketch: s.active_sketch,
            visibility: s.visibility.clone(),
        }
    }
    fn matches(&self, s: &Session) -> bool {
        self.doc.as_ptr() == std::sync::Arc::as_ptr(&s.doc)
            && self.saved.as_ptr() == std::sync::Arc::as_ptr(s.saved_document_arc())
            && self.path == s.path
            && self.sketch == s.active_sketch
            && self.visibility == s.visibility
    }
}
#[derive(Clone)]
pub(crate) struct FileCheck {
    doc: std::sync::Weak<Document>,
    stamp: (u64, std::time::SystemTime),
}
pub(crate) fn file_stamp(path: &str) -> Option<(u64, std::time::SystemTime)> {
    std::fs::metadata(path).ok().and_then(|m| Some((m.len(), m.modified().ok()?)))
}
pub(crate) fn source_file_changed(app: &mut SolveApp, path: &str, destination: usize) -> bool {
    let file = key(path);
    let Some(stamp) = file_stamp(path) else { return true };
    let Some(doc) = documents::session_at(app, destination).map(|s| std::sync::Arc::downgrade(&s.doc)) else { return true };
    let checks = app.docs.source_files.entry(file).or_default();
    checks.retain(|c| c.doc.strong_count() > 0);
    checks.iter().find(|c| c.doc.as_ptr() == doc.as_ptr()).is_none_or(|c| c.stamp != stamp)
}
pub(crate) fn remember_file(app: &mut SolveApp, path: &str, stamp: (u64, std::time::SystemTime)) {
    let doc = std::sync::Arc::downgrade(&app.session.doc);
    let checks = app.docs.source_files.entry(key(path)).or_default();
    checks.retain(|c| c.doc.strong_count() > 0);
    if let Some(check) = checks.iter_mut().find(|c| c.doc.as_ptr() == doc.as_ptr()) {
        check.stamp = stamp;
        return;
    }
    if checks.len() >= 64 {
        checks.remove(0);
    }
    checks.push(FileCheck { doc, stamp });
}

/// Recover older snapshot-local additions before Open/Save could overlook their file owner.
pub(crate) fn prepare_local(session: &mut Session, component: Option<u64>) -> Result<(), String> {
    let local: Vec<_> = session
        .doc
        .links
        .iter()
        .filter(|(c, link)| {
            component.is_none_or(|wanted| wanted == **c)
                && !session.doc.source_edits.contains_key(c)
                && session.doc.features.iter().any(|f| f.component == **c && f.id != link.feature)
        })
        .map(|(c, _)| *c)
        .collect();
    for c in local {
        session.execute("component.edit_source", &json!({"component":c})).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(crate) fn forget_unused(app: &mut SolveApp) {
    app.docs.source_checked.clear();
    let mut files = std::collections::BTreeSet::new();
    for index in 0..app.docs.count() {
        if let Some(session) = documents::session_at(app, index) {
            for link in session.doc.links.values() {
                files.insert(linked_file(session, &link.path));
            }
        }
    }
    app.docs.source_shared.retain(|path, _| files.contains(path));
    app.docs.source_files.retain(|path, _| files.contains(path));
}

pub(crate) fn synchronize(app: &mut SolveApp) -> Result<(), String> {
    if unchanged(app) {
        return Ok(());
    }
    app.docs.source_checked.clear();
    let mut groups: BTreeMap<PathBuf, Vec<(usize, u64)>> = BTreeMap::new();
    for index in 0..app.docs.count() {
        if let Some(session) = documents::session_at(app, index) {
            for (component, link) in &session.doc.links {
                groups.entry(linked_file(session, &link.path)).or_default().push((index, *component));
            }
        }
    }
    app.docs.source_shared.retain(|path, _| groups.contains_key(path));
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by_key(|(_, owners)| {
        std::cmp::Reverse(owners.iter().filter_map(|(i, c)| documents::session_at(app, *i).map(|s| s.doc.first_path(*c).len())).max().unwrap_or(0))
    });
    for (file, owners) in groups {
        let source_tab = (0..app.docs.count()).find(|index| {
            documents::session_at(app, *index).is_some_and(|s| s.path.as_deref().is_some_and(|p| key(p) == file) && s.doc.drawing.is_none())
        });
        let materialized = owners.iter().any(|(index, c)| documents::session_at(app, *index).is_some_and(|s| s.doc.source_edits.contains_key(c)));
        let tab_dirty = source_tab.and_then(|i| documents::session_at(app, i)).is_some_and(Session::is_dirty);
        if (!materialized && !tab_dirty) || (source_tab.is_none() && owners.len() == 1) {
            continue;
        }
        let mut revisions = Vec::new();
        if let Some(index) = source_tab {
            let s = documents::session_at(app, index).ok_or("Source tab changed")?;
            if s.active_sketch.is_some() || s.doc.marker.is_some_and(|m| m < s.doc.features.len()) {
                return Err("Finish the source sketch or history edit before synchronizing its assembly instances.".into());
            }
            revisions.push(Revision {
                tab: index,
                component: None,
                doc: (*s.doc).clone(),
                baseline: s.saved_document().clone(),
                comparable: comparable(&s.source_comparison(false)),
                baseline_comparable: comparable(&s.source_comparison(true)),
                dirty: s.is_dirty(),
                label: format!("{} (source tab)", s.doc.name),
            });
        }
        for (index, component) in owners {
            let s = documents::session_at_mut(app, index).ok_or("Assembly tab changed")?;
            if s.active_sketch.is_some() || s.doc.marker.is_some_and(|m| m < s.doc.features.len()) {
                return Err("Finish the assembly's sketch or history edit before synchronizing its source tabs.".into());
            }
            let result = s.execute("component.edit_source", &json!({"component":component,"open":true})).map_err(|e| e.to_string())?;
            let doc: Document = serde_json::from_value(result["document"].clone()).map_err(|e| e.to_string())?;
            let baseline: Document = serde_json::from_value(result["baseline"].clone()).map_err(|e| e.to_string())?;
            let comparison_baseline: Document = serde_json::from_value(result["comparison_baseline"].clone()).map_err(|e| e.to_string())?;
            revisions.push(Revision {
                tab: index,
                component: Some(component),
                comparable: comparable(&doc),
                baseline_comparable: comparable(&comparison_baseline),
                doc,
                baseline,
                dirty: result["dirty"] == true,
                label: format!("{} (assembly tab {})", s.doc.name, index + 1),
            });
        }
        let Some(first) = revisions.first() else {
            continue;
        };
        let common = app.docs.source_shared.get(&file).cloned().unwrap_or_else(|| first.baseline_comparable.clone());
        let canonical: Vec<_> = revisions.iter().map(|r| &r.comparable).collect();
        let baselines: Vec<_> = revisions.iter().map(|r| &r.baseline_comparable).collect();
        let changed: Vec<_> = canonical
            .iter()
            .enumerate()
            .filter(|(i, doc)| {
                let stale_clean =
                    !revisions[*i].dirty && **doc == baselines[*i] && revisions.iter().enumerate().any(|(j, r)| r.dirty && baselines[j] == **doc);
                **doc != &common && !stale_clean
            })
            .map(|(i, _)| i)
            .collect();
        let chosen = changed.first().copied().or_else(|| canonical.iter().position(|doc| **doc == common)).unwrap_or(0);
        if let Some(other) = changed.iter().find(|index| canonical[**index] != canonical[chosen]) {
            let name = file.file_name().unwrap_or_default().to_string_lossy();
            return Err(format!(
                "{name} has different edits in {} and {}. Both drafts are kept; review their tabs before saving.",
                revisions[chosen].label, revisions[*other].label
            ));
        }
        let saved = revisions.iter().find(|r| !r.dirty && &r.baseline_comparable == canonical[chosen]);
        let doc = saved.map_or(&revisions[chosen].doc, |r| &r.baseline).clone();
        for (index, revision) in revisions.iter().enumerate() {
            let needs_baseline = saved.is_some() && revision.baseline != doc;
            if canonical[index] == canonical[chosen] && !needs_baseline {
                continue;
            }
            let s = documents::session_at_mut(app, revision.tab).ok_or("Source context changed")?;
            s.execute("component.edit_source", &json!({"component":revision.component.unwrap_or(0),"document":doc,"saved":saved.is_some()}))
                .map_err(|e| e.to_string())?;
        }
        app.docs.source_shared.insert(file, (*canonical[chosen]).clone());
    }
    app.docs.source_checked = (0..app.docs.count()).filter_map(|i| documents::session_at(app, i).map(Checkpoint::new)).collect();
    Ok(())
}

pub(crate) fn unchanged(app: &SolveApp) -> bool {
    app.docs.source_checked.len() == app.docs.count()
        && app.docs.source_checked.iter().enumerate().all(|(i, c)| documents::session_at(app, i).is_some_and(|s| c.matches(s)))
}

pub(crate) fn validate_source_save(app: &SolveApp) -> Result<(), String> {
    let Some(path) = app.session.path.as_deref() else {
        return Ok(());
    };
    let linked = (0..app.docs.count())
        .filter(|i| *i != app.docs.active)
        .any(|i| documents::session_at(app, i).is_some_and(|s| crate::linked_source::is_source_of(s, path)));
    if !linked {
        return Ok(());
    }
    validate_file(&app.session, path)
}
pub(crate) fn validate_file(session: &Session, path: &str) -> Result<(), String> {
    let (doc, cache) = solidcraft_engine::io::read_design_file(path, None::<fn(solidcraft_engine::io::DesignPreview)>).map_err(|e| e.to_string())?;
    let current = Session::opened(doc, cache.as_ref());
    if *current.doc != *session.saved_document() {
        return Err(
            "The part file changed outside this session. Its unsaved draft is kept; save a copy or resolve the newer file before overwriting it."
                .into(),
        );
    }
    Ok(())
}
