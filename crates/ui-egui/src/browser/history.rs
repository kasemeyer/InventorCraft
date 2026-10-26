//! The browser as the design's history (a profile whose part root holds `history`): features at
//! the root in history order, each sketch and work feature folded under the first feature that
//! consumes it, and End of Part as a marker row that drags to roll the model back or forward.

use std::collections::BTreeMap;

use solidcraft_engine::doc::Document;

use super::*;

/// A work feature (construction plane, axis or point): adopted by the first later feature that
/// names it.
fn is_work(kind: &FeatureKind) -> bool {
    matches!(kind, FeatureKind::ConstructionPlane { .. } | FeatureKind::ConstructionAxis { .. } | FeatureKind::ConstructionPoint { .. })
}

/// Child feature id → the feature that consumes it (Inventor's auto-consume): a sketch nests under
/// the first later feature whose `input_sketches()` holds it; a work feature (construction plane,
/// axis or point) under the first later feature that `names_feature` it. Only consumers before
/// `marker` (all when None) adopt; component 0's features only.
pub(crate) fn parents(doc: &Document, marker: Option<usize>) -> BTreeMap<u64, u64> {
    let own: Vec<(usize, &solidcraft_engine::doc::Feature)> = doc.features.iter().enumerate().filter(|(_, f)| f.component == 0).collect();
    let mut out = BTreeMap::new();
    for (k, &(i, f)) in own.iter().enumerate() {
        if !marker.is_none_or(|m| i < m) {
            continue;
        }
        let earlier = own.get(..k).unwrap_or_default();
        for s in f.kind.input_sketches() {
            if s != f.id && earlier.iter().any(|(_, e)| e.id == s && matches!(e.kind, FeatureKind::Sketch { .. })) {
                out.entry(s).or_insert(f.id);
            }
        }
        for (_, w) in earlier.iter().filter(|(_, w)| is_work(&w.kind) && f.kind.names_feature(&w.name)) {
            out.entry(w.id).or_insert(f.id);
        }
    }
    out
}

/// The top-level rows in history order: (feature index, feature id), children excluded.
pub(crate) fn top_rows(doc: &Document) -> Vec<(usize, u64)> {
    top_rows_of(doc, &parents(doc, doc.marker))
}

/// [`top_rows`] with the frame's [`parents`] already worked out.
fn top_rows_of(doc: &Document, adopted: &BTreeMap<u64, u64>) -> Vec<(usize, u64)> {
    doc.features.iter().enumerate().filter(|(_, f)| f.component == 0 && !adopted.contains_key(&f.id)).map(|(i, f)| (i, f.id)).collect()
}

/// Where End of Part lands when dropped at height `y`: 0 above the first row, else the index
/// after the nearest top-level row above `y`. `rows` are (feature index, row centre y).
pub(crate) fn drop_position(rows: &[(usize, f32)], y: f32) -> usize {
    rows.iter().rfind(|(_, centre)| *centre < y).map_or(0, |(index, _)| index + 1)
}

/// Roll the history so End of Part sits at `position` (`None` = the end).
pub(crate) fn move_end_of_part(app: &mut SolveApp, position: Option<usize>) {
    let _ = app.run_heavy("timeline.roll_to", position.map_or(json!({}), |p| json!({"position": p})));
}

/// What one frame's history rows share: who folds under whom, the marker, and the sketch and
/// work-plane rows' looks (their eyes, selection and editing state, as in their folders).
struct Ctx {
    children: BTreeMap<u64, Vec<usize>>,
    marker: usize,
    sketches: Vec<Entry>,
    planes: Vec<Entry>,
}

pub(super) fn rows(app: &mut SolveApp, ui: &mut egui::Ui, depth: usize, acts: &mut Actions) {
    let doc = &app.session.doc;
    let n = doc.features.len();
    let marker = doc.marker.unwrap_or(n);
    let mut children: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    let adopted = parents(doc, doc.marker);
    for (i, f) in doc.features.iter().enumerate() {
        if let Some(parent) = adopted.get(&f.id) {
            children.entry(*parent).or_default().push(i);
        }
    }
    let top = top_rows_of(doc, &adopted);
    let ctx = Ctx { children, marker, sketches: entries(app, 0, "sketches"), planes: entries(app, 0, "construction") };
    let mut end = None;
    let mut centres = Vec::new();
    for (index, _) in top {
        if end.is_none() && index >= marker {
            end = Some(end_of_part(app, ui, depth, acts));
        }
        if let Some(y) = row(app, ui, &ctx, index, depth, acts) {
            centres.push((index, y));
        }
    }
    let end = match end {
        Some(end) => end,
        None => end_of_part(app, ui, depth, acts),
    };
    // Dragging End of Part: a line where it would land; on release, the history rolls there.
    let Some(resp) = end.resp else { return };
    let pointer = ui.input(|i| i.pointer.interact_pos());
    if resp.dragged()
        && let Some(p) = pointer
    {
        let t = Tokens::get();
        ui.painter().hline(resp.rect.x_range(), p.y, Stroke::new(2.0, t.accent));
    }
    if resp.drag_stopped()
        && let Some(p) = pointer
    {
        move_end_of_part(app, Some(drop_position(&centres, p.y)).filter(|p| *p < n));
    }
}

