//! Linked designs: a component whose bodies come from another design file (Insert Design).
//!
//! The linked design is opened and evaluated on its own, and its solid bodies, as placed in it,
//! join a new component through an Import feature (exact B-rep, kept in this design so it opens
//! without the part file). The link remembers the file, relative to this design's folder once it
//! is saved; Update Links reads the file again and replaces the Import feature's bodies, and
//! Break Link keeps the bodies and forgets the file. The bodies' faces are named after the names
//! the design gives them (`Feature::link_faces`), so a joint on a linked face stays on that face
//! when the design changes, as it would inside the design, and reports it when the face is gone.

use std::path::{Component as PathPart, Path, PathBuf};

use serde_json::{Value, json};
use solidcraft_doc::{DesignLink, Document, FeatureKind};

use super::CommandSpec;
use crate::params::{bad, str_, vec3};
use crate::{EngineError, Result, Session};

#[path = "component_extract.rs"]
mod extract;
pub(super) use extract::same_solids;

#[cfg(test)]
#[path = "component_relink_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod relink_tests;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("component.insert_design", "Insert Part or Assembly", insert_design).at("SOLID", "ASSEMBLE").icon("component").enabled(super::document_kind::needs_assembly).params(
        "path: a design (.tcipart, .tciasm, .tcidrw or .solidcraft) → a new component linked to it, holding its solid bodies as placed in that design; \
         name? (default: the design's name); parent?: component id or name (default: the active one); translate?: [x,y,z]; \
         reuse?: bool (default false): insert a free occurrence, reusing an existing link to the same file when available; \
         at?: [x,y,z]: place it there in its own orientation (identity rotation, in the parent's coordinates; not with translate)",
    ),
    CommandSpec::new("component.update_links", "Update Links", update_links).icon("update").params(
        "component?: id or name (default: every linked component) — reads the linked designs again and replaces their bodies where they changed; \
         → updated, unchanged, warnings (also joints whose faces are gone or that no longer solve)",
    ),
    CommandSpec::new("component.relink", "Relink Source", relink).icon("update").params(
        "component: id or name of a linked component; path: a saved design — replaces the source and linked bodies for all its occurrences, preserving identities and placements; returns path, previous_path, warnings, joint_errors and joint_conflicts",
    ),
    CommandSpec::new("component.break_link", "Break Link", break_link)
        .icon("break")
        .params("component: id or name of a linked component — keeps its bodies as they are and forgets the design file"),
    CommandSpec::new("component.extract", "Extract to File", extract::extract).icon("export").enabled(super::document_kind::needs_assembly).params(
        "component: id or name of a component that is not linked; path: the design file to write (.tcipart or .tciasm, or .solidcraft; not .tcidrw), whose kind its extension names (a component holding linked components needs .tciasm) → its history, subcomponents and their joints \
         move to that design, and the component becomes linked to it (as component.insert_design makes it); returns path, file, feature, bodies, \
         notices (what stays behind), warnings and copies (components that build the same bodies: see component.replace_with_link)",
    ),
    CommandSpec::new("component.copies", "Find Identical Copies", extract::copies).icon("inspect").noundo().params(
        "component: id or name of a linked component → copies: [{component, name}], the components not linked whose own history builds the same bodies",
    ),
    CommandSpec::new("component.replace_with_link", "Replace with Link", extract::replace_with_link).icon("chain").enabled(super::document_kind::needs_assembly).params(
        "component or components: [ids or names] of components that are not linked; with: a linked component, or path: a design (.tcipart, .tciasm, .tcidrw or .solidcraft) \
         (its link here, else a new linked component) → each component's history goes and its placements become placements of the linked \
         component; refused when its bodies are not the same as the link's; returns component, name, replaced and warnings",
    ),
    CommandSpec::new("component.links", "List Links", list_links)
        .icon("chain")
        .noundo()
        .params("→ linked components: component, name, path, file (resolved), found"),
];

/// The folder relative link paths are kept against: the design's own, once it has a file.
fn base_dir(path: Option<&str>) -> Option<PathBuf> {
    path.and_then(|p| absolute(Path::new(p)).parent().map(Path::to_path_buf))
}

