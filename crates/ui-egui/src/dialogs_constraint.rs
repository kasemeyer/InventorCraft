//! The Constrain panel (spec §6.2, Plan E-2 Task 2): the Place Constraint dialog as captured —
//! the tab strip (only Assembly is built), the type, the picks, the offset or angle, the type's
//! solutions, Suppressed / Predict / Lock turn / Preview, and behind More the name and the limits.
//! The form builds an E-1 [`Draft`]; an edit starts from `Draft::edit(&record)` and overlays only
//! the fields the user touched (R147, R162). Picking and the live preview are Task 3's.

use std::collections::BTreeSet;

use serde_json::Value;
use solidcraft_engine::constraints::{Draft, Preview, classify};
use solidcraft_engine::doc::constraints::{ConstraintKind, ConstraintSolution, GeoRef, PrimKind, next_name};

use crate::SolveApp;
use crate::dialogs::Kind;
use crate::dialogs_assembly::Asm;
use crate::i18n::tr;
use crate::selection::SelInput;
use crate::theme::Tokens;
use crate::ui_kit;

/// The types in the order the Type row shows them.
pub const TYPES: [ConstraintKind; 5] =
    [ConstraintKind::Mate, ConstraintKind::Angle, ConstraintKind::Tangent, ConstraintKind::Insert, ConstraintKind::Symmetry];

const MATE: [ConstraintSolution; 2] = [ConstraintSolution::MateFace, ConstraintSolution::Flush];
const MATE_AXES: [ConstraintSolution; 3] = [ConstraintSolution::Opposed, ConstraintSolution::Aligned, ConstraintSolution::Undirected];
const ANGLE: [ConstraintSolution; 3] = [ConstraintSolution::Directed, ConstraintSolution::Undirected, ConstraintSolution::ExplicitVector];
const TANGENT: [ConstraintSolution; 2] = [ConstraintSolution::Inside, ConstraintSolution::Outside];
const INSERT: [ConstraintSolution; 2] = [ConstraintSolution::Opposed, ConstraintSolution::Aligned];

/// The Constrain panel's state.
#[derive(Clone, Debug)]
pub struct ConstraintForm {
    pub kind: ConstraintKind,
    /// `None`: the type's default.
    pub solution: Option<ConstraintSolution>,
    pub picks: [Option<GeoRef>; 3],
    /// The pick slot the next viewport pick fills.
    pub armed: usize,
    pub pick_part_first: bool,
    pub part: Option<u64>,
    /// The offset or angle as typed, a number or an expression; "" = measure (Predict).
    pub value: String,
    pub suppressed: bool,
    pub predict: bool,
    pub show_preview: bool,
    pub lock: bool,
    pub expanded: bool,
    pub name: String,
    pub max: (bool, String),
    pub min: (bool, String),
    pub editing: Option<u64>,
    /// The last draft previewed and its answer.
    pub preview: Option<(Draft, Preview)>,
    pub hover: Option<(GeoRef, PrimKind)>,
    /// Editing: `Draft::edit(&record)` as loaded, and the fields the user has touched since (R147, R162).
    pub base: Option<Draft>,
    pub touched: BTreeSet<&'static str>,
    /// The document's length unit ("mm", "in"): a length value's default is `0 <units>`.
    pub units: String,
}

/// A value box's default for `kind`: `0` in document units, `0.00 deg` for an Angle.
fn default_value(kind: ConstraintKind, units: &str) -> String {
    if kind.is_angle() { "0.00 deg".into() } else { format!("0 {units}") }
}

impl ConstraintForm {
    /// The captured defaults (spec §6.2): Mate, its default solution, pick 1 armed, value 0,
    /// Predict off, Preview on, collapsed.
    pub fn new(units: &str) -> ConstraintForm {
        let kind = ConstraintKind::Mate;
        ConstraintForm {
            kind,
            solution: None,
            picks: [None, None, None],
            armed: 0,
            pick_part_first: false,
            part: None,
            value: default_value(kind, units),
            suppressed: false,
            predict: false,
            show_preview: true,
            lock: false,
            expanded: false,
            name: String::new(),
            max: (false, default_value(kind, units)),
            min: (false, default_value(kind, units)),
            editing: None,
            preview: None,
            hover: None,
            base: None,
            touched: BTreeSet::new(),
            units: units.to_string(),
        }
    }

