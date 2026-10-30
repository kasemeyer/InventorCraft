//! Assembly constraints (Inventor's Constrain): Mate, Angle, Tangent, Insert and Symmetry records
//! saved in the assembly (`Assembly::constraints`). Each names two or three picks on occurrence
//! geometry by persistent face, edge and origin names; every solve compiles the unsuppressed ones
//! into transient joints (ids from [`COMPILED_BASE`]) and writes their solved values back into
//! `state`. This module holds the record, its file data, its names and its checks.

use serde::{Deserialize, Deserializer, Serialize};
use solidcraft_geom::Vec3;

use crate::joints::Assembly;
use crate::{DocError, Result};

/// Constraints a design keeps at most (as `joints::MAX_JOINTS`).
pub const MAX_CONSTRAINTS: usize = 10_000;
/// A constraint's name is at most this many characters.
pub const MAX_NAME_CHARS: usize = 128;
/// A distance offset or limit (mm) is within ± this.
pub const MAX_OFFSET_MM: f64 = 1e7;
/// An angle offset or limit (degrees) is within ± this.
pub const MAX_OFFSET_DEG: f64 = 1e6;
/// The first id of the transient joints a solve compiles constraints into (constraint id * 4 + k
/// above it); never saved, never listed as joints.
pub const COMPILED_BASE: u64 = 1 << 48;

/// An Angle constraint's solution when none is asked for (owner Q5).
pub const ANGLE_DEFAULT: ConstraintSolution = ConstraintSolution::Undirected;

/// One assembly constraint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Constraint {
    pub id: u64,
    /// `Mate:1`; unique among constraints and joints.
    pub name: String,
    pub kind: ConstraintKind,
    pub solution: ConstraintSolution,
    /// Two picks, or three (Angle with an explicit vector; Symmetry).
    pub picks: Vec<GeoRef>,
    /// mm (Mate, Tangent, Insert) or degrees (Angle).
    pub offset: f64,
    #[serde(default, skip_serializing_if = "ConstraintLimits::is_empty")]
    pub limits: ConstraintLimits,
    #[serde(default, skip_serializing_if = "ConstraintExprs::is_empty")]
    pub exprs: ConstraintExprs,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
    /// Insert only: lock rotation.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lock: bool,
    /// Directed Angle: the frozen a×b in A's frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<Vec3>,
    /// The solved free values of its compiled joints. Not trusted on load: a state of the wrong
    /// length or with a non-finite value (a NaN is written as `null`) is re-seeded by the solve.
    #[serde(default, skip_serializing_if = "Vec::is_empty", deserialize_with = "lenient_state")]
    pub state: Vec<f64>,
    /// What `state` was solved for (R132): the compiled joint's kind and flip. A state whose
    /// signature differs from what the constraint compiles to now (Mate↔Flush, Opposed↔Aligned,
    /// an Undirected sense change, a lock) or that has none (a file from before R132) is
    /// re-seeded, not reused with a new meaning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<StateSignature>,
}

/// A compiled joint's kind and flip: what a constraint's `state` values mean (R132).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateSignature {
    pub kind: crate::joints::JointKind,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
}

impl StateSignature {
    /// A joint's signature.
    pub fn of(j: &crate::joints::Joint) -> Self {
        Self { kind: j.kind, flip: j.flip }
    }
}

impl Constraint {
    /// The saved state, when it fits a compiled joint with `n` values and signature `sig`: as many
    /// values, all finite, solved for the same kind and flip (R132).
    pub fn state_for(&self, n: usize, sig: StateSignature) -> Option<&Vec<f64>> {
        (self.state.len() == n && self.state.iter().all(|v| v.is_finite()) && self.signature == Some(sig)).then_some(&self.state)
    }
}

