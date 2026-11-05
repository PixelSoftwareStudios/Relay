//! Output sink that prints either text or JSON.

use std::fmt::Display;
use std::io::Write;

use serde::Serialize;

use crate::cli::CliError;

/// Where a command writes its result.
pub struct Out<'a> {
	json: bool,
	writer: &'a mut dyn Write,
}

impl<'a> Out<'a> {
	/// Wrap a writer; `json` selects the format.
	pub fn new(json: bool, writer: &'a mut dyn Write) -> Self {
		Self { json, writer }
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
