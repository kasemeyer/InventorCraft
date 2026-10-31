//! Copy, Cut and Paste from the keyboard (Ctrl+C / Ctrl+X / Ctrl+V, ⌘C / ⌘X / ⌘V) and the context
//! menu (#1480).
//!
//! In a sketch they copy the selected sketch geometry (`sketch.copy`) and paste it at the cursor
//! (`sketch.paste` with `at`); Cut copies the sketch geometry and deletes it (`sketch.cut`), or offset from the original when the cursor is not over the
//! viewport. Outside a sketch they copy the selected timeline features and paste them at the
//! history marker (`timeline.copy` / `timeline.paste`).
//!
//! The window system turns Ctrl+C / Ctrl+V into clipboard events, not key presses, and only sends
//! the paste event while the system clipboard holds text. So a copy also puts a short line of
//! text on the system clipboard (a menu's Copy leaves it for the next frame, which has the
//! context to write it).

use std::sync::Mutex;

use egui::Pos2;
use serde_json::{Value, json};
use solidcraft_engine::Sel;

use crate::SolveApp;

/// Text for the system clipboard, written on the next frame.
static PENDING_TEXT: Mutex<Option<String>> = Mutex::new(None);

fn leave_text(text: String) {
    if let Ok(mut pending) = PENDING_TEXT.lock() {
        *pending = Some(text);
    }
}

