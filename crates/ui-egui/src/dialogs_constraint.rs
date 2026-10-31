//! The Constrain panel (spec §6.2, Plan E-2 Task 2): the Place Constraint dialog as captured —
//! the tab strip (only Assembly is built), the type, the picks, the offset or angle, the type's
//! solutions, Suppressed / Predict / Lock turn / Preview, and behind More the name and the limits.
//! The form builds an E-1 [`Draft`]; an edit starts from `Draft::edit(&record)` and overlays only
//! the fields the user touched (R147, R162). Task 3: viewport and browser picks fill the armed
//! slot ([`take`], [`pick_origin`]), the hovered candidate is memoised ([`candidate`]), the live
//! preview re-runs once per changed draft ([`refresh_preview`]) and OK / Apply commit
//! ([`commit`]).

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;

use serde_json::Value;
use solidcraft_engine::constraints::{Draft, Preview, accepts, classify};
use solidcraft_engine::doc::constraints::{
    ConstraintKind, ConstraintSolution, Geo, GeoRef, OriginItem, PickWhat, Prim, PrimKind, geo_at, next_name, resolve_world,
};
use solidcraft_engine::doc::{Mat, joints::occurrence_world};
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::{Sel, Session};

use crate::SolveApp;
use crate::dialogs::Kind;
use crate::dialogs_assembly::Asm;
use crate::i18n::tr;
use crate::selection::{AXES, Accept, EDGES, FACES, ORIGIN, PLANES, SelInput, VERTICES};
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
    /// Each pick as resolved in the world when it was taken (what the overlay draws).
    prims: [Option<Prim>; 3],
    /// The hovered candidate as resolved in the world.
    hover_prim: Option<Prim>,
    /// Hover lookups by entity (R162): `geo_at` + resolve run only when the key changes.
    memo: RefCell<Vec<(HoverKey, Option<(GeoRef, Prim)>)>>,
    /// The design revision the last preview ran on.
    preview_rev: u64,
}

/// What a hover lookup depends on: the entity under the pointer (a face or an edge by index, not
/// the point on it) and what the armed slot takes.
#[derive(Clone, Debug, PartialEq)]
struct HoverKey {
    entity: Sel,
    armed: usize,
    kind: ConstraintKind,
    first: Option<PrimKind>,
    part: Option<u64>,
    taken: [Option<u64>; 2],
    revision: u64,
}

