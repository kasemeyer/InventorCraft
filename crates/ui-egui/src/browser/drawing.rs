//! The drawing browser (Plan D-2 Task 7, spec §2.10): a Drawing document's tree in the profile's
//! `drawing_root` order. `<drawing>` › Drawing Resources (Borders, Title Blocks) › `Sheet:<n>` ›
//! Border, Title Block, then each base view `VIEW<n>:<model file>` with its projected views
//! nested under it and, last, the read-only model. Every profile's drawings use this tree
//! (SolidCraft's take the upstream labels); its menus are lists built by `drawing_menu`.
//! Drawing Properties (the title block's fields, R8, R87) lives here too.

use serde_json::{Map, Value, json};
use solidcraft_engine::doc::drawing::{DrawingModel, DrawingView, ModelKind, ViewId};
use solidcraft_engine::drawing::{COMPUTED_FIELDS, PaperPick, TITLE_FIELDS};

use super::{Row, draw_row, folder_label, is_open, toggle};
use crate::SolveApp;
use crate::context_menu::Target;
use crate::profile::RootNode;

const ROOT: &str = "d:root";

/// The drawing's tree, from its root row at `depth`.
pub(crate) fn rows(app: &mut SolveApp, ui: &mut egui::Ui, depth: usize) {
    let Some(m) = app.session.doc.drawing.clone() else { return };
    let missing = missing(app);
    // The file's stem once saved, else the document's name.
    let saved = app.session.path.as_deref().and_then(|p| std::path::Path::new(p).file_stem()).map(|s| s.to_string_lossy().into_owned());
    let label = saved.clone().unwrap_or_else(|| app.session.doc.name.clone());
    let open = is_open(app, ROOT, true);
    let r = draw_row(
        ui,
        ui.id().with(ROOT),
        &Row { depth, fold: Some(open), icon: "ws_drawing", label: &label, name: saved.is_some(), ..Default::default() },
    );
    if r.fold {
        toggle(app, ROOT, true);
    }
    if let Some(p) = r.secondary {
        crate::context_menu::open_for(app, p, Target::DrawingRoot);
    }
    if !open {
        return;
    }
    for node in super::root_order(app) {
        match node {
            RootNode::DrawingResources => resources(app, ui, &m, depth + 1),
            RootNode::Sheets => {
                for index in 0..m.sheets.len() {
                    sheet(app, ui, &m, index, &missing, depth + 1);
                }
            }
            _ => {}
        }
    }
}

/// A fixed node's label: the profile's catalog id for it, else upstream's words.
fn node_label(app: &SolveApp, node: &str) -> String {
    let label = folder_label(app, node);
    crate::i18n::tr_label(&label).to_string()
}

/// Drawing Resources: the border and title block definitions the sheets use, read-only.
fn resources(app: &mut SolveApp, ui: &mut egui::Ui, m: &DrawingModel, depth: usize) {
    let open = folder(app, ui, "d:resources", &node_label(app, "drawing_resources"), depth);
    if !open {
        return;
    }
    let mut borders: Vec<String> = m.sheets.iter().filter_map(|s| block_name(m, s.border)).map(|b| block_display(&b)).collect();
    borders.sort();
    borders.dedup();
    let mut titles: Vec<String> = m.sheets.iter().filter_map(|s| block_name(m, s.title_block)).map(|b| block_display(&b)).collect();
    titles.sort();
    titles.dedup();
    for (key, node, names, icon) in [("d:borders", "borders", borders, "rect"), ("d:title_blocks", "title_blocks", titles, "text")] {
        if folder(app, ui, key, &node_label(app, node), depth + 1) {
            for name in names {
                draw_row(ui, ui.id().with((key, &name)), &Row { depth: depth + 2, icon, label: &name, name: true, ..Default::default() });
            }
        }
    }
}

/// A border or title block's name as the browser shows it: the template's blocks by their
/// catalog names ("ANSI D Border", "ANSI Title Block"), any other block by its own name.
pub(crate) fn block_display(block: &str) -> String {
    if block == solidcraft_engine::drawing::TITLE_BLOCK {
        return crate::i18n::tr("ui.drawing.title_block_ansi").to_string();
    }
    let Some(rest) = block.strip_prefix("TCI_BORDER_") else { return block.to_string() };
    let (letter, key) = match rest.strip_suffix("_PORTRAIT") {
        Some(letter) => (letter, "ui.drawing.border_block_portrait"),
        None => (rest, "ui.drawing.border_block"),
    };
    match solidcraft_engine::doc::drawing::AnsiSize::parse(letter) {
        Some(size) => crate::i18n::format(key, &[("size", size.letter())]),
        None => block.to_string(),
    }
}

