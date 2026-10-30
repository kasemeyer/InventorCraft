//! Joints between occurrences: joint origins (snap frames on component geometry, re-found on
//! every solve so they follow edits), joints (rigid, revolute, slider, cylindrical, pin-slot,
//! planar, ball, and relations that tie only part of a placement, `joints_relations.rs`) with
//! their motion values and limits, motion links (gears, racks and screws are
//! links whose ratio comes from radii or a pitch; a cam's follower follows a table worked out from
//! its profile, `joints_cam.rs`), and the solver that places occurrences from the joint graph.
//!
//! A joint makes B's joint frame coincide with A's frame moved by the joint's motion:
//! `T_B · F_b = T_A · F_a · M(q) · Flip · Tail(q)`, so `T_B = T_A · F_a · M(q) · Flip · Tail(q) · F_b⁻¹`
//! (`Tail` is a relation's tilt and turns after the mate; the identity for the others), where `T`
//! are occurrence world transforms, `F` the joint frames in each component's own frame, and `q`
//! the joint's values (angles in radians, distances in mm). The solver walks the graph from
//! grounded occurrences (and the root, which never moves). Joints that close a loop (a four-bar
//! linkage, a slider-crank) are met by moving the values of the other joints around the loop;
//! those that still disagree are reported.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Aabb3, Vec3};

use solidcraft_sketch::linalg;

use crate::constraints::{COMPILED_BASE, Compiled, compile, owner_of};
use crate::{DocError, Document, IDENTITY, Mat, ModelState, Result, apply_point, apply_vector, mat_inverse, mat_mul};

/// Joints the solver (and the URDF export) reads at most.
pub const MAX_JOINTS: usize = 10_000;

/// Where a joint origin sits on a component's geometry (component coordinates).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Snap {
    /// An explicit frame (origin, z axis, x axis).
    Frame {
        origin: Vec3,
        z: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<Vec3>,
    },
    /// The centre of the planar face nearest `pick`, z along its outward normal. `face` is the
    /// face's persistent name: when it names a planar face, that face's own plane and centre are
    /// taken (the pick only tells the pieces of a split face apart).
    FaceCenter {
        pick: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<String>,
        /// What turns the frame about z (default: any direction square to z).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<XRef>,
    },
    /// The centre of a circular end of the cylindrical face at `pick`, z along its axis
    /// (pointing out of the face's end). `face` is the face's persistent name: when it names a
    /// cylindrical face, that face's own axis, radius and extent are taken. `end` is the name of
    /// the face across the rim at the chosen end (the plate's face a hole opens onto), so the
    /// end stays the same when the design changes; without it (or when neither end has that
    /// face any more), the end nearer `pick` along the axis.
    CircleCenter {
        pick: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end: Option<String>,
        /// What turns the frame about z (default: any direction square to z).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<XRef>,
    },
    /// A point on an edge of a planar face: the edge's middle, or the corner where it meets the
    /// edge `corner` names (`joints_edge.rs`). `edge` and `face` are the persistent names of the
    /// edge and the face (found first); `pick` is a point on the edge as it was picked.
    Edge {
        pick: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        edge: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        corner: Option<String>,
    },
    /// A point, z along `z` (default +Z) and x toward `x` laid square to z (default: one chosen
    /// from z alone).
    Point {
        point: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        z: Option<Vec3>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<Vec3>,
    },
}

/// What sets a joint origin's x axis (its turn about z): a direction, or a face found by its
/// persistent name, so a round origin keeps its turn when the design changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum XRef {
    /// A direction (component coordinates); its part square to z is taken.
    Dir(Vec3),
    /// A face: a plane's outward normal, a cylinder's way from the origin to its axis (another
    /// hole), any other face's way from the origin to the point of it nearest `pick` (component
    /// coordinates; it also tells the pieces of a split face apart, default the origin).
    Face {
        face: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pick: Option<Vec3>,
    },
}

impl Snap {
    /// The persistent face and edge names the snap refers to (its face, its end's, its x reference's;
    /// an edge origin's edge, face and corner edge).
    pub fn face_names(&self) -> Vec<&String> {
        let mut out = Vec::new();
        if let Snap::FaceCenter { face, x, .. } | Snap::CircleCenter { face, x, .. } = self {
            out.extend(face.iter());
            if let Some(XRef::Face { face, .. }) = x {
                out.push(face);
            }
        }
        if let Snap::CircleCenter { end: Some(end), .. } = self {
            out.push(end);
        }
        if let Snap::Edge { edge, face, corner, .. } = self {
            out.extend(edge.iter().chain(face.iter()).chain(corner.iter()));
        }
        out
    }

    /// [`Snap::face_names`], to rewrite them (a copied component's faces have new names).
    pub fn face_names_mut(&mut self) -> Vec<&mut String> {
        let mut out = Vec::new();
        match self {
            Snap::FaceCenter { face, x, .. } => {
                out.extend(face.iter_mut());
                if let Some(XRef::Face { face, .. }) = x {
                    out.push(face);
                }
            }
            Snap::CircleCenter { face, end, x, .. } => {
                out.extend(face.iter_mut());
                out.extend(end.iter_mut());
                if let Some(XRef::Face { face, .. }) = x {
                    out.push(face);
                }
            }
            Snap::Edge { edge, face, corner, .. } => out.extend(edge.iter_mut().chain(face.iter_mut()).chain(corner.iter_mut())),
            _ => {}
        }
        out
    }

    /// The direction a face or circle snap keeps as its frame's x (`XRef::Dir`), in the
    /// component's coordinates; `None` for a face reference or none.
    pub fn frame_x(&self) -> Option<Vec3> {
        match self {
            Snap::FaceCenter { x: Some(XRef::Dir(d)), .. } | Snap::CircleCenter { x: Some(XRef::Dir(d)), .. } => Some(*d),
            _ => None,
        }
    }

    /// A face or circle snap given no x reference keeps `x` (the frame's x as it is now): each
    /// solve lays it onto the face, or square to the hole's axis, as the geometry is then, so a
    /// part on a face that tilts keeps its turn about z. Without it (designs made before) x
    /// follows from z alone, and turns through a right angle where z crosses 0.9 along the
    /// component's X. A snap with a reference keeps it.
    pub fn with_frame_x(mut self, x: Vec3) -> Snap {
        if let Snap::FaceCenter { x: slot @ None, .. } | Snap::CircleCenter { x: slot @ None, .. } = &mut self {
            *slot = Some(XRef::Dir(x));
        }
        self
    }

    /// Whether every point and direction in the snap is finite.
    pub fn is_finite(&self) -> bool {
        let x_ok = |x: &Option<XRef>| match x {
            Some(XRef::Dir(d)) => d.is_finite(),
            Some(XRef::Face { pick, .. }) => pick.is_none_or(|p| p.is_finite()),
            None => true,
        };
        match self {
            Snap::Frame { origin, z, x } => origin.is_finite() && z.is_finite() && x.is_none_or(|x| x.is_finite()),
            Snap::FaceCenter { pick, x, .. } | Snap::CircleCenter { pick, x, .. } => pick.is_finite() && x_ok(x),
            Snap::Point { point, z, x } => point.is_finite() && z.is_none_or(|z| z.is_finite()) && x.is_none_or(|x| x.is_finite()),
            Snap::Edge { pick, .. } => pick.is_finite(),
        }
    }
}

/// A joint origin: a snap on an occurrence's component (occurrence 0 = the root design).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JointOrigin {
    pub occurrence: u64,
    pub snap: Snap,
}

/// A named joint origin (Joint Origin command), referenced by joints by name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedOrigin {
    pub name: String,
    pub origin: JointOrigin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JointKind {
    Rigid,
    Revolute,
    Slider,
    Cylindrical,
    PinSlot,
    Planar,
    Ball,
    /// B's z stays parallel to A's (against it unless flipped); B moves and turns about z freely.
    Parallel,
    /// B's z stays square to A's; B moves freely and turns about either z.
    Perpendicular,
    /// B's z stays at the joint's `angle` from mated (about their common x); B moves freely and
    /// turns about either z.
    Angle,
    /// B's origin stays on A's xy plane (`offset` off it); B slides on it and turns any way.
    PointOnPlane,
    /// B's origin stays on A's z axis; B slides along it and turns any way.
    PointOnLine,
    /// B's z axis lies in A's xy plane (`offset` off it: a cylinder's radius makes it tangent);
    /// B slides on the plane, turns about A's z and about its own axis.
    LineOnPlane,
    /// The solver's own, never a user's: B anywhere, its values a translation and a rotation
    /// vector from A's frame (spec §3.5). A part held only by loop equations is placed by one
    /// from the root, seeded where it is. Never saved, listed or parsed.
    #[serde(skip)]
    Free,
}

impl JointKind {
    pub fn parse(s: &str) -> Option<JointKind> {
        Some(match s.to_ascii_lowercase().replace([' ', '_', '-'], "").as_str() {
            "rigid" => JointKind::Rigid,
            "revolute" | "rotate" => JointKind::Revolute,
            "slider" | "slide" => JointKind::Slider,
            "cylindrical" => JointKind::Cylindrical,
            "pinslot" => JointKind::PinSlot,
            "planar" => JointKind::Planar,
            "ball" => JointKind::Ball,
            "parallel" => JointKind::Parallel,
            "perpendicular" | "square" => JointKind::Perpendicular,
            "angle" => JointKind::Angle,
            "pointonplane" | "pointonface" => JointKind::PointOnPlane,
            "pointonline" | "pointonaxis" => JointKind::PointOnLine,
            "lineonplane" | "lineonface" => JointKind::LineOnPlane,
            _ => return None,
        })
    }
    /// Names of the joint's values, in order (angles in radians, distances in mm).
    pub fn dofs(self) -> &'static [&'static str] {
        match self {
            JointKind::Rigid => &[],
            JointKind::Revolute => &["angle"],
            JointKind::Slider => &["distance"],
            JointKind::Cylindrical => &["angle", "distance"],
            JointKind::PinSlot => &["angle", "slide"],
            JointKind::Planar => &["x", "y", "angle"],
            JointKind::Ball => &["yaw", "pitch", "roll"],
            JointKind::Parallel => &["x", "y", "z", "angle"],
            JointKind::Perpendicular | JointKind::Angle => &["x", "y", "z", "turn", "spin"],
            JointKind::PointOnPlane => &["x", "y", "yaw", "pitch", "roll"],
            JointKind::PointOnLine => &["slide", "yaw", "pitch", "roll"],
            JointKind::LineOnPlane => &["x", "y", "turn", "spin"],
            JointKind::Free => &["x", "y", "z", "rx", "ry", "rz"],
        }
    }
    /// Is value `i` an angle (radians)? A `Free` member's rotation vector counts (R117), so loop
    /// scaling and the steps it is solved in treat it as one.
    pub fn is_angle(self, i: usize) -> bool {
        matches!(self.dofs().get(i), Some(&("angle" | "yaw" | "pitch" | "roll" | "turn" | "spin" | "rx" | "ry" | "rz")))
    }
    /// A relation that only ties some directions or points (parallel, on a plane…): made where
    /// the parts are, its values start as the nearest that meet it, so B moves no more than it
    /// must.
    pub fn is_relation(self) -> bool {
        matches!(
            self,
            JointKind::Parallel
                | JointKind::Perpendicular
                | JointKind::Angle
                | JointKind::PointOnPlane
                | JointKind::PointOnLine
                | JointKind::LineOnPlane
        )
    }
}

/// A joint between two occurrences.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Joint {
    pub id: u64,
    pub name: String,
    pub kind: JointKind,
    pub a: JointOrigin,
    pub b: JointOrigin,
    /// Current values of the joint's degrees of freedom.
    #[serde(default)]
    pub values: Vec<f64>,
    /// Per value: (min, max) limits.
    #[serde(default)]
    pub limits: Vec<Option<(f64, f64)>>,
    /// Don't mate: B's z along A's instead of against it (by default the origins mate, as two
    /// faces meeting).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// Distance from A's frame to B's along z, and a turn about it (radians).
    #[serde(default)]
    pub offset: f64,
    #[serde(default)]
    pub angle: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
    /// The expressions the offset, the angle and the values were typed as: they are worked out
    /// again whenever the design changes, so a joint follows the parameters it is defined by.
    #[serde(default, skip_serializing_if = "JointExprs::is_empty")]
    pub exprs: JointExprs,
}

/// Expressions a joint's numbers follow (over the design's parameters): the offset (a length),
/// the angle and each value (angles or lengths as the joint's values are). A number typed in
/// place of one drops it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct JointExprs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<String>,
    /// By value index, as in `Joint::values`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<Option<String>>,
}

impl JointExprs {
    pub fn is_empty(&self) -> bool {
        self.offset.is_none() && self.angle.is_none() && self.values.iter().all(Option::is_none)
    }

    /// The expression value `i` follows.
    pub fn value(&self, i: usize) -> Option<&str> {
        self.values.get(i).and_then(Option::as_deref)
    }

    /// Value `i` follows `e` (or nothing).
    pub fn set_value(&mut self, i: usize, e: Option<String>) {
        if e.is_some() && self.values.len() <= i {
            self.values.resize(i + 1, None);
        }
        if let Some(slot) = self.values.get_mut(i) {
            *slot = e;
        }
        while self.values.last().is_some_and(Option::is_none) {
            self.values.pop();
        }
    }
}

/// Largest offset or value an expression may give a joint (mm, or radians for an angle).
const MAX_EXPR_VALUE: f64 = 1e9;

/// A joint's offset, angle and values worked out from its expressions (those without one keep
/// their numbers), or why one can't be: an unknown name, a wrong unit, or a number too large.
pub fn expressed(doc: &Document, j: &Joint) -> Result<(f64, f64, Vec<f64>)> {
    let eval = |e: &str, kind: crate::expr::Kind, what: &str| -> Result<f64> {
        let v = doc.eval(e, kind).map_err(|err| DocError::Invalid(format!("joint {}: {what} `{e}`: {err}", j.name)))?;
        if v.is_finite() && v.abs() < MAX_EXPR_VALUE { Ok(v) } else { Err(DocError::Invalid(format!("joint {}: {what} `{e}` gives {v}", j.name))) }
    };
    let x = &j.exprs;
    let offset = match &x.offset {
        Some(e) => eval(e, crate::expr::Kind::Length, "offset")?,
        None => j.offset,
    };
    let angle = match &x.angle {
        Some(e) => eval(e, crate::expr::Kind::Angle, "angle")?,
        None => j.angle,
    };
    let mut values = j.values.clone();
    for (i, e) in x.values.iter().enumerate().take(j.kind.dofs().len()) {
        let Some(e) = e else { continue };
        let kind = if j.kind.is_angle(i) { crate::expr::Kind::Angle } else { crate::expr::Kind::Length };
        let name = j.kind.dofs().get(i).copied().unwrap_or("value");
        let v = eval(e, kind, name)?;
        if values.len() <= i {
            values.resize(i + 1, 0.0);
        }
        if let Some(slot) = values.get_mut(i) {
            *slot = v;
        }
    }
    Ok((offset, angle, values))
}

/// Values the loop solver leaves alone: those a motion link sets and those an expression does.
fn set_values(asm: &Assembly) -> BTreeSet<(u64, usize)> {
    let mut set: BTreeSet<(u64, usize)> = asm.links.iter().map(|l| (l.b, l.ib)).collect();
    for j in &asm.joints {
        set.extend(j.exprs.values.iter().enumerate().filter(|(_, e)| e.is_some()).map(|(i, _)| (j.id, i)));
    }
    set
}

/// Value of joint `b` (index `ib`) = ratio · value of joint `a` (index `ia`) + offset, or for a
/// cam, its `lift` table at a's value + offset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MotionLink {
    pub a: u64,
    #[serde(default)]
    pub ia: usize,
    pub b: u64,
    #[serde(default)]
    pub ib: usize,
    pub ratio: f64,
    #[serde(default)]
    pub offset: f64,
    /// Gears, a rack and pinion, a screw or a cam: what the ratio (or a cam's table) comes from.
    /// It is worked out from it again whenever the design changes (`coupling_values`), so it
    /// follows the radii, the pitch and the cam's profile; the last one worked out is kept in
    /// `ratio` and `lift`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coupling: Option<Coupling>,
    /// A cam's follower values over one turn of a (`cam::CAM_SAMPLES` of them, in b's units, the
    /// offset left out); empty for every other link.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lift: Vec<f64>,
}

impl MotionLink {
    /// b's value for a's value `x` (before b's limits). A cam's table that is not whole (a
    /// hand-edited file) gives no value, so b keeps where it is; a table on any other link is
    /// not read.
    pub fn follow(&self, x: f64) -> f64 {
        match &self.coupling {
            Some(Coupling::Cam(_)) if self.lift.len() == cam::CAM_SAMPLES => cam::lift_at(&self.lift, x) + self.offset,
            Some(Coupling::Cam(_)) => f64::NAN,
            _ => self.ratio * x + self.offset,
        }
    }
}

/// A length a coupling's ratio comes from: millimetres, an expression over the design's
/// parameters, or the radius of the round face picked on an occurrence (a circle-centre snap,
/// found again on every solve like a joint origin).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Size {
    Length(f64),
    Expr(String),
    Circle(JointOrigin),
}

