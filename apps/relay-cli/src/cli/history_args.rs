//! Arguments of `relay history`.

use clap::{Args, Subcommand};

/// Most entries `relay history` lists.
pub const MAX_LIMIT: u32 = 200;

/// `relay history [--limit N]`; `relay history clear` deletes the list.
#[derive(Debug, Clone, Args)]
pub struct HistoryArgs {
	/// How many of the newest calls to list (1 to 200).
	#[arg(long, default_value_t = 20, value_name = "N", value_parser = clap::value_parser!(u32).range(1..=200))]
	pub limit: u32,
	/// What to do instead of listing.
	#[command(subcommand)]
	pub command: Option<HistoryCommand>,
}

/// `relay history ...`
#[derive(Debug, Clone, Subcommand)]
pub enum HistoryCommand {
	/// Delete every entry of the call history on this device.
	Clear,
}