/// A path as given, made absolute when the platform can, with `.` and `..` worked out.
fn absolute(p: &Path) -> PathBuf {
    let full = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut out = PathBuf::new();
    for c in full.components() {
        match c {
            PathPart::CurDir => {}
            PathPart::ParentDir if matches!(out.components().next_back(), Some(PathPart::Normal(_))) => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

/// `target` relative to the folder `base` (both absolute), when they share a root.
fn relative(base: &Path, target: &Path) -> Option<PathBuf> {
    let (b, t): (Vec<_>, Vec<_>) = (base.components().collect(), target.components().collect());
    let root = |c: &[PathPart]| c.iter().take_while(|x| matches!(x, PathPart::Prefix(_) | PathPart::RootDir)).count();
    if root(&b) == 0 || b.get(..root(&b)) != t.get(..root(&b)) {
        return None;
    }
    let common = b.iter().zip(&t).take_while(|(x, y)| x == y).count();
    let mut out = PathBuf::new();
    for _ in common..b.len() {
        out.push("..");
    }
    for c in t.iter().skip(common) {
        out.push(c.as_os_str());
    }
    Some(out)
}

/// How a link stores `file` for a design saved at `design` (`None`: not saved yet).
pub(crate) fn stored(design: Option<&str>, file: &Path) -> String {
    let file = absolute(file);
    match base_dir(design).and_then(|d| relative(&d, &file)) {
        // Relative paths are written with `/`, so a design moves between systems with its parts.
        Some(rel) => rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"),
        None => file.to_string_lossy().into_owned(),
    }
}

/// The file a stored link path names, for a design saved at `design`.
pub(crate) fn resolve(design: Option<&str>, link: &str) -> PathBuf {
    let p = Path::new(link);
    match base_dir(design) {
        Some(d) if p.is_relative() => d.join(p),
        _ => p.to_path_buf(),
    }
}

/// The links of `doc` stored for a design saved at `to` instead of at `from`.
pub(super) fn rebased(doc: &Document, from: Option<&str>, to: &str) -> std::collections::BTreeMap<u64, DesignLink> {
    doc.links.iter().map(|(c, l)| (*c, DesignLink { path: stored(Some(to), &resolve(from, &l.path)), feature: l.feature })).collect()
}

/// The bodies of a linked design: its name, the Import feature that holds them and the names
/// the design gives their faces (`Feature::link_faces`).
type Linked = (String, solidcraft_io::StepFeature, Vec<Vec<String>>);

/// The solid bodies of the design at `file`, evaluated, as an Import feature.
fn linked_bodies(file: &Path, own: Option<&str>) -> Result<Linked> {
    let shown = file.display().to_string();
    if own.is_some_and(|o| absolute(Path::new(o)) == absolute(file)) {
        return Err(EngineError::Other(format!("{shown}: a design can't be linked into itself")));
    }
    // A linked design is saved with the evaluated geometry the viewport shows. Use it when it
    // still matches the document, just as opening the design does; rebuilding the full history
    // here made even an unchanged link pay for every boolean in its source part.
    let (doc, saved) =
        solidcraft_io::read_design_file(&shown, None::<fn(solidcraft_io::DesignPreview)>).map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
    session_bodies(&Session::opened(doc, saved.as_ref()), &shown)
}

/// The solid bodies of `part`, the design saved as `shown`, as an Import feature.
pub(super) fn session_bodies(part: &Session, shown: &str) -> Result<Linked> {
    let name = part.doc.name.clone();
    if let Some(e) = part.model.results.iter().find_map(|r| r.error.clone()) {
        return Err(EngineError::Other(format!("{shown}: the design has a feature that fails ({e}); fix it before linking it")));
    }
    let world = part.world_state();
    let solids: Vec<String> = world.bodies.iter().filter(|b| !b.body.is_mesh()).map(|b| b.name.clone()).collect();
    if solids.is_empty() {
        return Err(EngineError::Other(format!("{shown}: the design has no solid bodies to link")));
    }
    let step = solidcraft_io::export(&part.doc.painted(&world), &solids, solidcraft_io::Format::Step, &name)
        .map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
    let mut feature = solidcraft_io::step_import_feature(&step, shown).map_err(|e| EngineError::Other(format!("{shown}: {e}")))?;
    if world.bodies.len() > solids.len() {
        feature.warnings.push(format!("{} mesh bodies of {shown} are not linked", world.bodies.len() - solids.len()));
    }
    // The STEP text keeps no face names: carry the design's own, so references into the linked
    // bodies follow them as they do inside the design (#3330).
    let faces = solidcraft_doc::naming::carried_face_names(&world, &feature.import.bodies);
    Ok((name, feature, faces))
}

fn component_key(s: &Session, p: &Value, key: &str, cmd: &str) -> Result<u64> {
    let k = match p.get(key) {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad(cmd, format!("`{key}` must be a component id or name"))),
    };
    s.doc.find_component(&k).ok_or_else(|| bad(cmd, format!("no component `{k}`")))
}

fn insert_design(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.insert_design";
    let given = str_(p, "path").filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(cmd, "`path` is required"))?;
    let translate = match p.get("translate") {
        None => solidcraft_geom::Vec3::ZERO,
        Some(v) => vec3(v).filter(|t| t.is_finite() && t.len() < 1e9).ok_or_else(|| bad(cmd, "`translate` must be [x, y, z] in mm"))?,
    };
    // `at`: an absolute placement in the part's own orientation (Place Component, R184).
    let at = match p.get("at") {
        None | Some(Value::Null) => None,
        Some(v) => Some(vec3(v).filter(|t| t.is_finite() && t.len() < 1e9).ok_or_else(|| bad(cmd, "`at` must be [x, y, z] in mm"))?),
    };
    if at.is_some() && p.get("translate").is_some() {
        return Err(bad(cmd, "give `at` or `translate`, not both"));
    }
    let translate = at.unwrap_or(translate);
    let file = absolute(Path::new(given));
    if s.path.as_deref().is_some_and(|own| absolute(Path::new(own)) == file) {
        return Err(bad(cmd, "a design can't be linked into itself"));
    }
    if p.get("reuse").and_then(Value::as_bool).unwrap_or(false) {
        let existing = s.doc.links.iter().find(|(_, link)| absolute(&resolve(s.path.as_deref(), &link.path)) == file).map(|(id, _)| *id);
        if let Some(component) = existing {
            let parent = p.get("parent").cloned().unwrap_or_else(|| json!(s.active_component));
            // With no root occurrence left the copy is the assembly's first part: grounded, as
            // a fresh paste into an empty assembly is (R123).
            let first = !s.doc.occurrences.iter().any(|o| o.parent == 0);
            let mut out = super::component::copy_occurrence(s, &json!({"component": component, "parent": parent, "translate": translate}))?;
            // A copy offsets the first occurrence's placement; `at` places it fresh instead.
            if let (Some(at), Some(id)) = (at, out["occurrence"].as_u64())
                && let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.id == id)
            {
                o.transform = solidcraft_doc::IDENTITY;
                o.transform[3] = [at.x, at.y, at.z, 1.0];
            }
            if first && let Some(id) = out["occurrence"].as_u64() {
                if let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.id == id) {
                    o.grounded = true;
                }
                out["grounded"] = json!(true);
            }
            return Ok(out);
        }
    }
    let (design_name, sf, faces) = linked_bodies(&file, s.path.as_deref())?;
    let name = str_(p, "name").filter(|n| !n.trim().is_empty()).map(str::to_string).unwrap_or(design_name);
    let mut create = json!({"name": name, "activate": false});
    // A paste (reuse) starts free beside other parts; into an empty assembly it is grounded
    // like a Place (R123).
    if p.get("reuse").and_then(Value::as_bool).unwrap_or(false) && s.doc.occurrences.iter().any(|o| o.parent == 0) {
        create["grounded"] = json!(false);
    }
    if let Some(parent) = p.get("parent") {
        create["parent"] = parent.clone();
    }
    let made = super::component::new_component(s, &create)?;
    let comp = made["component"].as_u64().ok_or_else(|| bad(cmd, "the component was not created"))?;
    let occ = made["occurrence"].as_u64();
    let id = s.doc_mut().add_feature(sf.kind, Some(&name))?;
    if let Some(f) = s.doc_mut().feature_mut(id) {
        f.component = comp;
        f.body_names = sf.body_names;
        f.link_faces = faces;
    }
    if let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| Some(o.id) == occ) {
        o.transform[3] = [translate.x, translate.y, translate.z, 1.0];
    }
    let path = stored(s.path.as_deref(), &file);
    s.doc_mut().links.insert(comp, DesignLink { path: path.clone(), feature: id });
    s.refresh();
    if let Some(e) = s.model.result(id).and_then(|r| r.error.clone()) {
        return Err(EngineError::Other(e));
    }
    let st = s.model.state();
    let bodies: Vec<&str> = st.bodies.iter().filter(|b| b.feature == id).map(|b| b.name.as_str()).collect();
    Ok(
        json!({"component": comp, "occurrence": occ, "name": name, "feature": id, "path": path, "bodies": bodies, "created_bodies": bodies, "warnings": sf.warnings}),
    )
}

