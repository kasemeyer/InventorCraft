//! Reversible projection of a linked assembly's definition/occurrence graph. File baselines
//! stay in source coordinates; placement paths are explicit in the containing assembly.
use super::{CMD, SourceEdit, bad, inverse, unique, unique_parameter};
use crate::{Result, Session};
use solidcraft_doc::{Document, Feature, FeatureKind, SourceGraph};
use std::collections::{BTreeMap, BTreeSet};

fn advanced(doc: &Document) -> bool {
    let a = &doc.assembly;
    !a.joints.is_empty()
        || !a.origins.is_empty()
        || !a.links.is_empty()
        || !a.contact_sets.is_empty()
        || a.contact_enabled
        || a.contact_all
        || !a.studies.is_empty()
        || !a.traces.is_empty()
        || !a.exploded.is_empty()
}
fn checked(doc: &Document) -> Result<()> {
    if doc.drawing.is_some() || !doc.configs.is_empty() || advanced(doc) || !doc.canvases.is_empty() {
        return Err(bad(
            CMD,
            "linked assembly editing currently supports solid history and assembly constraints; use Open for configurations, joints, motion or canvases",
        ));
    }
    if doc.components.len() > 10000 || doc.occurrences.len() > 10000 || doc.features.len() > 10000 || doc.params.len() > 10000 {
        return Err(bad(CMD, "linked assembly exceeds editing limits"));
    }
    Ok(())
}
fn members(doc: &Document, c: u64) -> BTreeSet<u64> {
    doc.components.iter().filter(|d| doc.component_within(d.id, c)).map(|d| d.id).chain(std::iter::once(c)).collect()
}

pub(super) fn scope(doc: &Document, c: u64, edit: &SourceEdit) -> Option<Document> {
    let owned = members(doc, c);
    let internal: BTreeSet<_> = doc.occurrences.iter().filter(|o| owned.contains(&o.parent)).map(|o| o.id).collect();
    let incoming: BTreeSet<_> = doc.occurrences.iter().filter(|o| o.component == c).map(|o| o.id).collect();
    let a = &doc.assembly;
    if a.joints.iter().any(|j| internal.contains(&j.a.occurrence) || internal.contains(&j.b.occurrence))
        || a.origins.iter().any(|o| internal.contains(&o.origin.occurrence))
        || a.contact_sets.iter().any(|s| s.occurrences.iter().any(|o| internal.contains(o)))
        || a.exploded.iter().any(|v| v.moves.iter().any(|(o, _)| internal.contains(o)))
        || !a.traces.is_empty()
        || doc.canvases.iter().any(|canvas| owned.contains(&canvas.component))
    {
        return None;
    }
    if doc.occurrences.iter().any(|o| owned.contains(&o.component) && o.component != c && !owned.contains(&o.parent)) {
        return None;
    }
    let mut out = Document {
        features: doc.features.iter().filter(|f| owned.contains(&f.component)).cloned().collect(),
        components: doc.components.iter().filter(|d| d.id != c && owned.contains(&d.id)).cloned().collect(),
        occurrences: doc.occurrences.iter().filter(|o| internal.contains(&o.id)).cloned().collect(),
        ..Document::default()
    };
    let mut needed: BTreeSet<_> = edit.parameters.values().cloned().collect();
    for feature in &out.features {
        needed.extend(feature.param_names.iter().cloned());
        for expr in feature.kind.expressions() {
            needed.extend(solidcraft_doc::expr::references(expr));
        }
        if let FeatureKind::Sketch { sketch, .. } = &feature.kind {
            needed.extend(sketch.constraints.iter().filter_map(|c| c.param.clone()));
        }
    }
    for constraint in &doc.assembly.constraints {
        if constraint.picks.iter().any(|p| internal.contains(&p.occurrence)) {
            if !constraint.picks.iter().all(|p| internal.contains(&p.occurrence) || incoming.contains(&p.occurrence)) {
                return None;
            }
            out.assembly.constraints.push(constraint.clone());
        }
    }
    for (_, _, expr) in out.assembly.exprs() {
        needed.extend(solidcraft_doc::expr::references(expr));
    }
    for _ in 0..doc.params.len().min(10000) {
        let before = needed.len();
        let more: Vec<_> = doc.params.iter().filter(|p| needed.contains(&p.name)).flat_map(|p| solidcraft_doc::expr::references(&p.expr)).collect();
        needed.extend(more);
        if needed.len() == before {
            break;
        }
    }
    out.params = doc.params.iter().filter(|p| needed.contains(&p.name)).cloned().collect();
    out.links = doc.links.iter().filter(|(id, _)| **id != c && owned.contains(id)).map(|(id, l)| (*id, l.clone())).collect();
    out.source_edits = doc.source_edits.iter().filter(|(id, _)| **id != c && owned.contains(id)).map(|(id, e)| (*id, e.clone())).collect();
    out.parts = doc.parts.iter().filter(|(id, _)| owned.contains(id)).map(|(id, p)| (*id, p.clone())).collect();
    out.body_components = doc.body_components.iter().filter(|(_, id)| owned.contains(id)).map(|(b, id)| (b.clone(), *id)).collect();
    out.body_offsets = doc.body_offsets.iter().filter(|(b, _)| out.body_components.contains_key(*b)).map(|(b, m)| (b.clone(), *m)).collect();
    let names: BTreeSet<_> = edit.bodies.values().collect();
    out.reserved_body_names = doc.reserved_body_names.iter().filter(|n| names.contains(n)).cloned().collect();
    super::properties::document(&super::properties::scope(doc, c, edit), &mut out);
    out.appearances.occurrences = doc.appearances.occurrences.iter().filter(|o| owned.contains(&o.owner)).cloned().collect();
    Some(out)
}

