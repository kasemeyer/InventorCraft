//! Shared test fixtures (test builds only). Task 0 of the assembly-constraints plan: a
//! procedural casing-tong assembly whose joints are compiled by hand, the density Inventor-style
//! constraints compile to (spec §8.1).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use solidcraft_doc::joints::{Joint, JointExprs, JointKind, JointOrigin, Snap};
use solidcraft_doc::{BodyName, Document, FeatureKind, Mat, Operation, apply_point, apply_vector, mat_inverse, point_expr};
use solidcraft_geom::Vec3;

/// Shared, not copied (R121): where circles about `p` (radius `r`) and `q` (radius `s`) meet,
/// the upper meeting point first.
pub(crate) use crate::cmd::joints::tests::meet;

/// One four-bar module of the tong: its members' and pins' occurrences, and two joints the spike
/// drives.
pub(crate) struct TongModule {
    pub jaw: u64,
    pub link: u64,
    pub lever: u64,
    /// Pins at A, B, C, D.
    pub pins: [u64; 4],
    /// The Cylindrical pin-A → jaw joint id.
    pub crank: u64,
    /// The jaw → carrier +Z Planar joint id.
    pub carrier_z: u64,
}

pub(crate) struct Tong {
    pub session: crate::Session,
    pub body: u64,
    pub modules: Vec<TongModule>,
    /// Degrees of freedom per occurrence, counted by hand.
    pub hand_dof: BTreeMap<u64, usize>,
    pub joints: usize,
}

