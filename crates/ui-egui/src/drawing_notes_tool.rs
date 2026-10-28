//! Text, Leader Text, Center Mark, Centerline Bisector and Hole and Thread Note on the drawing
//! sheet (Plan D-2 Task 9, spec §2.8): Inventor's way, in every profile (SolidCraft's drawings run
//! this same code, reached through its DRAWING tab's rows).
//!
//! - **Text** (T). A click, or a press-drag that sets the two corners of a box, then the text
//!   panel; OK runs one `drawing.note.text`. The tool goes on for the next note.
//! - **Leader Text** (LE). The first click is the arrow end (attached to the view edge under it,
//!   `{view, edge: {at}}`, so the arrow follows that edge), later clicks add bends; Continue
//!   (Enter, or the list's `ui.done`) opens the text panel, whose OK runs one
//!   `drawing.note.leader`. The right-click list adds Back (drop the last point) and
//!   Single-Segment Leader (the second click ends each leader).
//! - **Center Mark** (CM). Each circle or arc clicked gets one `drawing.centermark`; the tool goes
//!   on until Esc.
//! - **Centerline Bisector** (CB). Two straight edges of one view, one
//!   `drawing.centerline.bisector`.
//! - **Hole and Thread Note** (HN). A hole's edge, then a click where the note goes, one
//!   `drawing.note.hole`.
//! - **Edit Note**. A double-click on a note with no tool running opens the text panel on it; OK
//!   sends one `drawing.note.edit` with only what changed.
//!
//! The text panel (R65, R82, R83): the text, its height typed with units (inches when bare), and
//! left / center / right. A text note starts left; a leader's text starts with none picked and
//! sends none, so the engine's by-direction default holds. Bold, italic, underline and the
//! parameter row are shown disabled and never sent. No font, colour, rotation, preset or
//! arrowhead choice (owner defaults, spec §8.4, §8.7).
//!
//! Rubber bands are preview curves, not commands: nothing reaches the document or its undo
//! history before the placing click or OK, and each placed annotation is one undo step.

use egui::{Pos2, Rect, Response};
use serde_json::{Map, Value, json};
use solidcraft_engine::doc::DocumentKind;
use solidcraft_engine::drawing::{CurveKind, NOTE_HEIGHT_IN, PaperCurve, PaperPick, note_at, pick};
use solidcraft_engine::geom::Vec2;

use crate::SolveApp;
use crate::drawing_ui::{DrawingPanel, DrawingTool};
use crate::navigation::CameraInput;
use crate::scenario;
use crate::sheet_canvas::paper_at;

/// A click picks within this many points of the cursor.
const PICK_PX: f64 = 4.0;
/// The tallest note the engine takes, inches.
const MAX_HEIGHT_IN: f64 = 10.0;
/// The parameter row's precision as the capture shows it (`3.123`, DW §4); never sent (R65).
const PARAMETER_PRECISION: usize = 3;

/// A running annotation tool (`DrawingTool::Note`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NoteTool {
    /// Text: where the note starts (the press, or the click), and the box's other corner when
    /// dragged. The panel is open while `position` is set.
    Text { position: Option<[f64; 2]>, corner: Option<[f64; 2]> },
    /// Leader Text: the arrow end first, then the bends; the view and click the arrow is
    /// attached at; Single-Segment Leader.
    Leader { points: Vec<[f64; 2]>, attach: Option<(u64, [f64; 2])>, single: bool },
    /// Center Mark: the view of the last mark placed.
    CenterMark { view: Option<u64> },
    /// Centerline Bisector: the first straight edge (its view and the click).
    Bisector { first: Option<(u64, [f64; 2])> },
    /// Hole and Thread Note: the hole's edge (its view and the click).
    HoleNote { edge: Option<(u64, [f64; 2])> },
}

/// Text justification, sent as `left`, `center` or `right`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Justify {
    Left,
    Center,
    Right,
}