/// A saved state with `null` entries (how serde_json writes a NaN) reads them as NaN, for the
/// solve to re-seed, instead of failing the open.
fn lenient_state<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Vec<f64>, D::Error> {
    let values: Vec<Option<f64>> = Vec::deserialize(d)?;
    Ok(values.into_iter().map(|v| v.unwrap_or(f64::NAN)).collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintKind {
    Mate,
    Angle,
    Tangent,
    Insert,
    Symmetry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintSolution {
    #[serde(rename = "mate")]
    MateFace,
    Flush,
    Opposed,
    Aligned,
    Undirected,
    Directed,
    ExplicitVector,
    Inside,
    Outside,
}

/// Limits on the offset, in its unit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ConstraintLimits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

impl ConstraintLimits {
    pub fn is_empty(&self) -> bool {
        self.min.is_none() && self.max.is_none()
    }
}

/// The expressions behind the offset and limits.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ConstraintExprs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<String>,
}

impl ConstraintExprs {
    pub fn is_empty(&self) -> bool {
        self.offset.is_none() && self.min.is_none() && self.max.is_none()
    }
}

/// A pick: geometry on an occurrence (0 = the assembly itself).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeoRef {
    pub occurrence: u64,
    pub geo: Geo,
}

/// Picked geometry by persistent name (`doc::naming`); `pick` tells pieces of a split face or
/// edge apart.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Geo {
    Face { name: String, pick: Vec3 },
    Edge { name: String, pick: Vec3 },
    Vertex { edge: String, end: EdgeEnd, pick: Vec3 },
    Origin { item: OriginItem },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeEnd {
    Start,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginItem {
    Xy,
    Xz,
    Yz,
    X,
    Y,
    Z,
    Center,
}

impl ConstraintKind {
    const ALL: [ConstraintKind; 5] = [Self::Mate, Self::Angle, Self::Tangent, Self::Insert, Self::Symmetry];

    /// The type word saved in names (`Mate:1`).
    pub fn word(self) -> &'static str {
        match self {
            Self::Mate => "Mate",
            Self::Angle => "Angle",
            Self::Tangent => "Tangent",
            Self::Insert => "Insert",
            Self::Symmetry => "Symmetry",
        }
    }

    /// A type word, any case.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.word().eq_ignore_ascii_case(s.trim()))
    }

    /// The solution a new constraint gets; `axes` when both picks are axes.
    pub fn default_solution(self, axes: bool) -> ConstraintSolution {
        match self {
            Self::Mate if axes => ConstraintSolution::Opposed,
            Self::Mate => ConstraintSolution::MateFace,
            Self::Angle => ANGLE_DEFAULT,
            Self::Tangent => ConstraintSolution::Outside,
            Self::Insert | Self::Symmetry => ConstraintSolution::Opposed,
        }
    }

    /// Is the offset an angle (degrees)?
    pub fn is_angle(self) -> bool {
        self == Self::Angle
    }

    /// How many picks the constraint takes with `solution`.
    pub fn picks(self, solution: ConstraintSolution) -> usize {
        match (self, solution) {
            (Self::Angle, ConstraintSolution::ExplicitVector) | (Self::Symmetry, _) => 3,
            _ => 2,
        }
    }
}

impl ConstraintSolution {
    /// A solution's file word (`mate`, `explicit_vector`, …).
    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.trim().to_string())).ok()
    }

    /// Can a constraint of `kind` have this solution?
    pub fn fits(self, kind: ConstraintKind) -> bool {
        use ConstraintSolution::*;
        match kind {
            ConstraintKind::Mate => matches!(self, MateFace | Flush | Opposed | Aligned | Undirected),
            ConstraintKind::Angle => matches!(self, Directed | Undirected | ExplicitVector),
            ConstraintKind::Tangent => matches!(self, Inside | Outside),
            ConstraintKind::Insert | ConstraintKind::Symmetry => matches!(self, Opposed | Aligned),
        }
    }

    /// The file word (`mate`, `explicit_vector`, …).
    fn word(self) -> String {
        match serde_json::to_value(self) {
            Ok(serde_json::Value::String(s)) => s,
            _ => format!("{self:?}"),
        }
    }
}

/// A name's own rules (not its uniqueness): non-empty and at most [`MAX_NAME_CHARS`].
fn check_name_text(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(DocError::Invalid("a name can't be empty".into()));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(DocError::Invalid(format!("a name is at most {MAX_NAME_CHARS} characters")));
    }
    Ok(())
}

