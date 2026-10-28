//! General Dimension and Edit Dimension on the drawing sheet (Plan D-2 Task 8, spec §2.7):
//! Inventor's way, in every profile (SolidCraft's drawings run this same code).
//!
//! - **General Dimension** (D, the ribbon's Dimension). Pick a straight edge, a circle or an arc,
//!   or two things (the second pick follows a straight edge or a polyline); a live dimension then
//!   follows the cursor, and a click places it where the cursor is: one `drawing.dim.general`,
//!   one undo step. The engine chooses the kind by the picks (linear, or aligned for an edge that
//!   is neither horizontal nor vertical; a diameter for a circle, a radius for an arc; the
//!   distance for two picks, horizontal or vertical by the placement). Picks are sent as the
//!   clicked points (`{at}`, R81), a polyline's with `"snap": "vertex"`. The tool goes on for
//!   the next dimension until Esc, Done or Cancel.
//! - **Edit Dimension** (a double-click on a dimension, or its list's Edit Dimension). The panel's
//!   Text tab holds the justification, Hide Dimension Value, the text (`<<>>` stands for the
//!   value) and a symbol row; its Precision tab 0-6 decimals or the style's. OK sends one
//!   `drawing.dim.edit` with only what changed. The footer's "edit dimension when created" opens
//!   the panel on each new dimension.
//! - **Move Dimension** (the list's Move): one click puts the text there
//!   (`drawing.annotation.move`).
//!
//! The live dimension is not a command: it is the command run on a scratch copy of the session
//! (`Session::scratch`), only when the cursor's 0.05 in cell changes (R89), and drawn as preview
//! curves; the document and its undo history are untouched until the click.

use cadcraft_render::Kind;
use egui::{Pos2, Rect, Response};
use serde_json::{Map, Value, json};
use solidcraft_engine::doc::DocumentKind;
use solidcraft_engine::drawing::{CurveKind, PaperCurve, PaperPick, VALUE_TOKEN, pick};
use solidcraft_engine::geom::Vec2;

use crate::SolveApp;
use crate::drawing_ui::{DrawingPanel, DrawingTool};
use crate::navigation::CameraInput;
use crate::scenario;
use crate::sheet_canvas::paper_at;

/// A click picks within this many points of the cursor.
const PICK_PX: f64 = 4.0;
/// The live dimension is computed again only when the cursor leaves its cell (R89), inches.
const CELL_IN: f64 = 0.05;
/// Chord tolerance of the live dimension's curves and text, inches.
const TOLERANCE_IN: f64 = 0.001;
/// The engine's text token for the measured value (DXF's `<>`, shown as `<<>>`).
const DXF_TOKEN: &str = "<>";
/// Horizontal justification as `drawing.dim.edit` names it (DXF textJust 0, 1, 2).
const HORIZONTAL: [&str; 3] = ["center", "first", "second"];
/// Vertical justification as `drawing.dim.edit` names it (DXF textAbove 0, 1, 4).
const VERTICAL: [&str; 3] = ["centered", "above", "below"];
/// The text box's symbol row.
const SYMBOLS: [&str; 3] = ["⌀", "°", "±"];

/// The General Dimension tool's state (`DrawingTool::Dimension`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DimTool {
    /// The view the first pick was on; later picks must be on it too.
    pub view: Option<u64>,
    /// The picks so far: the clicked paper point and the kind of curve under it.
    pub picks: Vec<([f64; 2], CurveKind)>,
    /// The dimensions this run placed, in order.
    pub placed: Vec<u64>,
    /// The parameters of the last `drawing.dim.general` sent.
    pub last_sent: Option<Value>,
    /// Move Dimension: the dimension whose text the next click places (the tool ends after it).
    pub moving: Option<u64>,
    /// The cell (0.05 in steps) and pick count the live dimension was computed for.
    preview_key: Option<([i64; 2], usize)>,
}