/// The geometry a motion link's ratio comes from. `reverse` turns the follower the other way.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Coupling {
    /// Two gears on revolute joints, `ra` on a's and `rb` on b's (pitch radii, or any two lengths
    /// in their proportion such as tooth counts): b turns ra/rb as far as a, the other way round,
    /// or the same way when one is an internal (ring) gear.
    Gear {
        ra: Size,
        rb: Size,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        internal: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        reverse: bool,
    },
    /// A pinion of pitch radius `radius` on a revolute joint and a rack on a slider: the rack
    /// travels the radius times the pinion's turn in radians (either joint may drive).
    RackPinion {
        radius: Size,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        reverse: bool,
    },
    /// A screw: the travel advances `pitch` per turn (a cylindrical joint's angle and distance,
    /// or a revolute joint and a slider). Right-handed unless reversed.
    Screw {
        pitch: Size,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        reverse: bool,
    },
    /// A cam on a revolute joint (a) and its follower (b, a slider or a revolute): b rides the
    /// cam's profile (`joints_cam.rs`).
    Cam(cam::Cam),
}

impl Coupling {
    /// The lengths the ratio comes from: a's gear and b's, the pinion's radius, the pitch, or a
    /// cam's roller radius.
    pub fn sizes(&self) -> Vec<&Size> {
        match self {
            Coupling::Gear { ra, rb, .. } => vec![ra, rb],
            Coupling::RackPinion { radius: s, .. } | Coupling::Screw { pitch: s, .. } => vec![s],
            Coupling::Cam(c) => match &c.follower {
                cam::Follower::Roller { radius } => vec![radius],
                _ => Vec::new(),
            },
        }
    }

    pub fn sizes_mut(&mut self) -> Vec<&mut Size> {
        match self {
            Coupling::Gear { ra, rb, .. } => vec![ra, rb],
            Coupling::RackPinion { radius: s, .. } | Coupling::Screw { pitch: s, .. } => vec![s],
            Coupling::Cam(c) => match &mut c.follower {
                cam::Follower::Roller { radius } => vec![radius],
                _ => Vec::new(),
            },
        }
    }
}

/// Largest length a coupling takes (mm).
const MAX_SIZE: f64 = 1e7;

/// A coupling length in millimetres: positive and finite.
fn size_of(doc: &Document, st: &ModelState, s: &Size) -> Result<f64> {
    let v = match s {
        Size::Length(v) => *v,
        Size::Expr(e) => doc.eval(e, crate::expr::Kind::Length)?,
        Size::Circle(o) => circle_at(doc, st, o)?.radius,
    };
    if v.is_finite() && v > 1e-9 && v <= MAX_SIZE {
        Ok(v)
    } else {
        Err(DocError::Invalid(format!("a gear, rack, screw or roller needs a positive length, not {v}")))
    }
}

/// The ratio a link's coupling gives it (b's value per a's, in radians and mm) and a cam's table
/// (`MotionLink::lift`; empty for the others), worked out from the geometry and parameters it is
/// defined by; `None` for a plain link.
pub fn coupling_values(doc: &Document, st: &ModelState, l: &MotionLink) -> Option<Result<(f64, Vec<f64>)>> {
    l.coupling.as_ref().map(|c| match c {
        Coupling::Cam(c) => cam::table(doc, st, l, c).map(|t| (1.0, t)),
        c => coupled(doc, st, l, c).map(|r| (r, Vec::new())),
    })
}

fn coupled(doc: &Document, st: &ModelState, l: &MotionLink, c: &Coupling) -> Result<f64> {
    let angle = |id: u64, i: usize| doc.assembly.joints.iter().find(|j| j.id == id).is_some_and(|j| j.kind.is_angle(i));
    let (turn_a, turn_b) = (angle(l.a, l.ia), angle(l.b, l.ib));
    // Travel per radian, for a turn coupled to a travel either way round.
    let travel = |k: f64| match (turn_a, turn_b) {
        (true, false) => Ok(k),
        (false, true) => Ok(1.0 / k),
        _ => Err(DocError::Invalid("a rack or a screw couples a turn and a travel".into())),
    };
    let sign = |reverse: bool| if reverse { -1.0 } else { 1.0 };
    match c {
        Coupling::Gear { ra, rb, internal, reverse } => {
            if !(turn_a && turn_b) {
                return Err(DocError::Invalid("gears couple two turns".into()));
            }
            Ok(size_of(doc, st, ra)? / size_of(doc, st, rb)? * sign(!internal) * sign(*reverse))
        }
        Coupling::RackPinion { radius, reverse } => Ok(travel(size_of(doc, st, radius)?)? * sign(*reverse)),
        Coupling::Screw { pitch, reverse } => Ok(travel(size_of(doc, st, pitch)? / std::f64::consts::TAU)? * sign(*reverse)),
        // A cam's follower follows its table (`coupling_values`), not a ratio.
        Coupling::Cam(_) => Ok(1.0),
    }
}

/// Joints, joint origins and motion links of a design.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Assembly {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub joints: Vec<Joint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub origins: Vec<NamedOrigin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<MotionLink>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contact_sets: Vec<crate::motion::ContactSet>,
    /// Contact sets take effect.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub contact_enabled: bool,
    /// Every occurrence is in contact with every other (Enable All Contact).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub contact_all: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub studies: Vec<crate::motion::MotionStudy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traces: Vec<crate::motion::MotionTrace>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exploded: Vec<crate::motion::ExplodedView>,
    /// Assembly constraints (`constraints.rs`), compiled into joints each solve.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<crate::constraints::Constraint>,
}

impl Assembly {
    /// Every expression the joints, motion links and constraints keep, with the joint (or the
    /// driven joint of a link, or the constraint) and what it sets: `offset`, `angle`, a value's
    /// name, a coupling's length, or a constraint's `offset`, `min` or `max`.
    pub fn exprs(&self) -> Vec<(u64, String, &str)> {
        let mut out = Vec::new();
        for j in &self.joints {
            let x = &j.exprs;
            out.extend(x.offset.as_deref().map(|e| (j.id, "offset".to_string(), e)));
            out.extend(x.angle.as_deref().map(|e| (j.id, "angle".to_string(), e)));
            for (i, e) in x.values.iter().enumerate() {
                if let Some(e) = e {
                    out.push((j.id, j.kind.dofs().get(i).copied().unwrap_or("value").to_string(), e.as_str()));
                }
            }
        }
        for l in &self.links {
            for s in l.coupling.as_ref().map(Coupling::sizes).unwrap_or_default() {
                if let Size::Expr(e) = s {
                    out.push((l.b, "coupling".to_string(), e.as_str()));
                }
            }
        }
        for trace in &self.traces {
            if let crate::motion::TraceMotion::Joint { joint, from, to, .. } = &trace.motion {
                for (label, value) in [("from", from), ("to", to)] {
                    if let serde_json::Value::String(e) = value {
                        out.push((*joint, format!("trace {} {label}", trace.sketch), e.as_str()));
                    }
                }
            }
        }
        for c in &self.constraints {
            let x = &c.exprs;
            for (what, e) in [("offset", &x.offset), ("min", &x.min), ("max", &x.max)] {
                out.extend(e.as_deref().map(|e| (c.id, what.to_string(), e)));
            }
        }
        out
    }

    /// The expressions of `exprs`, to rewrite (a renamed parameter).
    pub fn exprs_mut(&mut self) -> Vec<&mut String> {
        let mut out = Vec::new();
        for j in &mut self.joints {
            let x = &mut j.exprs;
            out.extend(x.offset.as_mut());
            out.extend(x.angle.as_mut());
            out.extend(x.values.iter_mut().flatten());
        }
        for l in &mut self.links {
            for s in l.coupling.as_mut().map(Coupling::sizes_mut).unwrap_or_default() {
                if let Size::Expr(e) = s {
                    out.push(e);
                }
            }
        }
        for trace in &mut self.traces {
            if let crate::motion::TraceMotion::Joint { from, to, .. } = &mut trace.motion {
                for value in [from, to] {
                    if let serde_json::Value::String(e) = value {
                        out.push(e);
                    }
                }
            }
        }
        for c in &mut self.constraints {
            let x = &mut c.exprs;
            out.extend([x.offset.as_mut(), x.min.as_mut(), x.max.as_mut()].into_iter().flatten());
        }
        out
    }

    /// Does the design have anything to solve: joints, or constraints (R115)? Every re-solve
    /// site asks this, so a constraints-only assembly re-solves as a jointed one does.
    pub fn solves(&self) -> bool {
        !self.joints.is_empty() || !self.constraints.is_empty()
    }

    pub fn is_empty(&self) -> bool {
        self.joints.is_empty()
            && self.origins.is_empty()
            && self.links.is_empty()
            && self.contact_sets.is_empty()
            && !self.contact_enabled
            && !self.contact_all
            && self.studies.is_empty()
            && self.traces.is_empty()
            && self.exploded.is_empty()
            && self.constraints.is_empty()
    }
}

/// A frame: origin and orthonormal axes, as a transform (local → parent).
fn frame(o: Vec3, z: Vec3, x: Option<Vec3>) -> Option<Mat> {
    let z = z.normalized()?;
    let x = x.and_then(|x| (x - z * x.dot(z)).normalized()).unwrap_or_else(|| z.any_perp());
    let y = z.cross(x).normalized()?;
    Some([[x.x, x.y, x.z, 0.0], [y.x, y.y, y.z, 0.0], [z.x, z.y, z.z, 0.0], [o.x, o.y, o.z, 1.0]])
}

fn rot_z(a: f64) -> Mat {
    let (s, c) = a.sin_cos();
    [[c, s, 0.0, 0.0], [-s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
}
fn rot_x(a: f64) -> Mat {
    let (s, c) = a.sin_cos();
    [[1.0, 0.0, 0.0, 0.0], [0.0, c, s, 0.0], [0.0, -s, c, 0.0], [0.0, 0.0, 0.0, 1.0]]
}
fn rot_y(a: f64) -> Mat {
    let (s, c) = a.sin_cos();
    [[c, 0.0, -s, 0.0], [0.0, 1.0, 0.0, 0.0], [s, 0.0, c, 0.0], [0.0, 0.0, 0.0, 1.0]]
}
fn shift(x: f64, y: f64, z: f64) -> Mat {
    let mut m = IDENTITY;
    m[3] = [x, y, z, 1.0];
    m
}

/// The joint's motion in A's frame for its values.
pub fn motion(kind: JointKind, q: &[f64]) -> Mat {
    let v = |i: usize| q.get(i).copied().unwrap_or(0.0);
    match kind {
        JointKind::Rigid => IDENTITY,
        JointKind::Revolute => rot_z(v(0)),
        JointKind::Slider => shift(0.0, 0.0, v(0)),
        JointKind::Cylindrical => mat_mul(&rot_z(v(0)), &shift(0.0, 0.0, v(1))),
        JointKind::PinSlot => mat_mul(&shift(v(1), 0.0, 0.0), &rot_z(v(0))),
        JointKind::Planar => mat_mul(&shift(v(0), v(1), 0.0), &rot_z(v(2))),
        JointKind::Ball => mat_mul(&mat_mul(&rot_z(v(0)), &rot_y(v(1))), &rot_z(v(2))),
        JointKind::Parallel => mat_mul(&shift(v(0), v(1), v(2)), &rot_z(v(3))),
        JointKind::Perpendicular | JointKind::Angle => mat_mul(&shift(v(0), v(1), v(2)), &rot_z(v(3))),
        // Their turn is about B's origin, after the offset (`tail`).
        JointKind::PointOnPlane => shift(v(0), v(1), 0.0),
        JointKind::PointOnLine => shift(0.0, 0.0, v(0)),
        JointKind::LineOnPlane => mat_mul(&shift(v(0), v(1), 0.0), &rot_z(v(2))),
        JointKind::Free => mat_mul(&shift(v(0), v(1), v(2)), &rot_vector(Vec3::new(v(3), v(4), v(5)))),
    }
}

/// The turn about `r`'s direction by its length (radians): Rodrigues' formula, with the series
/// near 0 so a turn of a millionth of a radian keeps its digits.
fn rot_vector(r: Vec3) -> Mat {
    let t2 = r.len2();
    let t = t2.sqrt();
    let (a, b) = if t < 1e-4 { (1.0 - t2 / 6.0, 0.5 - t2 / 24.0) } else { (t.sin() / t, (1.0 - t.cos()) / t2) };
    // R = I + a K + b K², K the cross-product matrix of r (row i, column j).
    let k = [[0.0, -r.z, r.y], [r.z, 0.0, -r.x], [-r.y, r.x, 0.0]];
    let mut m = IDENTITY;
    for (i, row) in k.iter().enumerate() {
        for j in 0..3 {
            let k2: f64 = (0..3).map(|n| row[n] * k[n][j]).sum();
            m[j][i] += a * row[j] + b * k2;
        }
    }
    m
}

/// A rigid transform's translation and rotation vector: `motion(Free, ..)`'s values for it.
fn free_values(m: &Mat) -> Vec<f64> {
    // Row i, column j of the rotation.
    let r = |i: usize, j: usize| m[j][i];
    let c = ((r(0, 0) + r(1, 1) + r(2, 2) - 1.0) / 2.0).clamp(-1.0, 1.0);
    // 2 sin θ times the axis.
    let v = Vec3::new(r(2, 1) - r(1, 2), r(0, 2) - r(2, 0), r(1, 0) - r(0, 1));
    let s = v.len() / 2.0;
    let t = s.atan2(c);
    let rot = if s > 1e-6 {
        v * (t / (2.0 * s))
    } else if c > 0.0 {
        // Near no turn: θ / (2 sin θ) → 1/2.
        v * 0.5
    } else {
        // Near a half turn: the axis from the symmetric part, R ≈ 2kkᵀ − I, its sign from v.
        let d = [r(0, 0), r(1, 1), r(2, 2)];
        let i = if d[0] >= d[1] && d[0] >= d[2] {
            0
        } else if d[1] >= d[2] {
            1
        } else {
            2
        };
        let ki = ((d[i] + 1.0) / 2.0).max(0.0).sqrt().max(1e-12);
        let mut k = [0.0; 3];
        for (j, slot) in k.iter_mut().enumerate() {
            *slot = if j == i { ki } else { (r(i, j) + r(j, i)) / (4.0 * ki) };
        }
        let k = Vec3::new(k[0], k[1], k[2]).normalized().unwrap_or(Vec3::Z);
        let k = if k.dot(v) < 0.0 { k * -1.0 } else { k };
        k * t
    };
    vec![m[3][0], m[3][1], m[3][2], rot.x, rot.y, rot.z]
}

fn turn_zyx(yaw: f64, pitch: f64, roll: f64) -> Mat {
    mat_mul(&mat_mul(&rot_z(yaw), &rot_y(pitch)), &rot_x(roll))
}

/// What follows the mate (`flip_of`) in a joint's link: for a relation that tilts B's z, the tilt
/// and B's turn about its own z (`spin`); for a point relation, B's turn about its origin (yaw,
/// pitch and roll about z, y and x: free of gimbal lock at the start, all 0); nothing for the
/// other joints.
fn tail(j: &Joint, q: &[f64]) -> Mat {
    let v = |i: usize| q.get(i).copied().unwrap_or(0.0);
    match j.kind {
        JointKind::Perpendicular => mat_mul(&rot_x(std::f64::consts::FRAC_PI_2), &rot_z(v(4))),
        JointKind::Angle => mat_mul(&rot_x(j.angle), &rot_z(v(4))),
        JointKind::LineOnPlane => mat_mul(&rot_y(std::f64::consts::FRAC_PI_2), &rot_z(v(3))),
        JointKind::PointOnPlane => turn_zyx(v(2), v(3), v(4)),
        JointKind::PointOnLine => turn_zyx(v(1), v(2), v(3)),
        _ => IDENTITY,
    }
}

/// B's joint frame in A's for the joint's values: `M(q) · Flip · tail`.
fn link_of(j: &Joint, q: &[f64]) -> Mat {
    mat_mul(&mat_mul(&motion(j.kind, q), &flip_of(j)), &tail(j, q))
}

/// Bodies of a component in the model (its own frame).
pub(crate) fn component_bodies<'a>(doc: &Document, st: &'a ModelState, component: u64) -> Vec<&'a crate::ModelBody> {
    st.bodies.iter().filter(|b| doc.model_body_component(b) == component).collect()
}

/// The component an occurrence places (0 for the root).
pub(crate) fn component_of(doc: &Document, occ: u64) -> Result<u64> {
    if occ == 0 {
        return Ok(0);
    }
    doc.occurrences.iter().find(|o| o.id == occ).map(|o| o.component).ok_or_else(|| DocError::Unknown(format!("occurrence {occ}")))
}

/// A snap's pick point moved onto its named face (an error when the face is gone: a joint must
/// not quietly attach elsewhere).
fn named_pick(doc: &Document, st: &ModelState, pick: Vec3, face: Option<&str>) -> Result<Vec3> {
    match face {
        Some(n) => face::named_face(doc, st, pick, n).map(|f| f.point),
        None => Ok(pick),
    }
}

/// The centre and outward normal of the planar face nearest `pick` (component coordinates; a
/// body moved in from another component placed by its offset, as a named face is).
fn nearest_plane(doc: &Document, st: &ModelState, o: &JointOrigin, pick: Vec3) -> Result<(Vec3, Vec3)> {
    let comp = component_of(doc, o.occurrence)?;
    let mut best: Option<(f64, Vec3, Vec3)> = None;
    for b in component_bodies(doc, st, comp) {
        let offset = doc.model_body_offset(b);
        for f in b.joint_faces()?.iter() {
            let Some(n) = f.plane_normal else { continue };
            let (c, n) = match offset {
                Some(m) => (apply_point(m, f.centroid), apply_vector(m, n)),
                None => (f.centroid, n),
            };
            let d = (pick - c).dot(n).abs() + 1e-3 * (pick - c).len();
            if best.as_ref().is_none_or(|(bd, _, _)| d < *bd) {
                best = Some((d, c, n));
            }
        }
    }
    best.map(|(_, c, n)| (c, n)).ok_or_else(|| DocError::Invalid("joint origin: no planar face there".into()))
}