struct Maps {
    features: BTreeMap<u64, u64>,
    names: BTreeMap<String, String>,
    bodies: BTreeMap<String, String>,
    params: BTreeMap<String, String>,
    components: BTreeMap<u64, u64>,
    occurrences: BTreeMap<u64, u64>,
    constraints: BTreeMap<u64, u64>,
    constraint_names: BTreeMap<String, String>,
    prefix: Vec<u64>,
    strip: BTreeSet<u64>,
}
fn id(map: &BTreeMap<u64, u64>, old: u64) -> Result<u64> {
    map.get(&old).copied().ok_or_else(|| bad(CMD, format!("unmapped source identity {old}")))
}
fn path(old: &[u64], maps: &Maps) -> Result<Vec<u64>> {
    let start = if maps.strip.is_empty() {
        0
    } else {
        old.iter().rposition(|o| maps.strip.contains(o)).map(|i| i + 1).ok_or_else(|| bad(CMD, "attachment leaves its source assembly"))?
    };
    let mut out = maps.prefix.clone();
    for o in old.iter().skip(start) {
        out.push(id(&maps.occurrences, *o)?);
    }
    if out.len() > 64 {
        return Err(bad(CMD, "source occurrence path exceeds depth limit"));
    }
    Ok(out)
}
fn feature(old: &Feature, maps: &Maps) -> Result<Feature> {
    let mut f = old.clone();
    f.id = id(&maps.features, f.id)?;
    f.name = maps.names.get(&f.name).cloned().unwrap_or(f.name);
    f.component = id(&maps.components, f.component)?;
    f.remap_clone(&maps.features, &maps.names, &maps.bodies, &maps.params)?;
    match &mut f.kind {
        FeatureKind::Sketch { plane, .. } => {
            if let Some(a) = crate::cmd::sketch::face::anchor_mut(plane) {
                a.component = id(&maps.components, a.component)?;
                a.path = path(&a.path, maps)?;
                a.author_path = path(&a.author_path, maps)?;
            }
        }
        FeatureKind::Emboss { face: Some(face), .. } => {
            face.path = path(&face.path, maps)?;
            face.author_path = path(&face.author_path, maps)?;
        }
        FeatureKind::BodyTransfer { from, to, .. } => {
            *from = id(&maps.components, *from)?;
            *to = id(&maps.components, *to)?;
        }
        _ => {}
    }
    Ok(f)
}
fn parameter(p: &solidcraft_doc::Param, maps: &Maps) -> solidcraft_doc::Param {
    let mut p = p.clone();
    p.name = maps.params.get(&p.name).cloned().unwrap_or(p.name);
    p.expr = solidcraft_doc::clone_expression(&p.expr, &maps.params);
    p
}
fn remap_edit(edit: &SourceEdit, maps: &Maps, from: Option<&str>, to: &str, depth: usize) -> Result<SourceEdit> {
    if depth > 32 {
        return Err(bad(CMD, "nested source edit limit exceeded"));
    }
    let mut e = edit.clone();
    for v in e.ids.values_mut() {
        *v = id(&maps.features, *v)?;
    }
    for v in e.names.values_mut() {
        if let Some(n) = maps.names.get(v) {
            *v = n.clone();
        }
    }
    for v in e.bodies.values_mut() {
        if let Some(n) = maps.bodies.get(v) {
            *v = n.clone();
        }
    }
    for v in e.parameters.values_mut() {
        if let Some(n) = maps.params.get(v) {
            *v = n.clone();
        }
    }
    e.baseline_features = edit.baseline_features.iter().map(|f| feature(f, maps)).collect::<Result<_>>()?;
    e.baseline_params = edit.baseline_params.iter().map(|p| parameter(p, maps)).collect();
    e.baseline_properties =
        edit.baseline_properties.as_deref().map(|p| Box::new(super::properties::remap(p, &maps.bodies, &maps.features, &maps.components)));
    if let Some(g) = &mut e.graph {
        for v in g.components.values_mut() {
            *v = id(&maps.components, *v)?;
        }
        for v in g.occurrences.values_mut() {
            *v = id(&maps.occurrences, *v)?;
        }
        for v in g.constraints.values_mut() {
            *v = id(&maps.constraints, *v)?;
        }
        for v in g.constraint_names.values_mut() {
            if let Some(n) = maps.constraint_names.get(v) {
                *v = n.clone();
            }
        }
        *g.baseline = remap(&g.baseline, maps, from, to, depth + 1)?;
    }
    Ok(e)
}
fn remap(doc: &Document, maps: &Maps, from: Option<&str>, to: &str, depth: usize) -> Result<Document> {
    let mut out = doc.clone();
    out.features = doc.features.iter().map(|f| feature(f, maps)).collect::<Result<_>>()?;
    out.params = doc.params.iter().map(|p| parameter(p, maps)).collect();
    for c in &mut out.components {
        c.id = id(&maps.components, c.id)?;
        c.parent = id(&maps.components, c.parent)?;
    }
    for o in &mut out.occurrences {
        o.id = id(&maps.occurrences, o.id)?;
        o.component = id(&maps.components, o.component)?;
        o.parent = id(&maps.components, o.parent)?;
    }
    out.links = doc
        .links
        .iter()
        .map(|(c, l)| {
            Ok((
                id(&maps.components, *c)?,
                solidcraft_doc::DesignLink {
                    feature: id(&maps.features, l.feature)?,
                    path: crate::cmd::component_link::stored(Some(to), &crate::cmd::component_link::resolve(from, &l.path)),
                },
            ))
        })
        .collect::<Result<_>>()?;
    out.source_edits =
        doc.source_edits.iter().map(|(c, e)| Ok((id(&maps.components, *c)?, remap_edit(e, maps, from, to, depth + 1)?))).collect::<Result<_>>()?;
    out.parts = doc.parts.iter().map(|(c, p)| Ok((id(&maps.components, *c)?, p.clone()))).collect::<Result<_>>()?;
    out.body_components = doc
        .body_components
        .iter()
        .map(|(b, c)| Ok((maps.bodies.get(b).cloned().unwrap_or_else(|| b.clone()), id(&maps.components, *c)?)))
        .collect::<Result<_>>()?;
    out.body_offsets = doc.body_offsets.iter().map(|(b, m)| (maps.bodies.get(b).cloned().unwrap_or_else(|| b.clone()), *m)).collect();
    out.reserved_body_names = doc.reserved_body_names.iter().map(|b| maps.bodies.get(b).cloned().unwrap_or_else(|| b.clone())).collect();
    let properties = super::properties::remap(&super::properties::source(doc), &maps.bodies, &maps.features, &maps.components);
    super::properties::document(&properties, &mut out);
    out.appearances.occurrences = doc
        .appearances
        .occurrences
        .iter()
        .map(|o| {
            let mut next = o.clone();
            next.owner = id(&maps.components, o.owner)?;
            next.path = path(&o.path, maps)?;
            let mut paint = solidcraft_doc::SourceProperties::default();
            paint.appearances.bodies = o.bodies.clone();
            paint.appearances.faces = o.faces.clone();
            let paint = super::properties::remap(&paint, &maps.bodies, &maps.features, &maps.components);
            next.bodies = paint.appearances.bodies;
            next.faces = paint.appearances.faces;
            Ok(next)
        })
        .collect::<Result<_>>()?;
    for c in &mut out.assembly.constraints {
        c.id = id(&maps.constraints, c.id)?;
        c.name = maps.constraint_names.get(&c.name).cloned().unwrap_or_else(|| c.name.clone());
        for p in &mut c.picks {
            if !p.path.is_empty() {
                p.path = path(&p.path, maps)?;
            }
            p.occurrence = if maps.strip.contains(&p.occurrence) { 0 } else { id(&maps.occurrences, p.occurrence)? };
            use solidcraft_doc::constraints::Geo;
            match &mut p.geo {
                Geo::Face { name, .. } | Geo::Edge { name, .. } | Geo::Vertex { edge: name, .. } => {
                    *name = solidcraft_doc::remap_topology(name, &maps.features, &maps.bodies)
                }
                Geo::Origin { .. } => {}
            }
        }
    }
    for expr in out.assembly.exprs_mut() {
        *expr = solidcraft_doc::clone_expression(expr, &maps.params);
    }
    Ok(out)
}

