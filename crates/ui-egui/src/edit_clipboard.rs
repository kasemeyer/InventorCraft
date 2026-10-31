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

/// What a selection stands for in an assembly (R186): the one resolver Copy / Paste (Ctrl+C/V,
/// the ring's and list's Copy, the Clipboard panel) and the Position panel's Move/Copy share.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Picked {
    /// Not placements (outside an assembly, in a sketch, nothing picked, or only root bodies,
    /// features or other items): each command keeps its own handling.
    Other,
    /// Every item is an occurrence's Browser row, a picked component row, or geometry (body,
    /// face, edge, vertex) of a placed component's body: these placements, by path.
    Placements(Vec<Vec<u64>>),
    /// Placements together with something else (a root body, a feature).
    Mixed,
}

/// The placements the selection resolves to; see [`Picked`]. Only in an assembly document: a
/// SolidCraft Part design with components keeps its own behaviour.
pub(crate) fn picked_placements(app: &SolveApp) -> Picked {
    let s = &app.session;
    if s.active_sketch.is_some() || s.doc.kind != solidcraft_engine::doc::DocumentKind::Assembly {
        return Picked::Other;
    }
    let exists = |path: &Vec<u64>| path.last().is_some_and(|id| s.doc.occurrences.iter().any(|o| o.id == *id));
    let mut out: Vec<Vec<u64>> = Vec::new();
    let mut other = false;
    // A component row picked in SolidCraft's Browser: its first placement.
    if s.selection.is_empty() && !app.tree.picked_components.contains(&0) {
        out.extend(app.tree.picked_components.iter().map(|c| s.doc.first_path(*c)).filter(exists));
    }
    for item in &s.selection {
        let path = match item {
            Sel::Occurrence { path } => Some(path.clone()),
            // A placed (linked) part's geometry stands for its placement: its bodies come from
            // the part's file. A component made here keeps its bodies as geometry.
            Sel::Body { name: body } | Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } => {
                crate::dialogs_assembly::occurrence_path_of_body(s, body).filter(|path| {
                    let component = path.last().and_then(|id| s.doc.occurrences.iter().find(|o| o.id == *id)).map(|o| o.component);
                    component.is_some_and(|c| s.doc.links.contains_key(&c))
                })
            }
            _ => None,
        };
        match path.filter(exists) {
            Some(path) if !out.contains(&path) => out.push(path),
            Some(_) => {}
            None => other = true,
        }
    }
    match (out.is_empty(), other) {
        (true, _) => Picked::Other,
        (false, false) => Picked::Placements(out),
        (false, true) => Picked::Mixed,
    }
}

/// The occurrences the selection resolves to (each placement's own), when it is placements only.
#[cfg(test)]
pub(crate) fn selected_occurrences(app: &SolveApp) -> Option<Vec<u64>> {
    match picked_placements(app) {
        Picked::Placements(paths) => Some(paths.iter().filter_map(|p| p.last().copied()).collect()),
        _ => None,
    }
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
    app.set_status(crate::i18n::format("ui.clipboard.copied", &[("names", &names)]), false);
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
            None => Err(crate::i18n::tr("ui.clipboard.other_assembly").to_string()),
        };
        match out {
            Ok(out) => made.extend(out.get("occurrence").and_then(Value::as_u64)),
            // Part-way: what was pasted stays, selected, and the status says where it stopped.
            Err(e) if !made.is_empty() => {
                select_pasted(app, &parent_path, &made);
                let names = occurrence_names(app, made.iter().copied());
                app.set_status(crate::i18n::format("ui.clipboard.pasted_partly", &[("names", &names), ("error", &e)]), true);
                return true;
            }
            Err(e) => {
                app.set_status(e, true);
                return false;
            }
        }
    }
    select_pasted(app, &parent_path, &made);
    let names = occurrence_names(app, made.iter().copied());
    app.set_status(crate::i18n::format("ui.clipboard.pasted", &[("names", &names)]), false);
    true
}

/// The pasted occurrences become the selection.
fn select_pasted(app: &mut SolveApp, parent_path: &[u64], made: &[u64]) {
    app.session.selection = made
        .iter()
        .map(|id| {
            let mut path = parent_path.to_vec();
            path.push(*id);
            Sel::Occurrence { path }
        })
        .collect();
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
    // A placed component pastes as a new occurrence of it, never a body at the root (R186). A
    // mix of components and other things is refused, and the clipboard keeps what it had.
    match picked_placements(app) {
        Picked::Placements(paths) => {
            let occurrences: Vec<u64> = paths.iter().filter_map(|p| p.last().copied()).collect();
            return copy_occurrences(app, &occurrences);
        }
        Picked::Mixed => {
            app.set_status(crate::i18n::tr("ui.clipboard.mixed"), true);
            return false;
        }
        Picked::Other => {}
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