impl Justify {
    const ALL: [Justify; 3] = [Justify::Left, Justify::Center, Justify::Right];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Justify::Left => "left",
            Justify::Center => "center",
            Justify::Right => "right",
        }
    }

    fn parse(name: &str) -> Option<Justify> {
        Justify::ALL.into_iter().find(|j| j.name() == name)
    }

    fn index(self) -> usize {
        match self {
            Justify::Left => 0,
            Justify::Center => 1,
            Justify::Right => 2,
        }
    }
}

/// The text panel (`DrawingPanel::Text`): a new text or leader note's text, or Edit Note's.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextPanel {
    pub text: String,
    /// The height as typed (`0.12 in`, `3 mm`; a bare number is inches).
    pub height_text: String,
    /// The height in inches, from `height_text` while it reads as one.
    pub height: f64,
    /// None: none picked (a leader's text, R83).
    pub justify: Option<Justify>,
    /// Edit Note: the note's handle (its text's, or its leader's).
    pub edit: Option<u64>,
    /// A hole note's text comes from its hole: the text box is read-only.
    pub hole: bool,
    /// Why OK was refused.
    pub error: Option<String>,
    /// Edit Note: the fields as the panel opened, so OK sends only what changed.
    original: Option<(String, f64, Option<Justify>)>,
    /// The text box takes the keyboard on the panel's first frame.
    focus: bool,
}

impl TextPanel {
    fn new(justify: Option<Justify>) -> TextPanel {
        TextPanel {
            text: String::new(),
            height_text: format_height(NOTE_HEIGHT_IN),
            height: NOTE_HEIGHT_IN,
            justify,
            edit: None,
            hole: false,
            error: None,
            original: None,
            focus: true,
        }
    }

    /// Whether OK would be taken: some text (or a hole note's) and a height that reads.
    pub(crate) fn can_ok(&self) -> bool {
        (self.hole || !self.text.trim().is_empty()) && parse_height(&self.height_text).is_some()
    }
}

/// `0.12 in`.
fn format_height(inches: f64) -> String {
    let s = format!("{inches:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s} in")
}

/// The panel's height in inches: Edit Note's own height while the box still shows it as it
/// opened (the box rounds it, so reading the box back would be a change nobody made).
fn typed_height(p: &TextPanel) -> Option<f64> {
    match &p.original {
        Some((_, height, _)) if p.height_text.trim() == format_height(*height) => Some(*height),
        _ => parse_height(&p.height_text),
    }
}

/// A typed height in inches: `0.25 in`, `3 mm`, or a bare number of inches; above 0, at most 10.
pub(crate) fn parse_height(text: &str) -> Option<f64> {
    let mm = crate::prefs::parse_length_mm(text, 1.0 / 25.4)?;
    let inches = mm / 25.4;
    (inches > 0.0 && inches <= MAX_HEIGHT_IN).then_some(inches)
}

fn tool(app: &SolveApp) -> Option<&NoteTool> {
    match &app.drawing.tool {
        Some(DrawingTool::Note(t)) => Some(t),
        _ => None,
    }
}

fn tool_mut(app: &mut SolveApp) -> Option<&mut NoteTool> {
    match &mut app.drawing.tool {
        Some(DrawingTool::Note(t)) => Some(t),
        _ => None,
    }
}

fn begin(app: &mut SolveApp, t: NoteTool) {
    crate::drawing_ui::end_tool(app);
    if app.session.doc.kind != DocumentKind::Drawing {
        return;
    }
    app.drawing.drag = None;
    app.drawing.selection.clear();
    app.drawing.tool = Some(DrawingTool::Note(t));
    if let Some(key) = prompt(app) {
        app.set_status(crate::i18n::tr(key).to_string(), false);
    }
}

/// Text (T, the ribbon's Text).
pub(crate) fn start_text(app: &mut SolveApp) {
    begin(app, NoteTool::Text { position: None, corner: None });
}

/// Leader Text (LE).
pub(crate) fn start_leader(app: &mut SolveApp) {
    begin(app, NoteTool::Leader { points: Vec::new(), attach: None, single: false });
}

/// Hole and Thread Note (HN).
pub(crate) fn start_hole_note(app: &mut SolveApp) {
    begin(app, NoteTool::HoleNote { edge: None });
}