/// A folding row with a folder icon, closed until opened; whether it is open.
fn folder(app: &mut SolveApp, ui: &mut egui::Ui, key: &str, label: &str, depth: usize) -> bool {
    let open = is_open(app, key, false);
    let r = draw_row(ui, ui.id().with(key), &Row { depth, fold: Some(open), icon: "folder", label, name: true, ..Default::default() });
    if r.fold {
        toggle(app, key, false);
    }
    open
}

/// The block an Insert places.
fn block_name(m: &DrawingModel, insert: cadcraft_doc::Handle) -> Option<String> {
    match &m.paper.entity(insert)?.kind {
        cadcraft_doc::EntityKind::Insert(ins) => Some(ins.block.clone()),
        _ => None,
    }
}

/// `Sheet:<n>` (dimmed unless active) › Border, Title Block, its base views.
fn sheet(app: &mut SolveApp, ui: &mut egui::Ui, m: &DrawingModel, index: usize, missing: &[ViewId], depth: usize) {
    let key = format!("d:sheet:{index}");
    let open = is_open(app, &key, true);
    let active = app.session.drawing_sheet.min(m.sheets.len().saturating_sub(1)) == index;
    let label = format!("{}:{}", node_label(app, "sheet"), index + 1);
    let r = draw_row(
        ui,
        ui.id().with(&key),
        &Row { depth, fold: Some(open), icon: "sheet", label: &label, name: true, dim: !active, ..Default::default() },
    );
    if r.fold {
        toggle(app, &key, true);
    }
    if r.double {
        activate_sheet(app, index);
    }
    if let Some(p) = r.secondary {
        crate::context_menu::open_for(app, p, Target::Sheet { index });
    }
    if !open {
        return;
    }
    for (node, icon) in [("border", "rect"), ("title_block", "text")] {
        let label = node_label(app, node);
        // Every sheet has these two: their handles carry the sheet (`Border@sheet2`).
        let handle = format!("{label}@sheet{}", index + 1);
        let row = Row { depth: depth + 1, icon, label: &label, name: true, handle: Some(&handle), ..Default::default() };
        draw_row(ui, ui.id().with((&key, node)), &row);
    }
    for v in m.views.values().filter(|v| v.sheet == index && v.parent.is_none_or(|p| !m.views.contains_key(&p))) {
        view(app, ui, m, v, missing, depth + 1);
    }
}

/// Make sheet `index` the active one (`drawing.sheet.activate`, view state, no undo step).
pub(crate) fn activate_sheet(app: &mut SolveApp, index: usize) {
    if app.run("drawing.sheet.activate", json!({ "sheet": index })).is_ok() {
        app.drawing.fit_pending = true;
    }
}

/// The model's file name as a view shows it.
fn model_file(v: &DrawingView) -> String {
    std::path::Path::new(&v.model.path).file_name().map_or_else(|| v.model.path.clone(), |n| n.to_string_lossy().into_owned())
}

/// A view's row label: `VIEW<n>:<model file>`, and the missing suffix when its model is found
/// nowhere (R77).
pub(crate) fn view_label(v: &DrawingView, missing: bool) -> String {
    let label = format!("{}:{}", v.identifier, model_file(v));
    if missing { format!("{label} {}", crate::i18n::tr("ui.drawing.model_missing")) } else { label }
}

