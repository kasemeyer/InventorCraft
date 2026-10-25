//! The project file (`.tciproj`): where a job's files live, so that a design link's relative
//! path survives a moved or shared tree. It holds no geometry and is not a document. Inventor's
//! `.ipj` is the analogue. It names the workspace (the job's root folder), the library search
//! paths (tried in order after the workspace) and the frequently-used folders (shortcuts, never
//! searched). Paths use `/` between folders, as `DesignLink.path` does, and a relative one is
//! relative to the `.tciproj`'s folder.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::DocError;

/// A `.tciproj` project: the workspace, the library search paths and the frequently-used folders.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectFile {
    pub name: String,
    /// The job's root folder.
    pub workspace: String,
    /// Folders searched, in order, after the workspace.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub libraries: Vec<String>,
    /// Folders the user goes to often: shortcuts, not searched.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frequently_used: Vec<String>,
}

impl ProjectFile {
    /// The project file's extension.
    pub const EXTENSION: &'static str = "tciproj";
    /// The largest project file read.
    pub const MAX_BYTES: usize = 1 << 20;

    pub fn from_json(text: &str) -> crate::Result<ProjectFile> {
        serde_json::from_str(text).map_err(|e| DocError::Invalid(format!("not a TCI-CAD project: {e}")))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// This project with every relative folder joined onto `project_dir` (the `.tciproj`'s
    /// folder). Absolute folders are kept.
    pub fn rooted(&self, project_dir: &Path) -> ProjectFile {
        let root = |folder: &String| project_dir.join(folder).to_string_lossy().into_owned();
        ProjectFile {
            name: self.name.clone(),
            workspace: root(&self.workspace),
            libraries: self.libraries.iter().map(root).collect(),
            frequently_used: self.frequently_used.iter().map(root).collect(),
        }
    }

    /// Where `link` may be, in the order tried: beside the design (`base`), under the
    /// workspace, then in each library. An absolute link is only itself.
    pub fn candidates(&self, base: &Path, link: &str) -> Vec<PathBuf> {
        let link = Path::new(link);
        if link.is_absolute() {
            return vec![link.to_path_buf()];
        }
        let mut out: Vec<PathBuf> = std::iter::once(base)
            .chain(std::iter::once(Path::new(&self.workspace)))
            .chain(self.libraries.iter().map(Path::new))
            .map(|folder| folder.join(link))
            .collect();
        out.dedup();
        out
    }

    /// The first of [`Self::candidates`] that `exists` accepts. The caller does the I/O.
    pub fn resolve(&self, base: &Path, link: &str, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
        self.candidates(base, link).into_iter().find(|c| exists(c))
    }
}

#[cfg(test)]
#[path = "project_file_tests.rs"]
mod tests;
