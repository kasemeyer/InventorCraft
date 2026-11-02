//! Explicit placement overrides. Stored against stable definition names and full paths.
use crate::params::{bad, string_list};
use crate::{Result, Session};
use serde_json::{Value, json};
use solidcraft_doc::appearance::{FaceLook, Look, OccurrenceAppearance};

const CMD: &str = "appearance.assign";

pub(super) fn assign(s: &mut Session, p: &Value, look: Option<Look>) -> Result<Value> {
    let world = s.world_state();
    let mut targets: Vec<(Vec<u64>, Option<String>, Option<FaceLook>)> = Vec::new();
    if let Some(paths) = p.get("paths") {
        let paths = paths.as_array().filter(|a| a.len() <= 10000).ok_or_else(|| bad(CMD, "paths must be a list of occurrence paths"))?;
        for path in paths {
            let ids = path
                .as_array()
                .filter(|a| !a.is_empty() && a.len() <= solidcraft_doc::MAX_PLACEMENT_DEPTH + 1)
                .ok_or_else(|| bad(CMD, "invalid occurrence path"))?;
            let path: Vec<_> =
                ids.iter().map(|id| id.as_u64().filter(|id| *id != 0).ok_or_else(|| bad(CMD, "invalid occurrence id"))).collect::<Result<_>>()?;
            if s.doc.path_placement(&path).is_none() {
                return Err(bad(CMD, "occurrence path no longer exists"));
            }
            targets.push((path, None, None));
        }
    }
    for name in string_list(p, "bodies") {
        let body = world
            .body(&name)
            .filter(|b| !b.occurrence_path.is_empty())
            .ok_or_else(|| bad(CMD, "occurrence body is missing; use definition scope for root bodies"))?;
        targets.push((body.occurrence_path.clone(), Some(body.definition_name().to_string()), None));
    }
    if let Some(faces) = p.get("faces") {
        let faces = faces.as_array().filter(|a| a.len() <= 10000).ok_or_else(|| bad(CMD, "faces must be a list"))?;
        for face in faces {
            let point = crate::params::vec3(face)
                .or_else(|| face.get("point").and_then(crate::params::vec3))
                .ok_or_else(|| bad(CMD, "face needs a finite point"))?;
            let given = face.get("body").and_then(Value::as_str).or_else(|| p.get("body").and_then(Value::as_str));
            let body = if let Some(name) = given {
                world.body(name)
            } else {
                world
                    .bodies
                    .iter()
                    .filter(|b| solidcraft_doc::appearance::face_index_at(b, point).is_some())
                    .min_by(|a, b| super::dist_to(a, point).total_cmp(&super::dist_to(b, point)))
            }
            .filter(|b| !b.occurrence_path.is_empty())
            .ok_or_else(|| bad(CMD, "face needs an existing occurrence body"))?;
            let index = solidcraft_doc::appearance::face_index_at(body, point).ok_or_else(|| bad(CMD, "no face at point"))?;
            let name = solidcraft_doc::naming::face_names(body).get(index).cloned();
            let inverse = solidcraft_doc::mat_inverse(&body.source_transform).ok_or_else(|| bad(CMD, "invalid body placement"))?;
            let point = solidcraft_doc::apply_point(&inverse, point);
            targets.push((
                body.occurrence_path.clone(),
                Some(body.definition_name().to_string()),
                Some(FaceLook {
                    body: body.definition_name().to_string(),
                    point,
                    look: look.clone().unwrap_or_else(|| Look::custom([0, 0, 0], 1.0)),
                    name,
                    strict_name: false,
                    certificate: None,
                }),
            ));
        }
    }
    if targets.is_empty() || targets.len() > 10000 {
        return Err(bad(CMD, "give occurrence paths, bodies or faces (at most 10000)"));
    }
    let owner = s.active_component;
    let author = s.active_occurrence_path.clone().unwrap_or_else(|| s.doc.first_path(owner));
    if owner != 0 && targets.iter().any(|(path, ..)| !path.starts_with(&author) || path.len() <= author.len()) {
        return Err(bad(CMD, "occurrence overrides must be inside the assembly being edited"));
    }
    if owner != 0 {
        let prefix = s.doc.first_path(owner);
        for (path, ..) in &mut targets {
            *path = prefix.iter().copied().chain(path.iter().skip(author.len()).copied()).collect();
        }
    }
    let mut overrides = s.doc.appearances.occurrences.clone();
    for (path, body, face) in &targets {
        let index = overrides.iter().position(|o| o.path == *path && o.owner == owner);
        let index = match index {
            Some(index) => index,
            None => {
                if overrides.len() >= 100000 {
                    return Err(bad(CMD, "too many occurrence appearances"));
                }
                overrides.push(OccurrenceAppearance { path: path.clone(), owner, ..Default::default() });
                overrides.len() - 1
            }
        };
        let Some(target) = overrides.get_mut(index) else {
            return Err(bad(CMD, "missing appearance target"));
        };
        if let Some(face) = face {
            target.faces.retain(|old| {
                !(old.body == face.body
                    && (old.name.as_ref().zip(face.name.as_ref()).is_some_and(|(a, b)| a == b) || old.point.dist(face.point) < 1e-9))
            });
            if look.is_some() {
                if target.faces.len() >= 100000 {
                    return Err(bad(CMD, "too many occurrence face appearances"));
                }
                target.faces.push(face.clone());
            }
        } else if let Some(body) = body {
            if let Some(look) = &look {
                target.bodies.insert(body.clone(), look.clone());
            } else {
                target.bodies.remove(body);
            }
        } else {
            target.look = look.clone();
        }
    }
    overrides.retain(|o| o.look.is_some() || !o.bodies.is_empty() || !o.faces.is_empty());
    if overrides != s.doc.appearances.occurrences {
        s.doc_mut().appearances.occurrences = overrides;
    }
    Ok(
        json!({"scope":"occurrence", "paths":targets.iter().map(|(path, ..)| path).collect::<Vec<_>>(), "appearance":look.as_ref().map(super::look_json)}),
    )
}