fn allocate(next: &mut u64) -> Result<u64> {
    let value = *next;
    if value == 0 || value >= solidcraft_doc::MAX_FEATURE_ID {
        return Err(bad(CMD, "source identifier limit exceeded"));
    }
    *next = value + 1;
    Ok(value)
}
fn reserve(next: &mut u64, values: impl Iterator<Item = u64>) -> Result<()> {
    for v in values {
        *next = (*next).max(v.checked_add(1).ok_or_else(|| bad(CMD, "source identifier limit exceeded"))?);
    }
    Ok(())
}
fn fill(map: &mut BTreeMap<u64, u64>, ids: impl Iterator<Item = u64>, next: &mut u64) -> Result<()> {
    for old in ids {
        if let std::collections::btree_map::Entry::Vacant(e) = map.entry(old) {
            e.insert(allocate(next)?);
        }
    }
    Ok(())
}

#[derive(Default)]
struct References {
    features: BTreeSet<u64>,
    components: BTreeSet<u64>,
    occurrences: BTreeSet<u64>,
    constraints: BTreeSet<u64>,
}
fn references(doc: &Document, refs: &mut References, depth: usize) -> Result<()> {
    if depth > 32 {
        return Err(bad(CMD, "nested source baseline depth exceeded"));
    }
    refs.features.extend(doc.features.iter().map(|f| f.id).chain(doc.links.values().map(|l| l.feature)));
    refs.components.extend(doc.components.iter().map(|c| c.id));
    refs.occurrences.extend(doc.occurrences.iter().map(|o| o.id));
    refs.constraints.extend(doc.assembly.constraints.iter().map(|c| c.id));
    for edit in doc.source_edits.values() {
        refs.features.extend(edit.ids.values().copied().chain(edit.baseline_features.iter().map(|f| f.id)));
        if let Some(g) = &edit.graph {
            refs.components.extend(g.components.values().copied());
            refs.occurrences.extend(g.occurrences.values().copied());
            refs.constraints.extend(g.constraints.values().copied());
            references(&g.baseline, refs, depth + 1)?;
        }
    }
    if refs.features.len() + refs.components.len() + refs.occurrences.len() + refs.constraints.len() > 100000 {
        return Err(bad(CMD, "source identity budget exceeded"));
    }
    Ok(())
}

