//! The Drive panel (spec §6.6, Plan E-2 Task 6), organised as the captured Drive Constraint
//! dialog [E: CAP §C] in our own words: the constraint's name, its one parameter (disabled),
//! the Position read-out, Start / End / Pause, the transport buttons, OK / Cancel, and behind
//! More: Stop on contact, Increment (by amount or total steps) and Repetitions. Play runs
//! `constraint.drive` once (`.noundo()`, the engine restores the design itself) and shows its
//! frames one by one as preview poses, a Pause apart; the document never changes, so closing
//! (OK or Cancel) leaves the drawing as it was (owner Q17). Not built (owner Q11): recording,
//! its rate and Drive adaptivity.

use std::cell::Cell;
use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcraft_engine::doc::constraints::{MAX_OFFSET_DEG, MAX_OFFSET_MM};
use solidcraft_engine::doc::expr::{Kind as ValueKind, Value as ExprValue};
use solidcraft_engine::doc::{Document, Mat};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::dialogs_assembly::Asm;
use crate::dialogs_constraint::value_text;
use crate::i18n::tr;
use crate::theme::Tokens;
use crate::ui_kit;

/// At most this many frames in one drive (pass × repetitions), as the engine counts them.
pub const MAX_FRAMES: usize = 10_000;
/// Total steps: 1-9,999 (steps + 1 values a pass, within [`MAX_FRAMES`]).
pub const MAX_STEPS: i64 = 9_999;
/// Repetitions: 1-1,000.
pub const MAX_REPEAT: i64 = 1_000;
/// The longest Pause, seconds.
pub const MAX_PAUSE_S: f64 = 60.0;

thread_local! {
    /// `constraint.drive` calls the panel made (the running count `constraint_drives`).
    static DRIVES: Cell<u64> = const { Cell::new(0) };
}

/// The Drive panel's state.
#[derive(Clone, Debug)]
pub struct DriveForm {
    pub constraint: u64,
    pub start: String,
    pub end: String,
    /// Seconds between frames (UI only: never sent).
    pub pause: String,
    pub by_steps: bool,
    pub increment: String,
    pub steps: String,
    pub start_end_start: bool,
    pub repeat: String,
    pub stop_on_contact: bool,
    pub expanded: bool,
    /// The last drive's frames: (value, world placements of the occurrences it moves).
    pub frames: Vec<(f64, BTreeMap<u64, Mat>)>,
    /// The frame shown.
    pub at: usize,
    /// Playing: forward (`true`) or backward.
    pub playing: Option<bool>,
    /// Why the last drive stopped short (contact, a limit, a loop).
    pub stopped: Option<String>,
    /// The constraint's name (the body's header row, R138).
    pub name: String,
    /// An Angle drives in degrees.
    pub angle: bool,
    /// The document's length unit.
    pub units: String,
    /// The engine's refusal of the last Play.
    pub error: Option<String>,
    /// The params the frames were made with: Play runs the drive again only when they change.
    ran: Option<Value>,
    /// When the next frame is due (`egui` input time, seconds).
    next_at: f64,
    /// The design's parameter values, for expressions in the boxes, and the revision they are of.
    vals: BTreeMap<String, ExprValue>,
    revision: u64,
}

/// A box that is not a usable amount: why.
enum Bad {
    NotAValue,
    TooLarge,
}

impl DriveForm {
    /// The panel on constraint `id` with the captured defaults [E: CAP §C]: Start = the current
    /// value, End = Start + 10 document units (degrees for an Angle), Pause 0 s, By amount 1,
    /// Start/End × 1, Stop on contact off, collapsed. None for a suppressed or Lost constraint
    /// (R162: the row menu says why) or an unknown one.
    pub fn new(app: &SolveApp, id: u64) -> Option<DriveForm> {
        let s = &app.session;
        let c = s.doc.assembly.constraint(&id.to_string())?;
        if crate::context_menu::drive_refusal(app, id).is_some() {
            return None;
        }
        let angle = c.kind.is_angle();
        let units = crate::dialogs_constraint::units(app);
        let now = s.constraint_report.rows.iter().find(|r| r.id == id).map_or(c.offset, |r| r.offset);
        // One document unit in the value's unit (mm, or degrees).
        let one = if angle { 1.0 } else { 1.0 / crate::prefs::unit_scale(&units).0 };
        let text = |v: f64| value_text(angle, v, &units);
        Some(DriveForm {
            constraint: id,
            start: text(now),
            end: text(now + 10.0 * one),
            pause: format!("{:.3} s", 0.0),
            by_steps: false,
            increment: text(one),
            steps: "10".into(),
            start_end_start: false,
            repeat: "1".into(),
            stop_on_contact: false,
            expanded: false,
            frames: Vec::new(),
            at: 0,
            playing: None,
            stopped: None,
            name: c.name.clone(),
            angle,
            units: units.clone(),
            error: None,
            ran: None,
            next_at: 0.0,
            vals: s.doc.param_values().0,
            revision: s.revision,
        })
    }