impl Constraint {
    /// The checks a constraint passes on load and on create: the picks its solution takes (2, or 3
    /// for Angle's explicit vector and Symmetry), finite numbers in range,
    /// `min ≤ max`, a solution its kind has, a non-empty name of at most 128 characters. A `state`
    /// that does not fit is not an error (the solve re-seeds it).
    pub fn validate(&self) -> Result<()> {
        let bad = |why: String| Err(DocError::Invalid(why));
        check_name_text(&self.name)?;
        if !self.solution.fits(self.kind) {
            return bad(format!("{} has no `{}` solution", self.kind.word(), self.solution.word()));
        }
        // Exactly the picks its solution takes (R128): a short Explicit vector or a long Mate never
        // reaches the solve.
        let picks = self.kind.picks(self.solution);
        if self.picks.len() != picks {
            return bad(format!("{} takes {picks} picks, not {}", self.kind.word(), self.picks.len()));
        }
        let (cap, unit) = if self.kind.is_angle() { (MAX_OFFSET_DEG, "deg") } else { (MAX_OFFSET_MM, "mm") };
        for (what, v) in [("offset", Some(self.offset)), ("min", self.limits.min), ("max", self.limits.max)] {
            if let Some(v) = v
                && !(v.is_finite() && v.abs() <= cap)
            {
                return bad(format!("{what} {v} is not a number within ±{cap} {unit}"));
            }
        }
        if let (Some(min), Some(max)) = (self.limits.min, self.limits.max)
            && min > max
        {
            return bad(format!("min {min} is above max {max}"));
        }
        if self.axis.is_some_and(|a| !a.is_finite()) {
            return bad("the axis is not finite".into());
        }
        for p in &self.picks {
            if let Geo::Face { pick, .. } | Geo::Edge { pick, .. } | Geo::Vertex { pick, .. } = &p.geo
                && !pick.is_finite()
            {
                return bad("a pick point is not finite".into());
            }
        }
        Ok(())
    }
}

/// Check every constraint of a design being opened: at most [`MAX_CONSTRAINTS`], each passing
/// [`Constraint::validate`], with an id below [`COMPILED_BASE`] and an id and name no other
/// constraint has (R128). The error names the constraint.
pub(crate) fn validate_loaded(asm: &Assembly) -> Result<()> {
    if asm.constraints.len() > MAX_CONSTRAINTS {
        return Err(DocError::Invalid("too many constraints".into()));
    }
    // A saved joint never takes an id the compiled joints use: the solver and `owner_of` would
    // take it for a constraint's.
    if let Some(j) = asm.joints.iter().find(|j| j.id >= COMPILED_BASE) {
        return Err(DocError::Invalid(format!("joint {}: id {} is in the range kept for compiled joints", j.name, j.id)));
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut names = std::collections::BTreeSet::new();
    for c in &asm.constraints {
        let refuse = |why: String| DocError::Invalid(format!("constraint {}: {why}", c.name));
        c.validate().map_err(|e| refuse(e.to_string()))?;
        if c.id >= COMPILED_BASE {
            return Err(refuse(format!("id {} is in the range kept for compiled joints", c.id)));
        }
        if !ids.insert(c.id) {
            return Err(refuse(format!("id {} is already another constraint's", c.id)));
        }
        if !names.insert(c.name.as_str()) {
            return Err(refuse("the name is already another constraint's".into()));
        }
    }
    Ok(())
}

/// The next generated name for `kind`: `<word>:<n>`, n one above the highest used for that word.
pub fn next_name(asm: &Assembly, kind: ConstraintKind) -> String {
    let word = kind.word();
    let highest = asm.constraints.iter().filter_map(|c| c.name.strip_prefix(word)?.strip_prefix(':')?.parse::<u64>().ok()).max().unwrap_or(0);
    format!("{word}:{}", highest.saturating_add(1))
}

/// Can a constraint (other than `except`) be called `name`? Non-empty, at most 128 characters,
/// and no other constraint or any joint has it.
pub fn check_name(asm: &Assembly, name: &str, except: Option<u64>) -> Result<()> {
    check_name_text(name)?;
    if asm.joints.iter().any(|j| j.name == name) {
        return Err(DocError::Invalid(format!("`{name}` is already a joint's name")));
    }
    if asm.constraints.iter().any(|c| c.name == name && Some(c.id) != except) {
        return Err(DocError::Invalid(format!("`{name}` is already a constraint's name")));
    }
    Ok(())
}

impl Assembly {
    /// A constraint by id or name.
    pub fn constraint(&self, key: &str) -> Option<&Constraint> {
        self.constraints.iter().find(|c| c.id.to_string() == key).or_else(|| self.constraints.iter().find(|c| c.name == key))
    }
}

#[path = "constraints_geo.rs"]
pub mod geo;
pub use geo::{PickWhat, Prim, PrimKind, geo_at, resolve, resolve_world};

#[path = "constraints_compile.rs"]
pub mod compile;
pub use compile::{Compiled, ConstraintStatus, compile, compiled_id, owner_of, statuses};

#[cfg(test)]
#[path = "constraints_tests.rs"]
mod constraints_tests;
