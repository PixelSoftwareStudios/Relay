//! Output sink that prints either text or JSON.

use std::fmt::Display;
use std::io::{BufRead, Read, Write};

use relay_io::localtime::LocalZone;
use serde::Serialize;

use crate::cli::CliError;
use crate::secret::{Secret, SecretInput, SecretSource, SystemSecrets};

use super::clipboard::{Clipboard, SystemClipboard};

/// Where a command writes its result.
pub struct Out<'a> {
	json: bool,
	writer: &'a mut dyn Write,
	secrets: Option<&'a mut dyn SecretSource>,
	clipboard: Option<&'a mut dyn Clipboard>,
	input: Option<&'a mut dyn BufRead>,
	zone: LocalZone,
}

impl<'a> Out<'a> {
	/// Wrap a writer; `json` selects the format.
	pub fn new(json: bool, writer: &'a mut dyn Write) -> Self {
		Self {
			json,
			writer,
			secrets: None,
			clipboard: None,
			input: None,
			zone: LocalZone::system(),
		}
	}

	/// Show message times in `zone` instead of the system zone.
	pub fn with_zone(mut self, zone: LocalZone) -> Self {
		self.zone = zone;
		self
	}

	/// The zone message times are shown in.
	pub fn zone(&self) -> &LocalZone {
		&self.zone
	}

	/// Read secrets from `source` instead of the terminal and stdin.
	pub fn with_secrets(mut self, source: &'a mut dyn SecretSource) -> Self {
		self.secrets = Some(source);
		self
	}

	/// Copy to `clipboard` instead of the system clipboard.
	pub fn with_clipboard(mut self, clipboard: &'a mut dyn Clipboard) -> Self {
		self.clipboard = Some(clipboard);
		self
	}

	/// Read stdin-like input from `input` instead of the process's stdin.
	pub fn with_input(mut self, input: &'a mut dyn BufRead) -> Self {
		self.input = Some(input);
		self
	}

	/// Read all of stdin as text, failing when it exceeds `max_bytes`.
	pub fn read_input(&mut self, max_bytes: usize) -> Result<String, CliError> {
		let mut bytes = Vec::new();
		let limit = u64::try_from(max_bytes)
			.unwrap_or(u64::MAX)
			.saturating_add(1);
		match self.input.as_deref_mut() {
			Some(input) => input.take(limit).read_to_end(&mut bytes)?,
			None => std::io::stdin()
				.lock()
				.take(limit)
				.read_to_end(&mut bytes)?,
		};
		if bytes.len() > max_bytes {
			return Err(CliError::Usage(format!(
				"the input is longer than {max_bytes} bytes"
			)));
		}
		String::from_utf8(bytes).map_err(|_| CliError::Usage("the input is not valid UTF-8".into()))
	}

	/// Ask a yes/no question on stderr and read one line; anything but `y`/`yes` is no.
	pub fn confirm(&mut self, question: &str) -> Result<bool, CliError> {
		eprint!("{question} [y/N] ");
		let mut line = String::new();
		match self.input.as_deref_mut() {
			Some(input) => input.read_line(&mut line)?,
			None => std::io::stdin().lock().read_line(&mut line)?,
		};
		let answer = line.trim().to_ascii_lowercase();
		Ok(answer == "y" || answer == "yes")
	}

	/// Read one secret (hidden prompt or stdin). Never echoed or logged.
	pub fn secret(&mut self, input: SecretInput) -> Result<Secret, CliError> {
		match self.secrets.as_deref_mut() {
			Some(source) => source.read(input),
			None => SystemSecrets.read(input),
		}
	}

	/// Copy text to the clipboard.
	pub fn copy(&mut self, text: &str) -> Result<(), String> {
		match self.clipboard.as_deref_mut() {
			Some(clipboard) => clipboard.copy(text),
			None => SystemClipboard.copy(text),
		}
	}

	/// Write a line to stdout-equivalent regardless of format (raw payloads).
	pub fn line(&mut self, text: &str) -> Result<(), CliError> {
		writeln!(self.writer, "{text}")?;
		Ok(())
	}

	/// Whether JSON was requested.
	pub fn is_json(&self) -> bool {
		self.json
	}

	/// Print a value as one JSON line, or as its `Display` text.
	pub fn emit<T: Serialize + Display>(&mut self, value: &T) -> Result<(), CliError> {
		if self.json {
			serde_json::to_writer(&mut *self.writer, value)?;
			writeln!(self.writer)?;
		} else {
			writeln!(self.writer, "{value}")?;
		}
		Ok(())
	}

	/// Print `value` as one JSON line, or the text `text` builds (not built for JSON).
	pub fn emit_with<T: Serialize>(
		&mut self,
		value: &T,
		text: impl FnOnce() -> String,
	) -> Result<(), CliError> {
		if self.json {
			serde_json::to_writer(&mut *self.writer, value)?;
			writeln!(self.writer)?;
		} else {
			writeln!(self.writer, "{}", text())?;
		}
		Ok(())
	}

	/// Print a plain line in text mode only (progress hints).
	pub fn note(&mut self, line: &str) -> Result<(), CliError> {
		if !self.json {
			writeln!(self.writer, "{line}")?;
		}
		Ok(())
	}
}
