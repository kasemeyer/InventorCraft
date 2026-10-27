//! What a design is: a part, an assembly, or a drawing. Inventor keeps these apart as `.ipt`,
//! `.iam` and `.idw` files; here one container (`Document`) serialises every kind, and the kind
//! says which one it is and which extension it is saved under.

use serde::{Deserialize, Serialize};

/// The kind of a design.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    /// Bodies with no linked components (Inventor's `.ipt`).
    Part,
    /// Linked components (Inventor's `.iam`).
    #[default]
    Assembly,
    /// A drawing (Inventor's `.idw`): its sheets, views and annotations are `Document::drawing`,
    /// populated by Plan D.
    Drawing,
}

impl DocumentKind {
    /// Every kind, in declaration order.
    pub const ALL: [DocumentKind; 3] = [DocumentKind::Part, DocumentKind::Assembly, DocumentKind::Drawing];

    /// Every kind's extension, in the order of `ALL`.
    pub const EXTENSIONS: [&'static str; 3] = {
        let mut out = [""; 3];
        let mut i = 0;
        while i < Self::ALL.len() {
            out[i] = Self::ALL[i].extension();
            i += 1;
        }
        out
    };

    /// The file extension a design of this kind is saved under, without a dot.
    pub const fn extension(&self) -> &'static str {
        match self {
            DocumentKind::Part => "tcipart",
            DocumentKind::Assembly => "tciasm",
            DocumentKind::Drawing => "tcidrw",
        }
    }

    /// The kind saved under extension `ext` (an optional leading dot, any case), if any.
    pub fn from_extension(ext: &str) -> Option<DocumentKind> {
        let ext = ext.trim_start_matches('.');
        Self::ALL.into_iter().find(|k| k.extension().eq_ignore_ascii_case(ext))
    }

    /// Every kind's extension with its dot, in the order of `ALL`, for messages:
    /// `".tcipart, .tciasm, .tcidrw"`.
    pub fn extension_list() -> String {
        Self::ALL.iter().map(|k| format!(".{}", k.extension())).collect::<Vec<_>>().join(", ")
    }
}

#[cfg(test)]
#[path = "kind_tests.rs"]
mod tests;
