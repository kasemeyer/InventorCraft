//! Loop equations: constraints with no joint form (spec §3.5), solved in the same loop groups as
//! closing joints. Defined here, where `constraints::compile` first carries them (R114).
//!
//! An equation reads only where its occurrences are: each side is a primitive in its
//! occurrence's component coordinates, carried into the world by the placement the solve is
//! trying. Its residual is one number, radians for an angle and millimetres for a distance; the
//! loop solver weighs a radian as far as it moves a point across the loop (`closure_gaps`).

use solidcraft_geom::Vec3;

use crate::constraints::geo::Prim;
use crate::{Mat, apply_point, apply_vector};

/// One side of an equation: a primitive in occurrence `occurrence`'s component coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Side {
    pub occurrence: u64,
    pub prim: Prim,
}

impl Side {
    /// The direction it gives an angle in the world: a plane's normal, a line's, a cylinder's or
    /// a circle's axis. `None` for a point or a sphere, or where its occurrence is not placed.
    fn direction(&self, world: &dyn Fn(u64) -> Option<Mat>) -> Option<Vec3> {
        let d = match self.prim {
            Prim::Plane { n, .. } => n,
            Prim::Line { d, .. } | Prim::Cylinder { d, .. } => d,
            Prim::Circle { axis, .. } => axis,
            Prim::Point { .. } | Prim::Sphere { .. } => return None,
        };
        apply_vector(&world(self.occurrence)?, d).normalized()
    }

    /// The line it gives in the world (a point on it, its unit direction).
    fn line(&self, world: &dyn Fn(u64) -> Option<Mat>) -> Option<(Vec3, Vec3)> {
        let (p, d) = self.prim.line()?;
        let m = world(self.occurrence)?;
        Some((apply_point(&m, p), apply_vector(&m, d).normalized()?))
    }
}

/// An equation's kind and its terms.
#[derive(Clone, Debug, PartialEq)]
pub enum Equation {
    /// The angle from a's direction to b's, both projected square to `axis` (None: unsigned,
    /// 0..π), minus `target` (radians). With `bounds`, 0 inside them instead.
    Angle { a: Side, b: Side, axis: Option<Side>, target: f64, bounds: Option<(f64, f64)> },
    /// Distance between two parallel lines minus `target` (mm). With `bounds`, 0 inside them
    /// instead.
    AxisDistance { a: Side, b: Side, target: f64, bounds: Option<(f64, f64)> },
}

/// `v` measured against a target or bounds (spec §3.5 `Bounded`): inside `[lo, hi]` 0, else how
/// far past the nearer bound (signed, so the solver sees which way to go); with no bounds,
/// `v − target`.
fn bounded(v: f64, target: f64, bounds: Option<(f64, f64)>) -> f64 {
    match bounds {
        Some((lo, _)) if v < lo => v - lo,
        Some((_, hi)) if v > hi => v - hi,
        Some(_) => 0.0,
        None => v - target,
    }
}

/// An angle within a half turn of 0: (−π, π].
fn wrapped(a: f64) -> f64 {
    let w = a.rem_euclid(std::f64::consts::TAU);
    if w > std::f64::consts::PI { w - std::f64::consts::TAU } else { w }
}

impl Equation {
    /// The occurrences it reads, each once, in pick order.
    pub fn occurrences(&self) -> Vec<u64> {
        let sides: Vec<&Side> = match self {
            Self::Angle { a, b, axis, .. } => [Some(a), Some(b), axis.as_ref()].into_iter().flatten().collect(),
            Self::AxisDistance { a, b, .. } => vec![a, b],
        };
        let mut out = Vec::new();
        for s in sides {
            if !out.contains(&s.occurrence) {
                out.push(s.occurrence);
            }
        }
        out
    }

    /// Is its residual an angle (radians), not a distance (mm)?
    pub fn is_angle(&self) -> bool {
        matches!(self, Self::Angle { .. })
    }

