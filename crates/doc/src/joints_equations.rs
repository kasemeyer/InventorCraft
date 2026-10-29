//! Loop equations: constraints with no joint form (spec §3.5), solved in the same loop groups as
//! closing joints. Defined here, where `constraints::compile` first carries them (R114); the
//! variants and their residuals arrive with the Angle and Tangent equations (Plan E-1 Task 6).

/// An equation's kind and its terms.
#[derive(Clone, Debug, PartialEq)]
pub enum Equation {}

/// One equation a constraint compiles to: its id (from `constraints::compile::compiled_id`),
/// the constraint's name, and the equation.
#[derive(Clone, Debug, PartialEq)]
pub struct Eq {
    pub id: u64,
    pub name: String,
    pub eq: Equation,
}
