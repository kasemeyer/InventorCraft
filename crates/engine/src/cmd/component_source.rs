//! Editable leaf-part histories projected into their assembly definition, with explicit source
//! saves. Geometry remains in definition coordinates; occurrence transforms stay in the assembly.
use super::CommandSpec;
use crate::params::bad;
use crate::{Result, Session};
use serde_json::{Value, json};
use solidcraft_doc::{Document, Feature, SourceEdit};
use std::collections::{BTreeMap, BTreeSet};

const CMD: &str = "component.edit_source";
pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new(CMD, "Edit Linked Part", edit)
        .icon("component")
        .noundo()
        .params("component: linked leaf-part id; open?: return its unsaved document and baseline; document?: synchronize a source document without writing files; saved?: source document matches its file; component:0 with document replaces a standalone source draft"),
    CommandSpec::new("component.source_status", "Source Save Status", status).icon("save").noundo().params("→ sources: component, file, dirty"),
    CommandSpec::new("component.save_source", "Save Linked Part", save)
        .icon("save")
        .noundo()
        .params("component: source-backed part id; refuses external file changes or failed features"),
];

fn component(p: &Value) -> Result<u64> {
    p.get("component").and_then(Value::as_u64).filter(|c| *c != 0).ok_or_else(|| bad(CMD, "component id required"))
}
fn file(s: &Session, c: u64) -> Result<std::path::PathBuf> {
    let link = s.doc.links.get(&c).ok_or_else(|| bad(CMD, "component has no source file"))?;
    Ok(super::component_link::resolve(s.path.as_deref(), &link.path))
}
fn read(path: &std::path::Path) -> Result<Session> {
    let (doc, cache) =
        solidcraft_io::read_design_file(&path.to_string_lossy(), None::<fn(solidcraft_io::DesignPreview)>).map_err(|e| bad(CMD, e.to_string()))?;
    Ok(Session::opened(doc, cache.as_ref()))
}
fn unique(base: &str, used: &mut BTreeSet<String>) -> String {
    let mut name = base.to_string();
    let mut n = 2usize;
    while used.contains(&name) {
        name = format!("{base} ({n})");
        n += 1;
    }
    used.insert(name.clone());
    name
}
fn unique_parameter(base: &str, used: &mut BTreeSet<String>) -> String {
    let mut name = base.to_string();
    let mut n = 2usize;
    while used.contains(&name) {
        name = format!("{base}_{n}");
        n += 1;
    }
    used.insert(name.clone());
    name
}
fn own_features(doc: &Document, c: u64) -> Vec<Feature> {
    doc.features.iter().filter(|f| f.component == c).cloned().collect()
}
fn own_params(doc: &Document, e: &SourceEdit) -> Vec<solidcraft_doc::Param> {
    doc.params.iter().filter(|p| e.parameters.values().any(|n| *n == p.name)).cloned().collect()
}
pub(crate) fn dirty(doc: &Document, c: u64) -> bool {
    doc.source_edits.get(&c).is_some_and(|e| {
        !doc.features.iter().filter(|f| f.component == c).eq(e.baseline_features.iter())
            || !doc.params.iter().filter(|p| e.parameters.values().any(|n| *n == p.name)).eq(e.baseline_params.iter())
    })
}

