//! The Base View panel (Plan D-2 Task 5, spec §2.3, §2.5, §2.6): the captured Drawing View dialog's
//! Component tab and the one Display option we build, in our words. It opens from the ribbon's Base
//! (and BV, through the drawing table), from Create Drawing View on a model (R13) and from a placed
//! view's Edit View (R93), where OK runs one `drawing.view.set_*` command per changed field (R87).
//! The same code serves every profile's drawings (SolidCraft's included).
//!
//! Placement at the cursor with the live preview and the on-sheet cube is Task 6; until then OK on a
//! new view places it at the active sheet's centre through `drawing.view.base`.

use serde_json::{Map, Value, json};
use solidcraft_engine::doc::DocumentKind;
use solidcraft_engine::doc::drawing::{CubeTarget, DrawingModel, DrawingView, LineStyle, SCALE_PRESETS, format_scale, parse_scale};
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::drawing_ui::DrawingPanel;
use crate::scenario;

/// Where the panel was opened from: its defaults and what OK does follow it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The ribbon's Base or the BV alias: Hidden Line only, Front [E: DW §2].
    Ribbon,
    /// Create Drawing View on a model (spec §2.1 path 2c): Hidden Line Removed + Shaded, the model
    /// tab's view direction.
    FromModel,
    /// Edit View on a placed view (its id).
    Edit(u64),
}

/// The panel's fields (spec §2.3). Task 6 reads `orientation` and `base_params` for the preview and
/// the placement.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ViewPanel {
    pub origin: Origin,
    /// The model file (absolute), `None` until one is chosen.
    pub model: Option<String>,
    /// 0 Component, 1 Display.
    pub tab: usize,
    pub hidden_lines: bool,
    pub removed: bool,
    pub shaded: bool,
    pub scale_text: String,
    pub identifier: String,
    pub label_visible: bool,
    pub tangent_edges: bool,
    /// The footer's preview checkbox (Task 6 shows the preview while it is on).
    pub preview: bool,
    pub orientation: CubeTarget,
    /// Why OK is off (a scale that does not parse) or why the last OK failed.
    pub error: Option<String>,
    /// A projected view follows its parent's style / scale (edit mode only).
    pub style_from_base: bool,
    pub scale_from_base: bool,
    /// The placed view as it was when the panel opened (edit mode).
    original: Option<Original>,
}

/// What an edited view held when the panel opened: OK compares against it (R87).
#[derive(Clone, Debug, PartialEq)]
struct Original {
    projected: bool,
    lines: LineStyle,
    shaded: bool,
    tangent_edges: bool,
    style_from_base: bool,
    scale: f64,
    scale_from_base: bool,
    identifier: String,
    label_visible: bool,
    orientation: CubeTarget,
    /// The parent's (lines, shaded, tangent edges, scale), shown when following it is turned on.
    parent: Option<(LineStyle, bool, bool, f64)>,
}

impl ViewPanel {
    fn new(app: &SolveApp, model: Option<String>, origin: Origin, look: Option<Vec3>) -> Option<ViewPanel> {
        let m = app.session.doc.drawing.as_deref()?;
        let front = CubeTarget { x: 0, y: -1, z: 0 };
        let mut p = ViewPanel {
            origin,
            model,
            tab: 0,
            hidden_lines: true,
            removed: false,
            shaded: false,
            scale_text: "1:1".into(),
            identifier: next_identifier(m),
            label_visible: false,
            tangent_edges: false,
            preview: true,
            orientation: look.map_or(front, CubeTarget::nearest),
            error: None,
            style_from_base: false,
            scale_from_base: false,
            original: None,
        };
        match origin {
            Origin::Ribbon => {}
            Origin::FromModel => (p.hidden_lines, p.removed, p.shaded) = (false, true, true),
            Origin::Edit(id) => {
                let v = m.views.get(&id)?;
                let parent = v.parent.and_then(|pid| m.views.get(&pid)).map(|pv| (pv.lines, pv.shaded, pv.tangent_edges, pv.scale));
                let original = Original {
                    projected: v.parent.is_some(),
                    lines: v.lines,
                    shaded: v.shaded,
                    tangent_edges: v.tangent_edges,
                    style_from_base: v.style_from_base,
                    scale: v.scale,
                    scale_from_base: v.scale_from_base,
                    identifier: v.identifier.clone(),
                    label_visible: v.label_visible,
                    orientation: orientation_of(v),
                    parent,
                };
                p.model = Some(v.model.path.clone());
                p.set_lines(v.lines);
                p.shaded = v.shaded;
                p.tangent_edges = v.tangent_edges;
                p.style_from_base = v.style_from_base && original.projected;
                p.scale_from_base = v.scale_from_base && original.projected;
                p.scale_text = format_scale(v.scale);
                p.identifier = v.identifier.clone();
                p.label_visible = v.label_visible;
                p.orientation = original.orientation;
                p.original = Some(original);
            }
        }
        Some(p)
    }