pub(super) fn begin(s: &mut Session, c: u64, source: &Session, mut edit: SourceEdit) -> Result<()> {
    checked(&source.doc)?;
    let root_path = s.doc.first_path(c);
    let root_occ = *root_path.last().ok_or_else(|| bad(CMD, "linked subassembly has no occurrence"))?;
    let own: BTreeSet<_> = s.doc.occurrences.iter().filter(|o| o.component == c).map(|o| o.id).collect();
    if s.doc.assembly.joints.iter().any(|j| own.contains(&j.a.occurrence) || own.contains(&j.b.occurrence)) {
        return Err(bad(CMD, "subassembly has joint relationships; use Open for this source"));
    }
    let previous = edit.graph.clone();
    if previous.is_none() && s.doc.components.iter().any(|d| d.id != c && s.doc.component_within(d.id, c)) {
        return Err(bad(CMD, "older snapshot-local component trees require separate reference repair"));
    }
    let snapshot_id = s.doc.links.get(&c).ok_or_else(|| bad(CMD, "missing source link"))?.feature;
    let local_features: BTreeSet<_> = if previous.is_none() {
        s.doc.features.iter().filter(|f| f.component == c && f.id != snapshot_id).map(|f| f.id).collect()
    } else {
        BTreeSet::new()
    };
    let mut next = s.doc.next_item_id()?;
    let mut components = previous.as_ref().map(|g| g.components.clone()).unwrap_or_else(|| BTreeMap::from([(0, c)]));
    let mut occurrences = previous.as_ref().map(|g| g.occurrences.clone()).unwrap_or_else(|| BTreeMap::from([(0, root_occ)]));
    let mut constraints = previous.as_ref().map(|g| g.constraints.clone()).unwrap_or_default();
    let mut refs = References::default();
    references(&source.doc, &mut refs, 0)?;
    reserve(&mut next, edit.ids.values().chain(components.values()).chain(occurrences.values()).chain(constraints.values()).copied())?;
    fill(&mut components, refs.components.into_iter(), &mut next)?;
    fill(&mut occurrences, refs.occurrences.into_iter(), &mut next)?;
    fill(&mut edit.ids, refs.features.into_iter(), &mut next)?;
    fill(&mut constraints, refs.constraints.into_iter(), &mut next)?;
    let mut used: BTreeSet<_> = s.doc.features.iter().map(|f| f.name.clone()).collect();
    for f in &source.doc.features {
        edit.names.entry(f.name.clone()).or_insert_with(|| unique(&f.name, &mut used));
    }
    let mut used: BTreeSet<_> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    used.extend(s.doc.reserved_body_names.iter().cloned());
    let mut projected = (*source.doc).clone();
    for (f, r) in projected.features.iter_mut().filter_map(|f| source.model.result(f.id).map(|r| (f, r))) {
        if f.body_names.is_empty() {
            f.body_names = r.state.bodies.iter().filter(|b| b.feature == f.id).map(|b| b.name.clone()).collect();
        }
        for b in &r.state.bodies {
            if !edit.bodies.contains_key(&b.name) {
                edit.bodies.insert(b.name.clone(), unique(&b.name, &mut used));
            }
        }
    }
    for name in &source.doc.reserved_body_names {
        if !edit.bodies.contains_key(name) {
            edit.bodies.insert(name.clone(), unique(name, &mut used));
        }
    }
    let mut used: BTreeSet<_> = s.doc.params.iter().map(|p| p.name.clone()).collect();
    for p in &source.doc.params {
        edit.parameters.entry(p.name.clone()).or_insert_with(|| unique_parameter(&format!("source{c}_{}", p.name), &mut used));
    }
    let mut used: BTreeSet<_> = s.doc.assembly.constraints.iter().map(|x| x.name.clone()).collect();
    let mut constraint_names = previous.as_ref().map(|g| g.constraint_names.clone()).unwrap_or_default();
    for x in &source.doc.assembly.constraints {
        constraint_names.entry(x.name.clone()).or_insert_with(|| unique(&x.name, &mut used));
    }
    let maps = Maps {
        features: edit.ids.clone(),
        names: edit.names.clone(),
        bodies: edit.bodies.clone(),
        params: edit.parameters.clone(),
        components: components.clone(),
        occurrences: occurrences.clone(),
        constraints: constraints.clone(),
        constraint_names,
        prefix: root_path,
        strip: BTreeSet::new(),
    };
    let file = super::file(s, c)?;
    let to = s.path.clone().unwrap_or_else(|| std::path::absolute("Untitled.tciasm").unwrap_or_default().to_string_lossy().into_owned());
    let projected = remap(&projected, &maps, Some(&file.to_string_lossy()), &to, 0)?;
    let projected_properties = super::properties::source(&projected);
    let projected_overrides = projected.appearances.occurrences.clone();
    let source_parameters: BTreeSet<_> = projected.params.iter().map(|p| p.name.clone()).collect();
    let mut scratch = s.scratch();
    scratch.path.clone_from(&s.path);
    let link = s.doc.links.get(&c).ok_or_else(|| bad(CMD, "missing source link"))?;
    let at = s.doc.feature_index(link.feature).ok_or_else(|| bad(CMD, "missing source snapshot"))?;
    scratch.doc_mut().features.splice(at..=at, projected.features);
    scratch.doc_mut().components.extend(projected.components);
    scratch.doc_mut().occurrences.extend(projected.occurrences);
    scratch.doc_mut().params.extend(projected.params);
    scratch.doc_mut().links.extend(projected.links);
    scratch.doc_mut().source_edits.extend(projected.source_edits);
    scratch.doc_mut().parts.extend(projected.parts);
    scratch.doc_mut().body_components.extend(projected.body_components);
    scratch.doc_mut().body_offsets.extend(projected.body_offsets);
    let names: BTreeSet<_> = edit.bodies.values().collect();
    scratch.doc_mut().reserved_body_names.retain(|n| !names.contains(n));
    scratch.doc_mut().reserved_body_names.extend(projected.reserved_body_names);
    scratch.doc_mut().assembly.constraints.extend(projected.assembly.constraints);
    scratch.doc_mut().next_id = next;
    scratch.doc_mut().marker = None;
    super::properties::install(scratch.doc_mut(), c, &edit, &projected_properties)?;
    scratch.doc_mut().appearances.occurrences.extend(projected_overrides);
    scratch.refresh();
    if scratch.model.results.iter().any(|r| r.error.is_some()) {
        return Err(bad(CMD, "linked subassembly history cannot be evaluated in its parent"));
    }
    if !crate::cmd::component_link::same_solids(
        &s.world_state().bodies.iter().collect::<Vec<_>>(),
        &scratch.world_state().bodies.iter().collect::<Vec<_>>(),
    )? {
        return Err(bad(CMD, "linked subassembly changed since insertion; update its link before editing"));
    }
    rebind_external(&mut scratch, s, c, &own)?;
    let baseline_properties = super::properties::scope(&scratch.doc, c, &edit);
    if previous.is_none() {
        super::properties::preserve_legacy(&mut scratch, s, c, &edit, &projected_properties)?;
    } else {
        super::properties::rebind_overrides(&mut scratch, s, c)?;
    }
    edit.graph =
        Some(SourceGraph { components, occurrences, constraints, constraint_names: maps.constraint_names, baseline: Box::new(Document::default()) });
    let mut baseline = scope(&scratch.doc, c, &edit).ok_or_else(|| bad(CMD, "subassembly references leave its source"))?;
    if !local_features.is_empty() {
        baseline.features.retain(|f| !local_features.contains(&f.id));
        baseline.params.retain(|p| source_parameters.contains(&p.name));
    }
    if let Some(g) = &mut edit.graph {
        *g.baseline = baseline;
    }
    edit.baseline_properties = Some(Box::new(baseline_properties));
    scratch.doc_mut().source_edits.insert(c, edit);
    s.accept_computed(scratch);
    Ok(())
}

