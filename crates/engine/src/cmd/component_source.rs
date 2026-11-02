//! Editable source histories projected into their assembly definition, with explicit source
//! saves. Geometry remains in definition coordinates; occurrence transforms stay in the assembly.
use super::CommandSpec;
use crate::params::bad;
use crate::{Result, Session};
use serde_json::{Value, json};
use solidcraft_doc::{Document, Feature, SourceEdit};
use std::collections::{BTreeMap, BTreeSet};

const CMD: &str = "component.edit_source";
#[path = "component_source_graph.rs"]
mod graph;
#[path = "component_source_properties.rs"]
mod properties;

pub(super) fn prepare_properties(s: &mut Session, components: impl IntoIterator<Item = u64>) -> Result<()> {
    let components: BTreeSet<_> = components.into_iter().filter(|c| s.doc.links.contains_key(c)).collect();
    for component in components {
        begin(s, component)?;
    }
    Ok(())
}

pub(super) fn property_body(s: &Session, before: &solidcraft_doc::ModelBody) -> Result<solidcraft_doc::ModelBody> {
    let world = s.world_state();
    if let Some(body) = world.bodies.iter().find(|b| b.occurrence_path == before.occurrence_path && b.definition_name() == before.definition_name()) {
        return Ok(body.clone());
    }
    let mut found = None;
    let candidates: Vec<_> = world.bodies.iter().filter(|b| b.occurrence_path.starts_with(&before.occurrence_path)).take(10001).collect();
    if candidates.len() > 10000 {
        return Err(bad(CMD, "property target matching exceeds source limit"));
    }
    for body in candidates {
        if super::component_link::same_solids(&[before], &[body])? {
            if found.is_some() {
                return Err(bad(CMD, "property target is ambiguous in the nested source"));
            }
            found = Some(body.clone());
        }
    }
    found.ok_or_else(|| bad(CMD, "property target cannot be matched to source history"))
}

pub(super) fn snapshot_properties(s: &mut Session, c: u64, source: &Session) -> Result<()> {
    let state = s.model.state();
    let source_world = source.world_state();
    let mut materials = BTreeMap::new();
    if !source.doc.materials.is_empty() {
        let mut pieces = Vec::new();
        for body in &source_world.bodies {
            for lump in body.body.lumps()? {
                pieces.push((
                    solidcraft_doc::ModelBody::new(body.name.clone(), lump, body.feature),
                    source.doc.body_material(body.definition_name()).to_string(),
                ));
            }
        }
        let mut budget = 100000usize;
        for body in state.bodies.iter().filter(|b| b.component == c) {
            let mut matches = BTreeSet::new();
            for (piece, material) in &pieces {
                budget = budget.checked_sub(1).ok_or_else(|| bad(CMD, "material matching exceeds source limit"))?;
                if super::component_link::same_solids(&[body], &[piece])? {
                    matches.insert(material.clone());
                }
            }
            if matches.len() != 1 {
                return Err(bad(CMD, "source physical materials cannot be uniquely matched to linked geometry"));
            }
            if let Some(material) = matches.first().filter(|m| m.as_str() != solidcraft_doc::DEFAULT_MATERIAL) {
                materials.insert(body.name.clone(), material.clone());
            }
        }
    }
    for material in &source.doc.user_materials {
        if let Some(existing) = s.doc.user_materials.iter().find(|m| m.name.eq_ignore_ascii_case(&material.name)) {
            if existing != material {
                return Err(bad(CMD, format!("source material `{}` conflicts with an assembly material", material.name)));
            }
        } else {
            s.doc_mut().user_materials.push(material.clone());
        }
    }
    if let Some(component) = s.doc_mut().components.iter_mut().find(|d| d.id == c) {
        component.metadata = source.doc.metadata.clone();
        component.properties = source.doc.properties.clone();
    }
    let names: BTreeSet<_> = state.bodies.iter().filter(|b| b.component == c).map(|b| b.name.clone()).collect();
    s.doc_mut().materials.retain(|b, _| !names.contains(b));
    s.doc_mut().materials.extend(materials);
    Ok(())
}

pub(super) fn rebase_edits(doc: &Document, from: Option<&str>, to: &str, depth: usize) -> Result<BTreeMap<u64, SourceEdit>> {
    if depth > 32 {
        return Err(bad(CMD, "nested source baseline depth exceeded"));
    }
    let mut edits = doc.source_edits.clone();
    for edit in edits.values_mut() {
        if let Some(graph) = &mut edit.graph {
            let edits = rebase_edits(&graph.baseline, from, to, depth + 1)?;
            graph.baseline.links = super::component_link::rebased(&graph.baseline, from, to);
            graph.baseline.source_edits = edits;
        }
    }
    Ok(edits)
}
pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new(CMD, "Edit Linked Part", edit)
        .icon("component")
        .noundo()
        .params("component: linked part or subassembly id; open?: return its unsaved document and baseline; document?: synchronize a source document without writing files; saved?: source document matches its file; component:0 with document replaces a standalone source draft"),
    CommandSpec::new("component.source_status", "Source Save Status", status).icon("save").noundo().params("→ sources: component, file, dirty"),
    CommandSpec::new("component.save_source", "Save Linked Part", save)
        .icon("save")
        .noundo()
        .params("component: source-backed part or subassembly id; refuses external file changes or failed features"),
];