/// A view, the views projected from it, and (for a base view) its model, read-only.
fn view(app: &mut SolveApp, ui: &mut egui::Ui, m: &DrawingModel, v: &DrawingView, missing: &[ViewId], depth: usize) {
    let key = format!("d:view:{}", v.id);
    let children: Vec<&DrawingView> = m.views.values().filter(|c| c.parent == Some(v.id)).collect();
    let base = v.parent.is_none();
    let folds = base || !children.is_empty();
    let open = is_open(app, &key, true);
    let label = view_label(v, missing.contains(&v.id));
    let pick = PaperPick::View { view: v.id };
    let selected = app.drawing.selection.contains(&pick);
    let r = draw_row(
        ui,
        ui.id().with(&key),
        &Row {
            depth,
            fold: folds.then_some(open),
            icon: if base { "camera" } else { "project" },
            label: &label,
            name: true,
            selected,
            ..Default::default()
        },
    );
    if r.fold {
        toggle(app, &key, true);
    }
    if r.clicked {
        let add = ui.input(|i| i.modifiers.command || i.modifiers.ctrl || i.modifiers.shift);
        if !add {
            app.drawing.selection.clear();
        }
        if selected && add {
            app.drawing.selection.retain(|p| *p != pick);
        } else if !app.drawing.selection.contains(&pick) {
            app.drawing.selection.push(pick.clone());
        }
    }
    if let Some(p) = r.secondary {
        // The menu acts on the selection: a right-click on an unselected view selects it.
        if !selected {
            app.drawing.selection = vec![pick];
        }
        crate::context_menu::open_for(app, p, Target::DrawingView { view: v.id });
    }
    if !folds || !open {
        return;
    }
    for c in children {
        view(app, ui, m, c, missing, depth + 1);
    }
    if base {
        let icon = if v.model.kind == ModelKind::Assembly { "component" } else { "body" };
        let file = model_file(v);
        // One model can sit under base views on several sheets: its handle carries the sheet and
        // the view (`block.tcipart@sheet2:VIEW4`).
        let handle = format!("{file}@sheet{}:{}", v.sheet + 1, v.identifier);
        let row = Row { depth: depth + 1, icon, label: &file, name: true, dim: missing.contains(&v.id), handle: Some(&handle), ..Default::default() };
        draw_row(ui, ui.id().with((&key, "model")), &row);
    }
}

/// The views whose model is found nowhere (R77): recomputed when the document changes (each edit,
/// open and tab switch), never per frame.
pub(crate) fn missing(app: &mut SolveApp) -> Vec<ViewId> {
    // The tab and the document's address: an address freed in one tab and reused in another
    // never reads the other's answer.
    let doc = (app.docs.active, std::sync::Arc::as_ptr(&app.session.doc) as usize);
    match &app.drawing.missing {
        Some((at, views)) if *at == doc => views.clone(),
        _ => {
            let views = solidcraft_engine::drawing::missing_views(&app.session);
            app.drawing.missing = Some((doc, views.clone()));
            views
        }
    }
}

/// Where view `view`'s model is now, if anywhere (R77).
pub(crate) fn model_path_of(app: &SolveApp, view: ViewId) -> Option<String> {
    let m = app.session.doc.drawing.as_deref()?;
    let v = m.views.get(&view)?;
    solidcraft_engine::drawing::model_path(&app.session, &v.model).map(|p| p.to_string_lossy().into_owned())
}

/// Open the model view `view` shows: the tab already holding it becomes active, else the file
/// opens in a new tab.
pub(crate) fn open_model(app: &mut SolveApp, view: ViewId) {
    let Some(path) = model_path_of(app, view) else {
        app.set_status(crate::i18n::tr("ui.drawing.model_missing").to_string(), true);
        return;
    };
    let wanted = std::path::absolute(&path).ok();
    let same = |p: &str| std::path::absolute(p).ok() == wanted;
    if let Some(tab) = crate::documents::tab_paths(app).iter().position(|p| p.as_deref().is_some_and(same)) {
        crate::documents::switch(app, tab);
        return;
    }
    app.open_path_async(&path);
}

/// The title block's fields of sheet `sheet`: (tag, value) in the block's order.
pub(crate) fn title_values(m: &DrawingModel, sheet: usize) -> Vec<(String, String)> {
    let Some(e) = m.sheets.get(sheet).and_then(|s| m.paper.entity(s.title_block)) else { return Vec::new() };
    match &e.kind {
        cadcraft_doc::EntityKind::Insert(ins) => ins.attribs.iter().map(|a| (a.tag.clone(), a.text.value.clone())).collect(),
        _ => Vec::new(),
    }
}

/// Drawing Properties: the title block's fields in `TITLE_FIELDS` order (owner default; §8.2).
/// The tags in `COMPUTED_FIELDS` are shown read-only (R8); OK sends only what changed (R87).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PropertiesPanel {
    /// (tag, value) per field, as edited.
    pub fields: Vec<(String, String)>,
    /// The values the panel opened with.
    pub original: Vec<(String, String)>,
}

impl PropertiesPanel {
    /// The active sheet's fields, or None when the document is not a drawing.
    pub(crate) fn new(app: &SolveApp) -> Option<PropertiesPanel> {
        let m = app.session.doc.drawing.as_deref()?;
        let sheet = app.session.drawing_sheet.min(m.sheets.len().saturating_sub(1));
        let shown = title_values(m, sheet);
        let fields: Vec<(String, String)> = TITLE_FIELDS
            .iter()
            .map(|tag| (tag.to_string(), shown.iter().find(|(t, _)| t == tag).map(|(_, v)| v.clone()).unwrap_or_default()))
            .collect();
        Some(PropertiesPanel { original: fields.clone(), fields })
    }