thread_local! {
    /// Previews run (the running count `constraint_previews`).
    static PREVIEWS: Cell<u64> = const { Cell::new(0) };
    /// Hover lookups run (`constraint_hover_resolves`).
    static RESOLVES: Cell<u64> = const { Cell::new(0) };
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
            prims: [None, None, None],
            hover_prim: None,
            memo: RefCell::new(Vec::new()),
            preview_rev: 0,
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
        let (doc, st) = (&app.session.doc, app.session.model.state());
        for (slot, g) in f.picks.clone().iter().enumerate() {
            f.prims[slot] = g.as_ref().and_then(|g| resolve_world(doc, &st, g).ok());
        }
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
        self.axes_in(&app.session)
    }

    fn axes_in(&self, s: &Session) -> bool {
        let axis = |i: usize| {
            let kind = self.kind_of(i).or_else(|| self.picks[i].as_ref().and_then(|g| classify(s, g)));
            kind.is_some_and(|k| matches!(k, PrimKind::Line | PrimKind::Cylinder))
        };
        axis(0) && axis(1)
    }

    /// What pick `slot` is, as taken.
    fn kind_of(&self, slot: usize) -> Option<PrimKind> {
        self.prims.get(slot).and_then(|p| p.as_ref()).map(Prim::kind)
    }

    /// The type's solution buttons, in the spec §6.2 table's order.
    pub fn solutions(&self, app: &SolveApp) -> &'static [ConstraintSolution] {
        self.solutions_in(&app.session)
    }

    fn solutions_in(&self, s: &Session) -> &'static [ConstraintSolution] {
        match self.kind {
            ConstraintKind::Mate if self.axes_in(s) => &MATE_AXES,
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
        // An edit sends its solution: the new type's default, never one that does not fit.
        self.solution = self.editing.map(|_| kind.default_solution(false));
        self.clear_picks();
        if unit_changes {
            self.value = default_value(kind, &self.units);
            self.max = (false, default_value(kind, &self.units));
            self.min = (false, default_value(kind, &self.units));
            self.touched.extend(["offset", "min", "max"]);
        }
        self.touched.extend(["kind", "solution", "a", "b", "ref"]);
    }

    /// No picks; pick 1 armed (a new type, Apply).
    pub fn clear_picks(&mut self) {
        self.picks = [None, None, None];
        self.prims = [None, None, None];
        self.armed = 0;
        self.part = None;
        self.hover = None;
        self.hover_prim = None;
        self.preview = None;
    }

    /// Pick part first is waiting for its part.
    fn wants_part(&self) -> bool {
        self.pick_part_first && self.part.is_none()
    }

    /// Put `g` (resolved as `prim`) in the armed slot and arm the next empty one. A solution the
    /// new picks no longer offer goes back to the type's default.
    fn fill(&mut self, s: &Session, g: GeoRef, prim: Prim) {
        let slot = self.armed.min(self.pick_count().saturating_sub(1));
        self.picks[slot] = Some(g);
        self.prims[slot] = Some(prim);
        self.touched.insert(["a", "b", "ref"][slot]);
        self.part = None;
        self.hover = None;
        self.hover_prim = None;
        self.armed = (0..self.pick_count()).find(|i| self.picks[*i].is_none()).unwrap_or(slot);
        if let Some(sol) = self.solution
            && !self.solutions_in(s).contains(&sol)
        {
            self.solution = self.editing.map(|_| self.kind.default_solution(self.axes_in(s)));
            self.touched.insert("solution");
        }
    }

    /// May `g` (a `kind`) go into the armed slot? The type and first pick accept it, it is not on
    /// the other pick's occurrence (spec §6.3 step 3), and with Pick part first it is on the part.
    fn legal(&self, g: &GeoRef, kind: PrimKind) -> bool {
        let slot = self.armed;
        let first = if slot == 0 { None } else { self.kind_of(0) };
        if !accepts(self.kind, slot, first, kind) {
            return false;
        }
        if slot < 2 && self.picks[1 - slot].as_ref().is_some_and(|other| other.occurrence == g.occurrence) {
            return false;
        }
        !(self.pick_part_first && self.part.is_some_and(|p| p != g.occurrence))
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
pub(crate) fn units(app: &SolveApp) -> String {
    let zero = crate::dialogs::length_default(&app.session, "0");
    let unit = zero.trim().trim_start_matches('0').trim();
    if unit.is_empty() { "mm".into() } else { unit.to_string() }
}

/// Edit constraint `id` in the Constrain panel ("EDIT Constrain", R143): the record's values and
/// the same pick input as a new one; OK sends only the touched fields ([`commit`], R162). A Lost
/// constraint opens too (its picks are the record's; a rename needs nothing re-resolved, R134).
pub fn open_edit(app: &mut SolveApp, id: u64) {
    let Some(form) = ConstraintForm::edit(app, id) else { return };
    app.tool = None;
    app.dialog =
        Some(crate::dialogs::Dialog::new(Kind::Assembly(Asm::Constraint(Box::new(form))), vec![SelInput::new("ui.constraint.picks", PICKS, false)]));
}

/// What the panel's one selection input takes: the viewport's hover, click and Select Other
/// reach the panel through it; a pick goes into the form ([`take`]), never into the input.
pub const PICKS: Accept = FACES | EDGES | VERTICES | PLANES | AXES | ORIGIN;

/// The panel for `constraint.create`, with its pick input.
pub fn start(app: &SolveApp) -> Option<(Kind, Vec<SelInput>)> {
    let form = ConstraintForm::new(&units(app));
    Some((Kind::Assembly(Asm::Constraint(Box::new(form))), vec![SelInput::new("ui.constraint.picks", PICKS, false)]))
}

/// The command Apply / OK run: `draft().params()` — `constraint.create`, or, when editing,
/// `constraint.edit` with only the touched-and-changed fields (`Draft::params` diffs against
/// `record`, R147/R162). The answer's `warnings` are shown in the status bar ([`warnings`]).
pub fn commit(_app: &SolveApp, f: &ConstraintForm) -> Result<(String, Value), String> {
    if !f.ready() {
        return Err(tr("ui.constraint.picks_needed").to_string());
    }
    let cmd = if f.editing.is_some() { "constraint.edit" } else { "constraint.create" };
    Ok((cmd.to_string(), f.draft().params()))
}

/// [`commit`] as the dialog's command list.
pub fn commands(app: &SolveApp, f: &ConstraintForm) -> Result<Vec<(String, Value)>, String> {
    commit(app, f).map(|c| vec![c])
}

/// What `constraint.create` / `constraint.edit` warned (a limited offset clamped, R153), for the
/// status bar.
pub fn warnings(cmd: &str, reply: &Value) -> Option<String> {
    if !matches!(cmd, "constraint.create" | "constraint.edit") {
        return None;
    }
    let all: Vec<&str> = reply.get("warnings")?.as_array()?.iter().filter_map(Value::as_str).collect();
    (!all.is_empty()).then(|| all.join("; "))
}

/// The origin item an origin plane, axis or point stands for.
fn origin_item(name: &str) -> Option<OriginItem> {
    Some(match name {
        "XY" => OriginItem::Xy,
        "XZ" => OriginItem::Xz,
        "YZ" => OriginItem::Yz,
        "X" => OriginItem::X,
        "Y" => OriginItem::Y,
        "Z" => OriginItem::Z,
        "O" => OriginItem::Center,
        _ => return None,
    })
}

/// A viewport selection as a pick (spec §6.3): a face, edge or vertex through `geo_at` at its world
/// point on the occurrence its body belongs to; an origin plane, axis or the origin point as the
/// assembly's own origin item (occurrence 0). With the pick resolved in the world.
fn resolve_sel(s: &Session, sel: &Sel) -> Option<(GeoRef, Prim)> {
    let on = |body: &str, what: PickWhat, at: Vec3| {
        let occurrence = crate::dialogs_assembly::occurrence_of_body(s, body)?;
        geo_at(&s.doc, &s.model.state(), occurrence, what, at).ok()
    };
    let g = match sel {
        Sel::Face { body, point, .. } => on(body, PickWhat::Face, *point)?,
        Sel::Edge { body, point, .. } => on(body, PickWhat::Edge, *point)?,
        Sel::Vertex { body, point } if !body.is_empty() => on(body, PickWhat::Vertex, *point)?,
        Sel::Vertex { .. } => GeoRef { occurrence: 0, geo: Geo::Origin { item: OriginItem::Center } },
        Sel::Plane { name } | Sel::Axis { name } => GeoRef { occurrence: 0, geo: Geo::Origin { item: origin_item(name)? } },
        _ => return None,
    };
    let prim = resolve_world(&s.doc, &s.model.state(), &g).ok()?;
    Some((g, prim))
}

/// A face or an edge by its index (the point on it does not change what it is).
fn entity(sel: &Sel) -> Sel {
    match sel {
        Sel::Face { body, index, .. } => Sel::Face { body: body.clone(), index: *index, point: Vec3::ZERO },
        Sel::Edge { body, index, .. } => Sel::Edge { body: body.clone(), index: *index, point: Vec3::ZERO },
        other => other.clone(),
    }
}

/// [`candidate`] with its world primitive, memoised on the hovered entity and what the armed slot
/// takes (R162): `geo_at` and the resolve run only when one of them changes.
fn candidate_prim(s: &Session, f: &ConstraintForm, sel: &Sel) -> Option<(GeoRef, Prim)> {
    if f.wants_part() {
        return None;
    }
    let key = HoverKey {
        entity: entity(sel),
        armed: f.armed,
        kind: f.kind,
        first: f.kind_of(0),
        part: f.part,
        taken: [f.picks[0].as_ref().map(|g| g.occurrence), f.picks[1].as_ref().map(|g| g.occurrence)],
        revision: s.revision,
    };
    if let Some((_, hit)) = f.memo.borrow().iter().find(|(k, _)| *k == key) {
        return hit.clone();
    }
    RESOLVES.set(RESOLVES.get() + 1);
    let found = resolve_sel(s, sel).filter(|(g, p)| f.legal(g, p.kind()));
    let mut memo = f.memo.borrow_mut();
    if memo.len() >= 16 {
        memo.remove(0);
    }
    memo.push((key, found.clone()));
    found
}

/// The pick under a viewport selection, or None when it is not legal for the armed slot (spec §6.3 step 2).
/// Memoised on (hovered entity, armed slot, kind, first pick's kind): `geo_at` + `classify` run only when the
/// pointer's entity or the selection changes, never every frame (R162; ~1.5-3 ms uncached).
pub fn candidate(app: &SolveApp, f: &ConstraintForm, sel: &Sel) -> Option<(GeoRef, PrimKind)> {
    candidate_prim(&app.session, f, sel).map(|(g, p)| (g, p.kind()))
}

/// Does the panel take a hovered selection? Waiting for its part (Pick part first), any piece of a
/// component; otherwise a legal [`candidate`].
pub(crate) fn takes(s: &Session, f: &ConstraintForm, sel: &Sel) -> bool {
    if f.wants_part() {
        return crate::dialogs_assembly::body_of(sel).and_then(|b| crate::dialogs_assembly::occurrence_of_body(s, b)).is_some_and(|o| o != 0);
    }
    candidate_prim(s, f, sel).is_some()
}

/// A click (or a Select Other row): with Pick part first waiting, the part; else the pick goes
/// into the armed slot and the next empty one is armed.
pub(crate) fn take(s: &Session, f: &mut ConstraintForm, sel: &Sel) {
    if f.wants_part() {
        f.part = crate::dialogs_assembly::body_of(sel).and_then(|b| crate::dialogs_assembly::occurrence_of_body(s, b)).filter(|o| *o != 0);
        f.hover = None;
        return;
    }
    // Resolved at the clicked point itself (the memo keeps the first point on the entity).
    if let Some((g, prim)) = resolve_sel(s, sel).filter(|(g, p)| f.legal(g, p.kind())) {
        f.fill(s, g, prim);
    }
}

/// A browser pick of an occurrence's origin item (`occurrence` 0: the assembly's).
pub fn pick_origin(app: &mut SolveApp, occurrence: u64, key: &str) {
    let Some(item) = origin_item(key) else { return };
    let Some(mut d) = app.dialog.take() else { return };
    if let Kind::Assembly(Asm::Constraint(f)) = &mut d.kind {
        let g = GeoRef { occurrence, geo: Geo::Origin { item } };
        let s = &app.session;
        if let Ok(prim) = resolve_world(&s.doc, &s.model.state(), &g)
            && !f.wants_part()
            && f.legal(&g, prim.kind())
        {
            f.fill(s, g, prim);
        }
    }
    app.dialog = Some(d);
}

/// Re-run `constraints::preview` when `f.draft()` differs from the cached draft; at most once per frame. Publishes
/// the running count `scenario::publish_count("constraint_previews", n)` and the number of occurrences drawn at a
/// preview pose as `"constraint_preview_moved"`.
pub fn refresh_preview(app: &SolveApp, f: &mut ConstraintForm) {
    if !f.ready() || !f.show_preview {
        f.preview = None;
        return;
    }
    let d = f.draft();
    if f.preview.as_ref().is_some_and(|(seen, _)| *seen == d) && f.preview_rev == app.session.revision {
        return;
    }
    let p = solidcraft_engine::constraints::preview(&app.session, &d);
    PREVIEWS.set(PREVIEWS.get() + 1);
    f.preview = Some((d, p));
    f.preview_rev = app.session.revision;
}

/// The occurrences the preview places away from where they are, at their preview poses (world).
pub fn moved(app: &SolveApp, f: &ConstraintForm) -> Vec<(u64, Mat)> {
    let Some((_, p)) = f.preview.as_ref().filter(|(_, p)| p.refused.is_none()) else { return Vec::new() };
    let doc = &app.session.doc;
    let far = |a: &Mat, b: &Mat| a.iter().flatten().zip(b.iter().flatten()).any(|(x, y)| (x - y).abs() > 1e-9);
    p.world.iter().filter(|(o, m)| far(m, &occurrence_world(doc, **o))).map(|(o, m)| (*o, *m)).collect()
}

/// The open Constrain panel's form.
fn open_form(app: &SolveApp) -> Option<&ConstraintForm> {
    match app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Assembly(Asm::Constraint(f))) => Some(f),
        _ => None,
    }
}