pub(crate) fn begin(s: &mut Session, c: u64) -> Result<()> {
    if s.doc.source_edits.contains_key(&c) {
        return Ok(());
    }
    let link = s.doc.links.get(&c).cloned().ok_or_else(|| bad(CMD, "component is not linked"))?;
    let has_local = s.doc.features.iter().any(|f| f.component == c && f.id != link.feature);
    if s.active_sketch.is_some() || s.doc.marker.is_some_and(|m| m < s.doc.features.len()) {
        return Err(bad(CMD, "finish the sketch and move End of Part to the end first"));
    }
    let path = file(s, c)?;
    let source = read(&path)?;
    if !source.doc.components.is_empty() || !source.doc.links.is_empty() || source.doc.drawing.is_some() || !source.doc.configs.columns.is_empty() {
        return Err(bad(CMD, "in-place source editing currently requires a leaf part without configurations; use Open for this source"));
    }
    if source.model.results.iter().any(|r| r.error.is_some()) {
        return Err(bad(CMD, "source has failed features"));
    }
    if source.doc.features.len() > 10000 || source.doc.params.len() > 10000 {
        return Err(bad(CMD, "source history exceeds edit limit"));
    }
    let edit = SourceEdit {
        source: Box::new((*source.doc).clone()),
        ids: BTreeMap::new(),
        names: BTreeMap::new(),
        bodies: BTreeMap::new(),
        parameters: BTreeMap::new(),
        baseline_features: Vec::new(),
        baseline_params: Vec::new(),
    };
    if has_local { migrate_local(s, c, &source, edit) } else { materialize(s, c, &source, edit, true) }
}

/// Older versions appended component features after a linked snapshot. Replace only that
/// snapshot, retaining the additions as pending source edits, after verifying both stages.
fn migrate_local(s: &mut Session, c: u64, source: &Session, mut edit: SourceEdit) -> Result<()> {
    let link = s.doc.links.get(&c).ok_or_else(|| bad(CMD, "missing source link"))?;
    let at = s.doc.feature_index(link.feature).ok_or_else(|| bad(CMD, "missing linked snapshot"))?;
    if s.doc.features.iter().take(at).any(|f| f.component == c) {
        return Err(bad(CMD, "local history precedes its linked snapshot; keep the assembly draft and resolve its history first"));
    }
    let mut scratch = s.scratch();
    scratch.path.clone_from(&s.path);
    scratch.doc_mut().marker = None;
    scratch.doc_mut().features.retain(|f| f.component != c || f.id == link.feature);
    scratch.refresh();
    if scratch.model.results.iter().any(|r| r.error.is_some()) {
        return Err(bad(CMD, "other assembly features depend on the local additions; their source migration needs explicit reference repair"));
    }
    let before = scratch.world_state();
    // Reserve names of retained local features as well as the assembly's other features.
    let mut names: BTreeSet<_> = s.doc.features.iter().filter(|f| f.id != link.feature).map(|f| f.name.clone()).collect();
    for feature in &source.doc.features {
        edit.names.insert(feature.name.clone(), unique(&feature.name, &mut names));
    }
    materialize(&mut scratch, c, source, edit, true)?;
    let after = scratch.world_state();
    if !super::component_link::same_solids(&before.bodies.iter().collect::<Vec<_>>(), &after.bodies.iter().collect::<Vec<_>>())? {
        return Err(bad(CMD, "the source file no longer matches the older linked snapshot; the assembly draft was kept unchanged"));
    }
    // Keep the original global ordering of local and unrelated features.
    let projected: Vec<_> = scratch.doc.features.iter().filter(|f| f.component == c).cloned().collect();
    let mut features = Vec::new();
    for feature in &s.doc.features {
        if feature.id == link.feature {
            features.extend(projected.iter().cloned());
        } else {
            features.push(feature.clone());
        }
    }
    scratch.doc_mut().features = features;
    scratch.refresh();
    if scratch.model.results.iter().any(|r| r.error.is_some()) {
        return Err(bad(CMD, "local additions cannot be replayed against the source history; the assembly draft was kept unchanged"));
    }
    rebind_relationships(&mut scratch, s, c)?;
    let before = s.world_state();
    let after = scratch.world_state();
    if !super::component_link::same_solids(&before.bodies.iter().collect::<Vec<_>>(), &after.bodies.iter().collect::<Vec<_>>())? {
        return Err(bad(CMD, "source migration would change the assembly geometry; the draft was kept unchanged"));
    }
    // Validate reverse references and geometry before accepting; a successful assembly
    // replay alone cannot prove that a standalone source resolves the same body names.
    let (document, _) = draft(&scratch, c)?;
    let part = Session::new(document);
    let component_state = scratch.model.state();
    let source_state = part.model.state();
    if !super::component_link::same_solids(
        &component_state.bodies.iter().filter(|b| scratch.doc.model_body_component(b) == c).collect::<Vec<_>>(),
        &source_state.bodies.iter().collect::<Vec<_>>(),
    )? {
        return Err(bad(CMD, "local additions resolve differently outside the assembly; the draft was kept unchanged"));
    }
    s.accept_computed(scratch);
    Ok(())
}