    /// The panel on constraint `id` as it is (`Draft::edit` of its record).
    pub fn edit(app: &SolveApp, id: u64) -> Option<ConstraintForm> {
        let record = app.session.doc.assembly.constraint(&id.to_string())?;
        let base = Draft::edit(record);
        let mut f = ConstraintForm::new(&units(app));
        f.kind = base.kind;
        f.solution = base.solution;
        f.picks = [base.a.clone(), base.b.clone(), base.reference.clone()];
        f.value = base.offset.clone().unwrap_or_default();
        f.suppressed = base.suppressed;
        f.lock = base.lock;
        f.name = base.name.clone().unwrap_or_default();
        let fallback = default_value(f.kind, &f.units);
        f.max = (base.max.is_some(), base.max.clone().unwrap_or_else(|| fallback.clone()));
        f.min = (base.min.is_some(), base.min.clone().unwrap_or(fallback));
        f.editing = Some(id);
        f.base = Some(base);
        Some(f)
    }

    /// The solution the form stands for: the chosen one, else the type's default.
    fn solution_now(&self) -> ConstraintSolution {
        self.solution.unwrap_or_else(|| self.kind.default_solution(false))
    }

    /// A Directed Angle takes no limits (R152).
    pub fn directed(&self) -> bool {
        self.kind == ConstraintKind::Angle && self.solution_now() == ConstraintSolution::Directed
    }

    /// How many picks the type and solution take.
    pub fn pick_count(&self) -> usize {
        self.kind.picks(self.solution_now())
    }

    /// The form's own fields as a draft (no record).
    fn own(&self) -> Draft {
        let limit = |(on, v): &(bool, String)| (*on && !self.directed() && !v.trim().is_empty()).then(|| v.trim().to_string());
        let value = self.value.trim();
        Draft {
            kind: self.kind,
            solution: self.solution,
            a: self.picks[0].clone(),
            b: self.picks[1].clone(),
            reference: if self.pick_count() > 2 { self.picks[2].clone() } else { None },
            offset: (!value.is_empty()).then(|| value.to_string()),
            min: limit(&self.min),
            max: limit(&self.max),
            lock: self.kind == ConstraintKind::Insert && self.lock,
            suppressed: self.suppressed,
            name: (!self.name.trim().is_empty()).then(|| self.name.trim().to_string()),
            editing: self.editing,
            record: None,
        }
    }

    /// Create: the form's fields. Edit: `base` with only the `touched` fields overlaid (never a
    /// re-formatted box, R162).
    pub fn draft(&self) -> Draft {
        let own = self.own();
        let Some(base) = &self.base else { return own };
        let mut d = base.clone();
        for key in &self.touched {
            match *key {
                "kind" => d.kind = own.kind,
                "solution" => d.solution = own.solution,
                "a" => d.a = own.a.clone(),
                "b" => d.b = own.b.clone(),
                "ref" => d.reference = own.reference.clone(),
                "offset" => d.offset = own.offset.clone(),
                "min" => d.min = own.min.clone(),
                "max" => d.max = own.max.clone(),
                "lock" => d.lock = own.lock,
                "suppressed" => d.suppressed = own.suppressed,
                "name" => d.name = own.name.clone().or_else(|| base.name.clone()),
                _ => {}
            }
        }
        d
    }

    /// Every pick the type and solution need is set (Symmetry is not built in v1).
    pub fn ready(&self) -> bool {
        self.kind != ConstraintKind::Symmetry && self.picks.iter().take(self.pick_count()).all(Option::is_some)
    }