/// Resolve a joint origin to a frame in its component's coordinates.
pub fn resolve_origin(doc: &Document, st: &ModelState, o: &JointOrigin) -> Result<Mat> {
    let bad = || DocError::Invalid("joint origin: degenerate frame".into());
    match &o.snap {
        Snap::Frame { origin, z, x } => frame(*origin, *z, *x).ok_or_else(bad),
        Snap::Point { point, z, x } => frame(*point, z.unwrap_or(Vec3::Z), *x).ok_or_else(bad),
        Snap::FaceCenter { pick, face, x } => {
            // A named planar face gives its own centre and normal; another face (or none) the
            // planar face nearest the point.
            let named = match face.as_deref() {
                Some(n) => Some(face::named_face(doc, st, *pick, n)?),
                None => None,
            };
            let (c, n) = match named.as_ref().map(face::NamedFace::plane).transpose()?.flatten() {
                Some(cn) => cn,
                None => nearest_plane(doc, st, o, named.as_ref().map_or(*pick, |f| f.point))?,
            };
            frame(c, n, face::x_axis(doc, st, x.as_ref(), c)?).ok_or_else(bad)
        }
        Snap::Edge { .. } => edge::edge_frame(doc, st, component_of(doc, o.occurrence)?, &o.snap),
        Snap::CircleCenter { x, .. } => {
            let r = round_of(doc, st, o)?;
            frame(r.centre, r.z, face::x_axis(doc, st, x.as_ref(), r.centre)?).ok_or_else(bad)
        }
    }
}

/// A circle-centre snap's cylinder and the end it takes (component coordinates): through its
/// named face when it names a cylinder, else the cylindrical face at its pick and the end nearer
/// it, as older builds found them. Any other snap is refused.
fn round_of(doc: &Document, st: &ModelState, o: &JointOrigin) -> Result<face::Round> {
    let Snap::CircleCenter { pick, face, end, .. } = &o.snap else { return Err(DocError::Invalid("pick a round face".into())) };
    if let Some(n) = face.as_deref()
        && let Some(r) = face::named_round(doc, st, *pick, n, end.as_deref())?
    {
        return Ok(r);
    }
    let pick = named_pick(doc, st, *pick, face.as_deref())?;
    let comp = component_of(doc, o.occurrence)?;
    // A body moved in from another component: looked at in its own coordinates, its cylinder
    // placed by its offset (as a named face is).
    let cyl = component_bodies(doc, st, comp)
        .into_iter()
        .filter_map(|b| match doc.model_body_offset(b) {
            Some(m) => solidcraft_kernel::cylinder_face_at(&b.body, apply_point(&mat_inverse(m)?, pick)).map(|c| face::placed(m, c)),
            None => solidcraft_kernel::cylinder_face_at(&b.body, pick),
        })
        .min_by(|a, b| {
            let off = |c: &solidcraft_kernel::CylinderFace| {
                let v = pick - c.axis_point;
                ((v - c.axis * v.dot(c.axis)).len() - c.radius).abs()
            };
            off(a).total_cmp(&off(b))
        })
        .ok_or_else(|| DocError::Invalid("joint origin: no cylindrical face there".into()))?;
    let s = (pick - cyl.axis_point).dot(cyl.axis);
    let (at, dir) = if (s - cyl.start).abs() <= (cyl.end - s).abs() { (cyl.start, -1.0) } else { (cyl.end, 1.0) };
    Ok(face::Round { cylinder: cyl, centre: cyl.axis_point + cyl.axis * at, z: cyl.axis * dir })
}

/// The cylindrical face of a circle-centre snap (component coordinates).
fn circle_at(doc: &Document, st: &ModelState, o: &JointOrigin) -> Result<solidcraft_kernel::CylinderFace> {
    round_of(doc, st, o).map(|r| r.cylinder)
}

/// World transform of an occurrence (through its parents).
pub fn occurrence_world(doc: &Document, occ: u64) -> Mat {
    let mut m = IDENTITY;
    let mut cur = doc.occurrences.iter().find(|o| o.id == occ);
    for _ in 0..1000 {
        let Some(o) = cur else { break };
        m = mat_mul(&o.transform, &m);
        if o.parent == 0 {
            break;
        }
        cur = doc.occurrence_of(o.parent);
    }
    m
}

/// Clamp values to the joint's limits.
pub fn clamp(j: &Joint, q: &mut [f64]) {
    for (i, v) in q.iter_mut().enumerate() {
        if let Some(Some((lo, hi))) = j.limits.get(i) {
            *v = v.clamp(lo.min(*hi), hi.max(*lo));
        }
    }
}

/// Work requested after placing occurrences and checking their joint constraints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Analysis {
    /// Placement, loop closure, conflicts and errors only; `Solution::dof` is empty.
    Placement,
    /// Also count each occurrence's remaining motion, including nearby singular poses.
    DegreesOfFreedom,
}

/// The result of placing occurrences by their joints.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Solution {
    /// New world transforms of occurrences the joints place.
    #[serde(skip)]
    pub world: BTreeMap<u64, Mat>,
    /// New values of the joints that moved to close loops (joint id → values).
    #[serde(skip)]
    pub values: BTreeMap<u64, Vec<f64>>,
    /// Joints that disagree with the placement found through others: (joint id, name, why).
    pub conflicts: Vec<(u64, String, String)>,
    /// Each conflict's gap as measured (R145): (mm, rad); `(NaN, NaN)` for a loop that meets but
    /// can't stay closed on the way. Read this, never the message text.
    #[serde(skip)]
    pub conflict_gaps: BTreeMap<u64, (f64, f64)>,
    /// Joints that could not be applied (bad origin…): (joint id, name, why).
    pub errors: Vec<(u64, String, String)>,
    /// Remaining degrees of freedom per occurrence (6 = free, 0 = fixed): how many independent
    /// values still move it, from the fixed occurrences, with loops kept closed and a value a
    /// motion link sets moving with the values its link chain starts from (a driven gear has its
    /// driving gear's one, a screw's travel its turn's). Empty for [`Analysis::Placement`].
    pub dof: BTreeMap<u64, usize>,
    /// Occurrences fixed in place (grounded, or the root's).
    pub fixed: Vec<u64>,
    /// The groups of loops the solve took on, in the order it took them (empty when no joint
    /// closes a loop).
    #[serde(skip)]
    pub groups: Vec<LoopGroup>,
    /// The final values of every compiled joint (constraints, R114): a tree member's seeded
    /// values, a closing one's loop-solved values; a locked Insert's turn; nothing for an
    /// equation (it keeps no values). Written into each owner's `state` by
    /// `apply_joint_solution`, in member order. The `Free` members are not here.
    #[serde(skip)]
    pub compiled: BTreeMap<u64, Vec<f64>>,
    /// Each compiled joint's kind and flip, what its `compiled` values mean (R132).
    #[serde(skip)]
    pub compiled_signatures: BTreeMap<u64, crate::constraints::StateSignature>,
}

/// One group of loops solved together: its closing joints, how many values it moves, and whether
/// a cap (`MAX_CLOSING`, `MAX_LOOPS`, `MAX_LOOP_VALUES` or `MAX_LOOP_WORK`) left it unsolved, so
/// it is only checked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoopGroup {
    /// The group's loop-closing joint ids.
    pub members: Vec<u64>,
    /// The values the solve moves to close it (0 when it was skipped before they were counted).
    pub values: usize,
    /// Skipped or stopped by a cap: checked, not solved.
    pub capped: bool,
}

/// Joint values by joint id.
type Values = BTreeMap<u64, Vec<f64>>;

/// How far two placements differ: (translation mm, rotation rad).
fn mismatch(a: &Mat, b: &Mat) -> (f64, f64) {
    let t = Vec3::new(a[3][0] - b[3][0], a[3][1] - b[3][1], a[3][2] - b[3][2]).len();
    // The turn between them from both its cosine and its sine: the cosine alone loses small
    // angles to rounding.
    let c = |i: usize, k: usize| (0..3).map(|n| a[i][n] * b[k][n]).sum::<f64>();
    let sin2 = Vec3::new(c(1, 2) - c(2, 1), c(2, 0) - c(0, 2), c(0, 1) - c(1, 0)).len();
    (t, sin2.atan2(c(0, 0) + c(1, 1) + c(2, 2) - 1.0))
}

/// Passes over the motion links at most: a chain of up to this many links carries its value
/// through, whatever order its links are stored in.
const MAX_LINK_PASSES: usize = 64;

/// Motion links, pass after pass until nothing changes (a chain stored from its end takes a pass
/// per link), each linked value within its joint's limits as it is set. The result is the same
/// whatever the values were before and however the links are stored, so applying them again
/// changes nothing (links that form a cycle stop after `MAX_LINK_PASSES`).
fn apply_links(asm: &Assembly, vals: &mut Values) {
    let limits: BTreeMap<(u64, usize), (f64, f64)> = asm
        .links
        .iter()
        .filter_map(|l| {
            let (lo, hi) = asm.joints.iter().find(|j| j.id == l.b)?.limits.get(l.ib).copied().flatten()?;
            Some(((l.b, l.ib), (lo.min(hi), hi.max(lo))))
        })
        .collect();
    // The linked values, to see whether a pass changed any (several links writing one value
    // change it within a pass, but settle from one pass to the next).
    let linked = |vals: &Values| -> Vec<Option<f64>> { asm.links.iter().map(|l| vals.get(&l.b).and_then(|q| q.get(l.ib)).copied()).collect() };
    for _ in 0..MAX_LINK_PASSES {
        let before = linked(vals);
        for l in &asm.links {
            let Some(src) = vals.get(&l.a).and_then(|q| q.get(l.ia)).copied() else { continue };
            let mut next = l.follow(src);
            // A hand-edited table (or source) that gives no number leaves b where it is.
            if !next.is_finite() {
                continue;
            }
            if let Some((lo, hi)) = limits.get(&(l.b, l.ib)) {
                next = next.max(*lo).min(*hi);
            }
            if let Some(v) = vals.get_mut(&l.b).and_then(|q| q.get_mut(l.ib)) {
                *v = next;
            }
        }
        if linked(vals) == before {
            break;
        }
    }
}

/// Effective values of every joint after motion links and limits; those of the `compiled` joints
/// (constraints) too: the owner's `state` when it fits (as many values as the joint has, all
/// finite), else the joint's own (`compile`'s nearest-value start). Callers with no solve to
/// make pass `&Compiled::default()`.
pub fn joint_values(asm: &Assembly, compiled: &Compiled) -> BTreeMap<u64, Vec<f64>> {
    let mut vals: Values = asm
        .joints
        .iter()
        .map(|j| {
            let mut q = j.values.clone();
            q.resize(j.kind.dofs().len(), 0.0);
            (j.id, q)
        })
        .collect();
    if !compiled.joints.is_empty() {
        let owners: BTreeMap<u64, &crate::constraints::Constraint> = asm.constraints.iter().map(|c| (c.id, c)).collect();
        // Each constraint's joint members in member (id) order.
        let mut members: BTreeMap<u64, usize> = BTreeMap::new();
        let mut sorted: Vec<&Joint> = compiled.joints.iter().collect();
        sorted.sort_by_key(|j| j.id);
        for j in sorted {
            let n = j.kind.dofs().len();
            let owner = owner_of(j.id).and_then(|c| owners.get(&c));
            let member = owner.map(|c| {
                let k = members.entry(c.id).or_insert(0);
                *k += 1;
                *k - 1
            });
            // R132: only a state solved for this joint's kind and flip.
            let state = owner.zip(member).and_then(|(c, m)| c.state_for(m, n, crate::constraints::StateSignature::of(j)));
            let mut q = state.map_or_else(|| j.values.clone(), <[f64]>::to_vec);
            q.resize(n, 0.0);
            // A limited constraint's value inside its limits (a limit's parameter may move).
            clamp(j, &mut q);
            vals.insert(j.id, q);
        }
    }
    apply_links(asm, &mut vals);
    for j in &asm.joints {
        if let Some(q) = vals.get_mut(&j.id) {
            clamp(j, q);
        }
    }
    vals
}

/// The turn and offset from A's joint frame to B's: mated (B's z against A's, as faces meet)
/// unless flipped.
fn flip_of(j: &Joint) -> Mat {
    // An angle relation's `angle` is its tilt (`tail`); a perpendicular one has none.
    let turn = if matches!(j.kind, JointKind::Angle | JointKind::Perpendicular) { 0.0 } else { j.angle };
    let mut m = rot_z(turn);
    m[3][2] = j.offset;
    if !j.flip { mat_mul(&m, &[[1.0, 0.0, 0.0, 0.0], [0.0, -1.0, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]) } else { m }
}

/// Component-space frames stay fixed throughout one solve. Cache their inverses without
/// regrouping products: reversed joints still invert the value-dependent link as before.
struct JointFrames {
    a: Mat,
    b: Mat,
    inverse_a: Option<Mat>,
    inverse_b: Option<Mat>,
}

impl JointFrames {
    fn new(a: Mat, b: Mat) -> Self {
        Self { a, b, inverse_a: mat_inverse(&a), inverse_b: mat_inverse(&b) }
    }

    fn through(&self, joint: &Joint, values: &[f64], from_a: bool, t: &Mat) -> Option<Mat> {
        let link = mat_mul(&link_of(joint, values), self.inverse_b.as_ref()?);
        if from_a { Some(mat_mul(&mat_mul(t, &self.a), &link)) } else { Some(mat_mul(&mat_mul(t, &mat_inverse(&link)?), self.inverse_a.as_ref()?)) }
    }
}

/// B's world transform from A's (or A's from B's, going the other way) for the joint's values.
fn through(j: &Joint, (fa, fb): &(Mat, Mat), q: &[f64], from_a: bool, t: &Mat) -> Option<Mat> {
    JointFrames::new(*fa, *fb).through(j, q, from_a, t)
}

/// The spanning tree the solver places occurrences along, out from the fixed ones.
struct Tree<'a> {
    joints: BTreeMap<u64, &'a Joint>,
    /// The loop equations of the solve (`Closing::Equation` indexes them).
    equations: &'a [Eq],
    /// Joint frames in component coordinates (A's, B's).
    frames: BTreeMap<u64, JointFrames>,
    fixed: BTreeMap<u64, Mat>,
    /// Placed occurrence → (the occurrence it is placed from, the joint, from the joint's A side).
    parent: BTreeMap<u64, (u64, u64, bool)>,
    /// Placed occurrences in placement order.
    order: Vec<u64>,
}

impl Tree<'_> {
    /// A closing member's id: the joint's, or the equation's (a compiled id).
    fn closing_id(&self, c: &Closing) -> Option<u64> {
        match c {
            Closing::Joint(id) => Some(*id),
            Closing::Equation(i) => self.equations.get(*i).map(|e| e.id),
        }
    }

    /// Every closing member's id, in order.
    fn closing_ids(&self, closing: &[Closing]) -> Vec<u64> {
        closing.iter().filter_map(|c| self.closing_id(c)).collect()
    }

    /// How many residuals a closing member gives `closure_gaps`: three points of a joint's frame,
    /// one number for an equation.
    fn width(c: &Closing) -> usize {
        match c {
            Closing::Joint(_) => 9,
            Closing::Equation(_) => 1,
        }
    }

    /// The occurrences a closing member ties: a joint's two sides, an equation's occurrences.
    fn closing_occurrences(&self, c: &Closing) -> Vec<u64> {
        match c {
            Closing::Joint(id) => self.joints.get(id).map(|j| vec![j.a.occurrence, j.b.occurrence]).unwrap_or_default(),
            Closing::Equation(i) => self.equations.get(*i).map(|e| e.eq.occurrences()).unwrap_or_default(),
        }
    }

    fn step(&self, joint: u64, from_a: bool, t: &Mat, vals: &Values) -> Option<Mat> {
        #[cfg(test)]
        tests::PLACEMENT_STEPS.with(|steps| steps.set(steps.get() + 1));
        let q = vals.get(&joint).map(Vec::as_slice).unwrap_or(&[]);
        self.frames.get(&joint)?.through(self.joints.get(&joint)?, q, from_a, t)
    }

    /// Where every placed occurrence is for these values.
    fn place(&self, vals: &Values) -> BTreeMap<u64, Mat> {
        let mut placed = self.fixed.clone();
        for occ in &self.order {
            let Some(&(from, joint, from_a)) = self.parent.get(occ) else { continue };
            if let Some(m) = placed.get(&from).and_then(|t| self.step(joint, from_a, t, vals)) {
                placed.insert(*occ, m);
            }
        }
        placed
    }

    /// The fixed occurrence an occurrence is placed from, and the tree joints from it up to that one.
    fn path(&self, mut occ: u64) -> (u64, Vec<(u64, bool)>) {
        let mut up = Vec::new();
        while let Some(&(from, joint, from_a)) = self.parent.get(&occ) {
            if up.len() > self.order.len() {
                break;
            }
            up.push((joint, from_a));
            occ = from;
        }
        (occ, up)
    }

    /// One occurrence's world placement for these values.
    fn world(&self, occ: u64, vals: &Values) -> Option<Mat> {
        let (root, up) = self.path(occ);
        let mut t = *self.fixed.get(&root)?;
        for (joint, from_a) in up.iter().rev() {
            t = self.step(*joint, *from_a, &t, vals)?;
        }
        Some(t)
    }
}