/// `translate(p) · rotZ(theta)`: a component modelled about its own origin, placed.
fn placed_at(p: Vec3, theta: f64) -> Mat {
    let (s, c) = theta.sin_cos();
    [[c, s, 0.0, 0.0], [-s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [p.x, p.y, p.z, 1.0]]
}

fn mm(v: f64) -> solidcraft_doc::Expr {
    solidcraft_doc::Expr::new(format!("{v} mm"))
}

fn boxed(corner: Vec3, l: f64, w: f64, h: f64, op: Operation) -> FeatureKind {
    FeatureKind::Box { corner: point_expr(corner), length: mm(l), width: mm(w), height: mm(h), operation: op }
}

fn cylinder(base: Vec3, diameter: f64, height: f64, op: Operation) -> FeatureKind {
    FeatureKind::Cylinder {
        base: point_expr(base),
        axis: solidcraft_doc::Dir(Vec3::Z),
        radius: mm(diameter / 2.0),
        height: mm(height),
        operation: op,
    }
}

/// The builder: components with their geometry about their own origins, and hand-compiled joints.
struct Builder {
    doc: Document,
    /// Occurrence → its transform (world: every component is a child of the root).
    place: BTreeMap<u64, Mat>,
    joints: Vec<Joint>,
}

impl Builder {
    /// A component named `name` whose bodies are `solid` (the first a new body named after the
    /// component, the rest cut from it), placed at `t`. Returns its occurrence.
    fn part(&mut self, name: &str, solid: Vec<FeatureKind>, t: Mat) -> u64 {
        let comp = self.doc.add_component(Some(name), 0).unwrap();
        let mut first = true;
        for kind in solid {
            let id = self.doc.add_feature(kind, None).unwrap();
            if first {
                self.doc.features.iter_mut().find(|f| f.id == id).unwrap().body_names = vec![name.to_string()];
                first = false;
            }
        }
        // Built in the root about the origin, then moved into its component as it is: the
        // component's own coordinates.
        let transfer = FeatureKind::BodyTransfer { bodies: vec![BodyName::from(name)], from: 0, to: comp, transform: solidcraft_doc::IDENTITY };
        self.doc.add_feature(transfer, None).unwrap();
        let o = self.doc.occurrences.iter_mut().find(|o| o.component == comp).unwrap();
        o.transform = t;
        let id = o.id;
        self.place.insert(id, t);
        id
    }

    /// A frame at world point `p`, z along `z`, x along `x`, in occurrence `occ`'s coordinates.
    fn origin(&self, occ: u64, p: Vec3, z: Vec3, x: Vec3) -> JointOrigin {
        let inv = mat_inverse(&self.place[&occ]).unwrap();
        JointOrigin { occurrence: occ, snap: Snap::Frame { origin: apply_point(&inv, p), z: apply_vector(&inv, z), x: Some(apply_vector(&inv, x)) } }
    }

    /// A joint A → B with both frames at world `p` (z along `z`), flipped (B's z along A's), at
    /// values 0: the built pose meets it with no gap.
    fn joint(&mut self, kind: JointKind, a: u64, b: u64, p: Vec3, z: Vec3) -> u64 {
        // x square to z: world X, or world Z for a z in the XY plane.
        let x = if z.z.abs() > 0.5 { Vec3::X } else { Vec3::Z };
        let id = self.doc.allocate_id().unwrap();
        let j = Joint {
            id,
            name: format!("{kind:?}{}", self.joints.len() + 1),
            kind,
            a: self.origin(a, p, z, x),
            b: self.origin(b, p, z, x),
            values: vec![0.0; kind.dofs().len()],
            limits: Vec::new(),
            flip: true,
            offset: 0.0,
            angle: 0.0,
            suppressed: false,
            exprs: JointExprs::default(),
        };
        self.joints.push(j);
        id
    }
}

/// A deterministic casing-tong assembly of `modules` four-bar modules (spike: 9).
///
/// Module `m` at z0 = 40·m: pivots A = (0, 0, z0), D = (45, 0, z0), B = (0, 20, z0), C =
/// `meet(B, 50, D, 40)[0]` (crank 20, coupler 50, rocker 40, ground 45). Members jaw J (A–B), link
/// L (B–C), lever V (C–D); a pin at each pivot; two fasteners per member; a die carrier at the
/// midpoint of J and of V, each with a die 5 mm above it. 17 occurrences and 46 joints a module.
pub(crate) fn tong(modules: usize) -> Tong {
    let mut b = Builder { doc: Document::new("tong"), place: BTreeMap::new(), joints: Vec::new() };
    let height = 40.0 * modules as f64 + 8.0;
    // The body: holes at A and D through every module.
    let through = |x: f64| cylinder(Vec3::new(x, 0.0, -9.0), 6.0, height + 2.0, Operation::Cut);
    let body = b.part(
        "Body",
        vec![boxed(Vec3::new(-30.0, -20.0, -8.0), 120.0, 60.0, height, Operation::NewBody), through(0.0), through(45.0)],
        solidcraft_doc::IDENTITY,
    );
    let mut hand_dof = BTreeMap::from([(body, 0)]);
    let mut out = Vec::new();
    let z = Vec3::Z;
    for m in 0..modules {
        let z0 = 40.0 * m as f64;
        let (pa, pd, pb) = (Vec3::new(0.0, 0.0, z0), Vec3::new(45.0, 0.0, z0), Vec3::new(0.0, 20.0, z0));
        let pc = meet(pb, 50.0, pd, 40.0)[0];
        // Members: (name, from, to).
        let segs = [("Jaw", pa, pb), ("Link", pb, pc), ("Lever", pc, pd)];
        let mut members = Vec::new();
        for (name, p, q) in segs {
            let (d, len) = (q - p, q.dist(p));
            let theta = d.y.atan2(d.x);
            let hole = |x: f64| cylinder(Vec3::new(x, 0.0, -1.0), 6.0, 10.0, Operation::Cut);
            let solid = vec![
                boxed(Vec3::new(-5.0, -5.0, 0.0), len + 10.0, 10.0, 8.0, Operation::NewBody),
                hole(0.0),
                hole(len),
                hole(len / 3.0),
                hole(2.0 * len / 3.0),
            ];
            let occ = b.part(&format!("{name} {m}"), solid, placed_at(p, theta));
            members.push((occ, p, q, theta, d * (1.0 / len)));
        }
        let (jaw, link, lever) = (members[0].0, members[1].0, members[2].0);
        // Pins: (pivot, home, other, the angle the pin is placed at).
        let pin_rows =
            [("A", pa, body, jaw, 0.0), ("B", pb, jaw, link, members[0].3), ("C", pc, link, lever, members[1].3), ("D", pd, body, lever, 0.0)];
        let mut pins = [0u64; 4];
        let mut crank = 0;
        for (k, (tag, p, home, other, theta)) in pin_rows.into_iter().enumerate() {
            let pin =
                b.part(&format!("Pin {tag} {m}"), vec![cylinder(Vec3::new(0.0, 0.0, -8.0), 6.0, 20.0, Operation::NewBody)], placed_at(p, theta));
            pins[k] = pin;
            b.joint(JointKind::Rigid, home, pin, p, z);
            let cyl = b.joint(JointKind::Cylindrical, pin, other, p, z);
            if k == 0 {
                crank = cyl;
            }
            b.joint(JointKind::Planar, home, other, p, z);
            b.joint(JointKind::Planar, pin, other, p, z);
        }
        // Fasteners at 1/3 and 2/3 of each member.
        for (mi, &(occ, p, q, theta, _)) in members.iter().enumerate() {
            for (k, t) in [1.0 / 3.0, 2.0 / 3.0].into_iter().enumerate() {
                let at = p + (q - p) * t;
                let f = b.part(
                    &format!("Fastener {m}.{mi}.{k}"),
                    vec![cylinder(Vec3::new(0.0, 0.0, -8.0), 6.0, 12.0, Operation::NewBody)],
                    placed_at(at, theta),
                );
                b.joint(JointKind::Rigid, occ, f, at, z);
                b.joint(JointKind::Cylindrical, occ, f, at, z);
                b.joint(JointKind::Planar, occ, f, at, z);
            }
        }
        // Die carriers at the middle of the jaw and the lever, each with a die above it.
        let mut carrier_z = 0;
        for (mi, &(occ, p, q, theta, u)) in [members[0], members[2]].iter().enumerate() {
            let mid = (p + q) * 0.5;
            let k = b.part(
                &format!("Carrier {m}.{mi}"),
                vec![boxed(Vec3::new(-8.0, -5.0, -3.0), 16.0, 10.0, 6.0, Operation::NewBody)],
                placed_at(mid, theta),
            );
            let up = b.joint(JointKind::Planar, occ, k, mid, z);
            if mi == 0 {
                carrier_z = up;
            }
            b.joint(JointKind::Planar, occ, k, mid, u);
            b.joint(JointKind::Planar, occ, k, mid, z.cross(u));
            let at = mid + Vec3::new(0.0, 0.0, 5.0);
            let e =
                b.part(&format!("Die {m}.{mi}"), vec![boxed(Vec3::new(-6.0, -4.0, -2.0), 12.0, 8.0, 4.0, Operation::NewBody)], placed_at(at, theta));
            b.joint(JointKind::Rigid, k, e, at, z);
            b.joint(JointKind::Planar, k, e, at, z);
            b.joint(JointKind::Planar, k, e, at, u);
            hand_dof.insert(k, 1);
            hand_dof.insert(e, 1);
        }
        // Hand count: the pins at A and D are fixed to the body; everything else in the module
        // moves with the four-bar's one motion.
        let fixed = [pins[0], pins[3]];
        for occ in b.place.keys().copied().filter(|o| *o != body && !hand_dof.contains_key(o)).collect::<Vec<_>>() {
            hand_dof.insert(occ, usize::from(!fixed.contains(&occ)));
        }
        out.push(TongModule { jaw, link, lever, pins, crank, carrier_z });
    }
    let joints = b.joints.len();
    b.doc.assembly.joints = b.joints;
    Tong { session: crate::Session::new(b.doc), body, modules: out, hand_dof, joints }
}

// Task 2 (assembly constraints): the blocks fixture and its helpers.

/// `s.execute(id, &p)`, panicking with the message on error.
pub(crate) fn run(s: &mut crate::Session, id: &str, p: serde_json::Value) -> serde_json::Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// Occurrence id of component `name` (as `cmd/joints_tests.rs` `occ`).
pub(crate) fn occ(s: &crate::Session, name: &str) -> u64 {
    let c = s.doc.find_component(name).unwrap_or_else(|| panic!("no component {name}"));
    s.doc.occurrence_of(c).map(|o| o.id).unwrap_or_else(|| panic!("component {name} is not placed"))
}

/// A world point of occurrence `name`'s component point `p`.
pub(crate) fn at(s: &crate::Session, name: &str, p: Vec3) -> Vec3 {
    apply_point(&solidcraft_doc::joints::occurrence_world(&s.doc, occ(s, name)), p)
}

/// Components by commands (`component.create {name, grounded}`, then the primitive, then
/// `component.activate {"component": "root"}`), in this order:
///   "A" grounded: solid.box {length 100, width 50, height 20, corner [0, 0, 0]};
///   "B": solid.box {100, 50, 20, corner [0, 0, 40]};
///   "Plate": solid.box {80, 80, 10, corner [150, -40, 0], name "Plate"}, then
///            solid.cylinder {diameter 20, height 12, base [190, 0, -1], operation "cut", name "Hole"};
///   "Pin": solid.cylinder {diameter 20, height 60, base [300, 0, 50], name "PinBody"};
///   "Drum": solid.cylinder {diameter 30, height 40, base [400, 0, 0]};
///   "Ball": solid.sphere {diameter 30, center [500, 0, 50]}.
pub(crate) fn blocks() -> crate::Session {
    use serde_json::json;
    let mut s = crate::Session::default();
    let parts: [(&str, bool, Vec<(&str, serde_json::Value)>); 6] = [
        ("A", true, vec![("solid.box", json!({"length": 100, "width": 50, "height": 20, "corner": [0, 0, 0]}))]),
        ("B", false, vec![("solid.box", json!({"length": 100, "width": 50, "height": 20, "corner": [0, 0, 40]}))]),
        (
            "Plate",
            false,
            vec![
                ("solid.box", json!({"length": 80, "width": 80, "height": 10, "corner": [150, -40, 0], "name": "Plate"})),
                ("solid.cylinder", json!({"diameter": 20, "height": 12, "base": [190, 0, -1], "operation": "cut", "name": "Hole"})),
            ],
        ),
        ("Pin", false, vec![("solid.cylinder", json!({"diameter": 20, "height": 60, "base": [300, 0, 50], "name": "PinBody"}))]),
        ("Drum", false, vec![("solid.cylinder", json!({"diameter": 30, "height": 40, "base": [400, 0, 0]}))]),
        ("Ball", false, vec![("solid.sphere", json!({"diameter": 30, "center": [500, 0, 50]}))]),
    ];
    for (name, grounded, steps) in parts {
        run(&mut s, "component.create", json!({"name": name, "grounded": grounded}));
        for (id, p) in steps {
            run(&mut s, id, p);
        }
        run(&mut s, "component.activate", json!({"component": "root"}));
    }
    s
}

// Task 3 (assembly constraints): a constraint record pushed from world picks.

/// Push a constraint of `kind` (`"mate"`, `"tangent"`, `"insert"`…) with `solution` built from
/// world picks `(component, what, point)`, offset `offset` mm and `lock`, then re-solve once
/// (`cmd::joints::resolve`; no `constraint.*` command yet). Returns its id.
pub(crate) fn add(
    s: &mut crate::Session,
    kind: &str,
    solution: &str,
    picks: &[(&str, solidcraft_doc::constraints::PickWhat, [f64; 3])],
    offset: f64,
    lock: bool,
) -> u64 {
    use solidcraft_doc::constraints::{Constraint, ConstraintKind, ConstraintSolution, geo_at, next_name};
    let kind = ConstraintKind::parse(kind).unwrap_or_else(|| panic!("no constraint kind {kind}"));
    let solution = ConstraintSolution::parse(solution).unwrap_or_else(|| panic!("no solution {solution}"));
    let st = s.model.state();
    let picks = picks
        .iter()
        .map(|(name, what, p)| {
            let p = Vec3::new(p[0], p[1], p[2]);
            geo_at(&s.doc, &st, occ(s, name), *what, p).unwrap_or_else(|e| panic!("{name} {what:?} at {p:?}: {e}"))
        })
        .collect();
    let doc = s.doc_mut();
    let id = doc.allocate_id().unwrap();
    let name = next_name(&doc.assembly, kind);
    let c = Constraint {
        id,
        name,
        kind,
        solution,
        picks,
        offset,
        limits: Default::default(),
        exprs: Default::default(),
        suppressed: false,
        lock,
        axis: None,
        state: Vec::new(),
        signatures: Vec::new(),
    };
    c.validate().unwrap();
    doc.assembly.constraints.push(c);
    crate::cmd::joints::resolve(s);
    id
}