    /// A box's amount in the value's unit (mm, or degrees): `None` when empty.
    fn amount(&self, text: &str) -> Option<Result<f64, Bad>> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let kind = if self.angle { ValueKind::Angle } else { ValueKind::Length };
        Some(match Document::eval_in(&self.vals, text, kind) {
            Ok(v) if v.is_finite() => {
                let (v, cap) = if self.angle { (v.to_degrees(), MAX_OFFSET_DEG) } else { (v, MAX_OFFSET_MM) };
                if v.abs() > cap { Err(Bad::TooLarge) } else { Ok(v) }
            }
            Err(e) if e.to_string().contains("finite") => Err(Bad::TooLarge),
            _ => Err(Bad::NotAValue),
        })
    }

    /// A whole-number box (steps, repetitions): `None` when empty or not a whole number.
    fn whole(&self, text: &str) -> Option<i64> {
        let v = Document::eval_in(&self.vals, text.trim(), ValueKind::Unitless).ok()?;
        (v.is_finite() && v.fract() == 0.0 && v.abs() < 1e15).then_some(v as i64)
    }

    /// The Pause in seconds ("0.1", "0.100 s").
    fn pause_s(&self) -> Option<f64> {
        let text = self.pause.trim();
        let text = text.strip_suffix('s').unwrap_or(text).trim();
        let v = Document::eval_in(&self.vals, text, ValueKind::Unitless).ok()?;
        (v.is_finite() && (0.0..=MAX_PAUSE_S).contains(&v)).then_some(v)
    }

    /// The command's params: empty fields omitted, exactly one of `increment` / `steps`, `pause` never sent (UI-only).
    pub fn params(&self) -> Value {
        let mut p = json!({ "constraint": self.constraint });
        for (key, text) in [("start", &self.start), ("end", &self.end)] {
            if let Some(Ok(v)) = self.amount(text) {
                p[key] = json!(v);
            }
        }
        if self.by_steps {
            if let Some(n) = self.whole(&self.steps).filter(|n| (1..=MAX_STEPS).contains(n)) {
                p["steps"] = json!(n);
            }
        } else if let Some(Ok(v)) = self.amount(&self.increment).filter(|v| v.as_ref().is_ok_and(|v| *v > 0.0)) {
            p["increment"] = json!(v);
        }
        if let Some(n) = self.whole(&self.repeat).filter(|n| (1..=MAX_REPEAT).contains(n)) {
            p["repeat"] = json!(n);
        }
        p["cycle"] = json!(if self.start_end_start { "start_end_start" } else { "start_end" });
        if self.stop_on_contact {
            p["stop_on_contact"] = json!(true);
        }
        p
    }

    /// The inline refusal for the current fields, before Play (ranges above), or None.
    pub fn problem(&self) -> Option<String> {
        let named = |key: &str, field: &str| crate::i18n::format(key, &[("field", field)]);
        let mut values = [None, None];
        for (i, (label, text)) in [(tr("ui.constraint.drive_start"), &self.start), (tr("ui.constraint.drive_end"), &self.end)].into_iter().enumerate()
        {
            match self.amount(text) {
                Some(Err(Bad::NotAValue)) => return Some(named("ui.constraint.drive_not_a_value", label)),
                Some(Err(Bad::TooLarge)) => return Some(named("ui.constraint.drive_too_large", label)),
                Some(Ok(v)) => values[i] = Some(v),
                None => {}
            }
        }
        if self.pause_s().is_none() {
            return Some(named("ui.constraint.drive_pause_range", tr("ui.constraint.drive_pause")));
        }
        // One pass's values, as the engine counts them.
        let pass = if self.by_steps {
            let label = tr("ui.constraint.drive_total_steps");
            match self.whole(&self.steps) {
                Some(n) if (1..=MAX_STEPS).contains(&n) => n as f64 + 1.0,
                _ => return Some(named("ui.constraint.drive_steps_range", label)),
            }
        } else {
            let label = tr("ui.constraint.drive_by_amount");
            let inc = match self.amount(&self.increment) {
                Some(Ok(v)) if v > 0.0 => v,
                Some(Err(Bad::TooLarge)) => return Some(named("ui.constraint.drive_too_large", label)),
                _ => return Some(named("ui.constraint.drive_above_zero", label)),
            };
            // Unset Start / End are the engine's defaults: the current value, Start + 10.
            let span = match values {
                [Some(a), Some(b)] => (b - a).abs(),
                [None, Some(_)] => 0.0,
                _ => 10.0,
            };
            let n = span / inc;
            if !(n < MAX_FRAMES as f64) {
                return Some(tr("ui.constraint.drive_frames").to_string());
            }
            let whole = (n * (1.0 + 1e-12)).floor();
            whole + 1.0 + if n - whole > 1e-9 { 1.0 } else { 0.0 }
        };
        let repeat = match self.whole(&self.repeat) {
            Some(n) if (1..=MAX_REPEAT).contains(&n) => n as f64,
            _ => return Some(named("ui.constraint.drive_repeat_range", tr("ui.constraint.drive_repetitions"))),
        };
        let pass = if self.start_end_start { pass * 2.0 - 1.0 } else { pass };
        (pass * repeat > MAX_FRAMES as f64).then(|| tr("ui.constraint.drive_frames").to_string())
    }

    /// The value the read-out shows: the shown frame's, else Start's.
    pub fn position(&self) -> String {
        match self.frames.get(self.at) {
            Some((v, _)) => value_text(self.angle, *v, &self.units),
            None => match self.amount(&self.start) {
                Some(Ok(v)) => value_text(self.angle, v, &self.units),
                _ => self.start.trim().to_string(),
            },
        }
    }

    /// Play (`forward`) or play backward: the drive runs once for these params, then its frames
    /// show from the start (forward) or the end (backward), continuing from the shown frame when
    /// it is not already at that end. `now`: the input time.
    pub fn play(&mut self, app: &mut SolveApp, forward: bool, now: f64) {
        if self.problem().is_some() {
            return;
        }
        let params = self.params();
        if self.ran.as_ref() != Some(&params) {
            DRIVES.set(DRIVES.get() + 1);
            match app.run("constraint.drive", params.clone()) {
                Ok(reply) => {
                    self.frames = frames(&reply);
                    self.stopped = self.stop(&reply);
                    self.error = None;
                    self.ran = Some(params);
                    self.at = if forward { 0 } else { self.frames.len().saturating_sub(1) };
                }
                Err(e) => {
                    self.frames.clear();
                    self.stopped = None;
                    self.error = Some(e.trim_start_matches("constraint.drive: ").to_string());
                    self.ran = None;
                    self.playing = None;
                    return;
                }
            }
        }
        let last = self.frames.len().saturating_sub(1);
        if self.frames.len() < 2 {
            self.playing = None;
            return;
        }
        if forward && self.at >= last {
            self.at = 0;
        } else if !forward && self.at == 0 {
            self.at = last;
        }
        self.playing = Some(forward);
        self.next_at = now + self.pause_s().unwrap_or(0.0);
    }

    /// One UI frame of playback at input time `now`: the next frame when it is due; the end
    /// stops it.
    pub fn tick(&mut self, now: f64) {
        let Some(forward) = self.playing else { return };
        let last = self.frames.len().saturating_sub(1);
        if now + 1e-9 < self.next_at {
            return;
        }
        if forward && self.at < last {
            self.at += 1;
        } else if !forward && self.at > 0 {
            self.at -= 1;
        }
        if (forward && self.at >= last) || (!forward && self.at == 0) {
            self.playing = None;
        }
        self.next_at = now + self.pause_s().unwrap_or(0.0);
    }

    /// Step back (-1) or on (+1) one frame, or to the start / the end; playback stops.
    fn go(&mut self, to: Go) {
        self.playing = None;
        let last = self.frames.len().saturating_sub(1);
        self.at = match to {
            Go::Start => 0,
            Go::Back => self.at.saturating_sub(1),
            Go::On => (self.at + 1).min(last),
            Go::End => last,
        };
    }

    /// Why `reply` stopped short, in our words; a stop with a `null` value (the first value
    /// already stops) says so without one.
    fn stop(&self, reply: &Value) -> Option<String> {
        let value = |v: &Value| v.get("value").and_then(Value::as_f64).map(|x| value_text(self.angle, x, &self.units));
        let word = |v: &Value| v.as_str().unwrap_or_default().to_string();
        if let Some(c) = reply.get("stopped_by_contact").filter(|v| v.is_object()) {
            let (a, b) = (word(&c["pair"][0]), word(&c["pair"][1]));
            return Some(match value(c) {
                Some(at) => crate::i18n::format("ui.constraint.drive_contact", &[("a", &a), ("b", &b), ("value", &at)]),
                None => crate::i18n::format("ui.constraint.drive_contact_first", &[("a", &a), ("b", &b)]),
            });
        }
        if let Some(l) = reply.get("stopped_by_limit").filter(|v| v.is_object()) {
            let key = if l["limit"] == "min" { "ui.constraint.drive_limit_min" } else { "ui.constraint.drive_limit_max" };
            return Some(crate::i18n::format(key, &[("value", &value(l).unwrap_or_default())]));
        }
        let l = reply.get("stopped_by_loop").filter(|v| v.is_object())?;
        let name = word(&l["constraint"]);
        Some(match value(l) {
            Some(at) => crate::i18n::format("ui.constraint.drive_loop", &[("constraint", &name), ("value", &at)]),
            None => crate::i18n::format("ui.constraint.drive_loop_first", &[("constraint", &name)]),
        })
    }
}