fn materialize(s: &mut Session, c: u64, source: &Session, mut edit: SourceEdit, clean: bool) -> Result<()> {
    if !source.doc.components.is_empty() || !source.doc.links.is_empty() || source.doc.drawing.is_some() || !source.doc.configs.columns.is_empty() {
        return Err(bad(CMD, "source synchronization currently requires an unconfigured leaf part"));
    }
    if source.doc.features.len() > 10000 || source.doc.params.len() > 10000 || source.model.results.iter().any(|r| r.error.is_some()) {
        return Err(bad(CMD, "source history is too large or has failed features"));
    }
    let link = s.doc.links.get(&c).cloned().ok_or_else(|| bad(CMD, "missing source link"))?;
    let mut scratch = s.scratch();
    scratch.path.clone_from(&s.path);
    if let Some(maximum) = edit.ids.values().max() {
        scratch.doc_mut().next_id = scratch.doc.next_id.max(maximum.checked_add(1).ok_or_else(|| bad(CMD, "feature id limit"))?);
    }
    let mut used: BTreeSet<String> = s.doc.features.iter().filter(|f| f.id != link.feature).map(|f| f.name.clone()).collect();
    for f in &source.doc.features {
        if let std::collections::btree_map::Entry::Vacant(entry) = edit.ids.entry(f.id) {
            let id = scratch.doc.next_id;
            scratch.doc_mut().next_id = id.checked_add(1).ok_or_else(|| bad(CMD, "feature id limit"))?;
            entry.insert(id);
        }
        let name = edit.names.get(&f.name).filter(|n| !used.contains(*n)).cloned().unwrap_or_else(|| unique(&f.name, &mut used));
        used.insert(name.clone());
        edit.names.insert(f.name.clone(), name);
    }
    for p in &source.doc.params {
        edit.parameters.entry(p.name.clone()).or_insert_with(|| format!("source{c}_{}", p.name));
    }
    let source_bodies = source.model.state();
    let assembly_bodies = s.model.state();
    let current: Vec<_> = assembly_bodies.bodies.iter().filter(|b| s.doc.model_body_component(b) == c).map(|b| b.name.clone()).collect();
    if source_bodies.bodies.len() != current.len() {
        return Err(bad(CMD, "source changed since insertion; update the link before editing"));
    }
    let mut body_names: BTreeSet<String> = assembly_bodies.bodies.iter().map(|b| b.name.clone()).collect();
    for (b, name) in source_bodies.bodies.iter().zip(current) {
        edit.bodies.insert(b.name.clone(), name);
    }
    for r in &source.model.results {
        for b in &r.state.bodies {
            if !edit.bodies.contains_key(&b.name) {
                edit.bodies.insert(b.name.clone(), unique(&b.name, &mut body_names));
            }
        }
    }
    let mut features = Vec::new();
    for original in &source.doc.features {
        let mut f = original.clone();
        f.id = *edit.ids.get(&original.id).ok_or_else(|| bad(CMD, "missing source identity"))?;
        f.name = edit.names.get(&original.name).cloned().ok_or_else(|| bad(CMD, "missing feature name"))?;
        f.component = c;
        if f.body_names.is_empty() {
            f.body_names = source
                .model
                .result(original.id)
                .map(|r| r.state.bodies.iter().filter(|b| b.feature == original.id).map(|b| b.name.clone()).collect())
                .unwrap_or_default();
        }
        f.remap_clone(&edit.ids, &edit.names, &edit.bodies, &edit.parameters)?;
        if let solidcraft_doc::FeatureKind::Sketch { plane, .. } = &mut f.kind
            && let Some(anchor) = super::sketch::face::anchor_mut(plane)
        {
            anchor.component = c;
            // Both sides use the same placement, so the attachment stays in part
            // coordinates while retaining the explicit path required by assembly evaluation.
            anchor.path = scratch.doc.first_path(c);
            anchor.author_path = anchor.path.clone();
        }
        features.push(f);
    }
    let at = scratch.doc.feature_index(link.feature).ok_or_else(|| bad(CMD, "linked snapshot is missing"))?;
    scratch.doc_mut().features.splice(at..=at, features);
    for original in &source.doc.params {
        let mut p = original.clone();
        p.name = edit.parameters.get(&p.name).cloned().ok_or_else(|| bad(CMD, "missing parameter identity"))?;
        if scratch.doc.param(&p.name).is_some() {
            return Err(bad(CMD, "source parameter name conflicts with assembly"));
        }
        p.expr = solidcraft_doc::clone_expression(&p.expr, &edit.parameters);
        scratch.doc_mut().params.push(p);
    }
    scratch.refresh();
    if let Some(failed) = scratch.model.results.iter().find(|r| r.error.is_some()) {
        return Err(bad(CMD, format!("source history cannot be evaluated in this assembly: feature {}: {:?}", failed.id, failed.error)));
    }
    rebind_relationships(&mut scratch, s, c)?;
    if clean {
        edit.baseline_features = own_features(&scratch.doc, c);
        edit.baseline_params = own_params(&scratch.doc, &edit);
    }
    scratch.doc_mut().source_edits.insert(c, edit);
    s.accept_computed(scratch);
    Ok(())
}
fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(value) = p.get("document") {
        let doc = Document::from_json(&value.to_string())?;
        if p.get("component").and_then(Value::as_u64) == Some(0) {
            let checked = Session::new(doc.clone());
            if checked.model.results.iter().any(|r| r.error.is_some()) {
                return Err(bad(CMD, "source draft has failed features"));
            }
            *s.doc_mut() = doc;
            s.refresh();
            if p.get("saved").and_then(Value::as_bool).unwrap_or(false) {
                s.saved = s.doc.clone();
            }
            return Ok(json!({"synchronized":true}));
        }
        let c = component(p)?;
        adopt(s, c, doc, p.get("saved").and_then(Value::as_bool).unwrap_or(false))?;
        return Ok(json!({"synchronized":true}));
    }
    let c = component(p)?;
    begin(s, c)?;
    if p.get("open").and_then(Value::as_bool).unwrap_or(false) {
        let (doc, edit) = draft(s, c)?;
        let baseline = edit.source.clone();
        if s.doc.source_edits.get(&c) != Some(&edit) {
            s.doc_mut().source_edits.insert(c, edit);
        }
        return Ok(json!({"document":doc,"baseline":baseline,"dirty":dirty(&s.doc,c)}));
    }
    Ok(json!({"component":c}))
}