/// Once a frame (from the preview update): the panel's preview refreshed, and the counters
/// published (`constraint_previews`, `constraint_hover_resolves` running; `constraint_preview_moved`
/// 0 when no panel shows one). The occurrences to draw at their preview poses.
pub(crate) fn frame(app: &mut SolveApp) -> Option<Vec<(u64, Mat)>> {
    let mut out = None;
    if let Some(mut d) = app.dialog.take() {
        if let Kind::Assembly(Asm::Constraint(f)) = &mut d.kind {
            refresh_preview(app, f);
            out = Some(moved(app, f));
        }
        app.dialog = Some(d);
    }
    crate::scenario::publish_count("constraint_previews", PREVIEWS.get() as f64);
    crate::scenario::publish_count("constraint_hover_resolves", RESOLVES.get() as f64);
    crate::scenario::publish_count("constraint_preview_moved", out.as_ref().map_or(0, Vec::len) as f64);
    out
}

/// The status bar's prompt while the panel waits for a pick: the armed slot's.
pub(crate) fn prompt(app: &SolveApp) -> Option<&'static str> {
    let f = open_form(app)?;
    if f.wants_part() {
        return Some("ui.constraint.prompt_part");
    }
    if f.ready() {
        return None;
    }
    Some(["ui.constraint.prompt_1", "ui.constraint.prompt_2", "ui.constraint.prompt_3"][f.armed.min(2)])
}