/// Center Mark (CM).
pub(crate) fn start_center_mark(app: &mut SolveApp) {
    begin(app, NoteTool::CenterMark { view: None });
}

/// Centerline Bisector (CB).
pub(crate) fn start_bisector(app: &mut SolveApp) {
    begin(app, NoteTool::Bisector { first: None });
}

/// End the tool (Esc, Done, Cancel): what it placed stays; its picks, rubber band and an open
/// text panel for a note not yet placed go.
pub(crate) fn finish(app: &mut SolveApp) {
    if let Some(key) = prompt(app) {
        // The tool's prompt leaves the status line with it.
        let shown = crate::i18n::tr(key).to_string();
        if app.status.as_ref().is_some_and(|(text, _, _)| *text == shown) {
            app.status = None;
        }
        app.drawing.tool = None;
        if matches!(&app.drawing.panel, Some(DrawingPanel::Text(p)) if p.edit.is_none()) {
            app.drawing.panel = None;
        }
    }
    app.drawing.preview.clear();
    app.drawing.selection.clear();
}

/// What the status line asks for while the tool runs (a catalog key).
pub(crate) fn prompt(app: &SolveApp) -> Option<&'static str> {
    tool(app).map(|t| match t {
        NoteTool::Text { .. } => "ui.drawing.prompt_text",
        NoteTool::Leader { .. } => "ui.drawing.prompt_leader",
        NoteTool::CenterMark { .. } => "ui.drawing.prompt_centermark",
        NoteTool::Bisector { .. } => "ui.drawing.prompt_bisector",
        NoteTool::HoleNote { .. } => "ui.drawing.prompt_hole",
    })
}

/// Whether Leader Text runs (its list and `ui.done` are Continue's).
pub(crate) fn leader_running(app: &SolveApp) -> bool {
    matches!(tool(app), Some(NoteTool::Leader { .. }))
}

/// The leader's points so far (the list's Back).
fn leader_points(app: &SolveApp) -> usize {
    match tool(app) {
        Some(NoteTool::Leader { points, .. }) => points.len(),
        _ => 0,
    }
}

/// A click's pick tolerance in inches.
fn tolerance(app: &SolveApp, rect: Rect) -> f64 {
    PICK_PX * 2.0 * app.cam.half_height() / f64::from(rect.height().max(1.0))
}

/// The view edge under paper point `at`: its view and curve kind.
fn edge_at(app: &SolveApp, rect: Rect, at: [f64; 2]) -> Option<(u64, CurveKind, PaperPick)> {
    match pick(&app.session, at, tolerance(app, rect)) {
        Some(p @ PaperPick::ViewEdge { view, curve, .. }) => Some((view, curve, p)),
        _ => None,
    }
}

/// The note under screen point `p` (a text, leader or hole note), if that is what is under it.
pub(crate) fn note_under(app: &SolveApp, rect: Rect, p: Pos2) -> Option<u64> {
    let Some(PaperPick::Annotation { handle }) = pick(&app.session, paper_at(app, rect, p), tolerance(app, rect)) else { return None };
    app.session.doc.drawing.as_deref().and_then(|m| note_at(m, handle)).map(|_| handle)
}

/// The sheet's pointer input while an annotation tool runs, and a double-click on a note with
/// no tool running (Edit Note). True when it used the frame's pointer input.
pub(crate) fn input(app: &mut SolveApp, rect: Rect, resp: &Response, input: &CameraInput, navigating: bool) -> bool {
    let hover = input.hover.filter(|_| input.inside);
    if tool(app).is_none() {
        if resp.double_clicked()
            && !navigating
            && app.drawing.tool.is_none()
            && let Some(note) = hover.and_then(|p| note_under(app, rect, p))
        {
            open_edit(app, note);
            return true;
        }
        return false;
    }
    // The text panel has the user's attention.
    if app.drawing.panel.is_some() {
        return true;
    }
    let clicked = resp.clicked() && !navigating;
    if leader_running(app) && !crate::marking_menu::holds_keys(app) && resp.ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
        continue_leader(app);
        return true;
    }
    if matches!(tool(app), Some(NoteTool::Text { .. })) && text_drag(app, rect, resp, navigating) {
        return true;
    }
    let Some(p) = hover else { return true };
    let at = paper_at(app, rect, p);
    if clicked {
        click(app, rect, at);
    }
    rubber_band(app, at);
    true
}