/// Storage shared by one residual callback's evaluations, scoped to its immutable tree.
/// Keep map nodes and path capacity, but invalidate every placement before reading new values.
struct PlacementWorkspace<'tree, 'doc> {
    tree: &'tree Tree<'doc>,
    placed: BTreeMap<u64, Option<Mat>>,
    path: Vec<(u64, u64, bool)>,
}

impl<'tree, 'doc> PlacementWorkspace<'tree, 'doc> {
    fn new(tree: &'tree Tree<'doc>) -> Self {
        Self { tree, placed: BTreeMap::new(), path: Vec::new() }
    }

    fn at<'values>(&'values mut self, values: &'values Values) -> Placements<'values, 'tree, 'doc> {
        for placed in self.placed.values_mut() {
            *placed = None;
        }
        Placements { workspace: self, values }
    }
}

/// Only this guard can read placements. Its values stay immutable until it is dropped;
/// obtaining the next guard clears validity while retaining the allocated storage.
struct Placements<'values, 'tree, 'doc> {
    workspace: &'values mut PlacementWorkspace<'tree, 'doc>,
    values: &'values Values,
}

impl Placements<'_, '_, '_> {
    fn world(&mut self, mut occurrence: u64) -> Option<Mat> {
        self.workspace.path.clear();
        let mut t = loop {
            if let Some(t) = self.workspace.placed.get(&occurrence).and_then(Option::as_ref).or_else(|| self.workspace.tree.fixed.get(&occurrence)) {
                break *t;
            }
            if self.workspace.path.len() >= self.workspace.tree.order.len() {
                return None;
            }
            let &(from, joint, from_a) = self.workspace.tree.parent.get(&occurrence)?;
            self.workspace.path.push((occurrence, joint, from_a));
            occurrence = from;
        };
        while let Some((occurrence, joint, from_a)) = self.workspace.path.pop() {
            t = self.workspace.tree.step(joint, from_a, &t, self.values)?;
            #[cfg(test)]
            if !self.workspace.placed.contains_key(&occurrence) {
                tests::PLACEMENT_INSERTS.with(|inserts| inserts.set(inserts.get() + 1));
            }
            self.workspace.placed.insert(occurrence, Some(t));
        }
        Some(t)
    }

    fn closure_gaps(&mut self, closing: &[Closing], scale: f64, out: &mut Vec<f64>) -> Option<()> {
        for c in closing {
            let id = match c {
                Closing::Joint(id) => id,
                Closing::Equation(i) => {
                    // One number: an angle weighed as far as it turns a point across the loop.
                    let e = self.workspace.tree.equations.get(*i)?;
                    let mut at: Vec<(u64, Mat)> = Vec::new();
                    for o in e.eq.occurrences() {
                        let m = self.world(o)?;
                        at.push((o, m));
                    }
                    let r = e.eq.residual(&|o| at.iter().find(|(k, _)| *k == o).map(|(_, m)| *m))?;
                    out.push(if e.eq.is_angle() { r * scale } else { r });
                    continue;
                }
            };
            let (joint, frames) = (self.workspace.tree.joints.get(id)?, self.workspace.tree.frames.get(id)?);
            let tb = self.world(joint.b.occurrence)?;
            let ta = self.world(joint.a.occurrence)?;
            let want = self.workspace.tree.step(*id, true, &ta, self.values)?;
            for p in [Vec3::ZERO, Vec3::X * scale, Vec3::Y * scale] {
                let at = apply_point(&frames.b, p);
                let d = apply_point(&want, at) - apply_point(&tb, at);
                out.extend([d.x, d.y, d.z]);
            }
        }
        Some(())
    }
}

/// Values the loop solver moves at most in one group of loops, and loop-closing joints it takes
/// on at most: larger ones are only checked.
const MAX_LOOP_VALUES: usize = 64;
const MAX_LOOPS: usize = 100;
/// Loop-closing joints one solve takes on at most, over all its groups: a part placed by several
/// relations closes a loop with each one after the first, so a panel of parts has many small
/// groups.
const MAX_CLOSING: usize = 5_000;
/// Residual evaluations one solve may spend on all its loops.
const MAX_LOOP_WORK: usize = 200_000;

/// Held joint values by (joint, value index): the value each moves from.
pub type Held = BTreeMap<(u64, usize), f64>;

/// The values that set the `around` joints' values through motion links, followed back through
/// every link to them (several links may write one value; `apply_links`' order decides which
/// wins on the way) for as many links as `apply_links` passes: all the values reached, and those
/// that no link sets (the ones a solve can move).
fn link_chains(asm: &Assembly, tree: &Tree, around: &BTreeSet<u64>, linked: &BTreeSet<(u64, usize)>) -> (BTreeSet<(u64, usize)>, Vec<(u64, usize)>) {
    let (mut feeds, mut starts) = (BTreeSet::new(), Vec::new());
    let mut frontier: BTreeSet<(u64, usize)> =
        around.iter().filter_map(|id| tree.joints.get(id)).flat_map(|j| (0..j.kind.dofs().len()).map(|i| (j.id, i))).collect();
    let mut seen = frontier.clone();
    for _ in 0..MAX_LINK_PASSES {
        let next: BTreeSet<(u64, usize)> =
            asm.links.iter().filter(|l| frontier.contains(&(l.b, l.ib))).map(|l| (l.a, l.ia)).filter(|v| !seen.contains(v)).collect();
        if next.is_empty() {
            break;
        }
        for &(id, i) in &next {
            feeds.insert((id, i));
            if !linked.contains(&(id, i)) && tree.joints.get(&id).is_some_and(|j| i < j.kind.dofs().len()) {
                starts.push((id, i));
            }
        }
        seen.extend(next.iter().copied());
        frontier = next;
    }
    (feeds, starts)
}

