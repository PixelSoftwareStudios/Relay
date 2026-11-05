//! `relay completions <shell>` and `relay man [--dir DIR]`. Neither connects to the daemon.

use clap::CommandFactory;
use clap_mangen::Man;

use super::Out;
use crate::cli::{Cli, CliError, CompletionsArgs, ManArgs};

/// The binary name the scripts and pages are generated for.
const BIN: &str = "relay";

/// A completion script for the chosen shell, on stdout.
pub fn completions(args: &CompletionsArgs, out: &mut Out<'_>) -> Result<(), CliError> {
	let mut script = Vec::new();
	clap_complete::generate(args.shell, &mut Cli::command(), BIN, &mut script);
	out.line(String::from_utf8_lossy(&script).trim_end())
}

/// The main page on stdout, or every page into a directory.
pub fn man(args: &ManArgs, out: &mut Out<'_>) -> Result<(), CliError> {
	let command = Cli::command().name(BIN);
	match &args.dir {
		None => {
			let mut page = Vec::new();
			Man::new(command).render(&mut page)?;
			out.line(String::from_utf8_lossy(&page).trim_end())
		}
		Some(dir) => {
			std::fs::create_dir_all(dir)?;
			clap_mangen::generate_to(command, dir)?;
			out.line(&format!("wrote man pages to {}", dir.display()))
		}
	}
}