/// Text's press-drag: the press is where the note starts, the release the box's other corner.
fn text_drag(app: &mut SolveApp, rect: Rect, resp: &Response, navigating: bool) -> bool {
    if resp.drag_started_by(egui::PointerButton::Primary)
        && !navigating
        && let Some(origin) = resp.ctx.input(|i| i.pointer.press_origin())
    {
        let at = paper_at(app, rect, origin);
        if let Some(NoteTool::Text { position, corner }) = tool_mut(app) {
            *position = Some(at);
            *corner = Some(at);
        }
    }
    let Some(NoteTool::Text { position: Some(from), corner: Some(_) }) = tool(app).cloned() else { return false };
    let latest = resp.ctx.input(|i| i.pointer.latest_pos()).map(|p| paper_at(app, rect, p));
    if let Some(to) = latest
        && let Some(NoteTool::Text { corner, .. }) = tool_mut(app)
    {
        *corner = Some(to);
    }
    let to = latest.unwrap_or(from);
    app.drawing.preview = vec![(poly(&[from, [to[0], from[1]], to, [from[0], to[1]], from]), false)];
    if resp.drag_stopped() {
        app.drawing.preview.clear();
        open_new(app, Some(Justify::Left));
    }
    true
}

fn poly(points: &[[f64; 2]]) -> PaperCurve {
    PaperCurve::Poly { points: points.iter().map(|p| Vec2::new(p[0], p[1])).collect() }
}

/// The rubber band from the tool's last point to the cursor (a leader's points, a hole note's
/// edge to its text).
fn rubber_band(app: &mut SolveApp, at: [f64; 2]) {
    let points: Vec<[f64; 2]> = match tool(app) {
        Some(NoteTool::Leader { points, .. }) if !points.is_empty() => points.iter().copied().chain([at]).collect(),
        Some(NoteTool::HoleNote { edge: Some((_, from)) }) => vec![*from, at],
        _ => Vec::new(),
    };
    let preview = if points.is_empty() { Vec::new() } else { vec![(poly(&points), false)] };
    if app.drawing.preview != preview {
        app.drawing.preview = preview;
    }
}

/// A click at paper point `at` for the running tool.
fn click(app: &mut SolveApp, rect: Rect, at: [f64; 2]) {
    let Some(t) = tool(app).cloned() else { return };
    let edge = edge_at(app, rect, at);
    match t {
        NoteTool::Text { .. } => {
            if let Some(NoteTool::Text { position, corner }) = tool_mut(app) {
                *position = Some(at);
                *corner = None;
            }
            open_new(app, Some(Justify::Left));
        }
        NoteTool::Leader { .. } => {
            let mut done = false;
            if let Some(NoteTool::Leader { points, attach, single }) = tool_mut(app) {
                if points.is_empty() {
                    *attach = edge.as_ref().map(|(view, ..)| (*view, at));
                }
                points.push(at);
                done = *single && points.len() >= 2;
            }
            if let Some((.., hit)) = edge.filter(|_| leader_points(app) == 1) {
                app.drawing.selection = vec![hit];
            }
            if done {
                continue_leader(app);
            }
        }
        NoteTool::CenterMark { .. } => {
            let Some((view, CurveKind::Circle | CurveKind::Arc, _)) = edge else { return };
            send(app, "drawing.centermark", json!({ "view": view, "edge": { "at": at } }));
            if let Some(NoteTool::CenterMark { view: last }) = tool_mut(app) {
                *last = Some(view);
            }
        }
        NoteTool::Bisector { first } => {
            let Some((view, CurveKind::Line, hit)) = edge else { return };
            match first {
                Some((v, a)) if v == view => {
                    send(app, "drawing.centerline.bisector", json!({ "view": view, "a": { "at": a }, "b": { "at": at } }));
                    app.drawing.selection.clear();
                    if let Some(NoteTool::Bisector { first }) = tool_mut(app) {
                        *first = None;
                    }
                }
                _ => {
                    app.drawing.selection = vec![hit];
                    if let Some(NoteTool::Bisector { first }) = tool_mut(app) {
                        *first = Some((view, at));
                    }
                }
            }
        }
        NoteTool::HoleNote { edge: Some((view, from)) } => {
            send(app, "drawing.note.hole", json!({ "view": view, "edge": { "at": from }, "placement": at }));
            app.drawing.selection.clear();
            app.drawing.preview.clear();
            if let Some(NoteTool::HoleNote { edge }) = tool_mut(app) {
                *edge = None;
            }
        }
        NoteTool::HoleNote { edge: None } => {
            let Some((view, _, hit)) = edge else { return };
            // Only a hole's edge is taken: the note tried on a scratch copy says so first.
            let probe = json!({ "view": view, "edge": { "at": at }, "placement": at });
            if let Err(e) = app.session.scratch().execute("drawing.note.hole", &probe) {
                app.set_status(e.to_string(), true);
                return;
            }
            app.drawing.selection = vec![hit];
            if let Some(NoteTool::HoleNote { edge }) = tool_mut(app) {
                *edge = Some((view, at));
            }
        }
    }
}