    /// Whether tag `tag` is typed in (not computed from the sheet and views).
    pub(crate) fn editable(tag: &str) -> bool {
        !COMPUTED_FIELDS.contains(&tag)
    }

    /// `drawing.properties.set`'s parameters: each changed editable field, keyed by its tag in
    /// lower case (R8); empty when nothing changed.
    pub(crate) fn changes(&self) -> Map<String, Value> {
        self.fields
            .iter()
            .zip(&self.original)
            .filter(|((tag, now), (_, was))| Self::editable(tag) && now != was)
            .map(|((tag, now), _)| (tag.to_ascii_lowercase(), Value::String(now.clone())))
            .collect()
    }
}

/// Drawing Properties… (the root's menu): open the panel on the active sheet's fields.
pub(crate) fn open_properties(app: &mut SolveApp) {
    if app.drawing.tool.is_some() {
        return;
    }
    if let Some(p) = PropertiesPanel::new(app) {
        app.drawing.panel = Some(crate::drawing_ui::DrawingPanel::Properties(p));
    }
}

/// OK: one `drawing.properties.set` with the changed fields, nothing when none changed (R87).
/// A changed company becomes the preference new drawings take (R43). False when the command
/// refused (the panel stays open).
pub(crate) fn apply_properties(app: &mut SolveApp, p: &PropertiesPanel) -> bool {
    let changes = p.changes();
    if changes.is_empty() {
        return true;
    }
    let company = changes.get("company").and_then(Value::as_str).map(str::to_string);
    if app.run("drawing.properties.set", Value::Object(changes)).is_err() {
        return false;
    }
    if let Some(company) = company {
        app.preferences.drawing_company = company;
    }
    true
}

/// The catalog id of a title block field's label.
fn field_label(tag: &str) -> &'static str {
    match tag {
        "TITLE" => "ui.drawing.field_title",
        "COMPANY" => "ui.drawing.field_company",
        "DWG_NO" => "ui.drawing.field_dwg_no",
        "REV" => "ui.drawing.field_rev",
        "SIZE" => "ui.drawing.field_size",
        "SCALE" => "ui.drawing.field_scale",
        "SHEET" => "ui.drawing.field_sheet",
        "DRAWN_BY" => "ui.drawing.field_drawn_by",
        "DRAWN_DATE" => "ui.drawing.field_drawn_date",
        "CHECKED_BY" => "ui.drawing.field_checked_by",
        "CHECKED_DATE" => "ui.drawing.field_checked_date",
        "APPROVED_BY" => "ui.drawing.field_approved_by",
        "APPROVED_DATE" => "ui.drawing.field_approved_date",
        _ => "ui.drawing.field_material",
    }
}

/// The Drawing Properties card (a `ui_kit::Window`): Enter or OK applies, Esc or Cancel closes.
pub(crate) fn properties_panel(app: &mut SolveApp, ctx: &egui::Context, mut p: PropertiesPanel) {
    let title = crate::i18n::tr("ui.drawing.properties_title");
    let mut enter = false;
    let (closed, action) = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_properties"), "settings", title)
        .placed(egui::Align2::LEFT_TOP, super::panel_pos(app))
        .actions(crate::i18n::tr_label("OK"), crate::i18n::tr("ui.cancel"), None)
        .show(ctx, true, |ui| {
            for (tag, value) in &mut p.fields {
                let label = crate::i18n::tr(field_label(tag));
                if PropertiesPanel::editable(tag) {
                    crate::ui_kit::row(ui, label, |ui| {
                        crate::scenario::publish_handle(&format!("drawing_properties:{tag}"), ui.available_rect_before_wrap().left_center());
                        enter |= crate::ui_kit::text_field(ui, value, "").0;
                    });
                } else {
                    crate::ui_kit::row(ui, label, |ui| crate::ui_kit::text(ui, value, crate::ui_kit::Text::Value("", false)));
                }
            }
        });
    if closed || action == crate::ui_kit::Action::Cancel {
        app.drawing.panel = None;
        return;
    }
    if enter || action == crate::ui_kit::Action::Ok {
        app.drawing.panel = if apply_properties(app, &p) { None } else { Some(crate::drawing_ui::DrawingPanel::Properties(p)) };
        return;
    }
    app.drawing.panel = Some(crate::drawing_ui::DrawingPanel::Properties(p));
}
