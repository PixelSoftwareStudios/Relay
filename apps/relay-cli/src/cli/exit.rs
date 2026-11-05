//! Process exit codes, documented in `docs/cli.md`.

/// Unclassified failure (I/O, serialization, daemon crash).
pub const FAILURE: u8 = 1;
/// Invalid arguments or a refused unsafe request (clap also uses 2).
pub const USAGE: u8 = 2;
/// The engine is not running or not reachable on the endpoint.
pub const UNREACHABLE: u8 = 3;
/// The engine rejected the request.
pub const REJECTED: u8 = 4;
/// A peer, call or device target was not found or is ambiguous.
pub const TARGET: u8 = 5;
/// The engine did not answer in time.
pub const TIMEOUT: u8 = 6;
