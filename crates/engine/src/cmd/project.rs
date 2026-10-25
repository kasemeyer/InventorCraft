//! The project file (`.tciproj`): making one, opening one, and finding a linked design through
//! it. The open project belongs to the session, not the design, so it outlasts new and opened
//! designs. `project.new` writes relative folders with `/` between them, as the project file
//! keeps them, and never replaces a file unless told to.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_doc::ProjectFile;

use super::CommandSpec;
use super::file::path_arg;
use crate::params::{bad, bool_, str_, string_list};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("project.new", "New Project", new).icon("new").noundo().params(
        "name; workspace: the root folder (relative: to the project file's folder); path: where to write the .tciproj (no extension: .tciproj is added; any other extension is refused); libraries?: [folders]; frequently_used?: [folders]; overwrite?: bool (default false): replace an existing project file → path",
    ),
    CommandSpec::new("project.open", "Open Project", open)
        .icon("open")
        .noundo()
        .params("path: a .tciproj → name, workspace, libraries, frequently_used (rooted), path"),
    CommandSpec::new("project.resolve", "Resolve Link", resolve).icon("chain").noundo().doc_only().params(
        "link: a design path as a link stores it; base?: the folder to look beside (default: the open design's folder, else the workspace) → path",
    ),
];

/// An I/O failure on `path`, in the shape the file commands use.
fn file_error(path: &str, e: impl std::fmt::Display) -> EngineError {
    EngineError::Other(format!("{path}: {e}"))
}

/// The folder a project file's relative paths are relative to.
fn folder_of(path: &str) -> &Path {
    Path::new(path).parent().unwrap_or(Path::new(""))
}

/// A folder as the project file keeps it: a relative one with `/` between its parts, so the
/// project moves between systems; one with a root or a drive (`C:\jobs`, `\jobs`, `C:jobs`) as given.
fn portable(folder: &str) -> String {
    use std::path::Component;
    let parts = Path::new(folder).components();
    if parts.clone().any(|c| matches!(c, Component::Prefix(_) | Component::RootDir)) {
        return folder.to_string();
    }
    parts.map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

fn new(s: &mut Session, p: &Value) -> Result<Value> {
    // Everything is checked before anything is written.
    let name = str_(p, "name").filter(|x| !x.trim().is_empty()).ok_or_else(|| bad("project.new", "`name` is required"))?;
    let workspace = str_(p, "workspace").filter(|x| !x.trim().is_empty()).ok_or_else(|| bad("project.new", "`workspace` is required"))?;
    let path = path_arg(p, "project.new")?;
    let ext = ProjectFile::EXTENSION;
    let path = match Path::new(path).extension() {
        None => format!("{path}.{ext}"),
        Some(e) if e.to_string_lossy().eq_ignore_ascii_case(ext) => path.to_string(),
        Some(e) => return Err(bad("project.new", format!("a project file is a .{ext}, not a .{}", e.to_string_lossy()))),
    };
    let project = ProjectFile {
        name: name.to_string(),
        workspace: portable(workspace),
        libraries: string_list(p, "libraries").iter().map(|f| portable(f)).collect(),
        frequently_used: string_list(p, "frequently_used").iter().map(|f| portable(f)).collect(),
    };
    solidcraft_io::vfs::check(&path).map_err(|e| file_error(&path, e))?;
    if !bool_(p, "overwrite").unwrap_or(false) && solidcraft_io::vfs::exists(&path) {
        return Err(bad("project.new", format!("{path} already exists: pass overwrite: true to replace it")));
    }
    solidcraft_io::write_atomic(Path::new(&path), project.to_json().as_bytes(), false).map_err(|e| file_error(&path, e))?;
    s.project_file = Some(Arc::new(project.rooted(folder_of(&path))));
    Ok(json!({"path": path}))
}

fn open(s: &mut Session, p: &Value) -> Result<Value> {
    let path = path_arg(p, "project.open")?;
    let bytes = solidcraft_io::vfs::read(path, ProjectFile::MAX_BYTES).map_err(|e| file_error(path, e))?;
    let text = String::from_utf8(bytes).map_err(|e| file_error(path, e))?;
    let project = Arc::new(ProjectFile::from_json(&text)?.rooted(folder_of(path)));
    let mut out = serde_json::to_value(&*project).map_err(|e| file_error(path, e))?;
    if let Some(fields) = out.as_object_mut() {
        fields.insert("path".into(), json!(path));
    }
    s.project_file = Some(project);
    Ok(out)
}

fn resolve(s: &mut Session, p: &Value) -> Result<Value> {
    let project = s.project_file.clone().ok_or_else(|| bad("project.resolve", "no project is open: project.open or project.new first"))?;
    let link = str_(p, "link").filter(|x| !x.trim().is_empty()).ok_or_else(|| bad("project.resolve", "`link` is required"))?;
    let base = match (str_(p, "base").filter(|x| !x.trim().is_empty()), s.path.as_deref()) {
        (Some(base), _) => PathBuf::from(base),
        (None, Some(design)) => folder_of(design).to_path_buf(),
        (None, None) => PathBuf::from(&project.workspace),
    };
    let found = project
        .resolve(&base, link, |c| solidcraft_io::vfs::exists(&c.to_string_lossy()))
        .ok_or_else(|| bad("project.resolve", format!("{link} is not beside the design, under the workspace or in a library")))?;
    Ok(json!({"path": found.to_string_lossy()}))
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