/// The selected sketch geometry of the active sketch.
fn sketch_selection(app: &SolveApp) -> Vec<String> {
    app.session
        .selection
        .iter()
        .filter_map(|s| match s {
            Sel::SketchCurve { id } | Sel::SketchPoint { id } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// Where an occurrence paste with no pointer over the view goes: this far from its source.
pub(crate) const PASTE_OFFSET: [f64; 3] = [10.0, 10.0, 0.0];

/// A placed component on the clipboard (R186): its source occurrence, where that sat in its
/// parent, and the design file it links to (a paste into another assembly inserts that file).
#[derive(Clone, Debug, PartialEq)]
pub struct CopiedOccurrence {
    pub occurrence: u64,
    pub component: u64,
    origin: [f64; 3],
    file: Option<String>,
}

/// The placements a selection resolves to (R186): every item an occurrence's Browser row or
/// geometry (body, face, edge, vertex) of a placed component's body. None when anything else
/// is selected (a root body keeps SolidCraft's body paste; a feature or sketch geometry keeps
/// its own copy) or nothing is.
pub(crate) fn selected_occurrences(app: &SolveApp) -> Option<Vec<u64>> {
    if app.session.active_sketch.is_some() {
        return None;
    }
    let mut out: Vec<u64> = Vec::new();
    // A component row picked in SolidCraft's Browser: its first placement.
    if app.session.selection.is_empty() && !app.tree.picked_components.contains(&0) {
        out.extend(app.tree.picked_components.iter().filter_map(|c| app.session.doc.occurrence_of(*c).map(|o| o.id)));
        return (!out.is_empty()).then_some(out);
    }
    for item in &app.session.selection {
        let occurrence = match item {
            Sel::Occurrence { path } => path.last().copied(),
            Sel::Body { name: body } | Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } => {
                crate::dialogs_assembly::occurrence_of_body(&app.session, body)
            }
            _ => None,
        };
        let occurrence = occurrence.filter(|id| *id != 0 && app.session.doc.occurrences.iter().any(|o| o.id == *id))?;
        if !out.contains(&occurrence) {
            out.push(occurrence);
        }
    }
    (!out.is_empty()).then_some(out)
}

/// The design file a linked component's link names, resolved against the assembly's folder.
fn linked_file(app: &SolveApp, component: u64) -> Option<String> {
    let link = app.session.doc.links.get(&component)?;
    let stored = std::path::Path::new(&link.path);
    let file = match app.session.path.as_deref().and_then(|p| std::path::Path::new(p).parent()) {
        Some(dir) if stored.is_relative() => dir.join(stored),
        _ => stored.to_path_buf(),
    };
    Some(file.to_string_lossy().into_owned())
}

fn occurrence_names(app: &SolveApp, ids: impl IntoIterator<Item = u64>) -> String {
    let names: Vec<String> =
        ids.into_iter().filter_map(|id| app.session.doc.occurrences.iter().find(|o| o.id == id).map(|o| o.name.clone())).collect();
    names.join(", ")
}

/// Copy the selected placements (R186).
fn copy_occurrences(app: &mut SolveApp, occurrences: &[u64]) -> bool {
    let copied: Vec<CopiedOccurrence> = occurrences
        .iter()
        .filter_map(|id| app.session.doc.occurrences.iter().find(|o| o.id == *id))
        .map(|o| CopiedOccurrence {
            occurrence: o.id,
            component: o.component,
            origin: [o.transform[3][0], o.transform[3][1], o.transform[3][2]],
            file: linked_file(app, o.component),
        })
        .collect();
    let names = occurrence_names(app, copied.iter().map(|c| c.occurrence));
    app.occurrence_clipboard = copied;
    app.linked_design_clipboard = None;
    app.set_status(format!("Copied {names}"), false);
    leave_text(format!("SolidCraft components: {names}"));
    true
}

/// Paste the copied placements as new occurrences in the active component (R186): at the
/// pointer when it is over the view (the first copy's origin there, the rest keeping their
/// offsets from it), else `PASTE_OFFSET` from their sources. A linked component goes in through
/// `component.insert_design {reuse, at}` (first-paste grounding, R123; any open assembly), one
/// that is not through `occurrence.copy` (its own assembly only). The pastes end up selected.
fn paste_occurrences(app: &mut SolveApp, at: Option<Pos2>) -> bool {
    let copied = app.occurrence_clipboard.clone();
    let Some(first) = copied.first() else { return false };
    let over_view = at.filter(|p| app.viewport.rect.is_some_and(|r| r.contains(*p)) && !crate::shell::covered(app, *p));
    let (parent_path, delta) = match over_view {
        Some(_) => {
            let (path, target) = paste_placement(app, over_view);
            (path, [target[0] - first.origin[0], target[1] - first.origin[1], target[2] - first.origin[2]])
        }
        None => (paste_placement(app, None).0, PASTE_OFFSET),
    };
    let mut made = Vec::new();
    for c in &copied {
        let target = [c.origin[0] + delta[0], c.origin[1] + delta[1], c.origin[2] + delta[2]];
        let here = app.session.doc.occurrences.iter().any(|o| o.id == c.occurrence && o.component == c.component);
        let out = match &c.file {
            Some(file) => app.run("component.insert_design", json!({"path": file, "reuse": true, "at": target})),
            None if here => {
                let parent = app.session.active_component;
                app.run("occurrence.copy", json!({"occurrence": c.occurrence, "parent": parent, "translate": delta}))
            }
            None => {
                app.set_status("Paste: that component belongs to another assembly; save it to a file to paste it here", true);
                return false;
            }
        };
        let Ok(out) = out else { return false };
        made.extend(out.get("occurrence").and_then(Value::as_u64));
    }
    app.session.selection = made
        .iter()
        .map(|id| {
            let mut path = parent_path.clone();
            path.push(*id);
            Sel::Occurrence { path }
        })
        .collect();
    let names = occurrence_names(app, made.iter().copied());
    app.set_status(format!("Pasted {names}"), false);
    true
}

/// Is there something Paste would paste here?
pub fn can_paste(app: &SolveApp) -> bool {
    if app.session.active_sketch.is_some() {
        app.session.sketch_clipboard.is_some()
    } else {
        !app.occurrence_clipboard.is_empty() || app.linked_design_clipboard.is_some() || !app.session.clipboard.is_empty()
    }
}

/// Copy the saved root design. The path stays in this app, outside per-document sessions.
pub fn copy_design(app: &mut SolveApp) -> bool {
    let Some(path) = app.session.path.clone().filter(|_| !app.session.is_dirty()) else {
        app.set_status("Save this design before copying it as a linked component", true);
        return false;
    };
    app.linked_design_clipboard = Some(path);
    app.occurrence_clipboard.clear();
    leave_text(format!("SolidCraft linked design: {}", app.session.doc.name));
    app.set_status("Copied design: switch to the assembly and paste to insert a linked component", false);
    true
}

/// Copy the selection: sketch geometry in a sketch, otherwise timeline features.
pub fn copy(app: &mut SolveApp) -> bool {
    if app.session.active_sketch.is_none() && app.tree.picked_components == [0] {
        return copy_design(app);
    }
    // A placed component pastes as a new occurrence of it, never a body at the root (R186).
    if let Some(occurrences) = selected_occurrences(app) {
        return copy_occurrences(app, &occurrences);
    }
    if app.session.active_sketch.is_some() {
        let entities = sketch_selection(app);
        if entities.is_empty() {
            app.set_status("Select sketch geometry to copy", true);
            return false;
        }
        let Ok(out) = app.run("sketch.copy", json!({ "entities": entities })) else { return false };
        app.linked_design_clipboard = None;
        app.occurrence_clipboard.clear();
        let n = out.get("curves").and_then(Value::as_u64).unwrap_or(0) + out.get("points").and_then(Value::as_u64).unwrap_or(0);
        let k = out.get("constraints").and_then(Value::as_u64).unwrap_or(0);
        let text = format!("SolidCraft sketch geometry: {n} entities, {k} constraints and dimensions");
        app.set_status(format!("Copied {n} sketch entities with {k} constraints and dimensions"), false);
        leave_text(text);
        return true;
    }
    let features = crate::timeline::selected_feature_ids(app);
    if features.is_empty() {
        app.set_status("Select sketch geometry or timeline features to copy", true);
        return false;
    }
    let Ok(out) = app.run("timeline.copy", json!({ "features": features })) else { return false };
    app.linked_design_clipboard = None;
    app.occurrence_clipboard.clear();
    let names: Vec<&str> = out.get("copied").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    app.set_status(format!("Copied {}", names.join(", ")), false);
    leave_text(format!("SolidCraft features: {}", names.join(", ")));
    true
}

/// Cut the selected sketch geometry: copy it, then delete it (one undo step).
pub fn cut(app: &mut SolveApp) -> bool {
    if app.session.active_sketch.is_none() {
        app.set_status("Cut works on sketch geometry: edit a sketch and select some", true);
        return false;
    }
    let entities = sketch_selection(app);
    if entities.is_empty() {
        app.set_status("Select sketch geometry to cut", true);
        return false;
    }
    let Ok(out) = app.run("sketch.cut", json!({ "entities": entities })) else { return false };
    app.linked_design_clipboard = None;
    let n = out.get("curves").and_then(Value::as_u64).unwrap_or(0) + out.get("points").and_then(Value::as_u64).unwrap_or(0);
    app.set_status(format!("Cut {n} sketch entities"), false);
    leave_text(format!("SolidCraft sketch geometry: {n} entities"));
    true
}

/// The point of the active sketch under `at` (screen), when `at` is over the viewport.
fn sketch_point(app: &SolveApp, at: Option<Pos2>) -> Option<[f64; 2]> {
    let at = at?;
    let rect = app.viewport.rect?;
    // Under a card or panel floating over the viewport is not over the sketch.
    if !rect.contains(at) || crate::shell::covered(app, at) {
        return None;
    }
    let proj = crate::viewport::projection(app, rect);
    let (p, _) = crate::viewport::sketch_point_at(app, &proj, at)?;
    Some([p.x, p.y])
}

/// Paste: in a sketch, the copied sketch geometry centred at `at` (or offset from the original
/// when `at` is not over the sketch); otherwise the copied features at the history marker.
pub fn paste(app: &mut SolveApp, at: Option<Pos2>) -> bool {
    if app.session.active_sketch.is_some() {
        if app.session.sketch_clipboard.is_none() {
            app.set_status("Nothing to paste: copy sketch geometry first", true);
            return false;
        }
        let params = match sketch_point(app, at) {
            Some(p) => json!({ "at": p }),
            None => json!({}),
        };
        let Ok(out) = app.run("sketch.paste", params) else { return false };
        let n = out.get("curves").and_then(Value::as_array).map_or(0, Vec::len) + out.get("points").and_then(Value::as_array).map_or(0, Vec::len);
        app.set_status(format!("Pasted {n} sketch entities (selected: drag them, or press M to move them)"), false);
        return true;
    }
    if !app.occurrence_clipboard.is_empty() {
        return paste_occurrences(app, at);
    }
    if let Some(path) = app.linked_design_clipboard.clone() {
        let (parent_path, translate) = paste_placement(app, at);
        let Ok(out) = app.run("component.insert_design", json!({"path": path, "reuse": true, "translate": translate})) else { return false };
        // Into an empty assembly the paste is grounded, like a Place (R123): nothing to move.
        let grounded = |id: u64| app.session.doc.occurrences.iter().any(|o| o.id == id && o.grounded);
        if let Some(id) = out.get("occurrence").and_then(Value::as_u64).filter(|id| !grounded(*id)) {
            let mut movement = crate::browser::OccMove::new(id);
            movement.path = parent_path;
            movement.path.push(id);
            movement.baseline = app.session.doc.occurrences.iter().find(|o| o.id == id).map(|o| o.transform);
            app.tree.occurrence_move = Some(movement);
        }
        if app.tree.occurrence_move.is_some() {
            app.set_status("Pasted linked component: use Move/Copy to position it", false);
        } else {
            app.set_status("Pasted linked component, grounded as the assembly's first part", false);
        }
        return true;
    }
    if app.session.clipboard.is_empty() {
        app.set_status("Nothing to paste: copy timeline features first", true);
        return false;
    }
    app.run("timeline.paste", json!({})).is_ok()
}

/// Place the copied design's origin on the pointed solid surface, otherwise its parent's
/// XY plane. With no viewport point or a parallel/behind-camera ray, use the parent origin.
pub(crate) fn paste_placement(app: &SolveApp, at: Option<Pos2>) -> (Vec<u64>, [f64; 3]) {
    use solidcraft_engine::{doc, geom::Vec3};
    let path = app.session.active_occurrence_path.clone().unwrap_or_else(|| app.session.doc.first_path(app.session.active_component));
    let frame = app.session.occurrence_placement(&path).map(|p| p.world).unwrap_or(doc::IDENTITY);
    let origin = doc::apply_point(&frame, Vec3::ZERO);
    let at = at.zip(app.viewport.rect).filter(|(point, rect)| rect.contains(*point));
    let point = at
        .and_then(|(at, rect)| {
            let projection = crate::viewport::projection(app, rect);
            if let Some(point) = crate::viewport::pick_all(app, &projection, at).into_iter().find_map(|hit| match hit {
                crate::viewport::Hit::Face { point, .. } => Some(point),
                _ => None,
            }) {
                return point.is_finite().then_some(point);
            }
            let (ray_origin, ray_direction) = projection.ray(at);
            let normal = doc::apply_vector(&frame, Vec3::Z);
            let denominator = ray_direction.dot(normal);
            if denominator.abs() < 1e-9 {
                return None;
            }
            let distance = (origin - ray_origin).dot(normal) / denominator;
            let point = ray_origin + ray_direction * distance;
            (distance >= 0.0 && point.is_finite()).then_some(point)
        })
        .unwrap_or(origin);
    let local = doc::mat_inverse(&frame).map(|inverse| doc::apply_point(&inverse, point)).filter(|p| p.is_finite()).unwrap_or(Vec3::ZERO);
    (path, [local.x, local.y, local.z])
}

/// The frame's Copy, Cut and Paste events (and Ctrl+C / Ctrl+X / Ctrl+V sent as key presses), plus any
/// clipboard text a menu's Copy left. Called by the global shortcuts when no text field has the
/// keyboard.
pub fn handle(app: &mut SolveApp, ctx: &egui::Context) {
    let (copies, cuts, pastes, pointer) = ctx.input(|i| {
        let mut copies = 0;
        let mut cuts = 0;
        let mut pastes = 0;
        // Firefox on Linux sends primary-selection Paste events during middle-click navigation.
        // Include release frames and clicks completed within one frame; explicit Ctrl/Cmd+V still works.
        let middle_navigation = i.pointer.button_down(egui::PointerButton::Middle)
            || i.pointer.button_pressed(egui::PointerButton::Middle)
            || i.pointer.button_released(egui::PointerButton::Middle);
        for e in &i.events {
            match e {
                egui::Event::Copy => copies += 1,
                egui::Event::Cut => cuts += 1,
                egui::Event::Paste(_) if !middle_navigation => pastes += 1,
                egui::Event::Key { key: egui::Key::C, pressed: true, modifiers, .. } if modifiers.command && !modifiers.shift && !modifiers.alt => {
                    copies += 1
                }
                egui::Event::Key { key: egui::Key::X, pressed: true, modifiers, .. } if modifiers.command && !modifiers.shift && !modifiers.alt => {
                    cuts += 1
                }
                egui::Event::Key { key: egui::Key::V, pressed: true, modifiers, .. } if modifiers.command && !modifiers.shift && !modifiers.alt => {
                    pastes += 1
                }
                _ => {}
            }
        }
        (copies, cuts, pastes, i.pointer.latest_pos())
    });
    if copies > 0 {
        copy(app);
    }
    if cuts > 0 {
        cut(app);
    }
    if pastes > 0 {
        paste(app, pointer);
    }
    flush(ctx);
}

/// Write clipboard text a copy left.
pub fn flush(ctx: &egui::Context) {
    if let Some(text) = PENDING_TEXT.lock().ok().and_then(|mut p| p.take()) {
        ctx.copy_text(text);
    }
}

#[cfg(test)]
#[path = "edit_clipboard_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "edit_clipboard_occurrence_tests.rs"]
mod occurrence_tests;