fn update_links(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.update_links";
    let only = if p.get("component").is_some() { Some(component_key(s, p, "component", cmd)?) } else { None };
    if let Some(c) = only
        && !s.doc.links.contains_key(&c)
    {
        return Err(bad(cmd, "that component is not linked to a design"));
    }
    let links: Vec<(u64, DesignLink)> = s.doc.links.iter().filter(|(c, _)| only.is_none_or(|o| o == **c)).map(|(c, l)| (*c, l.clone())).collect();
    let (mut updated, mut unchanged, mut warnings) = (Vec::new(), Vec::new(), Vec::new());
    let mut refresh = false;
    for (comp, link) in links {
        let cname = s.doc.components.iter().find(|c| c.id == comp).map(|c| c.name.clone());
        let Some(cname) = cname else {
            // The component was deleted: the link goes with it.
            s.doc_mut().links.remove(&comp);
            refresh = true;
            continue;
        };
        if s.doc.source_edits.contains_key(&comp) {
            if super::component_source::refresh(s, comp)? {
                updated.push(cname);
            } else {
                unchanged.push(cname);
            }
            continue;
        }
        let Some(FeatureKind::Import { step: old, .. }) = s.doc.feature(link.feature).filter(|f| f.component == comp).map(|f| &f.kind) else {
            return Err(bad(
                cmd,
                format!("{cname}: its linked bodies (feature {}) are gone; break the link or insert the design again", link.feature),
            ));
        };
        let old = old.clone();
        let file = resolve(s.path.as_deref(), &link.path);
        let (_, sf, faces) = linked_bodies(&file, s.path.as_deref()).map_err(|e| bad(cmd, format!("{cname}: {e}")))?;
        warnings.extend(sf.warnings.iter().cloned());
        let same = matches!(&sf.kind, FeatureKind::Import { step, .. } if *step == old);
        let kept = s.doc.feature(link.feature).map(|f| &f.link_faces);
        if same && kept == Some(&faces) {
            unchanged.push(cname);
            continue;
        }
        // The same bodies, their faces named for the first time (a link made before names were
        // kept): nothing a reference finds changes. Names the design gave other faces since do.
        let changed = !same || kept.is_some_and(|k| !k.is_empty());
        if !same {
            for name in numbered_references(s, comp, link.feature) {
                warnings.push(format!(
                    "{name} refers to faces of {cname} by their place in the linked file, which this update can change: check it, and pick its faces again so it follows them by name"
                ));
            }
        }
        if let Some(f) = s.doc_mut().feature_mut(link.feature) {
            if !same {
                // Bodies keep their names where the design still has as many.
                if f.body_names.len() != sf.body_names.len() {
                    f.body_names = sf.body_names;
                }
                f.kind = sf.kind;
            }
            // A link made before the design's face names were kept gets them here.
            f.link_faces = faces;
        }
        refresh = true;
        if changed { updated.push(cname) } else { unchanged.push(cname) }
    }
    if refresh {
        s.refresh();
    }
    if let Some(e) = s.doc.links.values().find_map(|l| s.model.result(l.feature).and_then(|r| r.error.clone())) {
        return Err(EngineError::Other(e));
    }
    // A reference whose face the design no longer has says so here, not only in the joint list.
    if !updated.is_empty() {
        let sol = solidcraft_doc::joints::solve(&s.doc, &s.model.state());
        warnings.extend(sol.errors.iter().chain(&sol.conflicts).map(|(_, name, why)| format!("joint {name}: {why}")));
    }
    Ok(json!({"updated": updated, "unchanged": unchanged, "warnings": warnings}))
}

/// The joints and joint origins on occurrences of `comp` that name its linked faces by their
/// place in the file (`F<feature>:face:<k>`, made before the design's own names were kept), which
/// an update can give to another face.
fn numbered_references(s: &Session, comp: u64, feature: u64) -> Vec<String> {
    let numbered = format!("F{feature}:face:");
    let on = |o: &solidcraft_doc::joints::JointOrigin| {
        s.doc.occurrences.iter().any(|x| x.id == o.occurrence && x.component == comp)
            && o.snap.face_names().iter().any(|n| n.trim_start_matches(solidcraft_doc::naming::LEGACY_MARK).starts_with(&numbered))
    };
    let joints = s.doc.assembly.joints.iter().filter(|j| on(&j.a) || on(&j.b)).map(|j| format!("joint {}", j.name));
    let origins = s.doc.assembly.origins.iter().filter(|o| on(&o.origin)).map(|o| format!("joint origin {}", o.name));
    joints.chain(origins).collect()
}

/// The design extensions a link or an extracted design may have, for messages: every kind's, then
/// the kind-less one.
pub(super) fn design_extensions() -> String {
    format!("{} or .{}", solidcraft_doc::DocumentKind::extension_list(), solidcraft_io::DESIGN_EXT)
}

fn relink(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.relink";
    let comp = component_key(s, p, "component", cmd)?;
    let link = s.doc.links.get(&comp).cloned().ok_or_else(|| bad(cmd, "that component is not linked to a design"))?;
    if !matches!(s.doc.feature(link.feature), Some(f) if f.component == comp && matches!(f.kind, FeatureKind::Import { .. })) {
        return Err(bad(cmd, "the linked bodies are gone; break the link or insert the design again"));
    }
    let given = str_(p, "path").filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(cmd, "`path` is required"))?;
    let file = absolute(Path::new(given));
    if !solidcraft_io::is_design_path(&file.to_string_lossy()) {
        return Err(bad(cmd, format!("choose a saved design ({})", design_extensions())));
    }
    // The ordinary lexical check in linked_bodies also works in the browser VFS. On native
    // hosts, reject an alias of this assembly too (a symlink or a path with different case).
    if let (Ok(candidate), Some(Ok(own))) = (file.canonicalize(), s.path.as_deref().map(std::fs::canonicalize))
        && candidate == own
    {
        return Err(bad(cmd, "a design can't be linked into itself"));
    }
    // Read/evaluate the candidate before changing anything. Session::execute owns the single
    // undo step and restores both the document and evaluated state on any later failure.
    let (_, sf, faces) = linked_bodies(&file, s.path.as_deref()).map_err(|e| bad(cmd, e.to_string()))?;
    let geometry_changed = match (s.doc.feature(link.feature).map(|f| &f.kind), &sf.kind) {
        (Some(FeatureKind::Import { step: old, .. }), FeatureKind::Import { step, .. }) => old != step,
        _ => true,
    };
    let path = stored(s.path.as_deref(), &file);
    let mut warnings = sf.warnings;
    if geometry_changed
        && s.doc
            .assembly
            .joints
            .iter()
            .any(|j| s.doc.occurrences.iter().any(|o| o.component == comp && (j.a.occurrence == o.id || j.b.occurrence == o.id)))
    {
        warnings.push("the linked geometry changed; check this component's joints, even when the solver reports no error".to_string());
    }
    if let Some(f) = s.doc_mut().feature_mut(link.feature) {
        if f.body_names.len() != sf.body_names.len() {
            f.body_names = sf.body_names;
        }
        f.kind = sf.kind;
        f.link_faces = faces;
    }
    s.doc_mut().links.insert(comp, DesignLink { path: path.clone(), feature: link.feature });
    s.refresh();
    if let Some(error) = s.model.result(link.feature).and_then(|r| r.error.clone()) {
        return Err(bad(cmd, error));
    }
    // Report the solver's diagnostics. A resolved joint can still describe the wrong
    // connection after a source edit; the reminder above does not depend on solve errors.
    let sol = solidcraft_doc::joints::solve(&s.doc, &s.model.state());
    let diagnostics =
        |rows: &[(u64, String, String)]| rows.iter().map(|(id, name, why)| json!({"joint": id, "name": name, "why": why})).collect::<Vec<_>>();
    for (_, name, why) in sol.errors.iter().chain(&sol.conflicts) {
        warnings.push(format!("joint {name}: {why}"));
    }
    Ok(json!({"component": comp, "feature": link.feature, "path": path, "previous_path": link.path, "geometry_changed": geometry_changed,
        "warnings": warnings, "joint_errors": diagnostics(&sol.errors), "joint_conflicts": diagnostics(&sol.conflicts)}))
}

