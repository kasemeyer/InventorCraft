//! Definition-owned property projection. Placement overrides never enter a source draft.
use solidcraft_doc::{Document, SourceEdit, SourceProperties};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn scope(doc: &Document, c: u64, edit: &SourceEdit) -> SourceProperties {
    let owned = |id| doc.component_within(id, c);
    let names: BTreeSet<_> = edit
        .bodies
        .values()
        .cloned()
        .chain(doc.features.iter().filter(|f| owned(f.component)).flat_map(|f| f.body_names.iter().cloned()))
        .chain(doc.body_components.iter().filter(|(_, id)| owned(**id)).map(|(b, _)| b.clone()))
        .collect();
    let component = doc.components.iter().find(|d| d.id == c);
    let materials: BTreeMap<_, _> = doc.materials.iter().filter(|(b, _)| names.contains(*b)).map(|(b, m)| (b.clone(), m.clone())).collect();
    let mut user_materials: Vec<_> = doc
        .user_materials
        .iter()
        .filter(|m| {
            materials.values().any(|n| n.eq_ignore_ascii_case(&m.name))
                || edit.source.user_materials.iter().any(|old| old.name.eq_ignore_ascii_case(&m.name))
        })
        .cloned()
        .collect();
    user_materials.sort_by(|a, b| a.name.cmp(&b.name));
    SourceProperties {
        metadata: component.map_or_else(|| doc.metadata.clone(), |d| d.metadata.clone()),
        properties: component.map_or_else(|| doc.properties.clone(), |d| d.properties.clone()),
        user_materials,
        materials,
        appearances: solidcraft_doc::appearance::Appearances {
            bodies: doc.appearances.bodies.iter().filter(|(b, _)| names.contains(*b)).map(|(b, l)| (b.clone(), l.clone())).collect(),
            faces: doc.appearances.faces.iter().filter(|f| names.contains(&f.body)).cloned().collect(),
            components: doc.appearances.components.iter().filter(|(id, _)| owned(**id)).map(|(id, l)| (*id, l.clone())).collect(),
            ..Default::default()
        },
    }
}

pub(super) fn remap(
    p: &SourceProperties,
    bodies: &BTreeMap<String, String>,
    ids: &BTreeMap<u64, u64>,
    components: &BTreeMap<u64, u64>,
) -> SourceProperties {
    let body = |name: &String| bodies.get(name).cloned().unwrap_or_else(|| name.clone());
    let mut out = p.clone();
    out.materials = p.materials.iter().map(|(b, m)| (body(b), m.clone())).collect();
    out.appearances.bodies = p.appearances.bodies.iter().map(|(b, l)| (body(b), l.clone())).collect();
    for f in &mut out.appearances.faces {
        f.body = body(&f.body);
        if let Some(name) = &mut f.name {
            *name = solidcraft_doc::remap_topology(name, ids, bodies);
        }
        if let Some(certificate) = &mut f.certificate {
            certificate.remap_features(ids);
        }
    }
    out.appearances.components = p.appearances.components.iter().filter_map(|(id, l)| components.get(id).map(|id| (*id, l.clone()))).collect();
    out
}

pub(super) fn document(p: &SourceProperties, doc: &mut Document) {
    doc.metadata = p.metadata.clone();
    doc.properties = p.properties.clone();
    doc.materials = p.materials.clone();
    for material in &p.user_materials {
        if let Some(existing) = doc.user_materials.iter_mut().find(|old| old.name.eq_ignore_ascii_case(&material.name)) {
            *existing = material.clone();
        } else {
            doc.user_materials.push(material.clone());
        }
    }
    // The source's own occurrence overrides are handled by the assembly graph, never by
    // the containing assembly's property snapshot.
    let occurrences = std::mem::take(&mut doc.appearances.occurrences);
    doc.appearances = p.appearances.clone();
    doc.appearances.occurrences = occurrences;
}

