//! Output sink that prints either text or JSON.

use std::fmt::Display;
use std::io::Write;

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
}

impl<'a> Out<'a> {
	/// Wrap a writer; `json` selects the format.
	pub fn new(json: bool, writer: &'a mut dyn Write) -> Self {
		Self {
			json,
			writer,
			secrets: None,
			clipboard: None,
		}
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

	/// Print a plain line in text mode only (progress hints).
	pub fn note(&mut self, line: &str) -> Result<(), CliError> {
		if !self.json {
			writeln!(self.writer, "{line}")?;
		}
		Ok(())
	}
}