/// Run one annotation command (one undo step); a refusal shows on the status line.
fn send(app: &mut SolveApp, cmd: &'static str, params: Value) -> Option<Value> {
    let result = app.run(cmd, params.clone());
    app.drawing.notes_last_sent = Some((cmd, params));
    match result {
        Ok(r) => Some(r),
        Err(e) => {
            app.set_status(e, true);
            None
        }
    }
}

/// Continue (Enter, the list's `ui.done`, a single segment's second click): the leader's text
/// panel opens once the leader has two points.
pub(crate) fn continue_leader(app: &mut SolveApp) {
    if app.drawing.panel.is_none() && leader_points(app) >= 2 {
        app.drawing.preview.clear();
        open_new(app, None);
    }
}

/// The text panel for a note the running tool places.
fn open_new(app: &mut SolveApp, justify: Option<Justify>) {
    app.drawing.panel = Some(DrawingPanel::Text(TextPanel::new(justify)));
}

/// Edit Note: the text panel on note `handle`, filled from what it has now.
pub(crate) fn open_edit(app: &mut SolveApp, handle: u64) {
    let Some(info) = app.session.doc.drawing.as_deref().and_then(|m| note_at(m, handle)) else { return };
    let justify = Justify::parse(info.justify);
    let panel = TextPanel {
        text: info.text.clone(),
        height_text: format_height(info.height),
        height: info.height,
        justify,
        edit: Some(handle),
        hole: info.hole,
        error: None,
        original: Some((info.text, info.height, justify)),
        focus: true,
    };
    app.drawing.panel = Some(DrawingPanel::Text(panel));
}

/// Edit Note's `drawing.note.edit` parameters for what changed, or None when nothing did.
pub(crate) fn edits(p: &TextPanel) -> Option<Value> {
    let (handle, (text, height, justify)) = (p.edit?, p.original.as_ref()?);
    let mut out = Map::new();
    if !p.hole && p.text != *text {
        out.insert("text".into(), json!(p.text));
    }
    if (p.height - height).abs() > 1e-9 {
        out.insert("height".into(), json!(p.height));
    }
    if p.justify != *justify
        && let Some(j) = p.justify
    {
        out.insert("justify".into(), json!(j.name()));
    }
    if out.is_empty() {
        return None;
    }
    out.insert("handle".into(), json!(handle));
    Some(Value::Object(out))
}

