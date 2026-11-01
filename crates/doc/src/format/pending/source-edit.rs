//! Assemblies can retain source-backed leaf-part histories and source baselines for conflict
//! detection, selective save and recovery. Older designs have no source_edits and stay linked
//! snapshots until explicitly edited. No old document data is rewritten on load.
