//! The model a view shows (spec §4.6): found from its `ModelRef`, asked of the host first (an
//! open tab may hold changes not saved yet), else read from its file; evaluated once per
//! revision and kept in the session's loaded-model cache.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use solidcraft_doc::{Document, DocumentKind, ModelRef, ModelRevision, revision_of};

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