/// The new note's command and parameters, from the running tool and the panel.
fn new_note(app: &SolveApp, p: &TextPanel) -> Option<(&'static str, Value)> {
    let mut params = match tool(app)? {
        NoteTool::Text { position: Some(position), corner } => {
            let mut v = json!({ "position": position, "text": p.text });
            if let Some(c) = corner.filter(|c| (c[0] - position[0]).abs() > 1e-6) {
                v["corner"] = json!(c);
            }
            ("drawing.note.text", v)
        }
        NoteTool::Leader { points, attach, .. } if points.len() >= 2 => {
            let mut v = json!({ "points": points, "text": p.text });
            if let Some((view, at)) = attach {
                v["attach"] = json!({ "view": view, "edge": { "at": at } });
            }
            ("drawing.note.leader", v)
        }
        _ => return None,
    };
    params.1["height"] = json!(p.height);
    if let Some(j) = p.justify {
        params.1["justify"] = json!(j.name());
    }
    Some(params)
}

/// OK: Edit Note's one `drawing.note.edit` (nothing when nothing changed), or the new note's one
/// command; the tool then goes on for the next. False when refused (the panel stays open).
pub(crate) fn apply(app: &mut SolveApp, p: &mut TextPanel) -> bool {
    let Some(height) = typed_height(p) else {
        p.error = Some(crate::i18n::tr("ui.drawing.bad_height").to_string());
        return false;
    };
    p.height = height;
    if p.edit.is_some() {
        let Some(params) = edits(p) else { return true };
        return match app.run("drawing.note.edit", params) {
            Ok(_) => true,
            Err(e) => {
                p.error = Some(e);
                false
            }
        };
    }
    let Some((cmd, params)) = new_note(app, p) else { return true };
    let result = app.run(cmd, params.clone());
    app.drawing.notes_last_sent = Some((cmd, params));
    if let Err(e) = result {
        p.error = Some(e);
        return false;
    }
    reset(app);
    true
}

/// The tool's pending note is done with (placed, or its panel cancelled): ready for the next.
pub(crate) fn reset(app: &mut SolveApp) {
    match tool_mut(app) {
        Some(NoteTool::Text { position, corner }) => {
            *position = None;
            *corner = None;
        }
        Some(NoteTool::Leader { points, attach, .. }) => {
            points.clear();
            *attach = None;
        }
        _ => return,
    }
    app.drawing.preview.clear();
    app.drawing.selection.clear();
}

/// Handles at the centres of equal segments across `rect`, in order.
fn publish_segments(rect: Rect, names: &[&str]) {
    let w = rect.width() / names.len().max(1) as f32;
    for (i, name) in names.iter().enumerate() {
        scenario::publish_handle(name, egui::pos2(rect.left() + w * (i as f32 + 0.5), rect.center().y));
    }
}

/// The panel's card, over the sheet.
pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context, mut p: TextPanel) {
    let tr = crate::i18n::tr;
    let title = if p.edit.is_some() { tr("ui.drawing.edit_note") } else { tr("ui.drawing.tab_text") };
    let valid = p.can_ok();
    let (closed, action) = crate::ui_kit::Window::new(egui::Id::new("sc_drawing_text"), "text", title)
        .placed(egui::Align2::LEFT_TOP, crate::browser::panel_pos(app))
        .actions(crate::i18n::tr_label("OK"), tr("ui.cancel"), None)
        // Esc is the application's (`toolbar::shortcuts`): the marking menu takes it first.
        .keep_open_on_escape()
        .show(ctx, valid, |ui| body(ui, &mut p));
    if closed || action == crate::ui_kit::Action::Cancel {
        // Cancel drops the note being placed; the tool goes on (`drawing_ui::panel_cancel`).
        app.drawing.panel = Some(DrawingPanel::Text(p));
        crate::drawing_ui::panel_cancel(app);
        return;
    }
    if action == crate::ui_kit::Action::Ok && apply(app, &mut p) {
        app.drawing.panel = None;
        return;
    }
    app.drawing.panel = Some(DrawingPanel::Text(p));
}