/// Bind a snapshot's parent relationships to exact descendants before replacing its history.
/// Ambiguous or unmatched geometry is refused transactionally instead of choosing a sibling.
fn rebind_external(next: &mut Session, old: &Session, component: u64, owners: &BTreeSet<u64>) -> Result<()> {
    use solidcraft_doc::constraints::{Geo, PickWhat, geo};
    let before = old.model.state();
    let mut snapshot = (*before).clone();
    snapshot.bodies.retain(|b| old.doc.model_body_component(b) == component);
    let after = next.model.state();
    let placements = next.doc.placements();
    let mut constraints = next.doc.assembly.constraints.clone();
    let mut attempts = 0usize;
    for constraint in &mut constraints {
        for pick in constraint.picks.iter_mut().filter(|p| owners.contains(&p.occurrence) && p.path.is_empty()) {
            let (point, what) = match &pick.geo {
                Geo::Origin { .. } => continue,
                Geo::Face { name, pick } => (solidcraft_doc::naming::point_on_face(&snapshot, name, *pick).map(|x| x.0), PickWhat::Face),
                Geo::Edge { name, pick } => {
                    (snapshot.bodies.iter().find_map(|b| solidcraft_doc::naming::find_edge(b, name, *pick).ok().map(|x| x.0)), PickWhat::Edge)
                }
                Geo::Vertex { pick, .. } => (Some(*pick), PickWhat::Vertex),
            };
            let point = point.ok_or_else(|| bad(CMD, "an existing subassembly relationship has lost its geometry"))?;
            let owner = next.doc.occurrences.iter().find(|o| o.id == pick.occurrence).ok_or_else(|| bad(CMD, "relationship owner is missing"))?;
            let mut prefix = next.doc.first_path(owner.parent);
            prefix.push(owner.id);
            let (_, frame) = next.doc.path_placement(&prefix).ok_or_else(|| bad(CMD, "relationship placement is missing"))?;
            let world = solidcraft_doc::apply_point(&frame, point);
            let mut matches = Vec::new();
            for (_, path, _) in placements.iter().filter(|(c, path, _)| next.doc.component_within(*c, component) && path.starts_with(&prefix)) {
                attempts += 1;
                if attempts > 100_000 {
                    return Err(bad(CMD, "subassembly relationship matching exceeds its processing budget"));
                }
                if let Ok(candidate) = geo::geo_at_path(&next.doc, &after, pick.occurrence, path, what, world) {
                    let (_, _, relative) = geo::geometry_frame(&next.doc, &candidate)?;
                    let inverse = solidcraft_doc::mat_inverse(&relative).ok_or_else(|| bad(CMD, "descendant placement is degenerate"))?;
                    let local = solidcraft_doc::apply_point(&inverse, point);
                    let (leaf, _) = next.doc.path_placement(path).ok_or_else(|| bad(CMD, "descendant is missing"))?;
                    // Picking an edge by proximity alone can select a remote collinear edge.
                    let exact = after.bodies.iter().filter(|b| next.doc.model_body_component(b) == leaf).any(|b| match &candidate.geo {
                        Geo::Face { .. } => solidcraft_doc::appearance::face_index_at(b, local).is_some(),
                        Geo::Edge { name, .. } | Geo::Vertex { edge: name, .. } => {
                            solidcraft_doc::naming::find_edge(b, name, local).is_ok_and(|(p, _)| p.dist(local) < 1e-5)
                        }
                        Geo::Origin { .. } => false,
                    });
                    if exact {
                        matches.push(candidate);
                        if matches.len() > 1 {
                            return Err(bad(CMD, "a constrained subassembly face or edge cannot be uniquely matched to its source placement"));
                        }
                    }
                }
            }
            if matches.len() != 1 {
                return Err(bad(CMD, "a constrained subassembly face or edge cannot be uniquely matched to its source placement"));
            }
            *pick = matches.pop().ok_or_else(|| bad(CMD, "missing relationship match"))?;
        }
    }
    next.doc_mut().assembly.constraints = constraints;
    crate::cmd::joints::resolve(next);
    Ok(())
}

