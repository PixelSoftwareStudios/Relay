//! Command line definitions, errors, exit codes and endpoint resolution.

mod args;
pub mod endpoint;
mod error;
pub mod exit;

pub use args::*;
pub use error::{CliError, TargetError};