    fn set_lines(&mut self, lines: LineStyle) {
        (self.hidden_lines, self.removed) = (lines == LineStyle::Hidden, lines == LineStyle::Removed);
    }

    fn lines(&self) -> LineStyle {
        if self.removed { LineStyle::Removed } else { LineStyle::Hidden }
    }

    /// The typed scale, if it parses (`a:b`, `a/b` or a decimal, > 0).
    pub(crate) fn scale(&self) -> Option<f64> {
        parse_scale(&self.scale_text)
    }

    /// The Model row can be changed: only for a new view (no command changes a view's model, R87).
    pub(crate) fn model_editable(&self) -> bool {
        !matches!(self.origin, Origin::Edit(_))
    }

    /// The on-sheet cube (Task 6) may change the orientation: not for a projected view (R87).
    pub(crate) fn cube_enabled(&self) -> bool {
        !self.original.as_ref().is_some_and(|o| o.projected)
    }

    /// Style from Base and Scale from Base can be changed: only for a placed projected view.
    pub(crate) fn follows_base_enabled(&self) -> bool {
        self.original.as_ref().is_some_and(|o| o.projected)
    }

    /// Recheck the typed scale: `error` says why OK is off.
    fn validate(&mut self) {
        let bad = crate::i18n::tr("ui.drawing.bad_scale");
        if self.scale_from_base || self.scale().is_some() {
            if self.error.as_deref() == Some(bad) {
                self.error = None;
            }
        } else {
            self.error = Some(bad.to_string());
        }
    }

    /// Whether OK would act now: a model, a scale and an identifier.
    pub(crate) fn can_ok(&self) -> bool {
        self.model.is_some() && (self.scale_from_base || self.scale().is_some()) && !self.identifier.trim().is_empty()
    }
}

/// A placed view's cube target: its preset, else the one its direction is nearest.
fn orientation_of(v: &DrawingView) -> CubeTarget {
    v.orientation.preset.or_else(|| CubeTarget::matching(&v.orientation)).unwrap_or_else(|| CubeTarget::nearest(v.orientation.dir))
}

/// The identifier `drawing.view.base` would give the next view (`VIEW<n>`).
fn next_identifier(m: &DrawingModel) -> String {
    let mut n = m.next_view_number.max(1);
    while m.views.values().any(|v| v.identifier == format!("VIEW{n}")) {
        n += 1;
    }
    format!("VIEW{n}")
}

/// Open the panel (a drawing only; never while a drawing tool runs).
pub(crate) fn open(app: &mut SolveApp, model: Option<String>, origin: Origin) {
    open_looking(app, model, origin, None);
}

/// Open the panel with the orientation nearest `look` (the model tab's view direction, path 2c).
pub(crate) fn open_looking(app: &mut SolveApp, model: Option<String>, origin: Origin, look: Option<Vec3>) {
    if app.session.doc.kind != DocumentKind::Drawing || app.drawing.tool.is_some() {
        return;
    }
    let model = model.or_else(|| if origin == Origin::Ribbon { open_models(app).into_iter().next() } else { None });
    if let Some(p) = ViewPanel::new(app, model, origin, look) {
        app.drawing.panel = Some(DrawingPanel::View(p));
    }
}

/// The ribbon's Base and the BV alias (`DRAWING_STARTS`): the panel on the ribbon path.
pub(crate) fn start(app: &mut SolveApp) {
    open(app, None, Origin::Ribbon);
}