fn adopt(s: &mut Session, c: u64, doc: Document, saved: bool) -> Result<()> {
    begin(s, c)?;
    let mut state = s.scratch();
    state.path.clone_from(&s.path);
    if state.doc.marker.is_some_and(|m| m >= state.doc.features.len()) {
        state.doc_mut().marker = None;
    }
    let mut edit = state.doc.source_edits.get(&c).cloned().ok_or_else(|| bad(CMD, "missing source edit"))?;
    let source = Session::new(doc.clone());
    let (_, sf, faces) = super::component_link::session_bodies(&source, "Unsaved source")?;
    let at = state.doc.features.iter().position(|f| f.component == c).ok_or_else(|| bad(CMD, "missing editable history"))?;
    let parameters: BTreeSet<_> = edit.parameters.values().cloned().collect();
    let relationships = state.doc.assembly.clone();
    state.doc_mut().assembly = Default::default();
    state.doc_mut().features.retain(|f| f.component != c);
    state.doc_mut().params.retain(|p| !parameters.contains(&p.name));
    state.doc_mut().source_edits.remove(&c);
    let id = state.doc_mut().add_feature(sf.kind, Some("Source draft"))?;
    let index = state.doc.feature_index(id).ok_or_else(|| bad(CMD, "missing source draft"))?;
    let mut feature = state.doc_mut().features.remove(index);
    feature.component = c;
    feature.body_names = sf.body_names.into_iter().map(|n| edit.bodies.get(&n).cloned().unwrap_or(n)).collect();
    feature.link_faces = faces;
    state.doc_mut().features.insert(at.min(index), feature);
    if let Some(link) = state.doc_mut().links.get_mut(&c) {
        link.feature = id;
    }
    if saved {
        edit.source = Box::new(doc);
    }
    state.refresh();
    materialize(&mut state, c, &source, edit, saved)?;
    state.doc_mut().assembly = relationships;
    rebind_relationships(&mut state, s, c)?;
    s.accept_computed(state);
    Ok(())
}