pub(super) fn draft(s: &Session, c: u64) -> Result<(Document, SourceEdit)> {
    let mut edit = s.doc.source_edits.get(&c).cloned().ok_or_else(|| bad(CMD, "missing source edit"))?;
    let mut scoped = scope(&s.doc, c, &edit).ok_or_else(|| bad(CMD, "subassembly references leave its source"))?;
    for feature in &mut scoped.features {
        if feature.body_names.is_empty() {
            feature.body_names = s
                .model
                .result(feature.id)
                .map(|r| r.state.bodies.iter().filter(|b| b.feature == feature.id).map(|b| b.name.clone()).collect())
                .unwrap_or_default();
        }
    }
    let g = edit.graph.as_mut().ok_or_else(|| bad(CMD, "missing source graph"))?;
    let mut next = edit.source.next_id;
    reserve(&mut next, edit.ids.keys().chain(g.components.keys()).chain(g.occurrences.keys()).chain(g.constraints.keys()).copied())?;
    let mut features = inverse(&edit.ids);
    let mut components = inverse(&g.components);
    let mut occurrences = inverse(&g.occurrences);
    let mut constraints = inverse(&g.constraints);
    let mut refs = References::default();
    references(&scoped, &mut refs, 0)?;
    fill(&mut components, refs.components.into_iter(), &mut next)?;
    fill(&mut occurrences, refs.occurrences.into_iter(), &mut next)?;
    fill(&mut features, refs.features.into_iter(), &mut next)?;
    fill(&mut constraints, refs.constraints.into_iter(), &mut next)?;
    let mut names = inverse(&edit.names);
    let mut bodies = inverse(&edit.bodies);
    let mut params = inverse(&edit.parameters);
    let mut constraint_names = inverse(&g.constraint_names);
    let mut used = names.values().cloned().collect();
    for f in &scoped.features {
        names.entry(f.name.clone()).or_insert_with(|| unique(&f.name, &mut used));
    }
    let mut used = bodies.values().cloned().collect();
    let owned = members(&s.doc, c);
    for r in &s.model.results {
        for b in &r.state.bodies {
            if owned.contains(&s.doc.model_body_component(b)) {
                bodies.entry(b.name.clone()).or_insert_with(|| unique(&b.name, &mut used));
            }
        }
    }
    let mut used = params.values().cloned().collect();
    for p in &scoped.params {
        params.entry(p.name.clone()).or_insert_with(|| unique_parameter(&p.name, &mut used));
    }
    let mut used = constraint_names.values().cloned().collect();
    for x in &scoped.assembly.constraints {
        constraint_names.entry(x.name.clone()).or_insert_with(|| unique(&x.name, &mut used));
    }
    let maps = Maps {
        features,
        names,
        bodies,
        params,
        components,
        occurrences,
        constraints,
        constraint_names,
        prefix: Vec::new(),
        strip: s.doc.occurrences.iter().filter(|o| o.component == c).map(|o| o.id).collect(),
    };
    let file = super::file(s, c)?;
    let mapped = remap(&scoped, &maps, s.path.as_deref(), &file.to_string_lossy(), 0)?;
    let mapped_properties = super::properties::source(&mapped);
    let mapped_overrides = mapped.appearances.occurrences.clone();
    let canonical = |p: &std::path::Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if mapped
        .links
        .values()
        .any(|link| canonical(&crate::cmd::component_link::resolve(Some(&file.to_string_lossy()), &link.path)) == canonical(&file))
    {
        return Err(bad(CMD, "cyclic source dependency: a source assembly cannot contain itself"));
    }
    let mut doc = (*edit.source).clone();
    doc.features = mapped.features;
    doc.params = mapped.params;
    doc.components = mapped.components;
    doc.occurrences = mapped.occurrences;
    doc.links = mapped.links;
    doc.source_edits = mapped.source_edits;
    doc.assembly = mapped.assembly;
    doc.parts = mapped.parts;
    doc.body_components = mapped.body_components;
    doc.body_offsets = mapped.body_offsets;
    super::properties::document(&mapped_properties, &mut doc);
    doc.appearances.occurrences = mapped_overrides;
    doc.next_id = next;
    doc.marker = None;
    checked(&doc)?;
    if s.model.results.iter().any(|r| scoped.features.iter().any(|f| f.id == r.id) && r.error.is_some()) {
        return Err(bad(CMD, "subassembly has failed features; its draft is kept"));
    }
    edit.ids = inverse(&maps.features);
    edit.names = inverse(&maps.names);
    edit.bodies = inverse(&maps.bodies);
    edit.parameters = inverse(&maps.params);
    if let Some(g) = &mut edit.graph {
        g.components = inverse(&maps.components);
        g.occurrences = inverse(&maps.occurrences);
        g.constraints = inverse(&maps.constraints);
        g.constraint_names = inverse(&maps.constraint_names);
    }
    Ok((doc, edit))
}