fn break_link(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.break_link";
    let comp = component_key(s, p, "component", cmd)?;
    let link = s.doc_mut().links.remove(&comp).ok_or_else(|| bad(cmd, "that component is not linked to a design"))?;
    s.doc_mut().source_edits.remove(&comp);
    Ok(json!({"component": comp, "path": link.path}))
}

fn list_links(s: &mut Session, _p: &Value) -> Result<Value> {
    let links: Vec<Value> = s
        .doc
        .links
        .iter()
        .map(|(c, l)| {
            let file = resolve(s.path.as_deref(), &l.path);
            let name = s.doc.components.iter().find(|x| x.id == *c).map(|x| x.name.clone());
            json!({"component": c, "name": name, "path": l.path, "file": file.display().to_string(), "found": file.is_file()})
        })
        .collect();
    Ok(json!({"links": links}))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn source_edit_subassembly_cycle_is_refused_without_writing_the_source() {
        let dir = std::env::temp_dir().join(format!("solidcraft-source-graph-cycle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Sub.tciasm");
        let mut source = Session::default();
        run(&mut source, "document.new_assembly", json!({}));
        run(&mut source, "component.create", json!({"name":"Block"}));
        run(&mut source, "solid.box", json!({"length":20,"width":10,"height":5}));
        run(&mut source, "component.finish_edit", json!({}));
        run(&mut source, "file.save", json!({"path":file}));
        let mut s = Session::default();
        run(&mut s, "document.new_assembly", json!({}));
        let sub = run(&mut s, "component.insert_design", json!({"path":file}));
        run(&mut s, "component.activate", json!({"component":sub["component"],"source_edit":true}));
        let cycle = run(&mut s, "component.insert_design", json!({"path":file,"parent":sub["component"]}));
        let before = s.doc.clone();
        let bytes = std::fs::read(&file).unwrap();
        let error = s.execute("component.edit_source", &json!({"component":cycle["component"]})).unwrap_err().to_string();
        assert!(error.contains("cyclic source dependency"), "{error}");
        assert_eq!(*s.doc, *before);
        let error = s.execute("component.save_source", &json!({"component":sub["component"]})).unwrap_err().to_string();
        assert!(error.contains("cyclic source dependency"), "{error}");
        assert_eq!(*s.doc, *before);
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn source_edit_subassembly_external_face_constraints_are_refused_without_changing_the_draft() {
        let dir = std::env::temp_dir().join(format!("solidcraft-source-graph-boundary-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Sub.tciasm");
        let mut source = Session::default();
        run(&mut source, "document.new_assembly", json!({}));
        run(&mut source, "component.create", json!({"name":"Block"}));
        run(&mut source, "solid.box", json!({"length":20,"width":10,"height":5}));
        run(&mut source, "component.finish_edit", json!({}));
        run(&mut source, "file.save", json!({"path":file}));
        let mut s = Session::default();
        run(&mut s, "document.new_assembly", json!({}));
        let sub = run(&mut s, "component.insert_design", json!({"path":file}));
        let other = run(&mut s, "component.create", json!({"name":"Other"}));
        run(&mut s, "solid.box", json!({"length":20,"width":10,"height":5,"corner":[60,0,0]}));
        run(&mut s, "component.finish_edit", json!({}));
        run(
            &mut s,
            "constraint.create",
            json!({"type":"mate","solution":"flush","a":{"occurrence":sub["occurrence"],"face":[10,5,5]},"b":{"occurrence":other["occurrence"],"face":[70,5,5]}}),
        );
        let before = s.doc.clone();
        let bytes = std::fs::read(&file).unwrap();
        let error = s.execute("component.edit_source", &json!({"component":sub["component"]})).unwrap_err().to_string();
        assert!(error.contains("external face relationships"), "{error}");
        assert_eq!(*s.doc, *before);
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn source_edit_nested_subassembly_preserves_shared_children_and_saves_in_file_order() {
        let dir = std::env::temp_dir().join(format!("solidcraft-source-nested-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let leaf = dir.join("Leaf.tcipart");
        let inner = dir.join("Inner.tciasm");
        let outer = dir.join("Moved/Outer.tciasm");
        std::fs::create_dir_all(dir.join("Moved")).unwrap();
        let mut part = Session::default();
        run(&mut part, "document.new_part", json!({}));
        run(&mut part, "solid.box", json!({"length":20,"width":10,"height":5}));
        run(&mut part, "file.save", json!({"path":leaf}));
        let mut sub = Session::default();
        run(&mut sub, "document.new_assembly", json!({}));
        let first = run(&mut sub, "component.insert_design", json!({"path":leaf}));
        let copy = run(&mut sub, "occurrence.copy", json!({"occurrence":first["occurrence"],"translate":[40,0,0]}));
        let constraint = run(
            &mut sub,
            "constraint.create",
            json!({"type":"mate","solution":"flush","name":"Internal Flush","a":{"occurrence":first["occurrence"],"face":[10,5,5]},"b":{"occurrence":copy["occurrence"],"face":[50,5,5]}}),
        );
        run(&mut sub, "file.save", json!({"path":inner}));
        let mut s = Session::default();
        run(&mut s, "document.new_assembly", json!({}));
        let made = run(&mut s, "component.insert_design", json!({"path":inner}));
        let c = made["component"].as_u64().unwrap();
        let second = run(&mut s, "occurrence.copy", json!({"occurrence":made["occurrence"],"translate":[0,60,0]}));
        let bytes = std::fs::read(&inner).unwrap();
        run(&mut s, "component.activate", json!({"component":c,"occurrence":second["occurrence"],"path":[second["occurrence"]],"source_edit":true}));
        assert_eq!(s.world_state().bodies.len(), 4);
        assert_eq!(s.doc.components.len(), 2);
        let child = s.doc.components.iter().find(|d| d.parent == c).unwrap().id;
        let child_occ = s.doc.occurrences.iter().rfind(|o| o.parent == c && o.component == child).unwrap().id;
        run(
            &mut s,
            "component.activate",
            json!({"component":child,"occurrence":child_occ,"path":[second["occurrence"],child_occ],"source_edit":true}),
        );
        run(&mut s, "solid.box", json!({"length":3,"width":3,"height":3,"corner":[25,0,0]}));
        assert_eq!(s.world_state().bodies.len(), 8);
        run(&mut s, "component.finish_edit", json!({}));
        assert_eq!(s.active_component, c);
        assert_eq!(s.active_occurrence_path, Some(vec![second["occurrence"].as_u64().unwrap()]));
        run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2,"corner":[0,25,0]}));
        run(&mut s, "component.finish_edit", json!({}));
        assert_eq!(s.world_state().bodies.len(), 10);
        assert_eq!(std::fs::read(&inner).unwrap(), bytes);
        let status = run(&mut s, "component.source_status", json!({}));
        assert_eq!(status["sources"][0]["component"], child);
        assert_eq!(status["sources"][1]["component"], c);
        // Selectively save the subassembly while leaving the leaf source pending.
        run(&mut s, "component.save_source", json!({"component":c}));
        run(&mut sub, "doc.open", json!({"path":inner}));
        assert_eq!(sub.world_state().bodies.len(), 5);
        assert_eq!(run(&mut sub, "component.source_status", json!({}))["sources"][0]["dirty"], true);
        run(&mut part, "doc.open", json!({"path":leaf}));
        assert_eq!(part.world_state().bodies.len(), 1);
        run(&mut s, "component.save_source", json!({"component":child}));
        run(&mut s, "component.save_source", json!({"component":c}));
        run(&mut s, "file.save", json!({"path":dir.join("Outer.tciasm")}));
        run(&mut s, "file.save", json!({"path":outer}));
        // Undo changes the draft; the saved-source baseline remains the last written revision.
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.world_state().bodies.len(), 8);
        run(&mut s, "component.save_source", json!({"component":c}));
        run(&mut sub, "doc.open", json!({"path":inner}));
        assert_eq!(sub.world_state().bodies.len(), 4);
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.world_state().bodies.len(), 10);
        run(&mut s, "component.save_source", json!({"component":c}));
        run(&mut s, "file.save", json!({"path":outer}));
        run(&mut sub, "doc.open", json!({"path":inner}));
        assert_eq!(sub.world_state().bodies.len(), 5);
        assert_eq!(sub.doc.components.len(), 1);
        assert_eq!(sub.doc.occurrences.len(), 2);
        assert_eq!(sub.doc.assembly.constraints[0].id, constraint["id"].as_u64().unwrap());
        assert_eq!(sub.doc.assembly.constraints[0].name, "Internal Flush");
        for pick in &sub.doc.assembly.constraints[0].picks {
            assert!(solidcraft_doc::constraints::resolve(&sub.doc, &sub.model.state(), pick).is_ok());
        }
        run(&mut part, "doc.open", json!({"path":leaf}));
        assert_eq!(part.world_state().bodies.len(), 2);
        run(&mut s, "doc.open", json!({"path":outer}));
        run(&mut s, "component.update_links", json!({}));
        assert_eq!(s.world_state().bodies.len(), 10);
        let state = run(&mut s, "component.source_status", json!({}));
        assert!(state["sources"].as_array().unwrap().iter().all(|r| r["dirty"] == false), "{state}");
        let mut fresh = Session::default();
        run(&mut fresh, "document.new_assembly", json!({}));
        let inserted = run(&mut fresh, "component.insert_design", json!({"path":inner}));
        run(&mut fresh, "component.activate", json!({"component":inserted["component"],"source_edit":true}));
        assert_eq!(fresh.world_state().bodies.len(), 5);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn source_edit_migrates_older_local_sketch_and_features_without_writing_on_edit() {
        let dir = std::env::temp_dir().join(format!("solidcraft-source-migrate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Source.tcipart");
        let mut part = Session::default();
        run(&mut part, "document.new_part", json!({}));
        run(&mut part, "sketch.create", json!({"plane":"XY"}));
        run(&mut part, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,20]}));
        run(&mut part, "sketch.finish", json!({}));
        run(&mut part, "solid.extrude", json!({"distance":10}));
        run(&mut part, "file.save", json!({"path":file}));
        let bytes = std::fs::read(&file).unwrap();
        let mut s = Session::default();
        run(&mut s, "document.new_assembly", json!({}));
        let made = run(&mut s, "component.insert_design", json!({"path":file}));
        let c = made["component"].as_u64().unwrap();
        run(&mut s, "occurrence.copy", json!({"occurrence":made["occurrence"],"translate":[80,0,0]}));
        run(&mut s, "component.activate", json!({"component":c}));
        run(&mut s, "sketch.create", json!({"plane":"XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[30,0],"p1":[35,5]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance":3,"operation":"new"}));
        run(&mut s, "component.finish_edit", json!({}));
        let old = s.doc.features.iter().filter(|f| f.component == c).skip(1).map(|f| f.id).collect::<Vec<_>>();
        run(&mut s, "component.activate", json!({"component":c,"source_edit":true}));
        assert_eq!(s.world_state().bodies.len(), 4);
        assert_eq!(s.doc.features.len(), 4);
        assert!(old.iter().all(|id| s.doc.feature(*id).is_some()));
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
        assert_eq!(run(&mut s, "component.source_status", json!({}))["sources"][0]["dirty"], true);
        run(&mut s, "component.save_source", json!({"component":c}));
        run(&mut part, "doc.open", json!({"path":file}));
        assert_eq!(part.doc.features.len(), 4);
        assert_eq!(part.world_state().bodies.len(), 2);
        let names: std::collections::BTreeSet<_> = part.doc.features.iter().map(|f| &f.name).collect();
        assert_eq!(names.len(), 4);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn source_edit_refuses_migration_when_source_changed_and_keeps_the_draft() {
        let dir = std::env::temp_dir().join(format!("solidcraft-source-migrate-conflict-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Source.tcipart");
        let mut part = Session::default();
        run(&mut part, "document.new_part", json!({}));
        run(&mut part, "solid.box", json!({"length":20,"width":20,"height":10}));
        run(&mut part, "file.save", json!({"path":file}));
        let mut s = Session::default();
        run(&mut s, "document.new_assembly", json!({}));
        let made = run(&mut s, "component.insert_design", json!({"path":file}));
        let c = made["component"].as_u64().unwrap();
        run(&mut s, "component.activate", json!({"component":c}));
        run(&mut s, "solid.box", json!({"length":4,"width":4,"height":4,"corner":[30,0,0]}));
        run(&mut part, "document.new_part", json!({}));
        run(&mut part, "solid.box", json!({"length":40,"width":20,"height":10}));
        run(&mut part, "file.save", json!({"path":file}));
        let before = s.doc.clone();
        let bytes = std::fs::read(&file).unwrap();
        let error = s.execute("component.edit_source", &json!({"component":c})).unwrap_err().to_string();
        assert!(error.contains("no longer matches"), "{error}");
        assert_eq!(*s.doc, *before);
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn source_edit_saves_original_history_and_two_placements_reopen() {
        let dir = std::env::temp_dir().join(format!("solidcraft-source-edit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("Block.tcipart");
        let assembly = dir.join("Pair.tciasm");
        let mut part = Session::default();
        run(&mut part, "document.new_part", json!({}));
        run(&mut part, "sketch.create", json!({"plane":"XY"}));
        run(&mut part, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[30,20]}));
        run(&mut part, "sketch.finish", json!({}));
        run(&mut part, "solid.extrude", json!({"distance":10}));
        run(&mut part, "file.save", json!({"path":source}));
        let original = part.doc.features.clone();
        let mut s = Session::default();
        run(&mut s, "document.new_assembly", json!({}));
        let inserted = run(&mut s, "component.insert_design", json!({"path":source}));
        let c = inserted["component"].as_u64().unwrap();
        let second = run(&mut s, "occurrence.copy", json!({"occurrence":inserted["occurrence"],"translate":[80,0,0]}));
        run(&mut s, "component.activate", json!({"component":c,"occurrence":second["occurrence"],"path":[second["occurrence"]],"source_edit":true}));
        assert_eq!(s.doc.features.iter().filter(|f| f.component == c).count(), original.len());
        run(&mut s, "solid.box", json!({"length":5,"width":5,"height":5,"corner":[40,0,0],"body_name":"Lug"}));
        run(&mut s, "solid.box", json!({"length":4,"width":4,"height":4,"corner":[50,0,0],"body_name":"Pad"}));
        assert_eq!(s.world_state().bodies.len(), 6);
        run(&mut s, "component.finish_edit", json!({}));
        assert_eq!(run(&mut s, "component.source_status", json!({}))["sources"][0]["dirty"], true);
        run(&mut s, "component.save_source", json!({"component":c}));
        run(&mut s, "file.save", json!({"path":assembly}));
        let mut reopened = Session::default();
        run(&mut reopened, "doc.open", json!({"path":source}));
        let mut prefix = reopened.doc.features[..original.len()].to_vec();
        for (actual, old) in prefix.iter_mut().zip(&original) {
            if old.body_names.is_empty() {
                actual.body_names.clear();
            }
        }
        assert_eq!(prefix, original, "original feature identities, parameters and complete sketch history survive");
        assert_eq!(reopened.doc.features.len(), original.len() + 2);
        assert_eq!(reopened.world_state().bodies.len(), 3);
        run(&mut reopened, "doc.open", json!({"path":assembly}));
        run(&mut reopened, "component.update_links", json!({}));
        assert_eq!(reopened.world_state().bodies.len(), 6);
        assert!(!reopened.is_dirty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn source_edit_skipped_save_recovers_and_external_changes_are_not_overwritten() {
        let dir = std::env::temp_dir().join(format!("solidcraft-source-conflict-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("Part.tcipart");
        save_box(&source, 10.);
        let mut s = Session::default();
        let c = run(&mut s, "component.insert_design", json!({"path":source}))["component"].as_u64().unwrap();
        run(&mut s, "component.activate", json!({"component":c,"source_edit":true}));
        run(&mut s, "solid.box", json!({"length":3,"width":3,"height":3,"corner":[30,0,0]}));
        run(&mut s, "file.save", json!({"path":dir.join("Skipped.tciasm")}));
        assert!(s.is_dirty(), "pending source remains dirty even after assembly-only save");
        let mut recovered = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
        recovered.path = s.path.clone();
        assert!(recovered.is_dirty());
        save_box(&source, 25.);
        let bytes = std::fs::read(&source).unwrap();
        assert!(recovered.execute("component.save_source", &json!({"component":c})).unwrap_err().to_string().contains("changed outside"));
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
        assert!(recovered.is_dirty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn source_edit_face_sketch_on_second_copy_saves_in_part_coordinates() {
        let dir = std::env::temp_dir().join(format!("solidcraft-source-face-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("Part.tcipart");
        save_box(&source, 10.);
        let mut s = Session::default();
        let made = run(&mut s, "component.insert_design", json!({"path":source}));
        let c = made["component"].as_u64().unwrap();
        let second = run(&mut s, "occurrence.copy", json!({"occurrence":made["occurrence"],"translate":[80,0,0]}));
        run(&mut s, "component.activate", json!({"component":c,"occurrence":second["occurrence"],"path":[second["occurrence"]],"source_edit":true}));
        run(&mut s, "sketch.create", json!({"plane":{"face":[85,10,10],"path":[second["occurrence"]]}}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-2,-2],"p1":[2,2]}));
        let entity = s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.curves.last().unwrap().id.clone();
        run(&mut s, "sketch.dimension", json!({"entities":[entity],"value":"4 mm"}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance":3,"operation":"join"}));
        let expected = volume(&mut s) / 2.;
        run(&mut s, "component.save_source", json!({"component":c}));
        let mut p = Session::default();
        run(&mut p, "doc.open", json!({"path":source}));
        assert!((volume(&mut p) - expected).abs() < 1e-5);
        assert!(p.model.results.iter().all(|r| r.error.is_none()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn source_edit_constraints_survive_materialization_and_undo_after_save() {
        let dir = std::env::temp_dir().join(format!("solidcraft-source-undo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("Part.tcipart");
        save_box(&source, 10.);
        let mut s = Session::default();
        let made = run(&mut s, "component.insert_design", json!({"path":source}));
        let c = made["component"].as_u64().unwrap();
        let second = run(&mut s, "occurrence.copy", json!({"occurrence":made["occurrence"],"translate":[80,0,0]}));
        run(
            &mut s,
            "constraint.create",
            json!({"type":"mate","a":{"occurrence":made["occurrence"],"face":[5,10,10]},"b":{"occurrence":second["occurrence"],"face":[85,10,10]}}),
        );
        run(&mut s, "component.activate", json!({"component":c,"source_edit":true}));
        for constraint in &s.doc.assembly.constraints {
            for pick in &constraint.picks {
                assert!(solidcraft_doc::constraints::resolve(&s.doc, &s.model.state(), pick).is_ok());
            }
        }
        run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2,"corner":[30,0,0]}));
        run(&mut s, "component.save_source", json!({"component":c}));
        run(&mut s, "edit.undo", json!({}));
        assert!(s.is_dirty());
        run(&mut s, "component.save_source", json!({"component":c}));
        let mut p = Session::default();
        run(&mut p, "doc.open", json!({"path":source}));
        assert_eq!(p.doc.features.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn source_edit_refreshes_a_clean_part_after_external_save_with_relative_links() {
        let dir = std::env::temp_dir().join(format!("solidcraft-source-refresh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("Part.tcipart");
        save_box(&source, 10.);
        let mut s = Session::default();
        let c = run(&mut s, "component.insert_design", json!({"path":source}))["component"].as_u64().unwrap();
        run(&mut s, "file.save", json!({"path":dir.join("Assembly.tciasm")}));
        run(&mut s, "component.activate", json!({"component":c,"source_edit":true}));
        save_box(&source, 25.);
        run(&mut s, "component.update_links", json!({}));
        assert!((volume(&mut s) - 5000.).abs() < 1e-5);
        assert_eq!(run(&mut s, "component.source_status", json!({}))["sources"][0]["dirty"], false);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn link_paths_are_kept_relative_to_the_design_and_follow_save_as() {
        let root = absolute(Path::new("projects"));
        let part = root.join("parts").join("plate.solidcraft");
        let design = root.join("assembly").join("case.solidcraft");
        let design = design.to_string_lossy().into_owned();
        assert_eq!(stored(Some(&design), &part), "../parts/plate.solidcraft");
        assert_eq!(absolute(&resolve(Some(&design), "../parts/plate.solidcraft")), part);
        // A design path given relative to the working folder works the same.
        assert_eq!(stored(Some("case.solidcraft"), &absolute(Path::new("parts/plate.solidcraft"))), "parts/plate.solidcraft");
        // Before the design has a file the path is kept whole; saving it makes it relative.
        let unsaved = stored(None, &part);
        assert!(Path::new(&unsaved).is_absolute(), "{unsaved}");
        let mut doc = Document::default();
        doc.links.insert(7, DesignLink { path: unsaved, feature: 3 });
        let moved = rebased(&doc, None, &design);
        assert_eq!(moved.get(&7).map(|l| l.path.as_str()), Some("../parts/plate.solidcraft"));
        let beside = root.join("parts").join("asm.solidcraft").to_string_lossy().into_owned();
        assert_eq!(
            rebased(&Document { links: moved, ..Document::default() }, Some(&design), &beside).get(&7).map(|l| l.path.as_str()),
            Some("plate.solidcraft")
        );
    }

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    fn save_box(path: &Path, height: f64) {
        let mut part = Session::default();
        if path.extension().is_some_and(|e| e == "tcipart") {
            run(&mut part, "document.new_part", json!({}));
        }
        run(&mut part, "solid.box", json!({"length": 10, "width": 20, "height": height, "body_name": "Plate"}));
        run(&mut part, "file.save", json!({"path": path}));
    }

    fn volume(s: &mut Session) -> f64 {
        run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
    }

    /// The params help reads as one text: no run of spaces from a lost line continuation.
    #[test]
    fn insert_design_params_text_has_no_stray_spaces() {
        let params = crate::cmd::find_command("component.insert_design").unwrap().params;
        assert!(!params.contains("  "), "{params}");
    }

    /// R184: `at` places a copy in the part's own orientation at that point, even when the first
    /// occurrence has been turned; `at` with `translate` is refused.
    #[test]
    fn at_places_a_copy_unrotated_where_asked() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-at-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("plate.solidcraft");
        save_box(&part, 30.0);
        let mut s = Session::default();
        let first = run(&mut s, "component.insert_design", json!({"path": part}));
        let o1 = first["occurrence"].as_u64().unwrap();
        let turned = solidcraft_doc::rigid(
            solidcraft_geom::Vec3::new(5.0, 0.0, 0.0),
            solidcraft_geom::Vec3::ZERO,
            solidcraft_geom::Vec3::Z,
            std::f64::consts::PI,
        );
        s.doc_mut().occurrences.iter_mut().find(|o| o.id == o1).unwrap().transform = turned;
        let copy = run(&mut s, "component.insert_design", json!({"path": part, "reuse": true, "at": [40, 20, 0]}));
        let o2 = copy["occurrence"].as_u64().unwrap();
        let m = s.doc.occurrences.iter().find(|o| o.id == o2).unwrap().transform;
        let mut want = solidcraft_doc::IDENTITY;
        want[3] = [40.0, 20.0, 0.0, 1.0];
        assert_eq!(m, want, "unrotated at the point");
        assert!(s.execute("component.insert_design", &json!({"path": part, "at": [0, 0, 0], "translate": [1, 0, 0]})).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_saved_assembly_reopens_with_its_link_updates_it_and_breaks_it() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-reopen-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("parts")).unwrap();
        std::fs::create_dir_all(dir.join("asm")).unwrap();
        let part = dir.join("parts").join("plate.solidcraft");
        let case = dir.join("asm").join("case.solidcraft");
        save_box(&part, 30.0);
        // Given an assembly holding the part, saved in a folder beside the part's.
        let mut a = Session::default();
        let inserted = run(&mut a, "component.insert_design", json!({"path": part}));
        assert_eq!(inserted["name"], "plate");
        run(&mut a, "file.save", json!({"path": case}));
        assert!((volume(&mut a) - 6000.0).abs() < 1e-6);
        // When it is opened again, the link is kept relative to it and finds the part.
        let mut b = Session::default();
        run(&mut b, "doc.open", json!({"path": case}));
        let links = run(&mut b, "component.links", json!({}));
        assert_eq!(links["links"][0]["path"], "../parts/plate.solidcraft", "{links}");
        assert_eq!(links["links"][0]["found"], true, "{links}");
        assert!((volume(&mut b) - 6000.0).abs() < 1e-6);
        // An unchanged part leaves the assembly as it is; an edited one comes in on update.
        assert_eq!(run(&mut b, "component.update_links", json!({}))["unchanged"], json!(["plate"]));
        save_box(&part, 45.0);
        assert_eq!(run(&mut b, "component.update_links", json!({}))["updated"], json!(["plate"]));
        assert!((volume(&mut b) - 9000.0).abs() < 1e-6, "{}", volume(&mut b));
        // A part file that has gone is reported, and the assembly keeps the bodies it had.
        std::fs::remove_file(&part).unwrap();
        let err = b.execute("component.update_links", &json!({})).unwrap_err().to_string();
        assert!(err.contains("plate"), "{err}");
        assert!((volume(&mut b) - 9000.0).abs() < 1e-6);
        // Break Link keeps the bodies and forgets the file.
        run(&mut b, "component.break_link", json!({"component": "plate"}));
        assert_eq!(run(&mut b, "component.links", json!({}))["links"], json!([]));
        assert!((volume(&mut b) - 9000.0).abs() < 1e-6);
        assert!(b.execute("component.update_links", &json!({"component": "plate"})).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inserting_a_missing_file_or_bad_parameters_changes_nothing() {
        let mut s = Session::default();
        let missing = std::env::temp_dir().join(format!("solidcraft-no-such-part-{}.solidcraft", std::process::id()));
        assert!(s.execute("component.insert_design", &json!({"path": missing})).is_err());
        assert!(s.execute("component.insert_design", &json!({})).is_err());
        assert!(s.execute("component.insert_design", &json!({"path": "x.solidcraft", "translate": [1, "a", 2]})).is_err());
        assert!(s.doc.components.is_empty() && s.doc.links.is_empty() && s.doc.features.is_empty());
    }

    /// #3330's part: a plate 60 × 40 × 5 with a Ø 6 hole at x = `hx` (10 mm), saved at `path`. The
    /// session stays open to edit the part as a user would.
    fn plate(path: &Path) -> Session {
        let mut part = Session::default();
        run(&mut part, "parameters.add", json!({"name": "hx", "expression": "10 mm"}));
        run(&mut part, "solid.box", json!({"length": 60, "width": 40, "height": 5, "corner": [-30, -20, 0], "body_name": "Plate"}));
        run(&mut part, "solid.cylinder", json!({"diameter": 6, "height": 7, "base": ["hx", 0, -1], "operation": "cut", "name": "Hole"}));
        run(&mut part, "file.save", json!({"path": path}));
        part
    }

    /// Another Ø 6 hole through the plate at `x`.
    fn hole(part: &mut Session, x: f64, name: &str) {
        run(part, "solid.cylinder", json!({"diameter": 6, "height": 7, "base": [x, 0, -1], "operation": "cut", "name": name}));
    }

    /// #3330's assembly: the plate linked in and grounded, and a pin rigidly jointed to the top edge
    /// of its hole. `older`: the link keeps no face names, as one made before them.
    fn pinned(path: &Path, older: bool) -> Session {
        let mut s = Session::default();
        let feature = run(&mut s, "component.insert_design", json!({"path": path, "name": "Plate"}))["feature"].as_u64().unwrap();
        if older {
            s.doc_mut().feature_mut(feature).unwrap().link_faces.clear();
            s.refresh();
        }
        run(&mut s, "occurrence.ground", json!({"occurrence": "Plate:1", "grounded": true}));
        run(&mut s, "solid.cylinder", json!({"diameter": 6, "height": 20, "base": [0, 50, 0], "operation": "new", "body_name": "PinBody"}));
        run(&mut s, "component.create", json!({"name": "Pin", "parent": "root", "activate": false}));
        run(&mut s, "component.move_bodies", json!({"bodies": ["PinBody"], "component": "Pin"}));
        let joint = json!({"type": "rigid", "name": "PinInHole", "flip": true,
            "a": {"occurrence": "Plate:1", "circle": [13, 0, 5]}, "b": {"occurrence": "Pin:1", "circle": [3, 50, 0]}});
        run(&mut s, "joint.create", joint);
        s
    }

    /// Asserts the pin's joint sits on the plate's top at x (world) and solves without errors.
    fn assert_joint_at(s: &mut Session, x: f64, case: &str) {
        let list = run(s, "joint.list", json!({}));
        assert_eq!(list["errors"], json!([]), "{case}: {list}");
        let o = &list["joints"][0]["frame_a"]["origin"];
        let at = [0, 1, 2].map(|i| o[i].as_f64().unwrap_or(f64::NAN));
        assert!((at[0] - x).abs() < 1e-6 && at[1].abs() < 1e-6 && (at[2] - 5.0).abs() < 1e-6, "{case}: the joint is at {at:?}, not x = {x}");
    }

    /// #3330: a joint on a linked part's hole stays on that hole when Update Links brings in an
    /// edit of the part, as it does inside one design, also when the edit adds a hole of the same
    /// size that takes the first one's place in the file.
    #[test]
    fn joints_on_a_linked_part_follow_its_faces_through_update_links() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-names-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.solidcraft");
        let hx = |part: &mut Session, x: &str| {
            run(part, "parameters.change", json!({"name": "hx", "expression": x}));
        };
        let edits: [(&str, &dyn Fn(&mut Session), f64); 3] = [
            ("the hole moved to x = 20", &|p: &mut Session| hx(p, "20 mm"), 20.0),
            (
                "a hole at x = -10 added before it in the timeline, and the hole moved to x = 20",
                &|p: &mut Session| {
                    run(p, "timeline.rollback", json!({"position": 1}));
                    hole(p, -10.0, "Hole0");
                    run(p, "timeline.rollback", json!({}));
                    hx(p, "20 mm");
                },
                20.0,
            ),
            (
                "the hole moved to x = 5, and a hole added at x = 15",
                &|p: &mut Session| {
                    hx(p, "5 mm");
                    hole(p, 15.0, "Hole2");
                },
                5.0,
            ),
        ];
        for (case, edit, x) in edits {
            let mut part = plate(&path);
            let mut s = pinned(&path, false);
            assert_joint_at(&mut s, 10.0, case);
            edit(&mut part);
            run(&mut part, "file.save", json!({}));
            let update = run(&mut s, "component.update_links", json!({}));
            assert_eq!(update["updated"], json!(["Plate"]), "{case}: {update}");
            assert_eq!(update["warnings"], json!([]), "{case}: {update}");
            assert_joint_at(&mut s, x, case);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #3330: when the part no longer has the face a joint is on, Update Links and the joint list
    /// say so instead of moving the joint to another hole of the same size.
    #[test]
    fn a_joint_whose_linked_face_is_gone_is_reported_not_moved() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-gone-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.solidcraft");
        let mut part = plate(&path);
        let mut s = pinned(&path, false);
        hole(&mut part, -10.0, "Hole0");
        run(&mut part, "timeline.delete", json!({"feature": "Hole"}));
        run(&mut part, "file.save", json!({}));
        let update = run(&mut s, "component.update_links", json!({}));
        let warnings = update["warnings"].to_string();
        assert!(warnings.contains("joint PinInHole") && warnings.contains("no longer exists"), "{update}");
        let list = run(&mut s, "joint.list", json!({}));
        assert_eq!(list["errors"][0]["name"], "PinInHole", "{list}");
        assert!(list["joints"][0]["frame_a"].is_null(), "{list}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A joint made on a link that kept no face names (an older design) names the faces by their
    /// place in the file: Update Links says it can't vouch for it, and keeps the names from then on.
    #[test]
    fn update_links_warns_about_joints_on_faces_named_by_their_place() {
        let dir = std::env::temp_dir().join(format!("solidcraft-link-numbered-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.solidcraft");
        let mut part = plate(&path);
        let mut s = pinned(&path, true);
        let feature = s.doc.links.values().next().unwrap().feature;
        assert!(s.doc.feature(feature).unwrap().link_faces.is_empty());
        // An unchanged part keeps the bodies; the names come in quietly.
        assert_eq!(run(&mut s, "component.update_links", json!({}))["unchanged"], json!(["Plate"]));
        assert!(!s.doc.feature(feature).unwrap().link_faces.is_empty());
        run(&mut part, "parameters.change", json!({"name": "hx", "expression": "20 mm"}));
        run(&mut part, "file.save", json!({}));
        let update = run(&mut s, "component.update_links", json!({}));
        assert!(update["warnings"].to_string().contains("joint PinInHole refers to faces of Plate by their place"), "{update}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