fn component(p: &Value) -> Result<u64> {
    p.get("component").and_then(Value::as_u64).filter(|c| *c != 0).ok_or_else(|| bad(CMD, "component id required"))
}
fn file(s: &Session, c: u64) -> Result<std::path::PathBuf> {
    let link = s.doc.links.get(&c).ok_or_else(|| bad(CMD, "component has no source file"))?;
    Ok(super::component_link::resolve(s.path.as_deref(), &link.path))
}
fn read(s: &Session, path: &std::path::Path) -> Result<Session> {
    s.read_source(path)
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
    if let Some(edit) = doc.source_edits.get(&c)
        && edit.baseline_properties.as_deref().is_some_and(|baseline| properties::scope(doc, c, edit) != *baseline)
    {
        return true;
    }
    if let Some(edit) = doc.source_edits.get(&c)
        && let Some(graph) = &edit.graph
    {
        return graph::scope(doc, c, edit).is_none_or(|now| now != *graph.baseline);
    }
    doc.source_edits.get(&c).is_some_and(|e| {
        !doc.features.iter().filter(|f| f.component == c).eq(e.baseline_features.iter())
            || !doc.params.iter().filter(|p| e.parameters.values().any(|n| *n == p.name)).eq(e.baseline_params.iter())
    })
}