#[derive(Clone, Copy)]
enum Go {
    Start,
    Back,
    On,
    End,
}

/// `constraint.drive`'s frames: (value, {occurrence id: world}); an empty list when it has none.
fn frames(reply: &Value) -> Vec<(f64, BTreeMap<u64, Mat>)> {
    reply
        .get("frames")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let v = f.get("value")?.as_f64()?;
            let world: BTreeMap<u64, Mat> = serde_json::from_value(f.get("world").cloned().unwrap_or_else(|| json!({}))).ok()?;
            Some((v, world))
        })
        .collect()
}

/// Open the Drive panel on constraint `id` (the row menu's Drive). A suppressed or Lost
/// constraint opens nothing; the status line says why (R162).
pub fn open(app: &mut SolveApp, id: u64) {
    if let Some(why) = crate::context_menu::drive_refusal(app, id) {
        app.set_status(why, true);
        return;
    }
    let Some(form) = DriveForm::new(app, id) else { return };
    app.tool = None;
    app.dialog = Some(Dialog::new(Kind::Assembly(Asm::DriveConstraint(Box::new(form))), vec![]));
}

/// `ui.constraintDrive` started without a row (the control channel): the first picked
/// constraint's panel.
pub fn start(app: &SolveApp) -> Option<(Kind, Vec<crate::selection::SelInput>)> {
    let id = *app.tree.picked_constraints.first()?;
    let form = DriveForm::new(app, id)?;
    Some((Kind::Assembly(Asm::DriveConstraint(Box::new(form))), vec![]))
}