/// The Edit Dimension panel (`DrawingPanel::Dimension`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DimPanel {
    pub dim: u64,
    /// 0 Text, 1 Precision.
    pub tab: usize,
    /// What the dimension says, `<<>>` standing for the value.
    pub text: String,
    pub hide_value: bool,
    /// Decimals, or None: the style's.
    pub precision: Option<u8>,
    /// Open this panel on every dimension placed from now on.
    pub edit_when_created: bool,
    /// Horizontal (`HORIZONTAL`) and vertical (`VERTICAL`) justification, as indices.
    pub justify: [usize; 2],
    /// Why OK was refused.
    pub error: Option<String>,
    /// The measured value, for the precision list's samples.
    measurement: f64,
    /// The fields as the panel opened, so OK sends only what changed.
    original: (String, bool, Option<u8>, [usize; 2]),
}

fn tool(app: &SolveApp) -> Option<&DimTool> {
    match &app.drawing.tool {
        Some(DrawingTool::Dimension(t)) => Some(t),
        _ => None,
    }
}

fn tool_mut(app: &mut SolveApp) -> Option<&mut DimTool> {
    match &mut app.drawing.tool {
        Some(DrawingTool::Dimension(t)) => Some(t),
        _ => None,
    }
}

fn cell(p: [f64; 2]) -> [i64; 2] {
    [(p[0] / CELL_IN).round() as i64, (p[1] / CELL_IN).round() as i64]
}

/// The ribbon's Dimension and D (`DRAWING_STARTS`): General Dimension begins.
pub(crate) fn start(app: &mut SolveApp) {
    crate::drawing_ui::end_tool(app);
    if app.session.doc.kind != DocumentKind::Drawing {
        return;
    }
    app.drawing.drag = None;
    app.drawing.selection.clear();
    app.drawing.tool = Some(DrawingTool::Dimension(DimTool::default()));
    app.set_status(crate::i18n::tr("ui.drawing.prompt_dimension").to_string(), false);
}

/// The list's Move Dimension: the next click puts dimension `dim`'s text there.
fn start_move(app: &mut SolveApp, dim: u64) {
    crate::drawing_ui::end_tool(app);
    app.drawing.drag = None;
    app.drawing.tool = Some(DrawingTool::Dimension(DimTool { moving: Some(dim), ..DimTool::default() }));
    app.set_status(crate::i18n::tr("ui.drawing.prompt_move_dim").to_string(), false);
}

/// End the tool (Esc, Done, Cancel): what it placed stays; picks and the live dimension go.
pub(crate) fn finish(app: &mut SolveApp) {
    if tool(app).is_some() {
        app.drawing.tool = None;
    }
    app.drawing.preview.clear();
    app.drawing.selection.clear();
}

/// What the status line asks for while the tool runs (a catalog key).
pub(crate) fn prompt(app: &SolveApp) -> Option<&'static str> {
    tool(app).map(|t| if t.moving.is_some() { "ui.drawing.prompt_move_dim" } else { "ui.drawing.prompt_dimension" })
}

/// A click's pick tolerance in inches.
fn tolerance(app: &SolveApp, rect: Rect) -> f64 {
    PICK_PX * 2.0 * app.cam.half_height() / f64::from(rect.height().max(1.0))
}

/// The dimension under screen point `p`, if that is what is under it.
pub(crate) fn dimension_at(app: &SolveApp, rect: Rect, p: Pos2) -> Option<u64> {
    match pick(&app.session, paper_at(app, rect, p), tolerance(app, rect)) {
        Some(PaperPick::Dimension { handle }) => Some(handle),
        _ => None,
    }
}

/// The curve a click at paper point `at` would pick now, pre-highlighted under the cursor (fix
/// wave 2): an edge, circle or arc first; then, after a straight edge or a vertex, another curve
/// of the same view. None while the next click places the dimension, or moves one.
pub(crate) fn hover_pick(app: &SolveApp, rect: Rect, at: [f64; 2]) -> Option<PaperPick> {
    let t = tool(app).filter(|t| t.moving.is_none())?;
    let hit = pick(&app.session, at, tolerance(app, rect))?;
    let PaperPick::ViewEdge { view, .. } = &hit else { return None };
    let takes = match t.picks.as_slice() {
        [] => true,
        [(_, CurveKind::Line | CurveKind::Poly)] => t.view == Some(*view),
        _ => false,
    };
    takes.then_some(hit)
}