pub(crate) fn begin(s: &mut Session, c: u64) -> Result<()> {
    if s.doc.source_edits.contains_key(&c) {
        if let Some(mut edit) = s.doc.source_edits.get(&c).filter(|e| e.baseline_properties.is_none()).cloned() {
            let components = edit.graph.as_ref().map_or_else(|| BTreeMap::from([(0, c)]), |g| g.components.clone());
            let projected = properties::remap(&properties::source(&edit.source), &edit.bodies, &edit.ids, &components);
            let mut scratch = s.scratch();
            properties::install(scratch.doc_mut(), c, &edit, &projected)?;
            edit.baseline_properties = Some(Box::new(properties::scope(&scratch.doc, c, &edit)));
            if let Some(graph) = &mut edit.graph {
                properties::document(&projected, &mut graph.baseline);
            }
            properties::preserve_legacy(&mut scratch, s, c, &edit, &projected)?;
            scratch.doc_mut().source_edits.insert(c, edit);
            s.accept_computed(scratch);
        }
        return Ok(());
    }
    let link = s.doc.links.get(&c).cloned().ok_or_else(|| bad(CMD, "component is not linked"))?;
    let has_local = s.doc.features.iter().any(|f| f.component == c && f.id != link.feature);
    if s.active_sketch.is_some() || s.doc.marker.is_some_and(|m| m < s.doc.features.len()) {
        return Err(bad(CMD, "finish the sketch and move End of Part to the end first"));
    }
    let path = file(s, c)?;
    let canonical = |p: &std::path::Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let target = canonical(&path);
    if s.path.as_deref().is_some_and(|p| canonical(std::path::Path::new(p)) == target)
        || s.doc.links.iter().any(|(parent, link)| {
            *parent != c && s.doc.component_within(c, *parent) && canonical(&super::component_link::resolve(s.path.as_deref(), &link.path)) == target
        })
    {
        return Err(bad(CMD, "cyclic source dependency: this file is already an ancestor of the selected component"));
    }
    let source = read(s, &path)?;
    if source.doc.drawing.is_some() || !source.doc.configs.columns.is_empty() {
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
        baseline_properties: None,
        graph: None,
    };
    if !source.doc.components.is_empty() || !source.doc.links.is_empty() {
        return graph::begin(s, c, &source, edit);
    }
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
    let mut scratch = s.scratch();
    scratch.path.clone_from(&s.path);
    if scratch.doc.marker.is_some_and(|m| m >= s.doc.features.len()) {
        scratch.doc_mut().marker = None;
    }
    if let Some(maximum) = edit.ids.values().max() {
        scratch.doc_mut().next_id = scratch.doc.next_id.max(maximum.checked_add(1).ok_or_else(|| bad(CMD, "feature id limit"))?);
    }
    let replacing = s.doc.source_edits.contains_key(&c);
    let mut used: BTreeSet<String> = s.doc.features.iter().filter(|f| f.component != c).map(|f| f.name.clone()).collect();
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
    if !replacing && source_bodies.bodies.len() != current.len() {
        return Err(bad(CMD, "source changed since insertion; update the link before editing"));
    }
    let mut body_names: BTreeSet<String> = assembly_bodies.bodies.iter().map(|b| b.name.clone()).collect();
    body_names.extend(s.doc.reserved_body_names.iter().cloned());
    if !replacing {
        for (b, name) in source_bodies.bodies.iter().zip(current) {
            edit.bodies.entry(b.name.clone()).or_insert(name);
        }
    }
    for r in &source.model.results {
        for b in &r.state.bodies {
            if !edit.bodies.contains_key(&b.name) {
                edit.bodies.insert(b.name.clone(), unique(&b.name, &mut body_names));
            }
        }
    }
    for name in &source.doc.reserved_body_names {
        if !edit.bodies.contains_key(name) {
            edit.bodies.insert(name.clone(), unique(name, &mut body_names));
        }
    }
    let owned_names: BTreeSet<_> = edit.bodies.values().collect();
    scratch.doc_mut().reserved_body_names.retain(|name| !owned_names.contains(name));
    scratch.doc_mut().reserved_body_names.extend(source.doc.reserved_body_names.iter().filter_map(|name| edit.bodies.get(name).cloned()));
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
    let at = scratch.doc.features.iter().position(|f| f.component == c).ok_or_else(|| bad(CMD, "linked history is missing"))?;
    scratch.doc_mut().features.retain(|f| f.component != c);
    scratch.doc_mut().features.splice(at..at, features);
    if replacing {
        let mapped: BTreeSet<_> = edit.parameters.values().collect();
        scratch.doc_mut().params.retain(|p| !mapped.contains(&p.name));
    }
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
    let projected = properties::remap(&properties::source(&source.doc), &edit.bodies, &edit.ids, &BTreeMap::from([(0, c)]));
    properties::install(scratch.doc_mut(), c, &edit, &projected)?;
    if clean {
        edit.baseline_properties = Some(Box::new(properties::scope(&scratch.doc, c, &edit)));
    }
    if replacing {
        properties::rebind_overrides(&mut scratch, s, c)?;
    } else {
        properties::preserve_legacy(&mut scratch, s, c, &edit, &projected)?;
    }
    scratch.doc_mut().source_edits.insert(c, edit);
    s.accept_computed(scratch);
    std::sync::Arc::make_mut(&mut s.source_models).insert(c, source.model.clone());
    Ok(())
}
fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(value) = p.get("document") {
        let doc = Document::from_json(&value.to_string())?;
        if p.get("component").and_then(Value::as_u64) == Some(0) {
            let mut checked = s.scratch();
            *checked.doc_mut() = doc;
            checked.refresh();
            if checked.model.results.iter().any(|r| r.error.is_some()) {
                return Err(bad(CMD, "source draft has failed features"));
            }
            s.accept_computed(checked);
            if p.get("saved").and_then(Value::as_bool).unwrap_or(false) {
                s.saved = s.doc.clone();
                s.saved_outputs = s
                    .model
                    .results
                    .iter()
                    .map(|r| (r.id, r.state.bodies.iter().filter(|b| b.feature == r.id).map(|b| b.name.clone()).collect()))
                    .collect();
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
        let mut comparison_baseline = (*baseline).clone();
        let projected = edit.graph.as_ref().map(|g| g.baseline.features.as_slice()).unwrap_or(&edit.baseline_features);
        let bodies = inverse(&edit.bodies);
        for f in &mut comparison_baseline.features {
            if f.body_names.is_empty()
                && let Some(p) = edit.ids.get(&f.id).and_then(|id| projected.iter().find(|f| f.id == *id))
            {
                f.body_names = p.body_names.iter().map(|b| bodies.get(b).cloned().unwrap_or_else(|| b.clone())).collect();
            }
        }
        if s.doc.source_edits.get(&c) != Some(&edit) {
            s.doc_mut().source_edits.insert(c, edit);
        }
        return Ok(json!({"document":doc,"baseline":baseline,"comparison_baseline":comparison_baseline,"dirty":dirty(&s.doc,c)}));
    }
    Ok(json!({"component":c}))
}

fn adopt(s: &mut Session, c: u64, doc: Document, saved: bool) -> Result<()> {
    begin(s, c)?;
    if s.doc.source_edits.get(&c).is_some_and(|e| e.graph.is_some()) {
        return graph::adopt(s, c, doc, saved);
    }
    let mut edit = s.doc.source_edits.get(&c).cloned().ok_or_else(|| bad(CMD, "missing source edit"))?;
    let mut source = Session::new(Document::default());
    *source.doc_mut() = doc.clone();
    source.model = s.source_models.get(&c).cloned().unwrap_or_default();
    let mut normalized = (*edit.source).clone();
    source.model.explicit_output_names(&mut normalized);
    source.model.evaluate(&source.doc);
    if saved {
        edit.source = Box::new(doc);
    }
    materialize(s, c, &source, edit, saved)
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
        for pick in constraint.picks.iter_mut().filter(|p| {
            own.contains(&p.occurrence) || (!p.path.is_empty() && next.doc.path_placement(&p.path).is_some_and(|(component, _)| component == c))
        }) {
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
    if s.doc.source_edits.get(&c).is_some_and(|e| e.graph.is_some()) {
        return graph::draft(s, c);
    }
    let mut edit = s.doc.source_edits.get(&c).cloned().ok_or_else(|| bad(CMD, "source history is not loaded"))?;
    let mut doc = (*edit.source).clone();
    let mut ids = inverse(&edit.ids);
    let mut names = inverse(&edit.names);
    let mut bodies = inverse(&edit.bodies);
    doc.reserved_body_names = s.doc.reserved_body_names.iter().filter_map(|name| bodies.get(name).cloned()).collect();
    let mut parameters = inverse(&edit.parameters);
    let features = own_features(&s.doc, c);
    let mut used_bodies: BTreeSet<_> = bodies.values().cloned().collect();
    for f in &features {
        if let Some(result) = s.model.result(f.id) {
            for body in result.state.bodies.iter().filter(|b| s.doc.model_body_component(b) == c) {
                if !bodies.contains_key(&body.name) {
                    let local = unique(&body.name, &mut used_bodies);
                    bodies.insert(body.name.clone(), local.clone());
                    edit.bodies.insert(local, body.name.clone());
                }
            }
        }
    }
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
        if f.body_names.is_empty() {
            f.body_names = s
                .model
                .result(f.id)
                .map(|r| r.state.bodies.iter().filter(|b| b.feature == f.id).map(|b| b.name.clone()).collect())
                .unwrap_or_default();
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
    if edit.baseline_properties.is_some() {
        let projected = properties::scope(&s.doc, c, &edit);
        let projected = properties::remap(&projected, &bodies, &ids, &BTreeMap::from([(c, 0)]));
        properties::document(&projected, &mut doc);
    }
    Ok((doc, edit))
}
fn status(s: &mut Session, _: &Value) -> Result<Value> {
    let mut components: Vec<_> = s.doc.source_edits.keys().copied().collect();
    components.sort_by_key(|c| std::cmp::Reverse(s.doc.first_path(*c).len()));
    Ok(
        json!({"sources":components.into_iter().map(|c|Ok(json!({"component":c,"file":file(s,c)?.to_string_lossy(),"dirty":dirty(&s.doc,c)}))).collect::<Result<Vec<_>>>()?}),
    )
}
fn save(s: &mut Session, p: &Value) -> Result<Value> {
    let c = component(p)?;
    let path = file(s, c)?;
    let (doc, mut edit) = draft(s, c)?;
    let current = read(s, &path)?;
    if *current.doc != *edit.source {
        return Err(bad("component.save_source", "source file changed outside this edit session; no file was overwritten"));
    }
    let mut part = Session::new(doc);
    if part.model.results.iter().any(|r| r.error.is_some()) {
        return Err(bad(CMD, "source does not build independently; its draft is kept and no file was written"));
    }
    part.execute("file.save", &json!({"path":path}))?;
    edit.source = Box::new((*part.doc).clone());
    edit.baseline_features = own_features(&s.doc, c);
    edit.baseline_params = own_params(&s.doc, &edit);
    edit.baseline_properties = Some(Box::new(properties::scope(&s.doc, c, &edit)));
    if edit.graph.is_some() {
        let baseline = graph::scope(&s.doc, c, &edit).ok_or_else(|| bad(CMD, "subassembly references leave its source"))?;
        if let Some(graph) = &mut edit.graph {
            *graph.baseline = baseline;
        }
    }
    s.doc_mut().source_edits.insert(c, edit);
    Ok(json!({"component":c,"file":path}))
}

pub(crate) fn refresh(s: &mut Session, c: u64) -> Result<bool> {
    let path = file(s, c)?;
    let current = read(s, &path)?;
    let edit = s.doc.source_edits.get(&c).ok_or_else(|| bad(CMD, "missing source edit"))?;
    if *current.doc == *edit.source {
        return Ok(false);
    }
    if dirty(&s.doc, c) {
        return Err(bad(CMD, "source changed while this assembly has pending part edits; save or resolve those edits first"));
    }
    if edit.graph.is_some() {
        graph::adopt(s, c, (*current.doc).clone(), true)?;
        return Ok(true);
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