/// Replace imported identities in relationships with those of the same geometric faces in
/// the editable history before discarding the snapshot. Placements themselves do not change.
fn rebind_relationships(next: &mut Session, old: &Session, c: u64) -> Result<()> {
    use solidcraft_doc::{constraints::Geo, naming};
    let previous = old.model.state();
    let state = next.model.state();
    let bodies: Vec<_> = state.bodies.iter().filter(|b| next.doc.model_body_component(b) == c).collect();
    let own: BTreeSet<_> = next.doc.occurrences.iter().filter(|o| o.component == c).map(|o| o.id).collect();
    let ids: BTreeMap<_, _> = old
        .doc
        .source_edits
        .get(&c)
        .into_iter()
        .flat_map(|e| e.ids.iter())
        .filter_map(|(source, id)| Some((*id, *next.doc.source_edits.get(&c)?.ids.get(source)?)))
        .collect();
    let rename = |name: &mut String| -> Result<()> {
        let candidate = solidcraft_doc::remap_topology(name, &ids, &BTreeMap::new());
        if !ids.is_empty() && naming::point_on_face(&state, &candidate, solidcraft_geom::Vec3::ZERO).is_some() {
            *name = candidate;
            return Ok(());
        }
        if let Some((point, _)) = naming::point_on_face(&previous, name, solidcraft_geom::Vec3::ZERO) {
            *name = bodies
                .iter()
                .find_map(|b| solidcraft_doc::appearance::face_index_at(b, point).and_then(|i| naming::face_names(b).get(i).cloned()))
                .ok_or_else(|| bad(CMD, "a constrained face cannot be matched to source history"))?;
        } else if let Some(point) = previous.bodies.iter().find_map(|b| naming::find_edge(b, name, solidcraft_geom::Vec3::ZERO).ok().map(|e| e.0)) {
            *name = bodies
                .iter()
                .find_map(|b| naming::edge_names_at(b, &[point]).pop())
                .ok_or_else(|| bad(CMD, "a constrained edge cannot be matched to source history"))?;
        } else {
            return Err(bad(CMD, "an existing relationship has lost its linked geometry"));
        }
        Ok(())
    };
    let mut assembly = next.doc.assembly.clone();
    for constraint in &mut assembly.constraints {
        for pick in constraint.picks.iter_mut().filter(|p| own.contains(&p.occurrence)) {
            match &mut pick.geo {
                Geo::Face { name, .. } | Geo::Edge { name, .. } | Geo::Vertex { edge: name, .. } => rename(name)?,
                Geo::Origin { .. } => {}
            }
        }
    }
    for joint in &mut assembly.joints {
        for origin in [&mut joint.a, &mut joint.b] {
            if own.contains(&origin.occurrence) {
                for name in origin.snap.face_names_mut() {
                    rename(name)?;
                }
            }
        }
    }
    for origin in &mut assembly.origins {
        if own.contains(&origin.origin.occurrence) {
            for name in origin.origin.snap.face_names_mut() {
                rename(name)?;
            }
        }
    }
    next.doc_mut().assembly = assembly;
    super::joints::resolve(next);
    Ok(())
}
fn inverse<K: Ord + Clone, V: Ord + Clone>(map: &BTreeMap<K, V>) -> BTreeMap<V, K> {
    map.iter().map(|(a, b)| (b.clone(), a.clone())).collect()
}

