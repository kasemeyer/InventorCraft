//! Assembly constraint commands (`constraint.*`, Plan E-1). The commands arrive in later tasks;
//! this module holds the (empty) table and the tests of pick resolution and compilation, which
//! need a `Session` to build geometry.

use super::CommandSpec;

pub static COMMANDS: &[CommandSpec] = &[];

#[cfg(test)]
#[path = "constraints_geo_tests.rs"]
mod constraints_geo_tests;

#[cfg(test)]
#[path = "constraints_compile_tests.rs"]
mod constraints_compile_tests;