/// The values that move each value a motion link sets: the values its link chain starts from,
/// which no link sets (none for a chain that comes back round on itself, which only a hand-edited
/// design has). A driven gear's turn is its driving gear's: one degree of freedom, not two.
fn link_sources(asm: &Assembly) -> BTreeMap<(u64, usize), BTreeSet<(u64, usize)>> {
    let mut into: BTreeMap<(u64, usize), Vec<(u64, usize)>> = BTreeMap::new();
    for l in &asm.links {
        into.entry((l.b, l.ib)).or_default().push((l.a, l.ia));
    }
    let mut sources = BTreeMap::new();
    if into.is_empty() {
        return sources;
    }
    // A link from a value its joint doesn't have (a hand-edited design) moves nothing.
    let real: BTreeSet<(u64, usize)> = asm.joints.iter().flat_map(|j| (0..j.kind.dofs().len()).map(|i| (j.id, i))).collect();
    for &v in into.keys() {
        // Back along the links, as far as a link chain carries a value (`MAX_LINK_PASSES`).
        let (mut from, mut seen) = (BTreeSet::new(), BTreeSet::from([v]));
        let mut frontier = vec![v];
        for _ in 0..MAX_LINK_PASSES {
            let mut next = Vec::new();
            for w in frontier.iter().filter_map(|w| into.get(w)).flatten() {
                if !seen.insert(*w) {
                    continue;
                }
                if into.contains_key(w) {
                    next.push(*w);
                } else if real.contains(w) {
                    from.insert(*w);
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        sources.insert(v, from);
    }
    sources
}

/// The held values among `touches` that move: (joint, index, from, to), `from` within the
/// joint's limits. A suppressed joint still sets values through its motion links.
fn held_moves(tree: &Tree, held: &Held, touches: &BTreeSet<(u64, usize)>, vals: &Values) -> Vec<(u64, usize, f64, f64)> {
    let mut moves = Vec::new();
    // The tree's joints: saved and compiled ones alike (a constraint's values can be held).
    for ((id, i), from) in held.iter().filter(|(v, _)| touches.contains(v)) {
        let (Some(j), Some(to)) = (tree.joints.get(id), vals.get(id).and_then(|q| q.get(*i))) else { continue };
        let from = match j.limits.get(*i).copied().flatten() {
            Some((lo, hi)) => from.max(lo.min(hi)).min(hi.max(lo)),
            None => *from,
        };
        if from != *to {
            moves.push((*id, *i, from, *to));
        }
    }
    moves
}

/// Close the loops, each group of loops that share joints on its own: see `close_group`.
/// Returns the moved joints' new stored values (and updates `vals`), and the closing joints of
/// the groups whose held values moved but which can't stay closed on the way (nothing in those
/// moves; a group that can't close without moving is left to the conflict check).
fn close_loops(
    tree: &Tree,
    closing: &[Closing],
    asm: &Assembly,
    held: &Held,
    vals: &mut Values,
    report: &mut Vec<LoopGroup>,
) -> (Values, BTreeSet<u64>) {
    let (mut changed, mut stuck) = (BTreeMap::new(), BTreeSet::new());
    if closing.len() > MAX_CLOSING {
        report.push(LoopGroup { members: tree.closing_ids(closing), values: 0, capped: true });
        return (Values::new(), stuck);
    }
    let linked = set_values(asm);
    let groups = loop_groups(tree, closing, asm, &linked);
    let spent = std::cell::Cell::new(0usize);
    for (group, around, touches) in &groups {
        let vars = unknowns(tree, around, asm, &linked, held);
        if group.len() > MAX_LOOPS || vars.len() > MAX_LOOP_VALUES {
            report.push(LoopGroup { members: tree.closing_ids(group), values: vars.len(), capped: true });
            continue;
        }
        let moves = held_moves(tree, held, touches, vals);
        let closed = close_group(tree, group, around, &vars, &moves, asm, &spent, vals);
        report.push(LoopGroup { members: tree.closing_ids(group), values: vars.len(), capped: spent.get() > MAX_LOOP_WORK });
        match closed {
            Some(m) => changed.extend(m),
            None if !moves.is_empty() => stuck.extend(tree.closing_ids(group)),
            None => {}
        }
    }
    // The stored values with what each group moved (groups may move different values of one
    // joint).
    let mut moved = Values::new();
    for ((id, i), v) in changed {
        let Some(j) = tree.joints.get(&id) else { continue };
        let q = moved.entry(id).or_insert_with(|| {
            let mut q = j.values.clone();
            q.resize(j.kind.dofs().len(), 0.0);
            q
        });
        if let Some(slot) = q.get_mut(i) {
            *slot = v;
        }
    }
    (moved, stuck)
}

/// A group of loops solved together: its closing joints, the joints around them, and the values
/// it touches.
type Group = (Vec<Closing>, BTreeSet<u64>, BTreeSet<(u64, usize)>);

/// The loops' groups. Each loop: its closing joint and the joints on the tree paths between its
/// sides; loops that share a value (of a joint, or one feeding them through links) are solved
/// together.
fn loop_groups(tree: &Tree, closing: &[Closing], asm: &Assembly, linked: &BTreeSet<(u64, usize)>) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    let side = |occ: u64| -> BTreeSet<u64> { tree.path(occ).1.iter().map(|(k, _)| *k).collect() };
    for c in closing {
        let mut around: BTreeSet<u64> = match c {
            Closing::Joint(id) => {
                let Some(j) = tree.joints.get(id) else { continue };
                // A joint between an occurrence and itself (only a hand-edited file has one) is only checked.
                if j.a.occurrence == j.b.occurrence {
                    continue;
                }
                let mut around: BTreeSet<u64> = side(j.a.occurrence).symmetric_difference(&side(j.b.occurrence)).copied().collect();
                around.insert(*id);
                around
            }
            // An equation has no values of its own: the loop is the tree paths between its
            // occurrences (three of them: the union over the pairs).
            Closing::Equation(_) => {
                let occs = tree.closing_occurrences(c);
                let mut around = BTreeSet::new();
                for (k, a) in occs.iter().enumerate() {
                    for b in occs.iter().skip(k + 1) {
                        around.extend(side(*a).symmetric_difference(&side(*b)).copied());
                    }
                }
                around
            }
        };
        // The values the loop depends on: its joints' and those its links read. Loops tie together
        // through a shared value only (a rigid joint on both paths has none; two values of one
        // joint can feed two independent loops).
        let mut touches = link_chains(asm, tree, &around, linked).0;
        touches.extend(around.iter().filter_map(|id| tree.joints.get(id)).flat_map(|j| (0..j.kind.dofs().len()).map(|i| (j.id, i))));
        let mut members = vec![*c];
        groups.retain(|(c, a, t)| {
            if t.is_disjoint(&touches) {
                return true;
            }
            members.extend(c.iter().copied());
            around.extend(a.iter().copied());
            touches.extend(t.iter().copied());
            false
        });
        groups.push((members, around, touches));
    }
    groups
}

/// The values a solve of the loops `around` moves: every value of their joints but the `held`
/// ones and motion-link targets, and the values the targets' link chains start from.
fn unknowns(tree: &Tree, around: &BTreeSet<u64>, asm: &Assembly, linked: &BTreeSet<(u64, usize)>, held: &Held) -> Vec<(u64, usize)> {
    let mut vars: Vec<(u64, usize)> = Vec::new();
    for j in around.iter().filter_map(|id| tree.joints.get(id)) {
        vars.extend((0..j.kind.dofs().len()).map(|i| (j.id, i)).filter(|v| !linked.contains(v) && !held.contains_key(v)));
    }
    for v in link_chains(asm, tree, around, linked).1 {
        if !held.contains_key(&v) && !vars.contains(&v) {
            vars.push(v);
        }
    }
    vars
}

/// Each value's limits, or none.
fn limit_bounds(tree: &Tree, vars: &[(u64, usize)]) -> Vec<(f64, f64)> {
    vars.iter()
        .map(|(id, i)| match tree.joints.get(id).and_then(|j| j.limits.get(*i).copied().flatten()) {
            Some((lo, hi)) => (lo.min(hi), hi.max(lo)),
            None => (f64::NEG_INFINITY, f64::INFINITY),
        })
        .collect()
}

/// The loops' size: the diagonal of the box around their joints' A frames, 1 mm to 1 km. Points
/// that far apart weigh turns like moves.
fn loop_scale(tree: &Tree, around: &BTreeSet<u64>, vals: &Values) -> f64 {
    let joints: Aabb3 = around
        .iter()
        .filter_map(|id| {
            let (j, frames) = (tree.joints.get(id)?, tree.frames.get(id)?);
            Some(apply_point(&mat_mul(&tree.world(j.a.occurrence, vals)?, &frames.a), Vec3::ZERO))
        })
        .collect();
    if joints.is_empty() { 1.0 } else { joints.diagonal().clamp(1.0, 1e6) }
}

/// How far each closing joint is from meeting: three points of B's joint frame, `scale` apart,
/// where the joint wants them against where the tree puts them (appended to `out`).
fn closure_gaps(workspace: &mut PlacementWorkspace, closing: &[Closing], vals: &Values, scale: f64, out: &mut Vec<f64>) -> Option<()> {
    workspace.at(vals).closure_gaps(closing, scale, out)
}

/// How far a closing joint may be off before it is a conflict: its frames apart by more than
/// this many millimetres or turned by more than this many radians.
const CONFLICT_MM: f64 = 1e-6;
const CONFLICT_RAD: f64 = 1e-8;

/// The largest gap (`closure_gaps`, for loops of size `scale`) that still passes the conflict
/// check, with room to spare.
fn loose_gap(scale: f64) -> f64 {
    CONFLICT_MM.min(CONFLICT_RAD * scale) / 4.0
}

/// Close loops whose gaps `f` gives from `x`, and the tolerance to keep them closed to from here
/// on: `tol`, or, where they come no closer than that but within `loose`, a little more than they
/// do come. Faces modelled a few nanometres apart along parallel axes leave a gap no value of a
/// planar loop takes out; the loop still moves, as closed as it is. `None` when they can't close.
fn close_within(f: &mut dyn FnMut(&[f64]) -> Option<Vec<f64>>, x: &mut Vec<f64>, bounds: &[(f64, f64)], tol: f64, loose: f64) -> Option<f64> {
    if least_squares(f, x, bounds, tol) {
        return Some(tol);
    }
    let left = largest(&f(x)?);
    (left <= loose).then(|| (4.0 * left).clamp(tol, loose.max(tol)))
}

/// Occurrences whose degrees of freedom are worked out through their loops, at most; the rest
/// keep the tree's count.
const MAX_DOF_OCCURRENCES: usize = 256;

/// How far `nearby_pose` moves each value: radians for an angle, a share of the loops' size for a
/// distance. Far enough for a part's motion that vanishes at a dead point to show in a rank, near
/// enough to stay on the mechanism's branch.
const NUDGE: f64 = 1e-3;

/// A pose of the loops near `vals`: each value they move nudged by an amount of its own within
/// its limits (so the nudge points in no special direction), then each group of loops closed
/// again. A group that can't close again, or only further away than a few nudges (on another
/// branch), keeps its values. It has a work budget of its own.
fn nearby_pose(tree: &Tree, groups: &[Group], asm: &Assembly, linked: &BTreeSet<(u64, usize)>, vals: &Values) -> Values {
    let mut near = vals.clone();
    let mut spent = 0usize;
    for (closing, around, _) in groups {
        let vars = unknowns(tree, around, asm, linked, &Held::new());
        if vars.is_empty() || vars.len() > MAX_LOOP_VALUES {
            continue;
        }
        let scale = loop_scale(tree, around, vals);
        let step: Vec<f64> =
            vars.iter().map(|(id, i)| NUDGE * if tree.joints.get(id).is_some_and(|j| j.kind.is_angle(*i)) { 1.0 } else { scale }).collect();
        let bounds = limit_bounds(tree, &vars);
        let x0: Vec<f64> = vars.iter().map(|(id, i)| near.get(id).and_then(|q| q.get(*i)).copied().unwrap_or(0.0)).collect();
        let mut x: Vec<f64> = x0
            .iter()
            .zip(step.iter().zip(&bounds))
            .enumerate()
            .map(|(k, (v, (d, (lo, hi))))| {
                // 0.5 to 1.5 nudges, alternately up and down.
                let w = 0.5 + (k as f64 * 0.618_034).fract();
                (v + d * if k % 2 == 0 { w } else { -w }).max(*lo).min(*hi)
            })
            .collect();
        let mut workspace = PlacementWorkspace::new(tree);
        let mut work = near.clone();
        let mut gaps = |x: &[f64]| -> Option<Vec<f64>> {
            spent += 1;
            if spent > MAX_LOOP_WORK {
                return None;
            }
            for ((id, i), v) in vars.iter().zip(x) {
                if let Some(slot) = work.get_mut(id).and_then(|q| q.get_mut(*i)) {
                    *slot = *v;
                }
            }
            apply_links(asm, &mut work);
            let mut out = Vec::with_capacity(closing.len() * 9);
            closure_gaps(&mut workspace, closing, &work, scale, &mut out)?;
            out.iter().all(|v| v.is_finite()).then_some(out)
        };
        if close_within(&mut gaps, &mut x, &bounds, (1e-10 * scale).min(1e-7), loose_gap(scale)).is_some()
            && x.iter().zip(&x0).zip(&step).all(|((a, b), d)| (a - b).abs() <= 4.0 * d)
            && gaps(&x).is_some()
        {
            near = work;
        }
    }
    near
}

/// The degrees of freedom of the occurrences loops constrain: how many independent ways each can
/// still move with every loop kept closed, the rank of its placement over the joint values once
/// the loops' closure is taken out (rank [J_loops; J_placement] - rank J_loops). An occurrence's
/// values are those on its tree path and the unknowns of the loops on that path. It is counted at
/// the pose and at a pose nearby, and the larger count stands: at a dead point of its loop (an
/// actuator in line with the path of its tip pin) a part's motion vanishes to first order, so the
/// pose alone counts 0. The tree's count stands where a loop group is over the solver's caps or
/// the work runs out.
fn loop_dof(
    tree: &Tree,
    closing: &[Closing],
    asm: &Assembly,
    vals: &Values,
    placed: &BTreeMap<u64, Mat>,
    dof: &mut BTreeMap<u64, usize>,
    mut memo: Option<&mut DofMemo>,
) {
    if closing.len() > MAX_CLOSING {
        return;
    }
    let linked = set_values(asm);
    let mut groups = loop_groups(tree, closing, asm, &linked);
    groups.retain(|(c, _, _)| c.len() <= MAX_LOOPS);
    // The joints loops depend on: those around them, those whose values they read, and those a
    // motion link carries any of these values into.
    let mut relevant: BTreeSet<u64> = groups.iter().flat_map(|(_, a, t)| a.iter().copied().chain(t.iter().map(|(j, _)| *j))).collect();
    for _ in 0..MAX_LINK_PASSES {
        let more: Vec<u64> = asm.links.iter().filter(|l| relevant.contains(&l.a) && !relevant.contains(&l.b)).map(|l| l.b).collect();
        if more.is_empty() {
            break;
        }
        relevant.extend(more);
    }
    // The occurrences a loop may hold or move: placed by such a joint, or below one that is
    // (worked out down the tree, parents first). The others keep the tree's count.
    let mut candidates: BTreeSet<u64> = BTreeSet::new();
    for occ in &tree.order {
        if let Some(&(from, joint, _)) = tree.parent.get(occ)
            && (relevant.contains(&joint) || candidates.contains(&from))
        {
            candidates.insert(*occ);
        }
    }
    // A memo kept under other motion links counts nothing again.
    let links = memo.as_ref().map(|_| links_key(asm));
    if let (Some(m), Some(l)) = (memo.as_deref_mut(), links.as_ref())
        && m.links != *l
    {
        m.counts.clear();
    }
    let mut kept: BTreeMap<u64, (Vec<u64>, Option<usize>)> = BTreeMap::new();
    // First every occurrence's values and loops: one whose inputs the memo already counted takes
    // that count, the others are counted below.
    let mut todo: Vec<DofTodo> = Vec::new();
    let mut counted = 0usize;
    for occ in tree.order.iter().filter(|o| candidates.contains(o)) {
        let path: Vec<u64> = tree.path(*occ).1.iter().map(|(k, _)| *k).collect();
        let (mut values, mut have): (Vec<(u64, usize)>, BTreeSet<(u64, usize)>) = (Vec::new(), BTreeSet::new());
        let mut add = |values: &mut Vec<(u64, usize)>, v: (u64, usize)| {
            if have.insert(v) {
                values.push(v);
            }
        };
        for j in path.iter().filter_map(|k| tree.joints.get(k)) {
            for v in (0..j.kind.dofs().len()).map(|i| (j.id, i)).filter(|v| !linked.contains(v)) {
                add(&mut values, v);
            }
            if values.len() > MAX_LOOP_VALUES {
                break;
            }
        }
        if values.len() > MAX_LOOP_VALUES {
            continue;
        }
        // A value on the path a motion link sets moves with the values its link chain starts from.
        let on_path: BTreeSet<u64> = path.iter().copied().collect();
        for v in link_chains(asm, tree, &on_path, &linked).1 {
            add(&mut values, v);
        }
        // The loops on the path, and every loop whose values those reach (a linked source may be
        // held by a loop of its own), each once.
        let (mut closers, mut around) = (Vec::new(), BTreeSet::new());
        let mut taken: BTreeSet<usize> = BTreeSet::new();
        loop {
            let more: Vec<usize> = groups
                .iter()
                .enumerate()
                .filter(|(g, _)| !taken.contains(g))
                .filter(|(_, (_, a, t))| path.iter().any(|k| a.contains(k)) || t.iter().any(|v| values.contains(v)))
                .map(|(g, _)| g)
                .collect();
            if more.is_empty() {
                break;
            }
            for (c, a, _) in more.iter().filter_map(|g| groups.get(*g)) {
                closers.extend(c.iter().copied());
                around.extend(a.iter().copied());
                for v in unknowns(tree, a, asm, &linked, &Held::new()) {
                    add(&mut values, v);
                }
            }
            taken.extend(more);
        }
        // No loop reaches this occurrence's values (not even through a motion link it drives):
        // the tree's count stands.
        if taken.is_empty() {
            continue;
        }
        counted += 1;
        if counted > MAX_DOF_OCCURRENCES {
            break;
        }
        if values.len() > MAX_LOOP_VALUES {
            continue;
        }
        let key = memo.as_ref().map(|_| dof_key(tree, &linked, vals, placed, *occ, &path, &values, &closers, &around));
        if let (Some(m), Some(k)) = (memo.as_deref(), key.as_ref())
            && let Some((was, count)) = m.counts.get(occ)
            && was == k
        {
            if let Some(n) = count {
                dof.insert(*occ, *n);
            }
            kept.insert(*occ, (k.clone(), *count));
            continue;
        }
        todo.push(DofTodo { occ: *occ, values, closers, around, taken, key });
    }
    // A pose nearby (for the loops still to count), when one differs.
    let wanted: BTreeSet<usize> = todo.iter().flat_map(|t| t.taken.iter().copied()).collect();
    let near_groups: Vec<Group> = groups.iter().enumerate().filter(|(g, _)| wanted.contains(g)).map(|(_, g)| g.clone()).collect();
    let near = (!near_groups.is_empty()).then(|| nearby_pose(tree, &near_groups, asm, &linked, vals)).filter(|n| n != vals);
    let mut spent = 0usize;
    for DofTodo { occ, values, closers, around, key, .. } in todo {
        if spent > MAX_LOOP_WORK {
            break;
        }
        let scale = loop_scale(tree, &around, vals);
        let (n, gaps) = (values.len(), closers.iter().map(Tree::width).sum::<usize>());
        // How many ways the occurrence moves at a pose, with the loops kept closed.
        let mut free_at = |at: &Values| -> Option<usize> {
            let mut workspace = PlacementWorkspace::new(tree);
            let mut work = at.clone();
            // The loops' gaps, then three points of the occurrence `scale` apart.
            let mut f = |x: &[f64]| -> Option<Vec<f64>> {
                spent += 1;
                for ((id, i), v) in values.iter().zip(x) {
                    if let Some(slot) = work.get_mut(id).and_then(|q| q.get_mut(*i)) {
                        *slot = *v;
                    }
                }
                apply_links(asm, &mut work);
                let mut out = Vec::with_capacity(closers.len() * 9 + 9);
                let mut placed = workspace.at(&work);
                placed.closure_gaps(&closers, scale, &mut out)?;
                let t = placed.world(occ)?;
                for p in [Vec3::ZERO, Vec3::X * scale, Vec3::Y * scale] {
                    let q = apply_point(&t, p);
                    out.extend([q.x, q.y, q.z]);
                }
                out.iter().all(|v| v.is_finite()).then_some(out)
            };
            let x0: Vec<f64> = values.iter().map(|(id, i)| at.get(id).and_then(|q| q.get(*i)).copied().unwrap_or(0.0)).collect();
            let jac = jacobian(&mut f, &x0, gaps + 9)?;
            let rank = |rows: usize| {
                let mut a = linalg::Mat::zeros(rows, n);
                let mut big = 0.0f64;
                for r in 0..rows {
                    for c in 0..n {
                        let v = jac.get(r * n + c).copied().unwrap_or(0.0);
                        big = big.max(v.abs());
                        a.set(r, c, v);
                    }
                }
                linalg::rref(&mut a, 1e-6 * big.max(1.0)).len()
            };
            Some(rank(gaps + 9).saturating_sub(rank(gaps)))
        };
        let here = free_at(vals);
        let there = near.as_ref().and_then(&mut free_at);
        let count = here.max(there).map(|free| free.min(6));
        if let Some(n) = count {
            dof.insert(occ, n);
        }
        if let Some(k) = key {
            kept.insert(occ, (k, count));
        }
    }
    if let (Some(m), Some(l)) = (memo, links) {
        *m = DofMemo { counts: kept, links: l };
    }
}

/// An occurrence `loop_dof` still has to count: its values, its loops' closing joints and the
/// joints around them, the groups it takes in, and its memo key.
struct DofTodo {
    occ: u64,
    values: Vec<(u64, usize)>,
    closers: Vec<Closing>,
    around: BTreeSet<u64>,
    taken: BTreeSet<usize>,
    key: Option<Vec<u64>>,
}

/// Degrees of freedom an earlier solve worked out through loops, by occurrence, each with
/// everything its count read (`dof_key`). A solve given the memo takes a count again where those
/// inputs are the same, so a re-solve after one edit counts only the occurrences whose loops the
/// edit touched (R130); the memo then holds this solve's counts.
#[derive(Clone, Debug, Default)]
pub struct DofMemo {
    counts: BTreeMap<u64, (Vec<u64>, Option<usize>)>,
    links: Vec<u64>,
}

impl DofMemo {
    /// How many occurrences' counts it keeps.
    pub fn len(&self) -> usize {
        self.counts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }
}

fn bits(out: &mut Vec<u64>, m: &Mat) {
    out.extend(m.iter().flatten().map(|v| v.to_bits()));
}

/// An equation's terms, as numbers (for the DOF memo's key).
fn equation_bits(out: &mut Vec<u64>, eq: &Equation) {
    use crate::constraints::geo::Prim;
    let side = |out: &mut Vec<u64>, s: &equations::Side| {
        out.push(s.occurrence);
        let v = |out: &mut Vec<u64>, v: Vec3| out.extend([v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]);
        match s.prim {
            Prim::Plane { p, n } => {
                out.push(0);
                v(out, p);
                v(out, n);
            }
            Prim::Line { p, d } => {
                out.push(1);
                v(out, p);
                v(out, d);
            }
            Prim::Point { p } => {
                out.push(2);
                v(out, p);
            }
            Prim::Circle { c, axis, r } => {
                out.push(3);
                v(out, c);
                v(out, axis);
                out.push(r.to_bits());
            }
            Prim::Cylinder { p, d, r } => {
                out.push(4);
                v(out, p);
                v(out, d);
                out.push(r.to_bits());
            }
            Prim::Sphere { c, r } => {
                out.push(5);
                v(out, c);
                out.push(r.to_bits());
            }
        }
    };
    let bounds = |out: &mut Vec<u64>, b: Option<(f64, f64)>| match b {
        Some((lo, hi)) => out.extend([1, lo.to_bits(), hi.to_bits()]),
        None => out.push(0),
    };
    match eq {
        Equation::Angle { a, b, axis, target, bounds: bd } => {
            out.push(0);
            side(out, a);
            side(out, b);
            match axis {
                Some(k) => {
                    out.push(1);
                    side(out, k);
                }
                None => out.push(0),
            }
            out.push(target.to_bits());
            bounds(out, *bd);
        }
        Equation::AxisDistance { a, b, target, bounds: bd } => {
            out.push(1);
            side(out, a);
            side(out, b);
            out.push(target.to_bits());
            bounds(out, *bd);
        }
    }
}

/// The motion links, as numbers (a memo kept under other links is dropped).
fn links_key(asm: &Assembly) -> Vec<u64> {
    let mut out = Vec::new();
    for l in &asm.links {
        out.extend([l.a, l.ia as u64, l.b, l.ib as u64, l.ratio.to_bits(), l.offset.to_bits(), l.lift.len() as u64]);
        out.extend(l.lift.iter().map(|v| v.to_bits()));
    }
    out
}

/// Everything an occurrence's loop count reads: its path, its values and loops, and, for every
/// joint among them, its kind, frames, flip, offset, angle, limits and values and where both its
/// occurrences are placed.
fn dof_key(
    tree: &Tree,
    linked: &BTreeSet<(u64, usize)>,
    vals: &Values,
    placed: &BTreeMap<u64, Mat>,
    occ: u64,
    path: &[u64],
    values: &[(u64, usize)],
    closers: &[Closing],
    around: &BTreeSet<u64>,
) -> Vec<u64> {
    let mut out = vec![occ, path.len() as u64];
    out.extend(path.iter().copied());
    out.push(values.len() as u64);
    for (id, i) in values {
        out.extend([*id, *i as u64, u64::from(linked.contains(&(*id, *i)))]);
    }
    out.push(closers.len() as u64);
    let place = |out: &mut Vec<u64>, o: u64| match placed.get(&o) {
        Some(m) => bits(out, m),
        None => out.push(u64::MAX),
    };
    // An equation closer: everything its residual reads, and where its occurrences are.
    for c in closers {
        match c {
            Closing::Joint(id) => out.extend([0, *id]),
            Closing::Equation(i) => {
                out.push(1);
                match tree.equations.get(*i) {
                    Some(e) => {
                        out.push(e.id);
                        equation_bits(&mut out, &e.eq);
                        for o in e.eq.occurrences() {
                            place(&mut out, o);
                        }
                    }
                    None => out.push(u64::MAX),
                }
            }
        }
    }
    let mut joints: BTreeSet<u64> = path.iter().chain(around).copied().collect();
    joints.extend(closers.iter().filter_map(|c| match c {
        Closing::Joint(id) => Some(*id),
        Closing::Equation(_) => None,
    }));
    joints.extend(values.iter().map(|(id, _)| *id));
    place(&mut out, occ);
    for id in joints {
        out.push(id);
        let (Some(j), Some(f)) = (tree.joints.get(&id), tree.frames.get(&id)) else {
            out.push(u64::MAX);
            continue;
        };
        out.extend([j.kind as u64, u64::from(j.flip), j.offset.to_bits(), j.angle.to_bits(), j.a.occurrence, j.b.occurrence, j.limits.len() as u64]);
        for l in &j.limits {
            match l {
                Some((lo, hi)) => out.extend([1, lo.to_bits(), hi.to_bits()]),
                None => out.push(0),
            }
        }
        bits(&mut out, &f.a);
        bits(&mut out, &f.b);
        let q = vals.get(&id).map(Vec::as_slice).unwrap_or_default();
        out.push(q.len() as u64);
        out.extend(q.iter().map(|v| v.to_bits()));
        place(&mut out, j.a.occurrence);
        place(&mut out, j.b.occurrence);
    }
    out
}

/// Close one group of loops: move `vars`, the values of the joints `around` them (all but the `held`
/// values, motion-link targets and rigid joints, within their limits; a linked value moves
/// through the start of its link chain) until every loop-closing joint meets. A loop open at the
/// start is closed first, from where it is. Then the held values `moves` go from the values they
/// are held from to their current ones in small steps, each solved from the last, so the mechanism
/// stays on its branch (an open four-bar does not flip crossed). Returns the values that changed
/// from what is stored, by (joint, index), and updates `vals` (empty when there is nothing to
/// do), or `None` when the loops can't close, or can't stay closed on the way. `close_loops`
/// leaves out groups over `MAX_LOOP_VALUES`.
fn close_group(
    tree: &Tree,
    closing: &[Closing],
    around: &BTreeSet<u64>,
    vars: &[(u64, usize)],
    moves: &[(u64, usize, f64, f64)],
    asm: &Assembly,
    spent: &std::cell::Cell<usize>,
    vals: &mut Values,
) -> Option<BTreeMap<(u64, usize), f64>> {
    let bounds = limit_bounds(tree, vars);
    let scale = loop_scale(tree, around, vals);
    // Closer than the conflict check needs: 1e-7 mm, and turns of 1e-10 rad across the loop.
    let tol = (1e-10 * scale).min(1e-7);
    // The values with the held ones `t` of the way along (and the unknowns at `x`).
    let place = |work: &mut Values, x: &[f64], t: f64| {
        for (id, i, from, to) in moves {
            if let Some(slot) = work.get_mut(id).and_then(|q| q.get_mut(*i)) {
                *slot = from + (to - from) * t;
            }
        }
        for ((id, i), (v, (lo, hi))) in vars.iter().zip(x.iter().zip(&bounds)) {
            if let Some(slot) = work.get_mut(id).and_then(|q| q.get_mut(*i)) {
                *slot = v.max(*lo).min(*hi);
            }
        }
        apply_links(asm, work);
    };
    let x0: Vec<f64> = vars.iter().map(|(id, i)| vals.get(id).and_then(|q| q.get(*i)).copied().unwrap_or(0.0)).collect();
    // Steps that move no value around the loops (links included) more than 5 degrees or a
    // twentieth of the loops' size.
    // How many such steps apart two sets of values are, over the loops' values.
    let stride = |a: &Values, b: &Values| -> f64 {
        let mut most = 0.0f64;
        for j in around.iter().filter_map(|id| tree.joints.get(id)) {
            for i in 0..j.kind.dofs().len() {
                let (Some(p), Some(q)) = (a.get(&j.id).and_then(|v| v.get(i)), b.get(&j.id).and_then(|v| v.get(i))) else { continue };
                let step = if j.kind.is_angle(i) { 5f64.to_radians() } else { 0.05 * scale };
                most = most.max((q - p).abs() / step);
            }
        }
        most
    };
    let effective = |x: &[f64], t: f64| {
        let mut v = vals.clone();
        place(&mut v, x, t);
        v
    };
    let mut first = 1.0f64;
    if !moves.is_empty() {
        first = first.min(1.0 / stride(&effective(&x0, 0.0), &effective(&x0, 1.0)));
    }
    let mut workspace = PlacementWorkspace::new(tree);
    let mut work = vals.clone();
    // How far each closing joint is from meeting with the held values `t` of the way along:
    // three points of B's joint frame, where the joint wants them against where the tree puts them.
    let mut gaps = |x: &[f64], t: f64| -> Option<Vec<f64>> {
        spent.set(spent.get() + 1);
        if spent.get() > MAX_LOOP_WORK {
            return None;
        }
        place(&mut work, x, t);
        let mut out = Vec::with_capacity(closing.len() * 9);
        closure_gaps(&mut workspace, closing, &work, scale, &mut out)?;
        out.iter().all(|v| v.is_finite()).then_some(out)
    };
    // Already closed, with no held value moving. (A moving one is followed all the way: a
    // rocker driven a full turn ends where it started, but can't get there.)
    if moves.is_empty() && gaps(&x0, 1.0).is_some_and(|r| largest(&r) <= tol) {
        return Some(BTreeMap::new());
    }
    // A loop open at the start closes first. One that comes only to within the conflict check
    // (`close_within`) moves on as closed as it is; with nothing held moving, one that started
    // that close is left as it is.
    let mut x = x0;
    let start = largest(&gaps(&x, 0.0)?);
    let tol = match close_within(&mut |x| gaps(x, 0.0), &mut x, &bounds, tol, loose_gap(scale))? {
        t if t > tol && moves.is_empty() && start <= t => return Some(BTreeMap::new()),
        t => t,
    };
    let (mut t, mut dt) = (if moves.is_empty() { 1.0 } else { 0.0 }, first);
    let mut at = effective(&x, t);
    while t < 1.0 {
        if dt < first * 1e-4 {
            return None;
        }
        let t1 = (t + dt).min(1.0);
        let mut trial = x.clone();
        if least_squares(&mut |x| gaps(x, t1), &mut trial, &bounds, tol) {
            // A step that moved some loop value too far (through a link and a limit, or close to
            // a dead point) is taken again in halves.
            let next = effective(&trial, t1);
            if stride(&at, &next) > 2.0 {
                dt *= 0.5;
                continue;
            }
            (x, t, dt, at) = (trial, t1, (dt * 2.0).min(first), next);
        } else if spent.get() > MAX_LOOP_WORK {
            return None;
        } else {
            dt *= 0.5;
        }
    }
    if largest(&gaps(&x, 1.0)?) > tol {
        return None;
    }
    // The values that changed from what is stored.
    let moved = vars
        .iter()
        .zip(&x)
        .filter(|((id, i), v)| tree.joints.get(id).is_some_and(|j| j.values.get(*i).is_none_or(|s| (s - *v).abs() > 1e-12)))
        .map(|(var, v)| (*var, *v))
        .collect();
    *vals = work;
    Some(moved)
}

fn largest(r: &[f64]) -> f64 {
    r.iter().fold(0.0f64, |m, v| m.max(v.abs()))
}

/// Levenberg–Marquardt: move `x` (kept within `bounds`) until every value of `f` is within
/// `tol` of zero. False when it can't get there.
/// The Jacobian of `f` (`m` values) at `x` by central differences, row-major.
fn jacobian(f: &mut dyn FnMut(&[f64]) -> Option<Vec<f64>>, x: &[f64], m: usize) -> Option<Vec<f64>> {
    const H: f64 = 1e-6;
    let n = x.len();
    let mut jac = vec![0.0; m * n];
    for k in 0..n {
        let (mut xp, mut xm) = (x.to_vec(), x.to_vec());
        if let (Some(p), Some(q)) = (xp.get_mut(k), xm.get_mut(k)) {
            *p += H;
            *q -= H;
        }
        let (rp, rm) = (f(&xp)?, f(&xm)?);
        for (i, (a, b)) in rp.iter().zip(&rm).enumerate().take(m) {
            if let Some(slot) = jac.get_mut(i * n + k) {
                *slot = (a - b) / (2.0 * H);
            }
        }
    }
    Some(jac)
}

fn least_squares(f: &mut dyn FnMut(&[f64]) -> Option<Vec<f64>>, x: &mut Vec<f64>, bounds: &[(f64, f64)], tol: f64) -> bool {
    let n = x.len();
    let norm = |r: &[f64]| r.iter().map(|v| v * v).sum::<f64>();
    let Some(mut r) = f(x) else { return false };
    if n == 0 {
        return largest(&r) <= tol;
    }
    let mut lambda = 1e-3;
    for _ in 0..50 {
        if largest(&r) <= tol {
            return true;
        }
        // The Jacobian by central differences, then the normal equations JᵀJ δ = −Jᵀr.
        let m = r.len();
        let Some(jac) = jacobian(f, x, m) else { return false };
        let mut jtj = linalg::Mat::zeros(n, n);
        let mut g = vec![0.0; n];
        for (i, ri) in r.iter().enumerate() {
            let row = jac.get(i * n..(i + 1) * n).unwrap_or(&[]);
            for (p, a) in row.iter().enumerate().filter(|(_, a)| **a != 0.0) {
                if let Some(gp) = g.get_mut(p) {
                    *gp -= a * ri;
                }
                for (q, b) in row.iter().enumerate().skip(p) {
                    jtj.add(p, q, a * b);
                }
            }
        }
        for p in 0..n {
            for q in 0..p {
                jtj.set(p, q, jtj.get(q, p));
            }
        }
        let mut stepped = false;
        while lambda < 1e12 {
            let mut a = jtj.clone();
            for p in 0..n {
                a.add(p, p, lambda * (1.0 + jtj.get(p, p)));
            }
            if let Some(d) = linalg::solve(a, g.clone()) {
                let next: Vec<f64> = x.iter().zip(&d).zip(bounds).map(|((v, dv), (lo, hi))| (v + dv).max(*lo).min(*hi)).collect();
                if let Some(rn) = f(&next)
                    && norm(&rn) < norm(&r)
                {
                    *x = next;
                    r = rn;
                    lambda = (lambda / 3.0).max(1e-9);
                    stepped = true;
                    break;
                }
            }
            lambda *= 4.0;
        }
        if !stepped {
            return false;
        }
    }
    largest(&r) <= tol
}

/// Place occurrences from the joint graph.
pub fn solve(doc: &Document, st: &ModelState) -> Solution {
    solve_holding(doc, st, &BTreeMap::new())
}

/// Place occurrences from the joint graph. Joints that close loops are met by moving the values
/// of the other joints around the loop. The `held` values (the ones being driven) keep theirs,
/// moving there from the values given (where the loops last closed) so the others follow them.
pub fn solve_holding(doc: &Document, st: &ModelState, held: &Held) -> Solution {
    solve_holding_with_analysis(doc, st, held, Analysis::DegreesOfFreedom)
}

/// Place occurrences and check constraints with the requested diagnostic work. Placement-only
/// solves still close and check every loop, respect held values and limits, and report errors;
/// they leave `Solution::dof` empty rather than returning the spanning tree's approximate count.
pub fn solve_holding_with_analysis(doc: &Document, st: &ModelState, held: &Held, analysis: Analysis) -> Solution {
    solve_core(doc, st, held, analysis, &compiled(doc, st), None)
}

/// [`solve_holding_with_analysis`] keeping a [`DofMemo`]: the degrees of freedom of occurrences
/// whose loops and values are as the memo's solve left them are taken from it, not worked out
/// again (R130). The session's re-solves use it.
pub fn solve_memo(doc: &Document, st: &ModelState, held: &Held, analysis: Analysis, memo: &mut DofMemo) -> Solution {
    solve_core(doc, st, held, analysis, &compiled(doc, st), Some(memo))
}

/// The design's constraints compiled (nothing to do without any).
fn compiled(doc: &Document, st: &ModelState) -> Compiled {
    if doc.assembly.constraints.is_empty() { Compiled::default() } else { compile(doc, st, &doc.assembly) }
}

/// Solve with `extra` (constraints compiled by the caller, a candidate among them) chained after
/// the saved joints; the design's own constraints are not compiled again.
pub fn solve_with(doc: &Document, st: &ModelState, held: &Held, analysis: Analysis, extra: &Compiled) -> Solution {
    solve_core(doc, st, held, analysis, extra, None)
}

fn solve_core(doc: &Document, st: &ModelState, held: &Held, analysis: Analysis, extra: &Compiled, memo: Option<&mut DofMemo>) -> Solution {
    let asm = &doc.assembly;
    let mut sol = Solution::default();
    // A constraint that compiles to nothing (a pick gone, kinds it can't join) is an error of its
    // first compiled id, named as the constraint.
    for (c, why) in &extra.lost {
        let name = asm.constraints.iter().find(|x| x.id == *c).map(|x| x.name.clone()).unwrap_or_default();
        sol.errors.push((crate::constraints::compiled_id(*c, 0), name, why.clone()));
    }
    // The joints' frames first: only those that resolve place a part (R151).
    let mut frames = Frames::default();
    let real: Vec<&Joint> = asm.joints.iter().chain(&extra.joints).filter(|j| !j.suppressed).take(MAX_JOINTS).collect();
    frames.resolve(doc, st, &real, &mut sol);
    let unresolved: BTreeSet<u64> = real.iter().map(|j| j.id).filter(|id| !frames.ok.contains_key(id)).collect();
    // A part only equations hold gets a provisional `Free` member from the root (spec §3.5).
    let free = free_members(doc, extra, &frames);
    let with_free;
    let extra = if free.is_empty() {
        extra
    } else {
        with_free =
            Compiled { joints: extra.joints.iter().cloned().chain(free).collect(), equations: extra.equations.clone(), lost: extra.lost.clone() };
        &with_free
    };
    let mut vals = joint_values(asm, extra);
    let Built { tree, mut placed, closing, mut dof } = build(doc, st, &vals, extra, &mut sol, frames);
    let mut stuck = BTreeSet::new();
    if !closing.is_empty() {
        (sol.values, stuck) = close_loops(&tree, &closing, asm, held, &mut vals, &mut sol.groups);
        if !sol.values.is_empty() {
            placed = tree.place(&vals);
        }
    }
    for c in &closing {
        let id = match c {
            Closing::Joint(id) => id,
            Closing::Equation(i) => {
                let Some(e) = extra.equations.get(*i) else { continue };
                let Some(r) = e.eq.residual(&|o| placed.get(&o).copied()) else {
                    sol.errors.push((e.id, e.name.clone(), not_placed(&e.eq, &placed, &real, &unresolved)));
                    continue;
                };
                // An angle's gap in degrees, a distance's in mm, as a joint's conflict says them.
                let (mm, rad) = if e.eq.is_angle() { (0.0, r.abs()) } else { (r.abs(), 0.0) };
                if mm > CONFLICT_MM || rad > CONFLICT_RAD {
                    sol.conflict_gaps.insert(e.id, (mm, rad));
                    sol.conflicts.push((
                        e.id,
                        e.name.clone(),
                        format!("off by {mm:.4} mm and {:.3}° from the placement the other joints give", rad.to_degrees()),
                    ));
                } else if stuck.contains(&e.id) {
                    sol.conflict_gaps.insert(e.id, (f64::NAN, f64::NAN));
                    sol.conflicts.push((e.id, e.name.clone(), "the loop can't stay closed on the way to these values".into()));
                }
                continue;
            }
        };
        let Some(j) = tree.joints.get(id) else { continue };
        let (Some(ta), Some(tb)) = (placed.get(&j.a.occurrence), placed.get(&j.b.occurrence)) else { continue };
        let (Some(want), Some(frames)) = (tree.step(*id, true, ta, &vals), tree.frames.get(id)) else { continue };
        // Measured at the joint, where the loop solver closes it.
        let (dt, da) = mismatch(&mat_mul(&want, &frames.b), &mat_mul(tb, &frames.b));
        if dt > CONFLICT_MM || da > CONFLICT_RAD {
            sol.conflict_gaps.insert(j.id, (dt, da));
            sol.conflicts.push((
                j.id,
                j.name.clone(),
                format!("off by {dt:.4} mm and {:.3}° from the placement the other joints give", da.to_degrees()),
            ));
        } else if stuck.contains(id) {
            // It meets there, but the loop would have to come apart on the way (a rocker
            // driven a full turn ends where it started).
            sol.conflict_gaps.insert(j.id, (f64::NAN, f64::NAN));
            sol.conflicts.push((j.id, j.name.clone(), "the loop can't stay closed on the way to these values".into()));
        }
    }
    // An equation that closes no loop reads a part nothing placed (R151): Lost, not a silent Ok.
    for (i, e) in extra.equations.iter().enumerate() {
        if !closing.contains(&Closing::Equation(i)) {
            sol.errors.push((e.id, e.name.clone(), not_placed(&e.eq, &placed, &real, &unresolved)));
        }
    }
    // Every compiled joint's final values (R114): a tree member's are the ones it was seeded with,
    // a closing one's what the loops moved it to. A locked Insert keeps its turn.
    for j in extra.joints.iter().filter(|j| j.kind != JointKind::Free && tree.frames.contains_key(&j.id)) {
        let q = if j.kind == JointKind::Rigid { vec![j.angle] } else { vals.get(&j.id).cloned().unwrap_or_default() };
        sol.compiled.insert(j.id, q);
        sol.compiled_signatures.insert(j.id, crate::constraints::StateSignature::of(j));
    }
    // An equation keeps no values: it is there so its constraint's state holds only its joints'
    // (an Angle that became an equation drops the relation's old values).
    for e in &extra.equations {
        sol.compiled.entry(e.id).or_default();
    }
    if analysis == Analysis::DegreesOfFreedom {
        if !closing.is_empty() {
            loop_dof(&tree, &closing, asm, &vals, &placed, &mut dof, memo);
        }
        for o in &doc.occurrences {
            dof.entry(o.id).or_insert(6);
        }
    } else {
        dof.clear();
    }
    placed.remove(&0);
    sol.world = placed;
    sol.dof = dof;
    sol
}

/// Why equation `eq` can't be measured (R151): a part it reads is not placed, named through the
/// nearest joint (along `joints` from such a part) whose frames don't resolve.
fn not_placed(eq: &Equation, placed: &BTreeMap<u64, Mat>, joints: &[&Joint], unresolved: &BTreeSet<u64>) -> String {
    let mut seen: BTreeSet<u64> = eq.occurrences().into_iter().filter(|o| !placed.contains_key(o)).collect();
    let mut queue: VecDeque<u64> = seen.iter().copied().collect();
    while let Some(o) = queue.pop_front() {
        for j in joints.iter().filter(|j| j.a.occurrence == o || j.b.occurrence == o) {
            if unresolved.contains(&j.id) {
                return format!("a part it reads is not placed ({} doesn't resolve)", j.name);
            }
            let other = if j.a.occurrence == o { j.b.occurrence } else { j.a.occurrence };
            if seen.insert(other) {
                queue.push_back(other);
            }
        }
    }
    "a part it reads is not placed".into()
}

/// The joint graph laid out for a solve: the tree, where it places each occurrence for the
/// values it was built with, the joints that close loops, and each occurrence's degrees of
/// freedom along the tree.
struct Built<'a> {
    tree: Tree<'a>,
    placed: BTreeMap<u64, Mat>,
    closing: Vec<Closing>,
    dof: BTreeMap<u64, usize>,
}

/// Lay out the joint graph: the spanning tree out from the fixed occurrences (the root, grounded
/// ones; with neither involved, the first joint's A side) and the joints left over, which close
/// loops. Joints whose origins can't be resolved go to `sol.errors`; `sol.fixed` is set.
fn build<'a>(doc: &'a Document, st: &ModelState, vals: &Values, extra: &'a Compiled, sol: &mut Solution, mut frames: Frames) -> Built<'a> {
    let asm = &doc.assembly;
    // The saved joints, then those the constraints compile to.
    let joints: Vec<&Joint> = asm.joints.iter().chain(&extra.joints).filter(|j| !j.suppressed).take(MAX_JOINTS).collect();
    // Joint frames in component coordinates (errors leave the joint out), those not found yet.
    frames.resolve(doc, st, &joints, sol);
    let frames = frames.ok;
    // Fixed: the root, grounded occurrences; with neither involved, the first joint's A side.
    let mut placed: BTreeMap<u64, Mat> = BTreeMap::new();
    let mut dof: BTreeMap<u64, usize> = BTreeMap::new();
    placed.insert(0, IDENTITY);
    dof.insert(0, 0);
    for o in doc.occurrences.iter().filter(|o| o.grounded) {
        placed.insert(o.id, occurrence_world(doc, o.id));
        dof.insert(o.id, 0);
    }
    // `Free` members (from the root) are left out: they place what the joints don't.
    if let Some(a) = floating_anchor(&placed, &joints, &extra.equations) {
        placed.insert(a, occurrence_world(doc, a));
        dof.insert(a, 0);
    }
    sol.fixed = placed.keys().copied().collect();
    let mut tree =
        // Every joint's values are known, suppressed ones too (their motion links still set values).
        Tree {
            joints: asm.joints.iter().chain(&extra.joints).map(|j| (j.id, j)).collect(),
            equations: &extra.equations,
            frames,
            fixed: placed.clone(),
            parent: BTreeMap::new(),
            order: Vec::new(),
        };
    let mut used: BTreeSet<u64> = BTreeSet::new();
    let mut queue: VecDeque<u64> = placed.keys().copied().collect();
    // An occurrence's degrees of freedom are the distinct values that move it: those of the
    // joints on its path, a value a motion link sets standing for the values its link chain
    // starts from (a driven gear turns with its driving gear's one value). Kept per occurrence as
    // a count of the values no link reads or sets, which no other path value can repeat, and the
    // set of the ones that may be repeated (links' sources).
    let sources = link_sources(asm);
    let shared: BTreeSet<(u64, usize)> = sources.values().flatten().copied().collect();
    let mut moved_by: BTreeMap<u64, (usize, BTreeSet<(u64, usize)>)> = BTreeMap::new();
    // The joints at each occurrence, in the joints' order (a large assembly is not walked joint by
    // joint for every occurrence).
    let mut at: BTreeMap<u64, Vec<&Joint>> = BTreeMap::new();
    for j in joints.iter().copied() {
        at.entry(j.a.occurrence).or_default().push(j);
        if j.b.occurrence != j.a.occurrence {
            at.entry(j.b.occurrence).or_default().push(j);
        }
    }
    // Tree preference (spec §3.6): of the joints a constraint compiled that could place an
    // occurrence, the one leaving the fewest values, so the loops carry fewer (stable: equal ones
    // keep their order). Saved joints keep their order, ahead of them: sorted too, they would
    // change which joint a saved design's conflicts name.
    for list in at.values_mut() {
        list.sort_by_key(|j| if j.id < COMPILED_BASE { 0 } else { 1 + j.kind.dofs().len() });
    }
    while let Some(occ) = queue.pop_front() {
        let Some(t) = placed.get(&occ).copied() else { continue };
        for j in at.get(&occ).map(Vec::as_slice).unwrap_or_default() {
            if used.contains(&j.id) || !tree.frames.contains_key(&j.id) {
                continue;
            }
            let (other, from_a) = if j.a.occurrence == occ {
                (j.b.occurrence, true)
            } else if j.b.occurrence == occ {
                (j.a.occurrence, false)
            } else {
                continue;
            };
            if placed.contains_key(&other) {
                continue;
            }
            let Some(m) = tree.step(j.id, from_a, &t, vals) else { continue };
            used.insert(j.id);
            placed.insert(other, m);
            tree.parent.insert(other, (occ, j.id, from_a));
            tree.order.push(other);
            let (mut own, mut common) = moved_by.get(&occ).cloned().unwrap_or_default();
            for v in (0..j.kind.dofs().len()).map(|i| (j.id, i)) {
                match sources.get(&v) {
                    Some(from) => common.extend(from.iter().copied()),
                    None if shared.contains(&v) => {
                        common.insert(v);
                    }
                    None => own += 1,
                }
            }
            dof.insert(other, own + common.len());
            moved_by.insert(other, (own, common));
            queue.push_back(other);
        }
    }
    // Joints that close loops: the solve moves the loops' values to meet them, then checks they
    // agree.
    let mut closing: Vec<Closing> = joints
        .iter()
        .filter(|j| !used.contains(&j.id) && tree.frames.contains_key(&j.id))
        .filter(|j| placed.contains_key(&j.a.occurrence) && placed.contains_key(&j.b.occurrence))
        .map(|j| Closing::Joint(j.id))
        .collect();
    // Every equation whose occurrences are all placed closes a loop over their paths.
    for (i, e) in extra.equations.iter().enumerate() {
        if e.eq.occurrences().iter().all(|o| placed.contains_key(o)) {
            closing.push(Closing::Equation(i));
        }
    }
    Built { tree, placed, closing, dof }
}

/// Joint frames in component coordinates, found once per joint: the joints `tried`, and the
/// frames of those that resolve (`ok`).
#[derive(Default)]
struct Frames {
    ok: BTreeMap<u64, JointFrames>,
    tried: BTreeSet<u64>,
}

impl Frames {
    /// Find the frames of the joints not tried yet. One whose expressions can't be worked out or
    /// whose origin does not resolve goes to `sol.errors` and is left out. An origin several
    /// joints share (a panel face every part sits on) is found once.
    fn resolve(&mut self, doc: &Document, st: &ModelState, joints: &[&Joint], sol: &mut Solution) {
        let mut found: BTreeMap<(u64, String), std::result::Result<Mat, String>> = BTreeMap::new();
        let mut resolve = |o: &JointOrigin| -> Result<Mat> {
            let Ok(comp) = component_of(doc, o.occurrence) else { return resolve_origin(doc, st, o) };
            found
                .entry((comp, format!("{:?}", o.snap)))
                .or_insert_with(|| resolve_origin(doc, st, o).map_err(|e| e.to_string()))
                .clone()
                .map_err(DocError::Invalid)
        };
        for j in joints {
            if !self.tried.insert(j.id) {
                continue;
            }
            // A joint whose expressions can't be worked out (a parameter gone) is left out rather
            // than placed by numbers that no longer say what it was defined by.
            if !j.exprs.is_empty()
                && let Err(e) = expressed(doc, j)
            {
                sol.errors.push((j.id, j.name.clone(), e.to_string()));
                continue;
            }
            match (resolve(&j.a), resolve(&j.b)) {
                (Ok(fa), Ok(fb)) => {
                    self.ok.insert(j.id, JointFrames::new(fa, fb));
                }
                (Err(e), _) | (_, Err(e)) => sol.errors.push((j.id, j.name.clone(), e.to_string())),
            }
        }
    }
}

/// With no fixed occurrence among the joints' and the equations' (the `Free` members aside), the
/// first joint's A side is held where it is. An equation touching a fixed occurrence counts
/// (R148): a loose pair angled to a grounded part turns as a pair, no side of it held.
fn floating_anchor(placed: &BTreeMap<u64, Mat>, joints: &[&Joint], equations: &[Eq]) -> Option<u64> {
    let real = || joints.iter().filter(|j| j.kind != JointKind::Free);
    let mut involved = real().flat_map(|j| [j.a.occurrence, j.b.occurrence]).chain(equations.iter().flat_map(|e| e.eq.occurrences()));
    if involved.any(|o| placed.contains_key(&o)) {
        return None;
    }
    real().next().map(|j| j.a.occurrence)
}

/// The first id of the `Free` members a solve makes (occurrence id above it): past every
/// compiled id, never saved.
const FREE_BASE: u64 = 1 << 62;

/// The provisional members of a solve with loop equations (spec §3.5): an occurrence an equation
/// reads that no joint (whose frames resolve) can place from a fixed occurrence gets a `Free` member from the root,
/// seeded where it is (its translation and rotation vector). One per group of occurrences the
/// joints tie together: the rest of the group is placed through it.
fn free_members(doc: &Document, extra: &Compiled, frames: &Frames) -> Vec<Joint> {
    if extra.equations.is_empty() {
        return Vec::new();
    }
    let asm = &doc.assembly;
    // Only joints whose frames resolve place anything (R151): a part reached only through one
    // that doesn't is placed by its own Free member.
    let joints: Vec<&Joint> =
        asm.joints.iter().chain(&extra.joints).filter(|j| !j.suppressed).take(MAX_JOINTS).filter(|j| frames.ok.contains_key(&j.id)).collect();
    let mut fixed: BTreeMap<u64, Mat> = BTreeMap::from([(0, IDENTITY)]);
    for o in doc.occurrences.iter().filter(|o| o.grounded) {
        fixed.insert(o.id, IDENTITY);
    }
    if let Some(a) = floating_anchor(&fixed, &joints, &extra.equations) {
        fixed.insert(a, IDENTITY);
    }
    let mut next: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for j in &joints {
        next.entry(j.a.occurrence).or_default().push(j.b.occurrence);
        next.entry(j.b.occurrence).or_default().push(j.a.occurrence);
    }
    let mut reached: BTreeSet<u64> = BTreeSet::new();
    let reach = |from: u64, reached: &mut BTreeSet<u64>| {
        let mut queue = VecDeque::from([from]);
        reached.insert(from);
        while let Some(o) = queue.pop_front() {
            for n in next.get(&o).map(Vec::as_slice).unwrap_or_default() {
                if reached.insert(*n) {
                    queue.push_back(*n);
                }
            }
        }
    };
    for o in fixed.keys() {
        reach(*o, &mut reached);
    }
    let mut out = Vec::new();
    let frame = |occurrence: u64| JointOrigin { occurrence, snap: Snap::Frame { origin: Vec3::ZERO, z: Vec3::Z, x: Some(Vec3::X) } };
    for occ in extra.equations.iter().flat_map(|e| e.eq.occurrences()) {
        if reached.contains(&occ) || !doc.occurrences.iter().any(|o| o.id == occ) {
            continue;
        }
        reach(occ, &mut reached);
        out.push(Joint {
            id: FREE_BASE.saturating_add(occ),
            name: String::new(),
            kind: JointKind::Free,
            a: frame(0),
            b: frame(occ),
            values: free_values(&occurrence_world(doc, occ)),
            limits: Vec::new(),
            // B's frame on A's, not mated against it: the values are the placement itself.
            flip: true,
            offset: 0.0,
            angle: 0.0,
            suppressed: false,
            exprs: JointExprs::default(),
        });
    }
    out
}

impl Document {
    /// Apply a solution: occurrence transforms (in their parents) for the occurrences it places,
    /// the values of the joints that moved to close loops, and the compiled joints' values into
    /// their constraints' `state`. Returns true when something moved or a state changed.
    pub fn apply_joint_solution(&mut self, sol: &Solution) -> bool {
        let mut changed = false;
        for (id, q) in &sol.values {
            if let Some(j) = self.assembly.joints.iter_mut().find(|j| j.id == *id) {
                j.values.clone_from(q);
                changed = true;
            }
        }
        // Compiled joints' values go to their constraints' `state`, in member order (R114).
        // One signature per joint member, in member order (an equation adds none).
        let mut states: BTreeMap<u64, (Vec<f64>, Vec<crate::constraints::StateSignature>)> = BTreeMap::new();
        for (id, q) in &sol.compiled {
            if let Some(c) = owner_of(*id) {
                let entry = states.entry(c).or_default();
                entry.0.extend(q.iter().copied());
                if let Some(sig) = sol.compiled_signatures.get(id) {
                    entry.1.push(*sig);
                }
            }
        }
        for c in self.assembly.constraints.iter_mut() {
            let Some((q, sigs)) = states.remove(&c.id) else { continue };
            // NaN-aware: a state read back as NaN is replaced.
            let same = c.state.len() == q.len() && c.state.iter().zip(&q).all(|(a, b)| a.to_bits() == b.to_bits());
            if !same || c.signatures != sigs {
                c.state = q;
                c.signatures = sigs;
                changed = true;
            }
        }
        let parents: BTreeMap<u64, u64> = self.occurrences.iter().map(|o| (o.id, o.parent)).collect();
        for (occ, world) in &sol.world {
            let parent = parents.get(occ).copied().unwrap_or(0);
            let pw = if parent == 0 { IDENTITY } else { self.component_transform(parent) };
            let Some(local) = mat_inverse(&pw).map(|inv| mat_mul(&inv, world)) else { continue };
            if let Some(o) = self.occurrences.iter_mut().find(|o| o.id == *occ) {
                let (dt, da) = mismatch(&o.transform, &local);
                if dt > 1e-9 || da > 1e-10 {
                    o.transform = local;
                    changed = true;
                }
            }
        }
        changed
    }

    /// A frame for an as-built joint: the world frame `w` seen from an occurrence.
    pub fn frame_in_occurrence(&self, occ: u64, w: &Mat) -> Option<Snap> {
        let t = if occ == 0 { IDENTITY } else { occurrence_world(self, occ) };
        let local = mat_mul(&mat_inverse(&t)?, w);
        let o = apply_point(&local, Vec3::ZERO);
        let z = apply_vector(&local, Vec3::Z);
        let x = apply_vector(&local, Vec3::X);
        Some(Snap::Frame { origin: o, z, x: Some(x) })
    }

    /// A joint origin's frame in the world (for as-built joints and display).
    pub fn origin_world(&self, st: &ModelState, o: &JointOrigin) -> Result<Mat> {
        let f = resolve_origin(self, st, o)?;
        let t = if o.occurrence == 0 { IDENTITY } else { occurrence_world(self, o.occurrence) };
        Ok(mat_mul(&t, &f))
    }
}

#[path = "joints_cam.rs"]
pub mod cam;

#[path = "joints_edge.rs"]
pub(crate) mod edge;
pub use edge::edge_snap;

#[path = "joints_reach.rs"]
mod reach;

#[path = "joints_face.rs"]
pub(crate) mod face;
pub use face::{end_faces, face_name_at, named_face_spot, pin_end, round_end_at};
#[path = "joints_equations.rs"]
mod equations;
pub use equations::{Closing, Eq, Equation, Side, measured_angle, wrapped};

#[path = "joints_relations.rs"]
mod relations;
pub use reach::{REACH_TOL, Reach, Reached, reach};
pub use relations::{nearest_values, relation_values};

/// The rest angle that turns B least as joint `j` is made, as B sits now: none or a half turn
/// about the joint's z. Origins whose x is chosen for them (a face, a circle, a point without
/// x) get opposite x when their z are opposite, so two faces that already meet would, at angle
/// 0, spin B a half turn about their shared normal.
pub fn least_turn_angle(doc: &Document, st: &ModelState, j: &Joint) -> f64 {
    let (Ok(fa), Ok(fb)) = (resolve_origin(doc, st, &j.a), resolve_origin(doc, st, &j.b)) else { return 0.0 };
    let (ta, tb) = (occurrence_world(doc, j.a.occurrence), occurrence_world(doc, j.b.occurrence));
    let mut q = j.values.clone();
    q.resize(j.kind.dofs().len(), 0.0);
    clamp(j, &mut q);
    let turn = |angle: f64| {
        let k = Joint { angle, ..j.clone() };
        through(&k, &(fa, fb), &q, true, &ta).map(|m| mismatch(&m, &tb).1)
    };
    match (turn(0.0), turn(std::f64::consts::PI)) {
        (Some(none), Some(half)) if half + 1e-9 < none => std::f64::consts::PI,
        _ => 0.0,
    }
}

/// The frame matrix for an explicit origin / z / x (public for commands).
pub fn frame_of(o: Vec3, z: Vec3, x: Option<Vec3>) -> Option<Mat> {
    frame(o, z, x)
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        pub(super) static PLACEMENT_STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        pub(super) static PLACEMENT_INSERTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    /// A `Free` member's values give back the placement they were read from: no turn, a small
    /// one, a general one and half turns (where the rotation vector's axis comes from the
    /// symmetric part).
    #[test]
    fn free_values_round_trip() {
        let turns = [
            IDENTITY,
            rot_z(1e-7),
            mat_mul(&rot_x(0.4), &rot_y(-1.1)),
            rot_x(std::f64::consts::PI),
            mat_mul(&rot_z(0.3), &mat_mul(&rot_y(std::f64::consts::PI), &rot_z(-0.3))),
            mat_mul(&rot_z(2.0), &rot_x(std::f64::consts::PI - 1e-9)),
        ];
        for (k, r) in turns.iter().enumerate() {
            let m = mat_mul(&shift(3.0, -4.0, 12.5), r);
            let back = motion(JointKind::Free, &free_values(&m));
            let off = m.iter().flatten().zip(back.iter().flatten()).fold(0.0f64, |w, (a, b)| w.max((a - b).abs()));
            assert!(off < 1e-9, "turn {k}: off by {off}");
        }
        assert!((0..6).all(|i| JointKind::Free.is_angle(i) == (i >= 3)));
    }

    pub(super) fn placement_tree(asm: &Assembly) -> Tree<'_> {
        Tree {
            joints: asm.joints.iter().map(|j| (j.id, j)).collect(),
            equations: &[],
            frames: asm.joints.iter().map(|j| (j.id, JointFrames::new(mat_mul(&shift(3.0, 4.0, 5.0), &rot_y(0.3)), rot_x(-0.2)))).collect(),
            fixed: BTreeMap::from([(0, IDENTITY)]),
            parent: BTreeMap::from([(1, (0, 1, true)), (2, (1, 2, false)), (3, (2, 3, true)), (4, (1, 4, true)), (99, (0, 5, true))]),
            order: vec![1, 2, 3, 4, 99],
        }
    }

    /// Shared ancestors are evaluated once, without visiting an unrelated assembly branch.
    /// The same arithmetic must be used after values change and on a reversed tree edge.
    #[test]
    fn residual_placements_reuse_ancestors_only_for_the_current_values() {
        let asm = chain(6, 0.2);
        let tree = placement_tree(&asm);
        let mut values = joint_values(&asm, &Compiled::default());
        let expected: BTreeMap<u64, Mat> = [1, 2, 3, 4].into_iter().map(|id| (id, tree.world(id, &values).unwrap())).collect();
        let mut workspace = PlacementWorkspace::new(&tree);
        PLACEMENT_STEPS.with(|steps| steps.set(0));
        {
            let mut placed = workspace.at(&values);
            for id in [3, 4, 3, 2, 4] {
                assert_eq!(placed.world(id), expected.get(&id).copied());
            }
            assert_eq!(placed.world(0), Some(IDENTITY));
            assert_eq!(placed.world(1234), None);
        }
        assert_eq!(PLACEMENT_STEPS.with(|steps| steps.get()), 4, "one step per needed occurrence; no unrelated branch");
        values.get_mut(&1).unwrap()[0] += 0.1;
        apply_links(&asm, &mut values);
        let changed = tree.world(3, &values).unwrap();
        assert_ne!(Some(&changed), expected.get(&3));
        let mut placed = workspace.at(&values);
        assert_eq!(placed.world(3), Some(changed), "a new residual evaluation cannot reuse the previous values' placements");
    }

    /// Cached static inverses retain the original multiplication order for every joint kind,
    /// flip and traversal direction. A non-invertible frame remains a failed step.
    #[test]
    fn prepared_joint_frames_match_fresh_inverses() {
        fn reference(j: &Joint, fa: &Mat, fb: &Mat, q: &[f64], from_a: bool, t: &Mat) -> Option<Mat> {
            let link = mat_mul(&link_of(j, q), &mat_inverse(fb)?);
            if from_a { Some(mat_mul(&mat_mul(t, fa), &link)) } else { Some(mat_mul(&mat_mul(t, &mat_inverse(&link)?), &mat_inverse(fa)?)) }
        }
        let mut joint = chain(1, 0.0).joints.remove(0);
        let fa = mat_mul(&shift(3.0, -7.0, 11.0), &rot_y(0.3));
        let fb = mat_mul(&shift(-2.0, 5.0, 13.0), &rot_x(-0.2));
        let t = mat_mul(&shift(17.0, 19.0, -23.0), &rot_z(0.7));
        let singular = [[0.0; 4]; 4];
        for (a, b) in [(fa, fb), (singular, fb), (fa, singular)] {
            let frames = JointFrames::new(a, b);
            for kind in [
                JointKind::Rigid,
                JointKind::Revolute,
                JointKind::Slider,
                JointKind::Cylindrical,
                JointKind::PinSlot,
                JointKind::Planar,
                JointKind::Ball,
                JointKind::Parallel,
                JointKind::Perpendicular,
                JointKind::Angle,
                JointKind::PointOnPlane,
                JointKind::PointOnLine,
                JointKind::LineOnPlane,
            ] {
                joint.kind = kind;
                joint.offset = 2.7;
                joint.angle = -0.41;
                for flip in [false, true] {
                    joint.flip = flip;
                    for angle in [0.0, 1e-6, -0.37, std::f64::consts::PI] {
                        let q: Vec<f64> = (0..kind.dofs().len()).map(|i| angle * (i + 1) as f64).collect();
                        for forward in [false, true] {
                            assert_eq!(
                                frames.through(&joint, &q, forward, &t),
                                reference(&joint, &a, &b, &q, forward, &t),
                                "{kind:?}, {flip}, {angle}, {forward}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// The loop solver's residual entry point reuses shared ancestors for each perturbation;
    /// row order, duplicate rows, append semantics and missing-origin failure stay unchanged.
    #[test]
    fn closure_residuals_reuse_shared_ancestors_for_each_perturbation() {
        let mut asm = chain(6, 0.2);
        asm.joints[5].a.occurrence = 3;
        asm.joints[5].b.occurrence = 4;
        let tree = placement_tree(&asm);
        let mut workspace = PlacementWorkspace::new(&tree);
        let mut cold = true;
        for start in [0.2, 0.2 + 1e-6, 0.2 - 1e-6, 1.3] {
            let mut values = joint_values(&asm, &Compiled::default());
            values.get_mut(&1).unwrap()[0] = start;
            apply_links(&asm, &mut values);
            let mut expected = vec![42.0];
            for id in [6, 6] {
                let joint = tree.joints.get(&id).unwrap();
                let frames = tree.frames.get(&id).unwrap();
                let a = tree.world(joint.a.occurrence, &values).unwrap();
                let b = tree.world(joint.b.occurrence, &values).unwrap();
                let want = through(joint, &(frames.a, frames.b), values.get(&id).unwrap(), true, &a).unwrap();
                for point in [Vec3::ZERO, Vec3::X * 37.0, Vec3::Y * 37.0] {
                    let at = apply_point(&frames.b, point);
                    let d = apply_point(&want, at) - apply_point(&b, at);
                    expected.extend([d.x, d.y, d.z]);
                }
            }
            PLACEMENT_STEPS.with(|steps| steps.set(0));
            PLACEMENT_INSERTS.with(|inserts| inserts.set(0));
            let mut actual = vec![42.0];
            assert_eq!(closure_gaps(&mut workspace, &[Closing::Joint(6), Closing::Joint(6)], &values, 37.0, &mut actual), Some(()));
            assert_eq!(actual, expected);
            assert_eq!(PLACEMENT_INSERTS.with(|inserts| inserts.get()), if cold { 4 } else { 0 }, "reuse cache storage between perturbations");
            cold = false;
            assert_eq!(PLACEMENT_STEPS.with(|steps| steps.get()), 6, "four tree placements and two closing-joint steps");
            assert_eq!(closure_gaps(&mut workspace, &[Closing::Joint(u64::MAX)], &values, 37.0, &mut actual), None);
            assert_eq!(actual, expected, "an unresolved joint does not insert a zero residual");
        }
    }

    fn close(a: &Mat, b: &Mat) -> bool {
        let (t, r) = mismatch(a, b);
        t < 1e-9 && r < 1e-9
    }

    /// Revolute joints 1…n (joint 1 at `start`, the others at 0) linked 1 → 2 → … → n with
    /// ratio 2, the links stored from the end of the chain; joint 3 limited to ±50.
    pub(super) fn chain(n: u64, start: f64) -> Assembly {
        let origin = JointOrigin { occurrence: 0, snap: Snap::Point { point: Vec3::ZERO, z: None, x: None } };
        let joints = (1..=n)
            .map(|id| Joint {
                id,
                name: format!("J{id}"),
                kind: JointKind::Revolute,
                a: origin.clone(),
                b: origin.clone(),
                values: vec![if id == 1 { start } else { 0.0 }],
                limits: vec![(id == 3).then_some((-50.0, 50.0))],
                flip: false,
                offset: 0.0,
                angle: 0.0,
                suppressed: false,
                exprs: Default::default(),
            })
            .collect();
        let links =
            (1..n).rev().map(|a| MotionLink { a, ia: 0, b: a + 1, ib: 0, ratio: 2.0, offset: 0.0, coupling: None, lift: Vec::new() }).collect();
        Assembly { joints, links, ..Assembly::default() }
    }

    /// A linked value moves with the free value its chain starts from, however the chain is
    /// stored; a chain that comes back round on itself, or a link from a value its joint lacks
    /// (hand-edited designs), moves it with nothing.
    #[test]
    fn linked_values_move_with_their_chains_start() {
        let mut asm = chain(5, 0.0);
        let sources = link_sources(&asm);
        for b in 2..=5 {
            assert_eq!(sources.get(&(b, 0)), Some(&BTreeSet::from([(1, 0)])), "J{b}: {sources:?}");
        }
        assert!(!sources.contains_key(&(1, 0)));
        let link = |a: u64, ia: usize, b: u64| MotionLink { a, ia, b, ib: 0, ratio: 1.0, offset: 0.0, coupling: None, lift: Vec::new() };
        asm.links.push(link(5, 0, 1));
        assert!(link_sources(&asm).values().all(BTreeSet::is_empty), "{:?}", link_sources(&asm));
        let mut asm = chain(2, 0.0);
        asm.links = vec![link(1, 7, 2)];
        assert_eq!(link_sources(&asm).get(&(2, 0)), Some(&BTreeSet::new()));
        assert!(link_sources(&Assembly::default()).is_empty());
    }

    /// Links carry a value down a chain stored from its end, each linked value within its limits
    /// as it is set, and applying them again changes nothing.
    #[test]
    fn links_settle_whatever_order_they_are_stored_in() {
        let asm = chain(8, 10.0);
        let vals = joint_values(&asm, &Compiled::default());
        let at = |id: u64| vals.get(&id).and_then(|q| q.first()).copied().unwrap_or(f64::NAN);
        // 10 → 20 → 40 (within joint 3's limit) → 80 → … → 1280.
        assert_eq!((at(1), at(2), at(3), at(4), at(8)), (10.0, 20.0, 40.0, 80.0, 1280.0));
        let asm = chain(8, 40.0);
        let vals = joint_values(&asm, &Compiled::default());
        let at = |id: u64| vals.get(&id).and_then(|q| q.first()).copied().unwrap_or(f64::NAN);
        // Joint 3 would be 160: held at 50, and what follows it follows the 50.
        assert_eq!((at(3), at(4), at(8)), (50.0, 100.0, 1600.0));
        let mut again = vals.clone();
        apply_links(&asm, &mut again);
        assert_eq!(again, vals);
    }

    /// A coupling saves as its definition: sizes as numbers, expressions or picked faces, flags
    /// only when set; a link without one (as every design before couplings has) reads as before.
    #[test]
    fn couplings_read_and_write_their_definition() {
        let old: MotionLink = serde_json::from_str(r#"{"a": 1, "b": 2, "ratio": 2}"#).unwrap();
        assert_eq!((old.coupling, old.offset, old.ia), (None, 0.0, 0));
        let text = r#"{"a": 1, "b": 2, "ratio": -0.5, "coupling": {"type": "gear", "ra": 10, "rb": "r2 * 2",
            "internal": true}}"#;
        let l: MotionLink = serde_json::from_str(text).unwrap();
        let Some(Coupling::Gear { ra, rb, internal, reverse }) = &l.coupling else { panic!("{l:?}") };
        assert_eq!((ra, rb, *internal, *reverse), (&Size::Length(10.0), &Size::Expr("r2 * 2".into()), true, false));
        let back = serde_json::to_value(&l).unwrap();
        assert_eq!(back["coupling"], serde_json::json!({"type": "gear", "ra": 10.0, "rb": "r2 * 2", "internal": true}));
        let picked: Size = serde_json::from_str(r#"{"occurrence": 3, "snap": {"type": "circle_center", "pick": [1, 0, 0]}}"#).unwrap();
        assert!(matches!(picked, Size::Circle(JointOrigin { occurrence: 3, snap: Snap::CircleCenter { .. } })));
        let screw: Coupling = serde_json::from_str(r#"{"type": "screw", "pitch": 1.5, "reverse": true}"#).unwrap();
        assert_eq!(screw, Coupling::Screw { pitch: Size::Length(1.5), reverse: true });
    }

    /// Ratios from the sizes: gears turn by their radii (the other way unless internal), a rack
    /// and a screw travel per radian whichever of the two joints drives.
    #[test]
    fn coupling_ratios_follow_their_sizes() {
        let mut doc = Document { assembly: chain(3, 0.0), ..Document::default() };
        if let Some(j) = doc.assembly.joints.get_mut(2) {
            j.kind = JointKind::Slider;
        }
        let st = ModelState::default();
        let ratio = |a: u64, b: u64, c: Coupling| {
            let l = MotionLink { a, ia: 0, b, ib: 0, ratio: 0.0, offset: 0.0, coupling: Some(c), lift: Vec::new() };
            coupling_values(&doc, &st, &l).map(|r| r.map(|(ratio, _)| ratio).map_err(|e| e.to_string()))
        };
        let len = Size::Length;
        assert_eq!(ratio(1, 2, Coupling::Gear { ra: len(10.0), rb: len(40.0), internal: false, reverse: false }), Some(Ok(-0.25)));
        assert_eq!(ratio(1, 2, Coupling::Gear { ra: len(10.0), rb: len(40.0), internal: true, reverse: false }), Some(Ok(0.25)));
        assert_eq!(ratio(1, 3, Coupling::RackPinion { radius: len(8.0), reverse: false }), Some(Ok(8.0)));
        assert_eq!(ratio(3, 1, Coupling::RackPinion { radius: len(8.0), reverse: true }), Some(Ok(-0.125)));
        let screw = ratio(1, 3, Coupling::Screw { pitch: len(std::f64::consts::TAU), reverse: false });
        assert_eq!(screw, Some(Ok(1.0)));
        assert!(matches!(ratio(1, 2, Coupling::RackPinion { radius: len(8.0), reverse: false }), Some(Err(_))));
        assert!(matches!(ratio(1, 3, Coupling::Gear { ra: len(1.0), rb: len(1.0), internal: false, reverse: false }), Some(Err(_))));
        for bad in [0.0, -2.0, f64::NAN, f64::INFINITY, 1e300] {
            assert!(matches!(ratio(1, 2, Coupling::Gear { ra: len(bad), rb: len(1.0), internal: false, reverse: false }), Some(Err(_))), "{bad}");
        }
        assert!(matches!(ratio(1, 2, Coupling::Gear { ra: Size::Expr("nope".into()), rb: len(1.0), internal: false, reverse: false }), Some(Err(_))));
    }

    #[test]
    fn motions_compose() {
        assert!(close(&motion(JointKind::Revolute, &[std::f64::consts::FRAC_PI_2]), &rot_z(std::f64::consts::FRAC_PI_2)));
        let m = motion(JointKind::Cylindrical, &[0.0, 5.0]);
        assert!((m[3][2] - 5.0).abs() < 1e-12);
        let m = motion(JointKind::PinSlot, &[0.0, 3.0]);
        assert!((m[3][0] - 3.0).abs() < 1e-12);
        assert_eq!(JointKind::Ball.dofs().len(), 3);
        assert!(JointKind::parse("Pin Slot").is_some() && JointKind::parse("nope").is_none());
        let f = frame(Vec3::new(1.0, 2.0, 3.0), Vec3::Z, None).unwrap();
        assert!(close(&mat_mul(&f, &mat_inverse(&f).unwrap()), &IDENTITY));
    }

    #[test]
    fn mismatch_resolves_tiny_turns() {
        let (_, a) = mismatch(&rot_z(1e-9), &IDENTITY);
        assert!((a - 1e-9).abs() < 1e-15, "{a}");
        let (_, a) = mismatch(&rot_z(3.0), &IDENTITY);
        assert!((a - 3.0).abs() < 1e-12, "{a}");
    }
    /// R151: an equation reading a part nothing places is an error naming the joint that would
    /// have placed it, not a silent pass.
    #[test]
    fn an_equation_over_an_unplaced_part_is_an_error() {
        let mut doc = Document::new("ghost");
        let comp = doc.add_component(Some("A"), 0).unwrap();
        let a = doc.occurrences.iter().find(|o| o.component == comp).unwrap().id;
        doc.occurrences.iter_mut().for_each(|o| o.grounded = true);
        let ghost = 999_999;
        let joint = Joint {
            id: COMPILED_BASE + 4,
            name: "Ghost".into(),
            kind: JointKind::Rigid,
            a: JointOrigin { occurrence: a, snap: Snap::Frame { origin: Vec3::ZERO, z: Vec3::Z, x: None } },
            b: JointOrigin { occurrence: ghost, snap: Snap::FaceCenter { pick: Vec3::ZERO, face: Some("gone".into()), x: None } },
            values: Vec::new(),
            limits: Vec::new(),
            flip: true,
            offset: 0.0,
            angle: 0.0,
            suppressed: false,
            exprs: JointExprs::default(),
        };
        let plane = |occurrence| Side { occurrence, prim: crate::constraints::Prim::Plane { p: Vec3::ZERO, n: Vec3::Z } };
        let eq = Eq {
            id: COMPILED_BASE + 8,
            name: "Angle:1".into(),
            eq: Equation::Angle { a: plane(a), b: plane(ghost), axis: None, target: 0.5, bounds: None },
        };
        let extra = Compiled { joints: vec![joint], equations: vec![eq], lost: Vec::new() };
        let sol = solve_with(&doc, &ModelState::default(), &Held::new(), Analysis::Placement, &extra);
        let why: Vec<&str> = sol.errors.iter().filter(|(id, ..)| *id == COMPILED_BASE + 8).map(|(_, _, w)| w.as_str()).collect();
        assert_eq!(why, ["a part it reads is not placed (Ghost doesn't resolve)"], "{:?}", sol.errors);
    }
}