/// The End of Part row: it drags, and its menu moves it to the top or the end.
fn end_of_part(app: &SolveApp, ui: &mut egui::Ui, depth: usize, acts: &mut Actions) -> RowResp {
    let label = folder_label(app, "end_of_part");
    let r = draw_row(ui, ui.id().with("end_of_part"), &Row { depth, icon: "error", label: &label, ..Default::default() });
    if let Some(p) = r.secondary {
        acts.menu = Some((p, Target::EndOfPart));
    }
    r
}

/// One feature's row (the `index`th of the design's features) and, when its fold is open, its
/// children; the row's centre height.
fn row(app: &mut SolveApp, ui: &mut egui::Ui, ctx: &Ctx, index: usize, depth: usize, acts: &mut Actions) -> Option<f32> {
    let feature = app.session.doc.features.get(index)?;
    let (id, name, icon) = (feature.id, feature.name.clone(), crate::timeline::icon_of(&feature.kind));
    let sketch = matches!(feature.kind, FeatureKind::Sketch { .. });
    let key = id.to_string();
    // A sketch or work plane keeps its folder row's look: its eye, its plane selection.
    let (folder, entry) = match (sketch, ctx.sketches.iter().find(|e| e.key == key), ctx.planes.iter().find(|e| e.key == key)) {
        (true, Some(e), _) => ("sketches", Some(e)),
        (false, _, Some(e)) => ("construction", Some(e)),
        _ => ("features", None),
    };
    let kids = ctx.children.get(&id);
    let fold_key = format!("history/{id}");
    let open = kids.is_some() && is_open(app, &fold_key, false);
    let rolled = index >= ctx.marker;
    let (selected, mark) = match entry {
        Some(e) => lit_row(acts.lit.entry(folder, e)),
        None => (app.session.selection.contains(&Sel::Feature { id }), Mark::None),
    };
    let row_key = if folder == "construction" { format!("p:{id}") } else { format!("f:{id}") };
    app.tree.order.push(row_key.clone());
    let r = draw_row(
        ui,
        ui.id().with(("history", id)),
        &Row {
            depth,
            fold: kids.map(|_| open),
            eye: entry.map(|e| e.visible),
            icon: entry.map_or(icon, |e| e.icon),
            label: entry.map_or(name.as_str(), |e| e.label.as_str()),
            name: true,
            selected,
            dim: rolled || entry.is_some_and(|e| !e.visible),
            color: entry.and_then(|e| e.color),
            mark,
            ..Default::default()
        },
    );
    if let Some(rect) = r.rect {
        crate::scenario::publish_handle(&format!("row:history:{id}"), rect.center());
    }
    if r.fold {
        toggle(app, &fold_key, false);
    }
    if r.eye
        && let Some(e) = entry
    {
        set_visible(app, folder, &[e], !e.visible);
    }
    if r.double {
        if sketch {
            app.edit_sketch(id);
        } else {
            app.edit_feature(id);
        }
    } else if r.clicked {
        match entry.and_then(|e| e.sel.clone()) {
            Some(plane @ Sel::Plane { .. }) => pick_from_browser(app, plane),
            _ => click_select(app, ui, row_key, Some(Sel::Feature { id })),
        }
    }
    if let Some(p) = r.secondary {
        let (sel, target) = match entry {
            Some(e) => (e.sel.clone(), e.target.clone()),
            None => (Some(Sel::Feature { id }), None),
        };
        if !selected && let Some(s) = sel {
            let _ = app.run("select.set", json!({ "items": [s] }));
        }
        acts.menu = Some((p, target.unwrap_or(Target::Feature { id })));
    }
    if open {
        for &child in kids.into_iter().flatten() {
            row(app, ui, ctx, child, depth + 1, acts);
        }
    }
    r.rect.map(|rect| rect.center().y)
}