/// The panel's rows: style (disabled), height, justification, the parameter row (disabled) and
/// the text.
fn body(ui: &mut egui::Ui, p: &mut TextPanel) {
    let tr = crate::i18n::tr;
    // R65: bold, italic and underline are shown, disabled, and never sent.
    ui.horizontal(|ui| {
        ui.add_enabled_ui(false, |ui| {
            let (mut bold, mut italic, mut underline) = (false, false, false);
            crate::ui_kit::text_emphasis(ui, &mut bold, &mut italic);
            let r = ui.toggle_value(&mut underline, egui::RichText::new("U").underline()).on_disabled_hover_text(tr("ui.drawing.underline"));
            scenario::publish_handle("drawing_text:underline", r.rect.center());
        });
    });
    for name in ["bold", "italic", "underline", "parameter"] {
        scenario::publish_count(&format!("drawing_text:{name}:enabled"), 0.0);
    }
    crate::ui_kit::row(ui, tr("ui.drawing.text_height"), |ui| {
        scenario::publish_handle("drawing_text:height", ui.available_rect_before_wrap().center());
        crate::ui_kit::text_field(ui, &mut p.height_text, "0.12 in");
    });
    if let Some(h) = typed_height(p) {
        p.height = h;
    }
    crate::ui_kit::row(ui, tr("ui.drawing.justify"), |ui| {
        let labels = [tr("sketch_text.align_left"), tr("sketch_text.align_center"), tr("sketch_text.align_right")];
        let mut index = p.justify.map_or(0, Justify::index);
        let states = Justify::ALL.map(|j| p.justify == Some(j));
        let shown =
            ui.scope(|ui| crate::ui_kit::segmented(ui, &mut index, &labels, crate::ui_kit::Segments { states: &states, ..Default::default() }));
        publish_segments(shown.response.rect, &["drawing_text:justify:left", "drawing_text:justify:center", "drawing_text:justify:right"]);
        if shown.inner {
            p.justify = Justify::ALL.get(index).copied();
        }
    });
    // The parameter row (Inventor inserts a model parameter's value): shown, disabled (R65).
    ui.add_enabled_ui(false, |ui| {
        crate::ui_kit::row(ui, crate::i18n::tr_label("Parameter"), |ui| {
            ui.horizontal(|ui| {
                let r = ui.button("—");
                scenario::publish_handle("drawing_text:parameter", r.rect.center());
                ui.label(tr("ui.drawing.precision"));
                ui.label(format!("{:.*}", PARAMETER_PRECISION, 3.123_f64));
            });
        });
    });
    let id = egui::Id::new("drawing_text:text");
    ui.add_enabled_ui(!p.hole, |ui| {
        let r = crate::ui_kit::multiline_text_field(ui, tr("ui.drawing.tab_text"), id, &mut p.text);
        scenario::publish_handle("drawing_text:text", r.rect.center());
        if std::mem::take(&mut p.focus) && !p.hole {
            r.request_focus();
        }
    });
    if let Some(error) = &p.error {
        crate::ui_kit::text(ui, error, crate::ui_kit::Text::Error("drawing_text:error"));
    }
}

/// A row of the leader's right-click list (`context_menu::run_item`): Back, Single-Segment
/// Leader.
pub(crate) fn run_menu(app: &mut SolveApp, id: &str) {
    // The open text panel holds the leader's typed text: its points stay as they are.
    if app.drawing.panel.is_some() {
        return;
    }
    match id {
        "ui.drawing.back" => {
            if let Some(NoteTool::Leader { points, attach, .. }) = tool_mut(app) {
                points.pop();
                if points.is_empty() {
                    *attach = None;
                }
            }
            if leader_points(app) == 0 {
                app.drawing.selection.clear();
                app.drawing.preview.clear();
            }
        }
        "ui.drawing.singleSegment" => {
            if let Some(NoteTool::Leader { single, .. }) = tool_mut(app) {
                *single = !*single;
            }
            if matches!(tool(app), Some(NoteTool::Leader { single: true, .. })) {
                continue_leader(app);
            }
        }
        _ => {}
    }
}

/// Whether the leader's Continue would open its text panel (two points at least).
pub(crate) fn can_continue(app: &SolveApp) -> bool {
    leader_points(app) >= 2
}

/// Whether the leader's list may go Back (a point to drop).
pub(crate) fn can_go_back(app: &SolveApp) -> bool {
    leader_points(app) > 0
}

#[cfg(test)]
#[path = "drawing_notes_tests.rs"]
mod tests;
