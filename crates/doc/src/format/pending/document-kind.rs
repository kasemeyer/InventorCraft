//! A design saves what it is: `"kind": "part" | "assembly" | "drawing"` (`DocumentKind`).
//! An older design has no `kind` and reads as an assembly, which is what upstream's single
//! Fusion-style design always was; nothing is rewritten as it opens. The last release
//! refuses a design saved with this (the newer format number).