/// The sheet's pointer input while the tool runs, and a double-click on a dimension with no tool
/// running (Edit Dimension). True when it used the frame's click.
pub(crate) fn input(app: &mut SolveApp, rect: Rect, resp: &Response, input: &CameraInput, navigating: bool) -> bool {
    let hover = input.hover.filter(|_| input.inside);
    let clicked = resp.clicked() && !navigating;
    let Some(moving) = tool(app).map(|t| t.moving) else {
        if resp.double_clicked()
            && !navigating
            && app.drawing.tool.is_none()
            && let Some(dim) = hover.and_then(|p| dimension_at(app, rect, p))
        {
            open_panel(app, dim);
            return true;
        }
        return false;
    };
    // The panel over the tool (edit dimension when created) has the user's attention.
    if app.drawing.panel.is_some() {
        return true;
    }
    let Some(p) = hover else { return true };
    let at = paper_at(app, rect, p);
    if let Some(dim) = moving {
        if clicked {
            app.drawing.preview.clear();
            if let Err(e) = app.run("drawing.annotation.move", json!({ "handle": dim, "placement": at })) {
                app.set_status(e, true);
            }
            finish(app);
        } else {
            refresh(app, at, |s| s.execute("drawing.annotation.move", &json!({ "handle": dim, "placement": at })).ok().map(|_| dim));
        }
        return true;
    }
    if clicked {
        click(app, rect, at);
    }
    let params = tool(app).and_then(|t| ready(t).then(|| params(t, at)));
    match params {
        Some(params) => refresh(app, at, |s| s.execute("drawing.dim.general", &params).ok().and_then(|r| r["handle"].as_u64())),
        None => clear_preview(app),
    }
    true
}

/// Whether the picks make a dimension with one more click: one straight edge, circle or arc, or
/// two picks.
fn ready(t: &DimTool) -> bool {
    matches!(t.picks.as_slice(), [(_, CurveKind::Line | CurveKind::Circle | CurveKind::Arc)] | [_, _])
}

/// `drawing.dim.general`'s parameters for the picks and a placement (R81).
fn params(t: &DimTool, placement: [f64; 2]) -> Value {
    let picks: Vec<Value> =
        t.picks.iter().map(|(at, kind)| if *kind == CurveKind::Poly { json!({ "at": at, "snap": "vertex" }) } else { json!({ "at": at }) }).collect();
    json!({ "view": t.view, "picks": picks, "placement": placement })
}

/// A click at paper point `at`: a pick, or the placement.
fn click(app: &mut SolveApp, rect: Rect, at: [f64; 2]) {
    let hit = pick(&app.session, at, tolerance(app, rect));
    let edge = match &hit {
        Some(PaperPick::ViewEdge { view, curve, .. }) => Some((*view, *curve)),
        _ => None,
    };
    let Some(t) = tool_mut(app) else { return };
    let same_view = edge.is_some_and(|(view, _)| t.view == Some(view));
    let pick_it = match t.picks.as_slice() {
        [] => edge.is_some(),
        // A straight edge or a polyline's vertex can be measured to a second pick.
        [(_, CurveKind::Line | CurveKind::Poly)] => same_view,
        _ => false,
    };
    if pick_it {
        if let Some((view, curve)) = edge {
            t.view = Some(view);
            t.picks.push((at, curve));
            t.preview_key = None;
        }
        if let Some(hit) = hit {
            app.drawing.selection.push(hit);
        }
        return;
    }
    if ready(t) {
        place(app, at);
    }
}

/// Place the dimension the picks make at `at`: one `drawing.dim.general`.
fn place(app: &mut SolveApp, at: [f64; 2]) {
    let Some(params) = tool(app).map(|t| params(t, at)) else { return };
    let result = app.run("drawing.dim.general", params.clone());
    app.drawing.preview.clear();
    app.drawing.selection.clear();
    let handle = match result {
        Ok(r) => r["handle"].as_u64(),
        Err(e) => {
            app.set_status(e, true);
            None
        }
    };
    if let Some(t) = tool_mut(app) {
        t.view = None;
        t.picks.clear();
        t.preview_key = None;
        t.last_sent = Some(params);
        t.placed.extend(handle);
    }
    if let Some(dim) = handle
        && edit_when_created(app)
    {
        open_panel(app, dim);
    }
}