    /// Both picks resolve to lines or cylinders (Mate's axis solutions).
    pub fn axes(&self, app: &SolveApp) -> bool {
        let axis =
            |g: &Option<GeoRef>| g.as_ref().and_then(|g| classify(&app.session, g)).is_some_and(|k| matches!(k, PrimKind::Line | PrimKind::Cylinder));
        axis(&self.picks[0]) && axis(&self.picks[1])
    }

    /// The type's solution buttons, in the spec §6.2 table's order.
    pub fn solutions(&self, app: &SolveApp) -> &'static [ConstraintSolution] {
        match self.kind {
            ConstraintKind::Mate if self.axes(app) => &MATE_AXES,
            ConstraintKind::Mate => &MATE,
            ConstraintKind::Angle => &ANGLE,
            ConstraintKind::Tangent => &TANGENT,
            ConstraintKind::Insert | ConstraintKind::Symmetry => &INSERT,
        }
    }

    /// A new type: the picks clear, pick 1 is armed and the solution goes back to the type's
    /// default; a value of the other unit goes back to its default.
    pub fn set_kind(&mut self, kind: ConstraintKind) {
        if kind == self.kind {
            return;
        }
        let unit_changes = kind.is_angle() != self.kind.is_angle();
        self.kind = kind;
        self.solution = None;
        self.picks = [None, None, None];
        self.armed = 0;
        self.hover = None;
        if unit_changes {
            self.value = default_value(kind, &self.units);
            self.max = (false, default_value(kind, &self.units));
            self.min = (false, default_value(kind, &self.units));
            self.touched.extend(["offset", "min", "max"]);
        }
        self.touched.extend(["kind", "solution", "a", "b", "ref"]);
    }

    /// A solution button pressed. Directed takes an Angle's limits away (R152): Max and Min
    /// uncheck, and an edit clears them.
    pub fn set_solution(&mut self, solution: ConstraintSolution) {
        self.solution = Some(solution);
        self.touched.insert("solution");
        if self.directed() {
            self.max.0 = false;
            self.min.0 = false;
            self.touched.extend(["min", "max"]);
        }
        self.armed = self.armed.min(self.pick_count().saturating_sub(1));
    }
}

/// The document's length unit ("mm" in a millimetre design).
fn units(app: &SolveApp) -> String {
    let zero = crate::dialogs::length_default(&app.session, "0");
    let unit = zero.trim().trim_start_matches('0').trim();
    if unit.is_empty() { "mm".into() } else { unit.to_string() }
}

/// The panel for `constraint.create`; it takes its picks itself (Task 3), not through selection inputs.
pub fn start(app: &SolveApp) -> Option<(Kind, Vec<SelInput>)> {
    Some((Kind::Assembly(Asm::Constraint(Box::new(ConstraintForm::new(&units(app))))), Vec::new()))
}

/// The command OK and Apply run: `constraint.create`, or `constraint.edit` with what changed.
pub fn commands(f: &ConstraintForm) -> Result<Vec<(String, Value)>, String> {
    if !f.ready() {
        return Err(tr("ui.constraint.picks_needed").to_string());
    }
    let cmd = if f.editing.is_some() { "constraint.edit" } else { "constraint.create" };
    Ok(vec![(cmd.to_string(), f.draft().params())])
}

/// OK and Apply may act: the picks are in and the last preview refused nothing.
pub fn acts(f: &ConstraintForm) -> bool {
    f.ready() && f.preview.as_ref().is_none_or(|(_, p)| p.refused.is_none())
}

/// The generated name an empty Name box stands for.
pub fn name_hint(app: &SolveApp, f: &ConstraintForm) -> String {
    match &f.base {
        Some(b) => b.name.clone().unwrap_or_default(),
        None => next_name(&app.session.doc.assembly, f.kind),
    }
}

fn type_label(kind: ConstraintKind) -> &'static str {
    tr(match kind {
        ConstraintKind::Mate => "constraint.type.mate",
        ConstraintKind::Angle => "constraint.type.angle",
        ConstraintKind::Tangent => "constraint.type.tangent",
        ConstraintKind::Insert => "constraint.type.insert",
        ConstraintKind::Symmetry => "constraint.type.symmetry",
    })
}