/// Once a frame (from the preview update): with the Drive panel open, the occurrences the shown
/// frame moves, at their world poses (none at the start pose). Publishes the running count
/// `constraint_drives` and `constraint_drive_moved` (0 when no panel shows a frame).
pub(crate) fn frame(app: &SolveApp) -> Option<Vec<(u64, Mat)>> {
    let out = match app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Assembly(Asm::DriveConstraint(f))) => {
            Some(f.frames.get(f.at).map(|(_, world)| world.iter().map(|(o, m)| (*o, *m)).collect()).unwrap_or_default())
        }
        _ => None,
    };
    crate::scenario::publish_count("constraint_drives", DRIVES.get() as f64);
    crate::scenario::publish_count("constraint_drive_moved", out.as_ref().map_or(0, Vec::len) as f64);
    out
}

// ---- the panel ----

type Glyph = fn(egui::Rect, egui::Color32) -> Vec<egui::Shape>;

fn triangle(r: egui::Rect, ink: egui::Color32, x: f32, right: bool) -> egui::Shape {
    let (h, w) = (r.height() * 0.3, r.width() * 0.22);
    let (tip, back) = if right { (x + w, x - w) } else { (x - w, x + w) };
    let c = r.center().y;
    egui::Shape::convex_polygon(vec![egui::pos2(back, c - h), egui::pos2(tip, c), egui::pos2(back, c + h)], ink, egui::Stroke::NONE)
}