/// The live dimension at `at`: `make` runs the command on a scratch copy of the session and
/// names the dimension it made or moved; its curves become the preview. Computed again only when
/// the cell or the picks change (R89).
fn refresh(app: &mut SolveApp, at: [f64; 2], make: impl FnOnce(&mut solidcraft_engine::Session) -> Option<u64>) {
    let Some(t) = tool(app) else { return };
    let key = (cell(at), t.picks.len());
    if t.preview_key == Some(key) {
        return;
    }
    let mut scratch = app.session.scratch();
    let curves = make(&mut scratch).map(|h| curves_of(&scratch, h)).unwrap_or_default();
    app.drawing.preview = curves;
    if let Some(t) = tool_mut(app) {
        t.preview_key = Some(key);
    }
}

fn clear_preview(app: &mut SolveApp) {
    if !app.drawing.preview.is_empty() {
        app.drawing.preview.clear();
    }
    if let Some(t) = tool_mut(app) {
        t.preview_key = None;
    }
}

/// Dimension `handle` of session `s`'s drawing as preview curves: its lines and text strokes,
/// and its arrowheads outlined.
fn curves_of(s: &solidcraft_engine::Session, handle: u64) -> Vec<(PaperCurve, bool)> {
    let Some(m) = s.doc.drawing.as_deref() else { return Vec::new() };
    let Some(e) = m.paper.entity(cadcraft_doc::Handle(handle)) else { return Vec::new() };
    let opts = cadcraft_render::Options { tolerance: TOLERANCE_IN, ..Default::default() };
    let list = cadcraft_render::build_entities(&m.paper, [&**e], &opts);
    let mut out = Vec::new();
    for p in &list.prims {
        let pts: Vec<Vec2> = list.points(p).iter().map(|v| Vec2::new(v.x, v.y)).collect();
        match p.kind {
            Kind::Polyline if pts.len() >= 2 => out.push((PaperCurve::Poly { points: pts }, false)),
            Kind::Tris => {
                for t in pts.as_chunks::<3>().0 {
                    out.push((PaperCurve::Poly { points: vec![t[0], t[1], t[2], t[0]] }, false));
                }
            }
            _ => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Edit Dimension
// ---------------------------------------------------------------------------------------------

/// Open Edit Dimension on dimension `dim`, filled from what it has now.
pub(crate) fn open_panel(app: &mut SolveApp, dim: u64) {
    let Some(m) = app.session.doc.drawing.as_deref() else { return };
    let Some(cadcraft_doc::EntityKind::Dimension(d)) = m.paper.entity(cadcraft_doc::Handle(dim)).map(|e| &e.kind) else { return };
    let stored = d.text.as_str();
    let (text, hide_value) = if stored.is_empty() {
        (VALUE_TOKEN.to_string(), false)
    } else if stored.contains(DXF_TOKEN) {
        (stored.replace(DXF_TOKEN, VALUE_TOKEN), false)
    } else {
        (stored.trim().to_string(), true)
    };
    let int = |key: &str| d.overrides.get(key).and_then(Value::as_u64);
    let precision = int("decimals").and_then(|n| u8::try_from(n).ok()).filter(|n| *n <= 6);
    let horizontal = int("textJust").map_or(0, |n| usize::try_from(n).unwrap_or(0).min(2));
    let vertical = match int("textAbove") {
        Some(1) => 1,
        Some(4) => 2,
        _ => 0,
    };
    let justify = [horizontal, vertical];
    let panel = DimPanel {
        dim,
        tab: 0,
        text: text.clone(),
        hide_value,
        precision,
        edit_when_created: edit_when_created(app),
        justify,
        error: None,
        measurement: d.measurement,
        original: (text, hide_value, precision, justify),
    };
    app.drawing.panel = Some(DrawingPanel::Dimension(panel));
}

/// `drawing.dim.edit`'s parameters for what changed, or None when nothing did.
pub(crate) fn edits(p: &DimPanel) -> Option<Value> {
    let (text, hide_value, precision, justify) = &p.original;
    let mut out = Map::new();
    if p.text != *text {
        out.insert("text".into(), json!(p.text));
    }
    if p.hide_value != *hide_value {
        out.insert("hide_value".into(), json!(p.hide_value));
    }
    if p.precision != *precision {
        // None: the style's precision again (`precision: null` clears the dimension's own).
        out.insert("precision".into(), json!(p.precision));
    }
    if p.justify != *justify {
        let name = |names: &[&'static str; 3], i: usize| names.get(i).copied().unwrap_or(names[0]);
        out.insert("justify".into(), json!({ "horizontal": name(&HORIZONTAL, p.justify[0]), "vertical": name(&VERTICAL, p.justify[1]) }));
    }
    if out.is_empty() {
        return None;
    }
    out.insert("dim".into(), json!(p.dim));
    Some(Value::Object(out))
}

/// "Edit dimension when created", saved per profile (`Prefs::dim_edit_when_created`).
pub(crate) fn edit_when_created(app: &SolveApp) -> bool {
    app.preferences.dim_edit_when_created.get(&crate::profile::active(app).id).copied().unwrap_or(false)
}

pub(crate) fn set_edit_when_created(app: &mut SolveApp, on: bool) {
    if edit_when_created(app) != on {
        let profile = crate::profile::active(app).id.clone();
        app.preferences.dim_edit_when_created.insert(profile, on);
    }
}

/// OK: one `drawing.dim.edit` with what changed (nothing when nothing did). False when refused.
pub(crate) fn apply(app: &mut SolveApp, p: &mut DimPanel) -> bool {
    set_edit_when_created(app, p.edit_when_created);
    let Some(params) = edits(p) else { return true };
    match app.run("drawing.dim.edit", params) {
        Ok(_) => true,
        Err(e) => {
            p.error = Some(e);
            false
        }
    }
}

/// Hide Dimension Value turned on or off: the value's token leaves or comes back to the text.
fn set_hide_value(p: &mut DimPanel) {
    if p.hide_value {
        p.text = p.text.replace(VALUE_TOKEN, "").trim().to_string();
    } else if !p.text.contains(VALUE_TOKEN) {
        let rest = p.text.trim();
        p.text = if rest.is_empty() { VALUE_TOKEN.to_string() } else { format!("{VALUE_TOKEN} {rest}") };
    }
}

/// Handles at the centres of equal segments across `rect`, in order.
fn publish_segments(rect: Rect, names: &[String]) {
    let w = rect.width() / names.len().max(1) as f32;
    for (i, name) in names.iter().enumerate() {
        scenario::publish_handle(name, egui::pos2(rect.left() + w * (i as f32 + 0.5), rect.center().y));
    }
}

/// The panel's card, over the sheet.
pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context, mut p: DimPanel) {
    let tr = crate::i18n::tr;
    let menu_enter = crate::marking_menu::holds_keys(app) && ctx.input(|i| i.key_pressed(egui::Key::Enter));
    let mut enter = false;
    let mut edit_when_created = p.edit_when_created;
    let (closed, action) = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_dim"), "edit", tr("ui.drawing.edit_dim"))
        .placed(egui::Align2::LEFT_TOP, crate::browser::panel_pos(app))
        .actions(crate::i18n::tr_label("OK"), tr("ui.cancel"), None)
        // Esc is the application's (`toolbar::shortcuts`): the marking menu takes it first.
        .keep_open_on_escape()
        .show_with_left(
            ctx,
            true,
            |ui| {
                let tabs = [tr("ui.drawing.tab_text"), tr("ui.drawing.precision")];
                let shown = ui.scope(|ui| crate::ui_kit::segmented(ui, &mut p.tab, &tabs, crate::ui_kit::Segments::default()));
                publish_segments(shown.response.rect, &["drawing_dim:tab:text".into(), "drawing_dim:tab:precision".into()]);
                if p.tab == 0 {
                    text_tab(ui, &mut p, &mut enter);
                } else {
                    precision_tab(ui, &mut p);
                }
                if let Some(error) = &p.error {
                    crate::ui_kit::text(ui, error, crate::ui_kit::Text::Error("drawing_dim:error"));
                }
            },
            |ui| {
                let r = crate::ui_kit::checkbox(ui, tr("ui.drawing.edit_when_created"), &mut edit_when_created);
                scenario::publish_handle("drawing_dim:edit_when_created", r.rect.center());
            },
        );
    p.edit_when_created = edit_when_created;
    set_edit_when_created(app, edit_when_created);
    if closed || action == crate::ui_kit::Action::Cancel {
        app.drawing.panel = None;
        return;
    }
    let ok = enter || action == crate::ui_kit::Action::Ok;
    if ok && !menu_enter && apply(app, &mut p) {
        app.drawing.panel = None;
        return;
    }
    app.drawing.panel = Some(DrawingPanel::Dimension(p));
}

/// The Text tab: justification, Hide Dimension Value, the text and its symbol row.
fn text_tab(ui: &mut egui::Ui, p: &mut DimPanel, enter: &mut bool) {
    let tr = crate::i18n::tr;
    for (axis, label, keys) in [
        (0, "ui.drawing.justify_h", ["ui.drawing.just_center", "ui.drawing.just_first", "ui.drawing.just_second"]),
        (1, "ui.drawing.justify_v", ["ui.drawing.just_centered", "ui.drawing.just_above", "ui.drawing.just_below"]),
    ] {
        crate::ui_kit::row(ui, tr(label), |ui| {
            let labels = keys.map(tr);
            let shown = ui.scope(|ui| crate::ui_kit::segmented(ui, &mut p.justify[axis], &labels, crate::ui_kit::Segments::default()));
            let words = if axis == 0 { HORIZONTAL } else { VERTICAL };
            let handles: Vec<String> = words.iter().map(|w| format!("drawing_dim:justify:{w}")).collect();
            publish_segments(shown.response.rect, &handles);
        });
    }
    let r = crate::ui_kit::checkbox(ui, tr("ui.drawing.hide_value"), &mut p.hide_value);
    scenario::publish_handle("drawing_dim:hide_value", r.rect.center());
    if r.changed() {
        set_hide_value(p);
    }
    crate::ui_kit::row(ui, tr("ui.drawing.tab_text"), |ui| {
        scenario::publish_handle("drawing_dim:text", ui.available_rect_before_wrap().center());
        *enter |= crate::ui_kit::text_field(ui, &mut p.text, "").0;
    });
    crate::ui_kit::row(ui, tr("ui.drawing.symbols"), |ui| {
        ui.horizontal(|ui| {
            for symbol in SYMBOLS {
                let r = crate::ui_kit::button(ui, symbol, crate::ui_kit::Button::Small);
                scenario::publish_handle(&format!("drawing_dim:symbol:{symbol}"), r.rect.center());
                if r.clicked() {
                    p.text.push_str(symbol);
                }
            }
        });
    });
}

/// The Precision tab: the style's precision, or 0 to 6 decimals (each row shows the value so).
fn precision_tab(ui: &mut egui::Ui, p: &mut DimPanel) {
    let style = crate::ui_kit::list_picker(ui, "drawing_dim:precision:style", crate::i18n::tr("ui.drawing.precision_style"), p.precision.is_none());
    scenario::publish_handle("drawing_dim:precision:style", style.rect.center());
    if style.clicked() {
        p.precision = None;
    }
    for n in 0..=6u8 {
        let sample = format!("{:.*}", usize::from(n), p.measurement);
        let row = crate::ui_kit::list_picker(ui, &format!("drawing_dim:precision:{n}"), &sample, p.precision == Some(n));
        scenario::publish_handle(&format!("drawing_dim:precision:{n}"), row.rect.center());
        if row.clicked() {
            p.precision = Some(n);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The dimension's list
// ---------------------------------------------------------------------------------------------

/// A row of a dimension's right-click list (`context_menu::run_item`'s `ui.drawing.*` arms).
pub(crate) fn run_menu(app: &mut SolveApp, id: &str, p: &Value) {
    let Some(dim) = p.get("dim").and_then(Value::as_u64) else { return };
    let edit = |app: &mut SolveApp, params: Value| {
        if let Err(e) = app.run("drawing.dim.edit", params) {
            app.set_status(e, true);
        }
    };
    match id {
        "ui.drawing.moveDim" => start_move(app, dim),
        "ui.drawing.editDim" => open_panel(app, dim),
        "ui.drawing.precision" => {
            if let Some(n) = p.get("precision").and_then(Value::as_u64) {
                edit(app, json!({ "dim": dim, "precision": n }));
            }
        }
        "ui.drawing.hideExt" => edit(app, json!({ "dim": dim, "ext_lines": { "first": false, "second": false } })),
        "ui.drawing.showExt" => edit(app, json!({ "dim": dim, "ext_lines": { "first": true, "second": true } })),
        _ => {}
    }
}

#[cfg(test)]
#[path = "drawing_dim_tests.rs"]
mod tests;
