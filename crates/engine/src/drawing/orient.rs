//! Projected views (spec §2.4): which view a click beside a parent makes (third angle, ANSI), the
//! orientation it shows and where it sits so that it stays aligned with its parent.

use solidcraft_doc::{CubeTarget, Orientation};
use solidcraft_geom::Vec3;

/// The view projected from a parent: one of the four orthographic neighbours, or an isometric
/// toward the corner `sx` (right +1, left −1) and `sy` (up +1, down −1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Projection {
    Top,
    Bottom,
    Left,
    Right,
    Iso { sx: i8, sy: i8 },
}

/// The projection a click at `at` makes from a parent centred at `parent`: 45-degree sectors centred on the axes.
pub fn projection_at(parent: [f64; 2], at: [f64; 2]) -> Projection {
    let (dx, dy) = (at[0] - parent[0], at[1] - parent[1]);
    let angle = dy.atan2(dx).to_degrees();
    let sign = |v: f64| if v < 0.0 { -1 } else { 1 };
    match angle {
        a if (-22.5..=22.5).contains(&a) => Projection::Right,
        a if (67.5..=112.5).contains(&a) => Projection::Top,
        a if (-112.5..=-67.5).contains(&a) => Projection::Bottom,
        a if a >= 157.5 || a <= -157.5 => Projection::Left,
        _ => Projection::Iso { sx: sign(dx), sy: sign(dy) },
    }
}

/// The orientation of projection `p` of a parent oriented `parent` (third angle): with the
/// parent's `d` (dir), `u` (up) and `r = d × u`, Top looks along −u with d up, Bottom along u with
/// −d up, Right along −r, Left along r (both with u up); an isometric looks along
/// unit(d − sx·r − sy·u) with u made perpendicular to it as up. `preset` is the cube target the
/// result matches (dir and up within 1e-9, R6).
pub fn projected_orientation(parent: &Orientation, p: Projection) -> Orientation {
    let (d, u) = (parent.dir, parent.up);
    let r = d.cross(u);
    let (dir, up) = match p {
        Projection::Top => (-u, d),
        Projection::Bottom => (u, -d),
        Projection::Right => (-r, u),
        Projection::Left => (r, u),
        Projection::Iso { sx, sy } => {
            let dir = (d - r * f64::from(sx) - u * f64::from(sy)).normalized().unwrap_or(d);
            let up = (u - dir * u.dot(dir)).normalized().unwrap_or(u);
            (dir, up)
        }
    };
    let mut o = Orientation { dir, up, preset: None };
    o.preset = CubeTarget::matching(&o);
    // A preset's own vectors, so a projected view's orientation is the named one exactly.
    if let Some(t) = o.preset {
        o = t.orientation();
    }
    o
}

/// `at` moved onto `p`'s alignment axis through `parent` (Iso: unchanged).
pub fn aligned_position(parent: [f64; 2], p: Projection, at: [f64; 2]) -> [f64; 2] {
    match p {
        Projection::Top | Projection::Bottom => [parent[0], at[1]],
        Projection::Left | Projection::Right => [at[0], parent[1]],
        Projection::Iso { .. } => at,
    }
}

/// `dir` and `up` as given (any length) as an orientation: `dir` made unit, `up` made
/// perpendicular to it and unit. Err: the message for a zero `dir` or an `up` along it.
pub(crate) fn orientation_from(dir: Vec3, up: Vec3) -> Result<Orientation, &'static str> {
    let dir = dir.normalized().ok_or("dir must be a non-zero vector")?;
    let along = up.normalized().ok_or("up must not be parallel to dir")?;
    if along.cross(dir).len() <= 1e-9 {
        return Err("up must not be parallel to dir");
    }
    let up = (along - dir * along.dot(dir)).normalized().ok_or("up must not be parallel to dir")?;
    let mut o = Orientation { dir, up, preset: None };
    o.preset = CubeTarget::matching(&o);
    Ok(o)
}