pub(super) fn install(doc: &mut Document, c: u64, edit: &SourceEdit, p: &SourceProperties) -> crate::Result<()> {
    let previous = scope(doc, c, edit);
    for material in &p.user_materials {
        if let Some(index) = doc.user_materials.iter().position(|m| m.name.eq_ignore_ascii_case(&material.name)) {
            let Some(existing) = doc.user_materials.get(index) else {
                return Err(super::bad(super::CMD, "missing material identity"));
            };
            if existing != material {
                let baseline_owned = edit.baseline_properties.as_ref().is_some_and(|p| p.user_materials.contains(existing))
                    || edit.source.user_materials.contains(existing);
                let external_use =
                    doc.materials.iter().any(|(body, name)| name.eq_ignore_ascii_case(&material.name) && !previous.materials.contains_key(body));
                let external_source = doc.source_edits.iter().any(|(id, other)| {
                    !doc.component_within(*id, c)
                        && !doc.component_within(c, *id)
                        && other.source.user_materials.iter().any(|m| m.name.eq_ignore_ascii_case(&material.name))
                });
                if baseline_owned && !external_use && !external_source {
                    if let Some(existing) = doc.user_materials.get_mut(index) {
                        *existing = material.clone();
                    }
                    continue;
                }
                return Err(super::bad(
                    super::CMD,
                    format!("source material `{}` conflicts with an assembly material; rename it before editing", material.name),
                ));
            }
        } else {
            doc.user_materials.push(material.clone());
        }
    }
    for body in previous.materials.keys() {
        doc.materials.remove(body);
    }
    for body in previous.appearances.bodies.keys() {
        doc.appearances.bodies.remove(body);
    }
    let faces: BTreeSet<_> = previous.appearances.faces.iter().map(|f| f.body.clone()).collect();
    doc.appearances.faces.retain(|f| !faces.contains(&f.body));
    for component in previous.appearances.components.keys() {
        doc.appearances.components.remove(component);
    }
    if let Some(component) = doc.components.iter_mut().find(|d| d.id == c) {
        component.metadata = p.metadata.clone();
        component.properties = p.properties.clone();
    }
    doc.materials.extend(p.materials.clone());
    doc.appearances.bodies.extend(p.appearances.bodies.clone());
    doc.appearances.faces.extend(p.appearances.faces.clone());
    doc.appearances.components.extend(p.appearances.components.clone());
    Ok(())
}

pub(super) fn source(doc: &Document) -> SourceProperties {
    SourceProperties {
        metadata: doc.metadata.clone(),
        properties: doc.properties.clone(),
        materials: doc.materials.clone(),
        user_materials: doc.user_materials.clone(),
        appearances: solidcraft_doc::appearance::Appearances { occurrences: Vec::new(), ..doc.appearances.clone() },
    }
}

/// Keep containing-assembly paint attached when a flattened snapshot becomes a graph.
pub(super) fn rebind_overrides(next: &mut crate::Session, old: &crate::Session, c: u64) -> crate::Result<()> {
    let previous = old.world_state();
    let world = next.world_state();
    let mut overrides = next.doc.appearances.occurrences.clone();
    let mut budget = 100000usize;
    for original in &old.doc.appearances.occurrences {
        if old.doc.path_placement(&original.path).is_none_or(|(component, _)| component != c) {
            continue;
        }
        let mut converted = Vec::new();
        let names: BTreeSet<_> = original.bodies.keys().cloned().chain(original.faces.iter().map(|f| f.body.clone())).collect();
        for name in names {
            let before = previous
                .bodies
                .iter()
                .find(|b| b.occurrence_path == original.path && b.definition_name() == name)
                .ok_or_else(|| super::bad(super::CMD, "an occurrence appearance lost its source body"))?;
            let exact = world.bodies.iter().find(|b| b.occurrence_path == original.path && b.definition_name() == name);
            let after = if let Some(exact) = exact {
                exact
            } else {
                let mut matches = Vec::new();
                for candidate in world.bodies.iter().filter(|b| b.occurrence_path.starts_with(&original.path)) {
                    budget = budget.checked_sub(1).ok_or_else(|| super::bad(super::CMD, "appearance matching exceeds edit limit"))?;
                    if crate::cmd::component_link::same_solids(&[before], &[candidate])? {
                        matches.push(candidate);
                    }
                    if matches.len() > 1 {
                        break;
                    }
                }
                if matches.len() != 1 {
                    return Err(super::bad(super::CMD, "an occurrence appearance cannot be uniquely matched to nested source geometry"));
                }
                *matches.first().ok_or_else(|| super::bad(super::CMD, "missing appearance match"))?
            };
            let mut paint =
                solidcraft_doc::appearance::OccurrenceAppearance { path: after.occurrence_path.clone(), owner: original.owner, ..Default::default() };
            if let Some(look) = original.bodies.get(&name) {
                paint.bodies.insert(after.definition_name().to_string(), look.clone());
            }
            for face in original.faces.iter().filter(|f| f.body == name) {
                let world_point = solidcraft_doc::apply_point(&before.source_transform, face.point);
                let inverse =
                    solidcraft_doc::mat_inverse(&after.source_transform).ok_or_else(|| super::bad(super::CMD, "invalid appearance placement"))?;
                let index = solidcraft_doc::appearance::face_index_at(after, world_point)
                    .ok_or_else(|| super::bad(super::CMD, "an occurrence face appearance lost its face"))?;
                let mut face = face.clone();
                face.body = after.definition_name().to_string();
                face.point = solidcraft_doc::apply_point(&inverse, world_point);
                face.name = solidcraft_doc::naming::face_names(after).get(index).cloned();
                if face.strict_name {
                    return Err(super::bad(super::CMD, "strict occurrence paint requires explicit reference repair"));
                }
                paint.faces.push(face);
            }
            converted.push(paint);
        }
        if names_empty(original) {
            continue;
        }
        if let Some(existing) = overrides.iter_mut().find(|o| o.path == original.path && o.owner == original.owner) {
            existing.bodies.clear();
            existing.faces.clear();
        }
        for paint in converted {
            if let Some(existing) = overrides.iter_mut().find(|o| o.path == paint.path && o.owner == paint.owner) {
                existing.bodies.extend(paint.bodies);
                existing.faces.extend(paint.faces);
            } else {
                overrides.push(paint);
            }
        }
    }
    overrides.retain(|o| o.look.is_some() || !o.bodies.is_empty() || !o.faces.is_empty());
    next.doc_mut().appearances.occurrences = overrides;
    Ok(())
}

