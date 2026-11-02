//! Assembly geometry references can carry the full occurrence path of nested geometry,
//! separately from the rigid occurrence that owns the constraint. Missing paths retain the
//! previous single-occurrence behavior. Scoped Origin visibility keys can carry slash-separated
//! occurrence paths, while older single-ID keys remain readable. No migration is required.