/// The predicted offset as the value box shows it: document units, three decimals.
fn predicted_text(f: &ConstraintForm) -> Option<String> {
    let p = f.preview.as_ref()?.1.predicted?;
    Some(value_text(f.kind.is_angle(), p.offset, &f.units))
}

/// An offset (mm, or degrees for an angle) in `units`, three decimals ("7.000 mm", "30.000 deg"):
/// the value box's prediction and a browser row's tooltip.
pub(crate) fn value_text(angle: bool, offset: f64, units: &str) -> String {
    if angle {
        return format!("{offset:.3} deg");
    }
    // The shared length display (`length_number` under it), as every other length label.
    crate::prefs::format_length(offset, units, 3)
}

// ---- overlay ----

/// The hovered candidate (found again only when the hovered entity changes) and the picks, each
/// in its slot's colour: a face as its plane's patch, a line or a cylinder's axis as a line
/// through it, a circle as itself, a point as a dot.
pub fn overlay(app: &SolveApp, painter: &egui::Painter, proj: &crate::viewport::Proj, f: &mut ConstraintForm) {
    let t = Tokens::get();
    let under = app.viewport.hover.as_ref().and_then(crate::viewport::hit_sel);
    match under.and_then(|sel| candidate_prim(&app.session, f, &sel)) {
        Some((g, prim)) => {
            f.hover = Some((g, prim.kind()));
            f.hover_prim = Some(prim);
        }
        None => {
            f.hover = None;
            f.hover_prim = None;
        }
    }
    let inks = [t.accent, t.warning, t.text];
    let size = app.cam.half_height() * 0.08;
    for (prim, ink) in f.prims.iter().zip(inks).take(f.pick_count()) {
        if let Some(prim) = prim {
            paint_prim(painter, proj, prim, ink, size);
        }
    }
    if let Some(prim) = &f.hover_prim {
        paint_prim(painter, proj, prim, inks[f.armed.min(2)], size);
    }
}

