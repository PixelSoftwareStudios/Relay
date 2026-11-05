//! Secret text (channel passwords, invite tokens) and where it is read from.
//!
//! Secrets never come from argv. They are read from a hidden terminal prompt or
//! from one line of stdin, held in `relay_client::Secret` (redacted `Debug`, wiped
//! on drop).

use std::io::BufRead;

pub use relay_client::Secret;
use zeroize::Zeroize;

use crate::cli::CliError;

/// How the user supplies a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretInput {
	/// Hidden prompt on the terminal, with this label.
	Prompt(&'static str),
	/// One line from stdin.
	Stdin,
}

/// Where secrets come from. Tests supply a scripted source.
pub trait SecretSource {
	/// Read one non-empty secret.
	fn read(&mut self, input: SecretInput) -> Result<Secret, CliError>;
}

/// Terminal prompt (no echo) and stdin.
#[derive(Debug, Default)]
pub struct SystemSecrets;

impl SecretSource for SystemSecrets {
	fn read(&mut self, input: SecretInput) -> Result<Secret, CliError> {
		match input {
			SecretInput::Prompt(label) => {
				let text = rpassword::prompt_password(format!("{label}: ")).map_err(|e| {
					CliError::Usage(format!(
						"cannot prompt for input ({e}); pass the secret on stdin with the --*-stdin flag"
					))
				})?;
				non_empty(text)
			}
			SecretInput::Stdin => read_line(&mut std::io::stdin().lock()),
		}
	}
}

/// Read one line, strip the line ending, and refuse an empty secret.
pub fn read_line(reader: &mut dyn BufRead) -> Result<Secret, CliError> {
	let mut line = String::new();
	reader.read_line(&mut line)?;
	while line.ends_with(['\n', '\r']) {
		line.pop();
	}
	non_empty(line)
}

fn non_empty(mut text: String) -> Result<Secret, CliError> {
	if text.is_empty() {
		text.zeroize();
		return Err(CliError::Usage("no secret was entered".into()));
	}
	Ok(Secret::new(text))
}