fn names_empty(o: &solidcraft_doc::appearance::OccurrenceAppearance) -> bool {
    o.bodies.is_empty() && o.faces.is_empty()
}

/// Older files stored assembly paint directly on linked definitions. Keep its visible result
/// as containing-document overrides rather than exporting that paint into the source file.
pub(super) fn preserve_legacy(
    next: &mut crate::Session,
    old: &crate::Session,
    c: u64,
    edit: &SourceEdit,
    projected: &SourceProperties,
) -> crate::Result<()> {
    let previous = scope(&old.doc, c, edit);
    if let Some(component) = next.doc_mut().components.iter_mut().find(|d| d.id == c) {
        component.metadata.0.extend(previous.metadata.0.clone());
        component.properties.extend(previous.properties.clone());
    }
    next.doc_mut().materials.extend(previous.materials.clone());
    let changed_component = previous.appearances.components.get(&c).filter(|look| projected.appearances.components.get(&c) != Some(*look));
    let changed_bodies: BTreeMap<_, _> = previous
        .appearances
        .bodies
        .iter()
        .filter(|(b, look)| projected.appearances.bodies.get(*b) != Some(*look))
        .map(|(b, l)| (b.clone(), l.clone()))
        .collect();
    let changed_faces = previous.appearances.faces.iter().any(|f| !projected.appearances.faces.contains(f));
    if changed_component.is_none() && changed_bodies.is_empty() && !changed_faces {
        return rebind_overrides(next, old, c);
    }
    let owner = old
        .doc
        .source_edits
        .iter()
        .filter(|(id, e)| **id != c && e.graph.is_some() && old.doc.component_within(c, **id))
        .max_by_key(|(id, _)| old.doc.first_path(**id).len())
        .map_or(0, |(id, _)| *id);
    let world = old.world_state();
    let mut reference = old.scratch();
    for (_, path, _) in old.doc.placements().into_iter().filter(|(component, ..)| *component == c) {
        let mut paint = solidcraft_doc::appearance::OccurrenceAppearance {
            path: path.clone(),
            owner,
            look: changed_component.cloned(),
            bodies: changed_bodies.clone(),
            ..Default::default()
        };
        for body in world.bodies.iter().filter(|b| b.occurrence_path == path) {
            if changed_component.is_some()
                && let Some(look) = old.doc.model_body_look(body)
            {
                paint.bodies.insert(body.definition_name().to_string(), look);
            }
            let inverse =
                solidcraft_doc::mat_inverse(&body.source_transform).ok_or_else(|| super::bad(super::CMD, "invalid legacy appearance placement"))?;
            let names = solidcraft_doc::naming::face_names(body);
            let mesh = body.mesh();
            for (index, look) in old.doc.face_colors(body) {
                let point = mesh
                    .triangles
                    .iter()
                    .zip(&mesh.tri_face)
                    .find_map(|(tri, face)| {
                        if *face as usize != index {
                            return None;
                        }
                        let [a, b, c] = mesh.tri(tri)?;
                        Some((a + b + c) / 3.0)
                    })
                    .ok_or_else(|| super::bad(super::CMD, "legacy face paint lost its face"))?;
                paint.faces.push(solidcraft_doc::appearance::FaceLook {
                    body: body.definition_name().to_string(),
                    point: solidcraft_doc::apply_point(&inverse, point),
                    look,
                    name: names.get(index).cloned(),
                    strict_name: false,
                    certificate: None,
                });
            }
        }
        if let Some(existing) = reference.doc_mut().appearances.occurrences.iter_mut().find(|o| o.owner == owner && o.path == path) {
            if existing.look.is_none() {
                existing.look = paint.look;
            }
            for (b, l) in paint.bodies {
                existing.bodies.entry(b).or_insert(l);
            }
            existing.faces.extend(paint.faces);
        } else {
            reference.doc_mut().appearances.occurrences.push(paint);
        }
    }
    for paint in &reference.doc.appearances.occurrences {
        if let Some(existing) = next.doc_mut().appearances.occurrences.iter_mut().find(|o| o.path == paint.path && o.owner == paint.owner) {
            *existing = paint.clone();
        } else {
            next.doc_mut().appearances.occurrences.push(paint.clone());
        }
    }
    rebind_overrides(next, &reference, c)
}
