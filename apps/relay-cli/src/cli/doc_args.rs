//! Arguments of `relay completions` and `relay man`.

use std::path::PathBuf;

use clap::Args;
use clap_complete::Shell;

/// Arguments of `relay completions`.
#[derive(Debug, Clone, Args)]
pub struct CompletionsArgs {
	/// Shell to generate the completion script for.
	pub shell: Shell,
}

/// Arguments of `relay man`.
#[derive(Debug, Clone, Args)]
pub struct ManArgs {
	/// Write `relay.1` and one page per subcommand into this directory instead of printing
	/// the main page.
	#[arg(long, value_name = "DIR")]
	pub dir: Option<PathBuf>,
}