fn solution_label(s: ConstraintSolution) -> &'static str {
    tr(match s {
        ConstraintSolution::MateFace => "constraint.solution.mate",
        ConstraintSolution::Flush => "constraint.solution.flush",
        ConstraintSolution::Opposed => "constraint.solution.opposed",
        ConstraintSolution::Aligned => "constraint.solution.aligned",
        ConstraintSolution::Undirected => "constraint.solution.undirected",
        ConstraintSolution::Directed => "constraint.solution.directed",
        ConstraintSolution::ExplicitVector => "constraint.solution.explicit_vector",
        ConstraintSolution::Inside => "constraint.solution.inside",
        ConstraintSolution::Outside => "constraint.solution.outside",
    })
}

/// The type's icon master (R166).
pub fn type_icon(kind: ConstraintKind) -> &'static str {
    match kind {
        ConstraintKind::Mate => "constraint_mate",
        ConstraintKind::Angle => "constraint_angle",
        ConstraintKind::Tangent => "constraint_tangent",
        ConstraintKind::Insert => "constraint_insert",
        ConstraintKind::Symmetry => "constraint_symmetry",
    }
}

/// The solution's icon master (R166).
pub fn solution_icon(s: ConstraintSolution) -> &'static str {
    match s {
        ConstraintSolution::MateFace => "solution_mate",
        ConstraintSolution::Flush => "solution_flush",
        ConstraintSolution::Opposed => "solution_opposed",
        ConstraintSolution::Aligned => "solution_aligned",
        ConstraintSolution::Undirected => "solution_undirected",
        ConstraintSolution::Directed => "solution_directed",
        ConstraintSolution::ExplicitVector => "solution_reference",
        ConstraintSolution::Inside => "solution_inside",
        ConstraintSolution::Outside => "solution_outside",
    }
}

/// One icon toggle `width` wide, its word the accessible name and the hover text `tip`.
fn icon_toggle(ui: &mut egui::Ui, width: f32, word: &'static str, icon: &'static str, on: bool, enabled: bool, tip: &str) -> bool {
    let t = Tokens::get();
    ui.push_id(word, |ui| {
        ui.allocate_ui(egui::vec2(width.max(t.control_height), t.control_height), |ui| {
            let r = ui.add_enabled_ui(enabled, |ui| {
                let mut picked = usize::from(!on);
                ui_kit::segmented_icons(ui, &mut picked, &[ui_kit::IconSegment { key: word, icon, tip: tip.to_string() }], &[on])
            });
            if !enabled {
                let _ = r.response.on_disabled_hover_text(tip);
            }
            r.inner
        })
        .inner
    })
    .inner
}

/// One toggle drawn on its own (so it can be disabled or underlined alone): a one-option
/// segmented control at its natural width. (clicked, its rect)
fn toggle(ui: &mut egui::Ui, text: &str, on: bool, enabled: bool, tip: &str) -> (bool, egui::Rect) {
    let mut picked = usize::from(!on);
    // Each toggle is its own scope: one-option segmented controls side by side would share ids.
    let r = ui
        .push_id(text, |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                ui_kit::segmented(ui, &mut picked, &[text], ui_kit::Segments { states: &[on], intrinsic: true, ..Default::default() })
            })
        })
        .inner;
    let rect = r.response.rect;
    if !enabled {
        let _ = r.response.on_disabled_hover_text(tip);
    }
    (r.inner, rect)
}

/// Recent values of this unit: the newest five distinct constraint offsets of the design.
fn recent(app: &SolveApp, angle: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in app.session.doc.assembly.constraints.iter().rev().filter(|c| c.kind.is_angle() == angle) {
        let text = c.exprs.offset.clone().unwrap_or_else(|| format!("{} {}", c.offset, if angle { "deg" } else { "mm" }));
        if !out.contains(&text) {
            out.push(text);
        }
        if out.len() == 5 {
            break;
        }
    }
    out
}

