//! Exposes repository subsystems and the deliberate desktop host entrypoint.

mod diagnostic_operation;
pub mod application;
pub mod browsing;
pub mod diagnostics;
pub mod diff;
pub mod git;
pub mod history;
mod host;
mod native_work;
pub mod inspection;
pub mod observation;
pub mod workspace;

pub use host::run;

#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod test_support;