    /// How far it is from being met with the occurrences where `world` puts them: radians for
    /// an angle, mm for a distance. `None` when a side has no direction or line there.
    pub fn residual(&self, world: &dyn Fn(u64) -> Option<Mat>) -> Option<f64> {
        match self {
            Self::Angle { a, b, axis, target, bounds } => {
                let (da, db) = (a.direction(world)?, b.direction(world)?);
                match axis {
                    // Unsigned: 0..π, from the sine and the cosine (no rounding loss near 0).
                    None => Some(bounded(da.cross(db).len().atan2(da.dot(db)), *target, *bounds)),
                    // Signed about the axis, both projected square to it (right-hand rule).
                    Some(k) => {
                        let k = k.direction(world)?;
                        let (pa, pb) = (da - k * da.dot(k), db - k * db.dot(k));
                        let v = pa.cross(pb).dot(k).atan2(pa.dot(pb));
                        Some(if bounds.is_some() { bounded(v, *target, *bounds) } else { wrapped(v - target) })
                    }
                }
            }
            Self::AxisDistance { a, b, target, bounds } => {
                let ((p, d), (q, _)) = (a.line(world)?, b.line(world)?);
                Some(bounded((q - p).cross(d).len(), *target, *bounds))
            }
        }
    }
}

/// One equation a constraint compiles to: its id (from `constraints::compile::compiled_id`),
/// the constraint's name, and the equation.
#[derive(Clone, Debug, PartialEq)]
pub struct Eq {
    pub id: u64,
    pub name: String,
    pub eq: Equation,
}

/// A member that closes a loop: a joint (by id) or an equation (by its place in
/// `Compiled::equations`).
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Ord, Hash)]
pub enum Closing {
    Joint(u64),
    Equation(usize),
}

impl std::cmp::Eq for Closing {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IDENTITY;

    fn plane(occurrence: u64, n: Vec3) -> Side {
        Side { occurrence, prim: Prim::Plane { p: Vec3::ZERO, n } }
    }

    fn world(_: u64) -> Option<Mat> {
        Some(IDENTITY)
    }

    #[test]
    fn residuals_measure_from_a_to_b() {
        let s30 = Vec3::new(0.5, -(0.75f64.sqrt()), 0.0);
        let k = Some(Side { occurrence: 1, prim: Prim::Line { p: Vec3::ZERO, d: Vec3::Z } });
        let thirty = 30f64.to_radians();
        let angle = |axis: Option<Side>, target: f64, bounds| Equation::Angle {
            a: plane(1, Vec3::new(0.0, -1.0, 0.0)),
            b: plane(2, s30),
            axis,
            target,
            bounds,
        };
        assert!(angle(None, thirty, None).residual(&world).is_some_and(|r| r.abs() < 1e-12));
        assert!(angle(k, thirty, None).residual(&world).is_some_and(|r| r.abs() < 1e-12));
        // −30 about z is 60° away; 330 is −30.
        assert!(angle(k, -thirty, None).residual(&world).is_some_and(|r| (r - 2.0 * thirty).abs() < 1e-12));
        assert!(angle(k, 330f64.to_radians(), None).residual(&world).is_some_and(|r| (r - 2.0 * thirty).abs() < 1e-12));
        // Bounds: 0 inside, the distance past the nearer one outside.
        assert_eq!(angle(None, 0.0, Some((0.2, 0.6))).residual(&world), Some(0.0));
        assert!(angle(None, 0.0, Some((0.6, 1.0))).residual(&world).is_some_and(|r| (r - (thirty - 0.6)).abs() < 1e-12));
        let line = |occurrence, p| Side { occurrence, prim: Prim::Cylinder { p, d: Vec3::Z, r: 1.0 } };
        let apart = Equation::AxisDistance { a: line(1, Vec3::ZERO), b: line(2, Vec3::new(3.0, 4.0, 9.0)), target: 2.0, bounds: None };
        assert!(apart.residual(&world).is_some_and(|r| (r - 3.0).abs() < 1e-12));
        assert_eq!(apart.occurrences(), vec![1, 2]);
        assert_eq!(angle(k, 0.0, None).occurrences(), vec![1, 2]);
    }
}