/// The panel's rows; true when something changed.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, f: &mut ConstraintForm) -> bool {
    rows_keys(app, ui, f).0
}

/// What the rows edit, to tell a change.
type Snapshot = (ConstraintKind, Option<ConstraintSolution>, usize, String, String, (bool, String), (bool, String), [bool; 6]);

fn snapshot(f: &ConstraintForm) -> Snapshot {
    (
        f.kind,
        f.solution,
        f.armed,
        f.value.clone(),
        f.name.clone(),
        f.max.clone(),
        f.min.clone(),
        [f.pick_part_first, f.suppressed, f.predict, f.show_preview, f.lock, f.expanded],
    )
}

/// [`rows`], and whether Enter was accepted in a value box.
pub(crate) fn rows_keys(app: &mut SolveApp, ui: &mut egui::Ui, f: &mut ConstraintForm) -> (bool, bool) {
    let t = Tokens::get();
    let before = snapshot(f);
    let mut enter = false;

    // Tabs: only Assembly is built (owner Q2).
    ui.horizontal(|ui| {
        toggle(ui, tr("ui.constraint.tab_assembly"), true, true, "");
        for key in ["ui.constraint.tab_motion", "ui.constraint.tab_transition", "ui.constraint.tab_sets"] {
            toggle(ui, tr(key), false, false, tr("ui.constraint.not_in_version"));
        }
    });

    // Type: Symmetry is drawn, disabled (owner Q3). The five words take the panel's width.
    ui_kit::text(ui, tr("ui.constraint.type"), ui_kit::Text::Heading("constraint_type"));
    ui.horizontal(|ui| {
        let width = (ui.available_width() - ui.spacing().item_spacing.x * (TYPES.len() - 1) as f32) / TYPES.len() as f32;
        for kind in TYPES {
            let built = kind != ConstraintKind::Symmetry;
            let tip = if built { type_label(kind) } else { tr("ui.constraint.later_version") };
            if icon_toggle(ui, width, type_label(kind), type_icon(kind), f.kind == kind, built, tip) && built {
                f.set_kind(kind);
            }
        }
    });

    // Picks, each with its colour bar; then Pick part first.
    ui_kit::row(ui, tr("ui.constraint.picks"), |ui| {
        let inks = [t.accent, t.warning, t.text];
        for (i, (label, ink)) in ["1", "2", "3"].into_iter().zip(inks).enumerate().take(f.pick_count()) {
            let (clicked, rect) = toggle(ui, label, f.armed == i, true, "");
            let bar = egui::Rect::from_min_max(
                egui::pos2(rect.left() + t.space_2, rect.bottom() - t.space_2),
                egui::pos2(rect.right() - t.space_2, rect.bottom()),
            );
            ui.painter().rect_filled(bar, 0.0, ink);
            if clicked {
                f.armed = i;
            }
        }
        ui_kit::checkbox(ui, tr("ui.constraint.pick_part_first"), &mut f.pick_part_first);
    });

    // Value: offset or angle, with the Measure / recent flyout (hidden for Symmetry).
    if f.kind != ConstraintKind::Symmetry {
        let label = tr(if f.kind.is_angle() { "ui.constraint.angle" } else { "ui.constraint.offset" });
        let recent = recent(app, f.kind.is_angle());
        ui_kit::row(ui, label, |ui| {
            let width = (ui.available_width() - t.control_height - t.space_12 - ui.spacing().item_spacing.x).max(0.0);
            ui.allocate_ui(egui::vec2(width, t.control_height), |ui| enter |= ui_kit::field(ui, &mut f.value));
            ui.allocate_ui(egui::vec2(t.control_height + t.space_12, t.control_height), |ui| {
                ui_kit::dropdown_menu(ui, "constraint_value_menu", "", |ui| {
                    if ui_kit::list_picker(ui, "constraint_measure", tr("ui.constraint.measure"), false).clicked() {
                        f.value.clear();
                    }
                    for v in &recent {
                        if ui_kit::list_picker(ui, &format!("constraint_recent_{v}"), v, false).clicked() {
                            f.value = v.clone();
                        }
                    }
                })
                .on_hover_text(tr("ui.constraint.measure"));
            });
        });
    }

    // Solution: the type's buttons.
    let list = f.solutions(app);
    let pressed = f.solution.filter(|s| list.contains(s)).unwrap_or_else(|| f.kind.default_solution(f.axes(app)));
    ui_kit::row(ui, tr("ui.constraint.solution"), |ui| {
        let segments: Vec<ui_kit::IconSegment> = list
            .iter()
            .map(|s| ui_kit::IconSegment { key: solution_label(*s), icon: solution_icon(*s), tip: solution_label(*s).to_string() })
            .collect();
        let states: Vec<bool> = list.iter().map(|s| *s == pressed).collect();
        let mut index = list.iter().position(|s| *s == pressed).unwrap_or(0);
        if ui_kit::segmented_icons(ui, &mut index, &segments, &states)
            && let Some(s) = list.get(index)
        {
            f.set_solution(*s);
        }
    });

    // Options.
    ui_kit::checkbox(ui, tr("ui.constraint.suppressed"), &mut f.suppressed);
    if matches!(f.kind, ConstraintKind::Mate | ConstraintKind::Angle) {
        ui_kit::checkbox(ui, tr("ui.constraint.predict"), &mut f.predict);
    }
    if f.kind == ConstraintKind::Insert {
        ui_kit::checkbox(ui, tr("ui.constraint.lock_turn"), &mut f.lock);
    }
    ui_kit::checkbox(ui, tr("ui.constraint.preview"), &mut f.show_preview);

    // Help, and More / Less.
    ui.horizontal(|ui| {
        let _ = ui_kit::button(ui, "?", ui_kit::Button::Small).on_hover_text(tr("ui.constraint.help"));
        let (rect, _) = ui.allocate_exact_size(egui::vec2(t.control_height, t.control_height), egui::Sense::hover());
        if ui_kit::disclosure_button_at(ui, rect, f.expanded, tr(if f.expanded { "ui.constraint.less" } else { "ui.constraint.more" })).clicked() {
            f.expanded = !f.expanded;
        }
    });

    if f.expanded {
        let hint = name_hint(app, f);
        enter |= ui_kit::text_row(ui, tr("ui.constraint.name"), &mut f.name, &hint).0;
        let directed = f.directed();
        ui_kit::section(ui, "constraint_limits", tr("ui.constraint.limits"), true, |ui| {
            ui.add_enabled_ui(false, |ui| {
                let mut rest = false;
                ui_kit::checkbox(ui, tr("ui.constraint.rest_at_offset"), &mut rest);
            });
            for (key, limit) in [("ui.constraint.max", &mut f.max), ("ui.constraint.min", &mut f.min)] {
                let r = ui.add_enabled_ui(!directed, |ui| {
                    ui_kit::row(ui, tr(key), |ui| {
                        let (on, value) = limit;
                        ui_kit::checkbox(ui, "", on);
                        let mut field_enter = false;
                        ui.add_enabled_ui(*on, |ui| field_enter = ui_kit::field(ui, value));
                        field_enter
                    })
                });
                enter |= r.inner;
                if directed {
                    let _ = r.response.on_disabled_hover_text(tr("ui.constraint.directed_no_limits"));
                }
            }
        });
    }

    let after = snapshot(f);
    for (changed, key) in [
        (after.3 != before.3, "offset"),
        (after.4 != before.4, "name"),
        (after.5 != before.5, "max"),
        (after.6 != before.6, "min"),
        (after.7[1] != before.7[1], "suppressed"),
        (after.7[4] != before.7[4], "lock"),
    ] {
        if changed {
            f.touched.insert(key);
        }
    }
    (after != before, enter)
}

#[cfg(test)]
#[path = "dialogs_constraint_tests.rs"]
mod dialogs_constraint_tests;