pub(super) fn adopt(s: &mut Session, c: u64, doc: Document, saved: bool) -> Result<()> {
    checked(&doc)?;
    let mut edit = s.doc.source_edits.get(&c).cloned().ok_or_else(|| bad(CMD, "missing source edit"))?;
    let old_baseline = edit.graph.as_ref().map(|g| g.baseline.clone());
    let old_scope = scope(&s.doc, c, &edit).ok_or_else(|| bad(CMD, "subassembly references leave its source"))?;
    let owned = members(&s.doc, c);
    let internal: BTreeSet<_> = old_scope.occurrences.iter().map(|o| o.id).collect();
    let constraint_ids: BTreeSet<_> = old_scope.assembly.constraints.iter().map(|x| x.id).collect();
    let params: BTreeSet<_> = old_scope.params.iter().map(|p| p.name.clone()).collect();
    let source = Session::new(doc.clone());
    let (_, snapshot, faces) = crate::cmd::component_link::session_bodies(&source, "Subassembly source revision")?;
    let mut scratch = s.scratch();
    scratch.path.clone_from(&s.path);
    let at = s.doc.features.iter().position(|f| owned.contains(&f.component)).ok_or_else(|| bad(CMD, "missing subassembly history"))?;
    scratch.doc_mut().features.retain(|f| !owned.contains(&f.component));
    scratch.doc_mut().components.retain(|d| d.id == c || !owned.contains(&d.id));
    scratch.doc_mut().occurrences.retain(|o| !internal.contains(&o.id));
    scratch.doc_mut().links.retain(|d, _| *d == c || !owned.contains(d));
    scratch.doc_mut().source_edits.retain(|d, _| !owned.contains(d));
    scratch.doc_mut().params.retain(|p| !params.contains(&p.name));
    scratch.doc_mut().parts.retain(|d, _| !owned.contains(d));
    scratch.doc_mut().body_components.retain(|_, d| !owned.contains(d));
    scratch.doc_mut().body_offsets.retain(|b, _| !old_scope.body_offsets.contains_key(b));
    scratch.doc_mut().assembly.constraints.retain(|x| !constraint_ids.contains(&x.id));
    scratch.doc_mut().appearances.occurrences.retain(|o| !owned.contains(&o.owner));
    let snapshot_id = scratch.doc_mut().add_feature(snapshot.kind, Some("Subassembly source"))?;
    let index = scratch.doc.feature_index(snapshot_id).ok_or_else(|| bad(CMD, "missing subassembly snapshot"))?;
    let mut snapshot_feature = scratch.doc_mut().features.remove(index);
    snapshot_feature.id = s.doc.links.get(&c).ok_or_else(|| bad(CMD, "missing source link"))?.feature;
    snapshot_feature.component = c;
    snapshot_feature.body_names = snapshot.body_names;
    snapshot_feature.link_faces = faces;
    scratch.doc_mut().features.insert(at.min(index), snapshot_feature);
    scratch.doc_mut().marker = None;
    scratch.refresh();
    if saved {
        edit.source = Box::new(doc);
    }
    begin(&mut scratch, c, &source, edit)?;
    if !saved
        && let Some(baseline) = old_baseline
        && let Some(edit) = scratch.doc_mut().source_edits.get_mut(&c)
        && let Some(graph) = &mut edit.graph
    {
        graph.baseline = baseline;
    }
    s.accept_computed(scratch);
    Ok(())
}
