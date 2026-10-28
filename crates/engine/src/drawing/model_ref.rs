//! The model a view shows (spec §4.6): found from its `ModelRef`, asked of the host first (an
//! open tab may hold changes not saved yet), else read from its file; evaluated once per
//! revision and kept in the session's loaded-model cache.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use solidcraft_doc::{Document, DocumentKind, DrawingModel, ModelRef, ModelRevision, ViewId, revision_of};

use crate::{EngineError, Result};

/// What the host knows of open documents: the app installs one so a view shows a model's tab as
/// it stands, saved or not.
pub trait ModelProvider: Send + Sync {
    /// The open (possibly unsaved) document at absolute `path`, if a tab holds it.
    fn document(&self, path: &std::path::Path) -> Option<Arc<Document>>;
}

/// A model loaded for a view: its own evaluated session, its revision and the file it is.
pub(crate) struct LoadedModel {
    pub session: crate::Session,
    pub revision: ModelRevision,
    /// Where it was found.
    pub path: std::path::PathBuf,
}

/// Loaded models by absolute path (shared by a session's scratch copies).
pub(crate) type ModelCache = Arc<Mutex<BTreeMap<PathBuf, Arc<LoadedModel>>>>;

/// `p` absolute, with `.` and `..` worked out.
pub(crate) fn absolute(p: &Path) -> PathBuf {
    let full = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut out = PathBuf::new();
    for c in full.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir if matches!(out.components().next_back(), Some(std::path::Component::Normal(_))) => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

/// Where `r` may be, in order: `path` against the drawing's folder (as given while the drawing is
/// unsaved), then the open project's candidates for it (`project_path` when it has one).
fn candidates(s: &crate::Session, r: &ModelRef) -> Vec<PathBuf> {
    let mut out = vec![absolute(&crate::cmd::component_link::resolve(s.path.as_deref(), &r.path))];
    if let Some(project) = &s.project_file {
        let base = match s.path.as_deref() {
            Some(p) => absolute(Path::new(p)).parent().map(Path::to_path_buf).unwrap_or_default(),
            None => PathBuf::from(&project.workspace),
        };
        let link = r.project_path.as_deref().unwrap_or(&r.path);
        out.extend(project.candidates(&base, link).into_iter().map(|c| absolute(&c)));
    }
    let mut seen = std::collections::BTreeSet::new();
    out.retain(|c| seen.insert(c.clone()));
    out
}

/// Spec §4.6 order: `path` against the drawing's folder (as given while the drawing is unsaved), then the
/// open project's `project.resolve`; at each candidate the provider is asked before the file is read.
pub(crate) fn load(s: &crate::Session, r: &ModelRef) -> crate::Result<Arc<LoadedModel>> {
    for path in candidates(s, r) {
        let doc = match s.model_provider.as_ref().and_then(|p| p.document(&path)) {
            Some(doc) => doc,
            None if solidcraft_io::vfs::exists(&path.to_string_lossy()) => Arc::new(read(&path)?),
            None => continue,
        };
        return loaded(s, path, doc);
    }
    Err(EngineError::Other(format!("model not found: {}", r.path)))
}

/// The design in the file at `path`.
fn read(path: &Path) -> Result<Document> {
    let shown = path.to_string_lossy();
    let bytes = solidcraft_io::vfs::read(&shown, solidcraft_io::MAX_DESIGN_BYTES).map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
    solidcraft_io::read_design(&bytes).map_err(|e| EngineError::Other(format!("{shown}: {e}")))
}

/// `doc` (the model at `path`) evaluated, from the cache when its revision is the cached one.
fn loaded(s: &crate::Session, path: PathBuf, doc: Arc<Document>) -> Result<Arc<LoadedModel>> {
    if doc.kind == DocumentKind::Drawing {
        return Err(EngineError::Other(format!("{}: a drawing view shows a part or an assembly, not a drawing", path.display())));
    }
    let revision = revision_of(&doc);
    if let Ok(cache) = s.loaded_models.lock()
        && let Some(hit) = cache.get(&path).filter(|m| m.revision == revision)
    {
        return Ok(hit.clone());
    }
    let mut session = crate::Session::new((*doc).clone());
    session.path = Some(path.to_string_lossy().into_owned());
    session.project_file.clone_from(&s.project_file);
    let model = Arc::new(LoadedModel { session, revision, path: path.clone() });
    if let Ok(mut cache) = s.loaded_models.lock() {
        cache.insert(path, model.clone());
    }
    Ok(model)
}

/// Each view's model loaded once per call (views of one model share it): the model, or None
/// when it cannot be found or read.
#[derive(Default)]
pub(crate) struct Loads(BTreeMap<(String, Option<String>), Option<Arc<LoadedModel>>>);

impl Loads {
    pub(crate) fn get(&mut self, s: &crate::Session, r: &ModelRef) -> Option<Arc<LoadedModel>> {
        self.0.entry((r.path.clone(), r.project_path.clone())).or_insert_with(|| load(s, r).ok()).clone()
    }
}

/// Where the model `r` names is now (R77): the first candidate (in [`load`]'s order) an open tab
/// holds or a file exists at; None when it is found nowhere. Nothing is read or evaluated.
pub fn model_path(s: &crate::Session, r: &ModelRef) -> Option<PathBuf> {
    candidates(s, r)
        .into_iter()
        .find(|path| s.model_provider.as_ref().is_some_and(|p| p.document(path).is_some()) || solidcraft_io::vfs::exists(&path.to_string_lossy()))
}

/// The views of the active drawing whose model is found nowhere ([`model_path`] is None), in
/// view id order; none when the active document is not a drawing (R77).
pub fn missing_views(s: &crate::Session) -> Vec<ViewId> {
    let Some(m) = s.doc.drawing.as_deref().filter(|_| s.doc.kind == DocumentKind::Drawing) else { return Vec::new() };
    m.views.values().filter(|v| model_path(s, &v.model).is_none()).map(|v| v.id).collect()
}

/// Views whose model's current revision differs from their stored one (models that cannot be loaded are
/// not stale; `drawing.update` reports them as missing). D-2 runs `drawing.update` when this is non-empty.
pub fn stale_views(s: &crate::Session) -> Vec<ViewId> {
    let Some(m) = s.doc.drawing.as_deref().filter(|_| s.doc.kind == DocumentKind::Drawing) else { return Vec::new() };
    let mut loads = Loads::default();
    m.views.values().filter(|v| loads.get(s, &v.model).is_some_and(|l| l.revision != v.revision)).map(|v| v.id).collect()
}

/// `m`'s ModelRefs re-stored for a drawing saved at `to` instead of `from` (the DesignLink rule).
pub(crate) fn rebased_models(m: &DrawingModel, from: Option<&str>, to: &str) -> DrawingModel {
    let mut out = m.clone();
    for v in out.views.values_mut() {
        v.model.path = rebased_path(&v.model.path, from, to);
    }
    out
}

fn rebased_path(path: &str, from: Option<&str>, to: &str) -> String {
    crate::cmd::component_link::stored(Some(to), &crate::cmd::component_link::resolve(from, path))
}

/// Whether saving `m` at `to` instead of `from` stores any model path differently.
pub(crate) fn rebase_changes(m: &DrawingModel, from: Option<&str>, to: &str) -> bool {
    m.views.values().any(|v| rebased_path(&v.model.path, from, to) != v.model.path)
}

/// Drops the loaded models no view of the active drawing refers to any more (after a view is
/// deleted, or a model is found somewhere else).
pub(crate) fn evict_unreferenced(s: &crate::Session) {
    let referenced: std::collections::BTreeSet<PathBuf> =
        s.doc.drawing.as_deref().map(|m| m.views.values().flat_map(|v| candidates(s, &v.model)).collect()).unwrap_or_default();
    if let Ok(mut cache) = s.loaded_models.lock() {
        cache.retain(|path, _| referenced.contains(path));
    }
}