/// Reconstruct a source document with original identities, parameters, units and metadata.
pub(crate) fn draft(s: &Session, c: u64) -> Result<(Document, SourceEdit)> {
    let mut edit = s.doc.source_edits.get(&c).cloned().ok_or_else(|| bad(CMD, "source history is not loaded"))?;
    let mut doc = (*edit.source).clone();
    let mut ids = inverse(&edit.ids);
    let mut names = inverse(&edit.names);
    let bodies = inverse(&edit.bodies);
    let mut parameters = inverse(&edit.parameters);
    let features = own_features(&s.doc, c);
    let mut used_names: BTreeSet<_> = names.values().cloned().collect();
    for feature in &features {
        if !names.contains_key(&feature.name) {
            let name = unique(&feature.name, &mut used_names);
            names.insert(feature.name.clone(), name.clone());
            edit.names.insert(name, feature.name.clone());
        }
    }
    for id in ids.values() {
        doc.next_id = doc.next_id.max(id.checked_add(1).ok_or_else(|| bad(CMD, "source id limit"))?);
    }
    for f in &features {
        if let std::collections::btree_map::Entry::Vacant(entry) = ids.entry(f.id) {
            let id = doc.next_id;
            doc.next_id = id.checked_add(1).ok_or_else(|| bad(CMD, "source id limit"))?;
            entry.insert(id);
            edit.ids.insert(id, f.id);
        }
    }
    for id in ids.values() {
        doc.next_id = doc.next_id.max(id.checked_add(1).ok_or_else(|| bad(CMD, "source id limit"))?);
    }
    let mut needed: BTreeSet<String> = parameters.keys().cloned().collect();
    for f in &features {
        needed.extend(f.param_names.iter().cloned());
        for expr in f.kind.expressions() {
            needed.extend(solidcraft_doc::expr::references(expr));
        }
        if let solidcraft_doc::FeatureKind::Sketch { sketch, .. } = &f.kind {
            needed.extend(sketch.constraints.iter().filter_map(|x| x.param.clone()));
        }
    }
    for _ in 0..s.doc.params.len().min(10000) {
        let count = needed.len();
        for p in s.doc.params.iter().filter(|p| needed.contains(&p.name)).collect::<Vec<_>>() {
            let refs = solidcraft_doc::expr::references(&p.expr);
            needed.extend(refs);
        }
        if needed.len() == count {
            break;
        }
    }
    let mut used: BTreeSet<_> = doc.params.iter().map(|p| p.name.clone()).collect();
    used.extend(parameters.values().cloned());
    for name in &needed {
        if !parameters.contains_key(name) {
            let local = unique_parameter(name, &mut used);
            parameters.insert(name.clone(), local.clone());
            edit.parameters.insert(local, name.clone());
        }
    }
    doc.features.clear();
    for mut f in features {
        if s.model.result(f.id).is_some_and(|r| r.error.is_some()) {
            return Err(bad(CMD, "part has failed features; fix them before saving"));
        }
        f.id = *ids.get(&f.id).ok_or_else(|| bad(CMD, "missing draft identity"))?;
        f.component = 0;
        if let Some(name) = names.get(&f.name) {
            f.name = name.clone();
        }
        f.remap_clone(&ids, &names, &bodies, &parameters)?;
        if let solidcraft_doc::FeatureKind::Sketch { plane, .. } = &mut f.kind
            && let Some(anchor) = super::sketch::face::anchor_mut(plane)
        {
            if anchor.component != c {
                return Err(bad(CMD, "cross-part face attachments require an assembly-context source session"));
            }
            anchor.component = 0;
            anchor.path.clear();
            anchor.author_path.clear();
        }
        doc.features.push(f);
    }
    doc.params.clear();
    for p in s.doc.params.iter().filter(|p| needed.contains(&p.name)) {
        let mut p = p.clone();
        p.name = parameters.get(&p.name).cloned().ok_or_else(|| bad(CMD, "missing draft parameter"))?;
        p.expr = solidcraft_doc::clone_expression(&p.expr, &parameters);
        doc.params.push(p);
    }
    doc.marker = None;
    let checked = Session::new(doc.clone());
    if checked.model.results.iter().any(|r| r.error.is_some()) {
        return Err(bad(CMD, "part depends on assembly geometry; independent source save is not supported for those references"));
    }
    Ok((doc, edit))
}
fn status(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(
        json!({"sources":s.doc.source_edits.keys().map(|c|Ok(json!({"component":c,"file":file(s,*c)?.to_string_lossy(),"dirty":dirty(&s.doc,*c)}))).collect::<Result<Vec<_>>>()?}),
    )
}
fn save(s: &mut Session, p: &Value) -> Result<Value> {
    let c = component(p)?;
    let path = file(s, c)?;
    let (doc, mut edit) = draft(s, c)?;
    let current = read(&path)?;
    if *current.doc != *edit.source {
        return Err(bad("component.save_source", "source file changed outside this edit session; no file was overwritten"));
    }
    let mut part = Session::new(doc);
    part.execute("file.save", &json!({"path":path}))?;
    edit.source = Box::new((*part.doc).clone());
    edit.baseline_features = own_features(&s.doc, c);
    edit.baseline_params = own_params(&s.doc, &edit);
    s.doc_mut().source_edits.insert(c, edit);
    Ok(json!({"component":c,"file":path}))
}