fn bar(r: egui::Rect, ink: egui::Color32, x: f32) -> egui::Shape {
    egui::Shape::rect_filled(egui::Rect::from_center_size(egui::pos2(x, r.center().y), egui::vec2(r.width() * 0.1, r.height() * 0.6)), 0.0, ink)
}

fn glyph_reverse(r: egui::Rect, ink: egui::Color32) -> Vec<egui::Shape> {
    vec![triangle(r, ink, r.center().x, false)]
}

fn glyph_play(r: egui::Rect, ink: egui::Color32) -> Vec<egui::Shape> {
    vec![triangle(r, ink, r.center().x, true)]
}

fn glyph_stop(r: egui::Rect, ink: egui::Color32) -> Vec<egui::Shape> {
    vec![egui::Shape::rect_filled(egui::Rect::from_center_size(r.center(), r.size() * 0.45), 1.0, ink)]
}

fn glyph_to_start(r: egui::Rect, ink: egui::Color32) -> Vec<egui::Shape> {
    let (c, d) = (r.center().x, r.width() * 0.18);
    vec![bar(r, ink, c - 1.6 * d), triangle(r, ink, c - 0.2 * d, false), triangle(r, ink, c + 1.0 * d, false)]
}

fn glyph_step_back(r: egui::Rect, ink: egui::Color32) -> Vec<egui::Shape> {
    let (c, d) = (r.center().x, r.width() * 0.18);
    vec![triangle(r, ink, c - 0.3 * d, false), bar(r, ink, c + 1.0 * d)]
}

fn glyph_step_on(r: egui::Rect, ink: egui::Color32) -> Vec<egui::Shape> {
    let (c, d) = (r.center().x, r.width() * 0.18);
    vec![bar(r, ink, c - 1.0 * d), triangle(r, ink, c + 0.3 * d, true)]
}

fn glyph_to_end(r: egui::Rect, ink: egui::Color32) -> Vec<egui::Shape> {
    let (c, d) = (r.center().x, r.width() * 0.18);
    vec![triangle(r, ink, c - 1.0 * d, true), triangle(r, ink, c + 0.2 * d, true), bar(r, ink, c + 1.6 * d)]
}

/// One transport button: a one-glyph segment, its words the accessible name and hover text.
fn transport(ui: &mut egui::Ui, key: &'static str, glyph: Glyph, on: bool, enabled: bool) -> bool {
    let t = Tokens::get();
    let tip = tr(key);
    ui.push_id(key, |ui| {
        ui.allocate_ui(egui::vec2(t.control_height + t.space_4, t.control_height), |ui| {
            let r = ui.add_enabled_ui(enabled, |ui| {
                let mut picked = usize::from(!on);
                ui_kit::segmented_glyphs(ui, &mut picked, &[ui_kit::GlyphSegment { key, tip: tip.to_string(), glyph }], &[on])
            });
            if !enabled {
                let _ = r.response.on_disabled_hover_text(tip);
            }
            enabled && r.inner
        })
        .inner
    })
    .inner
}