/// The open part and assembly tabs' files (absolute), in tab order: the Model combo's rows.
pub(crate) fn open_models(app: &SolveApp) -> Vec<String> {
    let kinds = crate::documents::tab_kinds(app);
    let mut out: Vec<String> = Vec::new();
    for (path, kind) in crate::documents::tab_paths(app).into_iter().zip(kinds) {
        if let Some(path) = path.filter(|_| matches!(kind, DocumentKind::Part | DocumentKind::Assembly)) {
            let path = std::path::absolute(&path).map_or(path, |p| p.to_string_lossy().into_owned());
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    out
}

/// The params `drawing.view.base` takes for this panel at paper `position` (Task 6 previews and places them).
pub(crate) fn base_params(p: &ViewPanel, position: [f64; 2]) -> Value {
    json!({
        "model": p.model,
        "orientation": p.orientation.name(),
        "lines": if p.removed { "removed" } else { "hidden" },
        "shaded": p.shaded,
        "scale": p.scale().unwrap_or(1.0),
        "position": position,
        "identifier": p.identifier.trim(),
        "tangent_edges": p.tangent_edges,
        "label_visible": p.label_visible,
    })
}

/// The active sheet's centre, inches: where OK places a new view until Task 6 places at the cursor.
fn sheet_centre(app: &SolveApp) -> [f64; 2] {
    let (w, h) = crate::sheet_canvas::paper_size(app);
    [w / 2.0, h / 2.0]
}

/// OK: a new view is placed (`drawing.view.base`, one step); an edited one runs one `set_*` command
/// per changed field (R87). False, with `error` set, when a command refused (the panel stays).
pub(crate) fn apply(app: &mut SolveApp, p: &mut ViewPanel) -> bool {
    p.validate();
    if !p.can_ok() {
        return false;
    }
    let Origin::Edit(view) = p.origin else {
        let params = base_params(p, sheet_centre(app));
        return match app.run("drawing.view.base", params) {
            Ok(_) => true,
            Err(e) => {
                p.error = Some(e);
                false
            }
        };
    };
    let Some(o) = p.original.clone() else { return true };
    for (cmd, params) in edits(p, &o, view) {
        match app.run(cmd, Value::Object(params)) {
            Ok(r) => {
                let n = r.get("suppressed").and_then(Value::as_u64).unwrap_or(0);
                if cmd == "drawing.view.set_style" && n > 0 {
                    app.set_status(crate::i18n::format("ui.drawing.suppressed", &[("n", &n.to_string())]), false);
                }
            }
            Err(e) => {
                p.error = Some(e);
                return false;
            }
        }
    }
    true
}

/// The commands an edit runs, in order: style (with tangent edges), scale, orientation, label.
fn edits(p: &ViewPanel, o: &Original, view: u64) -> Vec<(&'static str, Map<String, Value>)> {
    let mut out = Vec::new();
    let with_view = |mut m: Map<String, Value>| {
        m.insert("view".into(), json!(view));
        m
    };
    // Style: following the base, or the fields that changed (which stops following it).
    let mut style = Map::new();
    if p.style_from_base {
        if !o.style_from_base {
            style.insert("style_from_base".into(), json!(true));
        }
    } else {
        if p.lines() != o.lines {
            style.insert("lines".into(), json!(if p.removed { "removed" } else { "hidden" }));
        }
        if p.shaded != o.shaded {
            style.insert("shaded".into(), json!(p.shaded));
        }
        if p.tangent_edges != o.tangent_edges {
            style.insert("tangent_edges".into(), json!(p.tangent_edges));
        }
        if style.is_empty() && o.style_from_base {
            style.insert("style_from_base".into(), json!(false));
        }
    }
    if !style.is_empty() {
        out.push(("drawing.view.set_style", with_view(style)));
    }
    // Scale.
    let mut scale = Map::new();
    if p.scale_from_base {
        if !o.scale_from_base {
            scale.insert("scale_from_base".into(), json!(true));
        }
    } else {
        match p.scale() {
            Some(s) if (s - o.scale).abs() > 1e-12 => {
                scale.insert("scale".into(), json!(s));
            }
            _ if o.scale_from_base => {
                scale.insert("scale_from_base".into(), json!(false));
            }
            _ => {}
        }
    }
    if !scale.is_empty() {
        out.push(("drawing.view.set_scale", with_view(scale)));
    }
    // Orientation (base views only).
    if !o.projected && p.orientation != o.orientation {
        let mut m = Map::new();
        m.insert("orientation".into(), json!(p.orientation.name()));
        out.push(("drawing.view.set_orientation", with_view(m)));
    }
    // Identifier and label.
    let mut label = Map::new();
    if p.identifier.trim() != o.identifier {
        label.insert("identifier".into(), json!(p.identifier.trim()));
    }
    if p.label_visible != o.label_visible {
        label.insert("label_visible".into(), json!(p.label_visible));
    }
    if !label.is_empty() {
        out.push(("drawing.view.set_label", with_view(label)));
    }
    out
}

/// The part outline every style glyph draws (no new icon, R91).
fn outline(r: egui::Rect, ink: egui::Color32) -> egui::Shape {
    egui::Shape::rect_stroke(r.shrink(r.width() * 0.18), 1.0, egui::Stroke::new(1.5, ink), egui::StrokeKind::Middle)
}

/// Hidden Lines: the outline with a hidden edge dashed across it.
fn glyph_hidden(r: egui::Rect, ink: egui::Color32) -> Vec<egui::Shape> {
    let b = r.shrink(r.width() * 0.18);
    let y = b.center().y;
    let mut v = vec![outline(r, ink)];
    v.extend(egui::Shape::dashed_line(&[egui::pos2(b.left() + 2.0, y), egui::pos2(b.right() - 2.0, y)], egui::Stroke::new(1.0, ink), 2.5, 2.0));
    v
}

/// No Hidden Lines: the outline only.
fn glyph_removed(r: egui::Rect, ink: egui::Color32) -> Vec<egui::Shape> {
    vec![outline(r, ink)]
}

/// Shaded: the outline over a fill.
fn glyph_shaded(r: egui::Rect, ink: egui::Color32) -> Vec<egui::Shape> {
    vec![egui::Shape::rect_filled(r.shrink(r.width() * 0.18), 1.0, ink.gamma_multiply(0.45)), outline(r, ink)]
}

/// The panel's card, drawn over the sheet; Enter or OK applies, Esc or Cancel closes.
pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context, mut p: ViewPanel) {
    p.validate();
    // Enter with the marking menu open over the panel is the menu's (as for feature panels).
    let menu_enter = crate::marking_menu::holds_keys(app) && ctx.input(|i| i.key_pressed(egui::Key::Enter));
    let models = open_models(app);
    let mut enter = false;
    let mut browse = false;
    let title = crate::i18n::tr(if p.model_editable() { "ui.drawing.base_view" } else { "ui.drawing.edit_view" });
    let valid = p.can_ok();
    // Task 6's on-sheet cube reads this; off for a projected view (R87).
    scenario::publish_count("drawing_view:cube:enabled", f64::from(u8::from(p.cube_enabled())));
    let mut preview = p.preview;
    let (closed, action) = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_view"), "camera", title)
        .placed(egui::Align2::LEFT_TOP, crate::browser::panel_pos(app))
        .actions(crate::i18n::tr_label("OK"), crate::i18n::tr("ui.cancel"), None)
        // Esc is the application's (`toolbar::shortcuts`): the marking menu takes it first.
        .keep_open_on_escape()
        .show_with_left(
            ctx,
            valid,
            |ui| {
                let tabs = [crate::i18n::tr("ui.drawing.tab_component"), crate::i18n::tr("ui.drawing.tab_display")];
                let shown = ui.scope(|ui| crate::ui_kit::segmented(ui, &mut p.tab, &tabs, crate::ui_kit::Segments::default()));
                publish_segments(shown.response.rect, &["drawing_view:tab:component", "drawing_view:tab:display"]);
                if p.tab == 0 {
                    component(ui, &mut p, &models, &mut enter, &mut browse);
                } else {
                    crate::ui_kit::row(ui, crate::i18n::tr("ui.drawing.tangent_edges"), |ui| {
                        let r = crate::ui_kit::checkbox(ui, "", &mut p.tangent_edges);
                        scenario::publish_handle("drawing_view:tangent_edges", r.rect.center());
                        // A choice of its own stops following the base view, as the style buttons do.
                        if r.clicked() {
                            p.style_from_base = false;
                        }
                    });
                }
                if let Some(error) = &p.error {
                    crate::ui_kit::text(ui, error, crate::ui_kit::Text::Error("drawing_view:error"));
                }
            },
            |ui| {
                let r = crate::ui_kit::checkbox(ui, crate::i18n::tr_label("Preview"), &mut preview);
                scenario::publish_handle("drawing_view:preview", r.rect.center());
            },
        );
    p.preview = preview;
    if browse {
        app.pick_open_for(&["tcipart", "tciasm"], |app, path| {
            if let (Some(path), Some(DrawingPanel::View(p))) = (path, app.drawing.panel.as_mut()) {
                p.model = Some(std::path::absolute(&path).map_or(path, |a| a.to_string_lossy().into_owned()));
            }
        });
    }
    if closed || action == crate::ui_kit::Action::Cancel {
        app.drawing.panel = None;
        return;
    }
    let ok = (enter && p.can_ok()) || action == crate::ui_kit::Action::Ok;
    if ok && !menu_enter && apply(app, &mut p) {
        app.drawing.panel = None;
        return;
    }
    app.drawing.panel = Some(DrawingPanel::View(p));
}

/// Handles at the centres of equal segments across `rect`, in order.
fn publish_segments(rect: egui::Rect, names: &[&str]) {
    let w = rect.width() / names.len().max(1) as f32;
    for (i, name) in names.iter().enumerate() {
        scenario::publish_handle(name, egui::pos2(rect.left() + w * (i as f32 + 0.5), rect.center().y));
    }
}

/// The Component tab (spec §2.3, the capture's order): Model, model state, Style, Style from Base,
/// View Identifier, Scale, Scale from Base, label visibility.
fn component(ui: &mut egui::Ui, p: &mut ViewPanel, models: &[String], enter: &mut bool, browse: &mut bool) {
    let file_name = |path: &str| std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned());
    // Model.
    let editable = p.model_editable();
    scenario::publish_count("drawing_view:model:editable", f64::from(u8::from(editable)));
    crate::ui_kit::row(ui, crate::i18n::tr("ui.drawing.model"), |ui| {
        let current = p.model.as_deref().map(file_name).unwrap_or_default();
        if editable {
            let r = crate::ui_kit::dropdown_menu(ui, "drawing_view_model", &current, |ui| {
                for path in models {
                    let row = ui.selectable_label(p.model.as_deref() == Some(path.as_str()), file_name(path));
                    scenario::publish_handle(&format!("drawing_view:model_row:{}", file_name(path)), row.rect.center());
                    if row.clicked() {
                        p.model = Some(path.clone());
                    }
                }
                let row = ui.selectable_label(false, crate::i18n::tr("ui.drawing.browse"));
                scenario::publish_handle("drawing_view:model:browse", row.rect.center());
                if row.clicked() {
                    *browse = true;
                }
            });
            scenario::publish_handle("drawing_view:model", r.rect.center());
        } else {
            crate::ui_kit::text(ui, &current, crate::ui_kit::Text::Value("drawing_view:model", false));
        }
    });
    // Model state: read-only, our models have one (spec §2.3).
    crate::ui_kit::row(ui, crate::i18n::tr("ui.drawing.model_state"), |ui| {
        crate::ui_kit::text(ui, crate::i18n::tr("ui.drawing.single_state"), crate::ui_kit::Text::Value("drawing_view:model_state", false));
    });
    // Style: Hidden Line, Hidden Line Removed, Shaded (the captured order); the first two exclusive.
    crate::ui_kit::row(ui, crate::i18n::tr_label("Style"), |ui| {
        let glyphs = [
            crate::ui_kit::GlyphSegment { key: "hidden", tip: crate::i18n::tr("ui.drawing.style_hidden").to_string(), glyph: glyph_hidden },
            crate::ui_kit::GlyphSegment { key: "removed", tip: crate::i18n::tr("ui.drawing.style_removed").to_string(), glyph: glyph_removed },
            crate::ui_kit::GlyphSegment { key: "shaded", tip: crate::i18n::tr("ui.drawing.style_shaded").to_string(), glyph: glyph_shaded },
        ];
        let mut clicked = usize::MAX;
        let shown = ui.scope(|ui| crate::ui_kit::segmented_glyphs(ui, &mut clicked, &glyphs, &[p.hidden_lines, p.removed, p.shaded]));
        publish_segments(shown.response.rect, &["drawing_view:style:hidden", "drawing_view:style:removed", "drawing_view:style:shaded"]);
        if shown.inner {
            match clicked {
                0 => p.set_lines(LineStyle::Hidden),
                1 => p.set_lines(LineStyle::Removed),
                2 => p.shaded = !p.shaded,
                _ => return,
            }
            // A style of its own stops following the base view.
            p.style_from_base = false;
        }
    });
    let follows = p.follows_base_enabled();
    let parent = p.original.as_ref().and_then(|o| o.parent);
    if base_check(ui, "ui.drawing.style_from_base", "drawing_view:style_from_base", follows, &mut p.style_from_base)
        && p.style_from_base
        && let Some((lines, shaded, tangent, _)) = parent
    {
        p.set_lines(lines);
        p.shaded = shaded;
        p.tangent_edges = tangent;
    }
    // View identifier.
    crate::ui_kit::row(ui, crate::i18n::tr("ui.drawing.identifier"), |ui| {
        scenario::publish_handle("drawing_view:identifier", ui.available_rect_before_wrap().center());
        *enter |= crate::ui_kit::text_field(ui, &mut p.identifier, "").0;
    });
    // Scale: an editable combo of the presets (spec §2.6).
    crate::ui_kit::row(ui, crate::i18n::tr("ui.drawing.field_scale"), |ui| {
        ui.add_enabled_ui(!p.scale_from_base, |ui| scale_combo(ui, p, enter));
    });
    if base_check(ui, "ui.drawing.scale_from_base", "drawing_view:scale_from_base", follows, &mut p.scale_from_base)
        && p.scale_from_base
        && let Some((.., scale)) = parent
    {
        p.scale_text = format_scale(scale);
    }
    // Label visibility (off by default [E: DW §6]).
    crate::ui_kit::row(ui, crate::i18n::tr("ui.drawing.label_visible"), |ui| {
        let r = crate::ui_kit::checkbox(ui, "", &mut p.label_visible);
        scenario::publish_handle("drawing_view:label_visible", r.rect.center());
    });
}