pub(crate) fn refresh(s: &mut Session, c: u64) -> Result<bool> {
    let path = file(s, c)?;
    let current = read(&path)?;
    let edit = s.doc.source_edits.get(&c).ok_or_else(|| bad(CMD, "missing source edit"))?;
    if *current.doc == *edit.source {
        return Ok(false);
    }
    if dirty(&s.doc, c) {
        return Err(bad(CMD, "source changed while this assembly has pending part edits; save or resolve those edits first"));
    }
    let link = s.doc.links.get(&c).cloned().ok_or_else(|| bad(CMD, "missing link"))?;
    let (_, sf, faces) = super::component_link::session_bodies(&current, &path.to_string_lossy())?;
    let mut scratch = s.scratch();
    scratch.path.clone_from(&s.path);
    let relationships = scratch.doc.assembly.clone();
    scratch.doc_mut().assembly = Default::default();
    let at = scratch.doc.features.iter().position(|f| f.component == c).ok_or_else(|| bad(CMD, "missing part history"))?;
    let params: BTreeSet<_> = edit.parameters.values().cloned().collect();
    scratch.doc_mut().features.retain(|f| f.component != c);
    scratch.doc_mut().params.retain(|p| !params.contains(&p.name));
    scratch.doc_mut().source_edits.remove(&c);
    let id = scratch.doc_mut().add_feature(sf.kind, Some("Linked source"))?;
    let index = scratch.doc.feature_index(id).ok_or_else(|| bad(CMD, "missing source snapshot"))?;
    let mut f = scratch.doc_mut().features.remove(index);
    f.component = c;
    f.body_names = sf.body_names;
    f.link_faces = faces;
    scratch.doc_mut().features.insert(at.min(index), f);
    if let Some(l) = scratch.doc_mut().links.get_mut(&c) {
        l.feature = id;
    }
    let _ = link;
    scratch.refresh();
    begin(&mut scratch, c)?;
    scratch.doc_mut().assembly = relationships;
    rebind_relationships(&mut scratch, s, c)?;
    s.accept_computed(scratch);
    Ok(true)
}