fn paint_prim(painter: &egui::Painter, proj: &crate::viewport::Proj, prim: &Prim, ink: egui::Color32, size: f64) {
    let t = Tokens::get();
    let stroke = egui::Stroke::new(t.space_3, ink);
    let line = |a: Vec3, b: Vec3| {
        if let (Some(a), Some(b)) = (proj.to_screen(a), proj.to_screen(b)) {
            painter.line_segment([a, b], stroke);
        }
    };
    let ring = |c: Vec3, n: Vec3, r: f64| {
        let (u, v) = (n.any_perp(), n.cross(n.any_perp()));
        let pts: Vec<egui::Pos2> = (0..=48)
            .filter_map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 48.0;
                proj.to_screen(c + u * (r * a.cos()) + v * (r * a.sin()))
            })
            .collect();
        painter.add(egui::Shape::line(pts, stroke));
    };
    match *prim {
        Prim::Plane { p, n } => {
            let (u, v) = (n.any_perp() * size, n.cross(n.any_perp()) * size);
            let corners = [p + u + v, p - u + v, p - u - v, p + u - v];
            let pts: Vec<egui::Pos2> = corners.iter().filter_map(|c| proj.to_screen(*c)).collect();
            if pts.len() == 4 {
                painter.add(egui::Shape::convex_polygon(pts, ink.gamma_multiply(0.35), stroke));
            }
        }
        Prim::Line { p, d } | Prim::Cylinder { p, d, .. } => line(p - d * size * 2.0, p + d * size * 2.0),
        Prim::Circle { c, axis, r } => {
            ring(c, axis, r);
            if let Some(at) = proj.to_screen(c) {
                painter.circle_filled(at, t.space_3, ink);
            }
        }
        Prim::Sphere { c, r } => ring(c, Vec3::Z, r),
        Prim::Point { p } => {
            if let Some(at) = proj.to_screen(p) {
                painter.circle_filled(at, t.space_4, ink);
            }
        }
    }
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
            // Predict with the box empty: the box shows what the parts measure (three decimals);
            // typing over it gives the value.
            let measured = if f.predict && f.value.trim().is_empty() { predicted_text(f) } else { None };
            ui.allocate_ui(egui::vec2(width, t.control_height), |ui| match measured {
                Some(text) => {
                    let mut shown = text.clone();
                    enter |= ui_kit::field(ui, &mut shown);
                    if shown != text {
                        f.value = shown;
                    }
                }
                None => enter |= ui_kit::field(ui, &mut f.value),
            });
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
        // A limited value outside its limits starts at the nearer one (R153/R160).
        if let Some(w) = f.preview.as_ref().and_then(|(_, p)| p.warning.as_deref()) {
            ui_kit::text(ui, w, ui_kit::Text::Warning("constraint_warning"));
        }
    }

    // Solution: the type's buttons.
    let list = f.solutions(app);
    // The chosen one; else, measuring, the predicted one; else the type's default.
    let predicted = f.preview.as_ref().and_then(|(_, p)| p.predicted).map(|p| p.solution).filter(|s| list.contains(s));
    let pressed = f.solution.filter(|s| list.contains(s)).or(predicted).unwrap_or_else(|| f.kind.default_solution(f.axes(app)));
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
    // A refused draft says why under Solution and keeps OK and Apply disabled (spec §6.3 step 4).
    if let Some(why) = f.preview.as_ref().and_then(|(_, p)| p.refused.as_deref()) {
        ui_kit::text(ui, why, ui_kit::Text::Report("constraint_refused", true));
    }

    // Options.
    ui_kit::checkbox(ui, tr("ui.constraint.suppressed"), &mut f.suppressed);
    if matches!(f.kind, ConstraintKind::Mate | ConstraintKind::Angle) {
        let was = f.predict;
        ui_kit::checkbox(ui, tr("ui.constraint.predict"), &mut f.predict);
        // Predict measures: the box empties (offset None); off again, an empty box is 0.
        if f.predict && !was {
            f.value.clear();
        } else if !f.predict && was && f.value.trim().is_empty() {
            f.value = default_value(f.kind, &f.units);
        }
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

#[cfg(test)]
#[path = "constraint_pick_tests.rs"]
mod constraint_pick_tests;