/// A "from base" checkbox row, enabled only for a projected view; true when clicked.
fn base_check(ui: &mut egui::Ui, label: &str, handle: &str, enabled: bool, value: &mut bool) -> bool {
    scenario::publish_count(&format!("{handle}:enabled"), f64::from(u8::from(enabled)));
    crate::ui_kit::row(ui, crate::i18n::tr(label), |ui| {
        let r = ui.add_enabled_ui(enabled, |ui| crate::ui_kit::checkbox(ui, "", value)).inner;
        scenario::publish_handle(handle, r.rect.center());
        r.clicked()
    })
}

/// The scale's text field with the preset list beside it.
fn scale_combo(ui: &mut egui::Ui, p: &mut ViewPanel, enter: &mut bool) {
    let t = crate::theme::Tokens::get();
    let full = ui.available_rect_before_wrap();
    let button = t.control_height.min(full.width() * 0.4);
    let field = egui::Rect::from_min_max(full.min, egui::pos2(full.right() - button - t.space_4, full.bottom()));
    let arrow = egui::Rect::from_min_max(egui::pos2(full.right() - button, full.top()), full.max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(field).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        scenario::publish_handle("drawing_view:scale", field.center());
        *enter |= crate::ui_kit::text_field(ui, &mut p.scale_text, "").0;
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(arrow).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        let r = crate::ui_kit::dropdown_menu(ui, "drawing_view_scale", "", |ui| {
            for (name, _) in SCALE_PRESETS {
                let row = ui.selectable_label(p.scale_text == name, name);
                scenario::publish_handle(&format!("drawing_view:scale_row:{name}"), row.rect.center());
                if row.clicked() {
                    p.scale_text = name.to_string();
                }
            }
        });
        scenario::publish_handle("drawing_view:scale:open", r.rect.center());
    });
}

#[cfg(test)]
#[path = "drawing_view_panel_tests.rs"]
mod tests;