/// The panel's rows. Enter in a box does not close the panel (OK does).
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, f: &mut DriveForm) -> bool {
    let now = ui.input(|i| i.time);
    if f.revision != app.session.revision {
        f.vals = app.session.doc.param_values().0;
        f.revision = app.session.revision;
    }
    // Playback first: a frame Play starts on stays up for its Pause.
    f.tick(now);
    if f.playing.is_some() {
        ui.ctx().request_repaint();
    }

    // The constraint's name heads the body (R138: the title stays "Drive").
    ui_kit::text(ui, &f.name, ui_kit::Text::Heading("drive_constraint"));
    // One parameter: the combo is shown, disabled.
    let word = tr(if f.angle { "ui.constraint.angle" } else { "ui.constraint.offset" });
    ui_kit::row(ui, tr("ui.constraint.drive_parameter"), |ui| {
        ui.add_enabled_ui(false, |ui| {
            let mut one = 0;
            ui_kit::dropdown(ui, "drive_parameter", &mut one, &[word]);
        });
    });
    let position = f.position();
    ui_kit::row(ui, tr("ui.constraint.drive_position"), |ui| {
        let r = ui_kit::text(ui, &position, ui_kit::Text::Value("drive_position", false));
        crate::scenario::publish_rect("dialog:drive_position", r.rect);
    });
    for (key, value) in
        [("ui.constraint.drive_start", &mut f.start), ("ui.constraint.drive_end", &mut f.end), ("ui.constraint.drive_pause", &mut f.pause)]
    {
        ui_kit::row(ui, tr(key), |ui| ui_kit::field(ui, value));
    }

    // Transport: play backward, play, stop, to start, step back, step on, to end.
    let problem = f.problem();
    let can_play = problem.is_none();
    let has = !f.frames.is_empty();
    ui.horizontal(|ui| {
        if transport(ui, "ui.constraint.drive_reverse", glyph_reverse, f.playing == Some(false), can_play) {
            f.play(app, false, now);
        }
        if transport(ui, "ui.constraint.drive_play", glyph_play, f.playing == Some(true), can_play) {
            f.play(app, true, now);
        }
        if transport(ui, "ui.constraint.drive_stop", glyph_stop, false, true) {
            f.playing = None;
        }
        for (key, glyph, to) in [
            ("ui.constraint.drive_to_start", glyph_to_start as Glyph, Go::Start),
            ("ui.constraint.drive_step_back", glyph_step_back, Go::Back),
            ("ui.constraint.drive_step_on", glyph_step_on, Go::On),
            ("ui.constraint.drive_to_end", glyph_to_end, Go::End),
        ] {
            if transport(ui, key, glyph, false, true) && has {
                f.go(to);
            }
        }
    });
    // Under the buttons: what keeps Play off, the engine's refusal, or why the drive stopped.
    if let Some(why) = problem.as_deref().or(f.error.as_deref()) {
        ui_kit::text(ui, why, ui_kit::Text::Report("drive_problem", true));
    } else if let Some(why) = &f.stopped {
        ui_kit::text(ui, why, ui_kit::Text::Report("drive_stopped", false));
    }

    // Help, and More / Less.
    let t = Tokens::get();
    ui.horizontal(|ui| {
        let _ = ui_kit::button(ui, "?", ui_kit::Button::Small).on_hover_text(tr("ui.constraint.drive_help"));
        let (rect, _) = ui.allocate_exact_size(egui::vec2(t.control_height, t.control_height), egui::Sense::hover());
        if ui_kit::disclosure_button_at(ui, rect, f.expanded, tr(if f.expanded { "ui.constraint.less" } else { "ui.constraint.more" })).clicked() {
            f.expanded = !f.expanded;
        }
    });
    if f.expanded {
        ui_kit::checkbox(ui, tr("ui.constraint.drive_stop_on_contact"), &mut f.stop_on_contact);
        let (amount, steps) = (tr("ui.constraint.drive_by_amount"), tr("ui.constraint.drive_total_steps"));
        ui_kit::row(ui, tr("ui.constraint.drive_increment"), |ui| {
            let mut pick = usize::from(f.by_steps);
            if ui_kit::segmented(ui, &mut pick, &[amount, steps], ui_kit::Segments { states: &[!f.by_steps, f.by_steps], ..Default::default() }) {
                f.by_steps = pick == 1;
            }
        });
        if f.by_steps {
            ui_kit::row(ui, steps, |ui| ui_kit::field(ui, &mut f.steps));
        } else {
            ui_kit::row(ui, amount, |ui| ui_kit::field(ui, &mut f.increment));
        }
        let (once, back) = (tr("ui.constraint.drive_start_end"), tr("ui.constraint.drive_start_end_start"));
        ui_kit::row(ui, tr("ui.constraint.drive_repetitions"), |ui| {
            let mut pick = usize::from(f.start_end_start);
            let states = [!f.start_end_start, f.start_end_start];
            if ui_kit::segmented(ui, &mut pick, &[once, back], ui_kit::Segments { states: &states, ..Default::default() }) {
                f.start_end_start = pick == 1;
            }
        });
        ui_kit::row(ui, tr("ui.constraint.drive_times"), |ui| ui_kit::field(ui, &mut f.repeat));
    }
    false
}

#[cfg(test)]
#[path = "dialogs_constraint_drive_tests.rs"]
mod dialogs_constraint_drive_tests;
