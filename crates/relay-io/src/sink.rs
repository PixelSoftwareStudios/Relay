//! The real destination for received files (`relay_messages::transfer::FileSink`).
//!
//! The file is written to an exclusively created, unpredictable temp name inside the
//! destination directory (same filesystem, so the final step is a rename), fsynced,
//! then committed under its sanitized name without ever replacing an existing file:
//! a taken name gets ` (1)`, ` (2)`... before the extension. See
//! [`crate::fs::rename_noreplace`] for the race notes. The temp file is deleted if
//! the transfer is aborted or the sink is dropped uncommitted.

use std::io::Write;
use std::path::{Path, PathBuf};

use relay_messages::SafeFileName;
use relay_messages::transfer::{FileSink, SinkError};

use crate::fs::{TempFile, free_space, rename_noreplace};

const MAX_ATTEMPTS: u32 = 1000;

/// Receives one file at a time into a directory.
#[derive(Debug)]
pub struct DirSink {
	dir: PathBuf,
	current: Option<(TempFile, String)>,
}

impl DirSink {
	/// A sink writing into `dir` (which must exist).
	pub fn new(dir: impl Into<PathBuf>) -> Self {
		Self {
			dir: dir.into(),
			current: None,
		}
	}

	/// The destination directory.
	pub fn dir(&self) -> &Path {
		&self.dir
	}
}

fn err(what: &str, e: &std::io::Error) -> SinkError {
	SinkError(format!("{what}: {e}"))
}

/// `name`, then `stem (n).ext` for n = 1.. .
fn candidate(name: &str, attempt: u32) -> String {
	if attempt == 0 {
		return name.to_owned();
	}
	match name.rfind('.').filter(|&i| i > 0) {
		Some(dot) => format!("{} ({attempt}){}", &name[..dot], &name[dot..]),
		None => format!("{name} ({attempt})"),
	}
}

impl FileSink for DirSink {
	fn free_space(&mut self) -> Result<u64, SinkError> {
		free_space(&self.dir).map_err(|e| err("free space", &e))
	}

	fn create_temp(&mut self, name: &SafeFileName, _size: u64) -> Result<(), SinkError> {
		self.abort();
		let temp = TempFile::create_in(&self.dir, "relay-recv-")
			.map_err(|e| SinkError(format!("create temp: {e}")))?;
		self.current = Some((temp, name.as_str().to_owned()));
		Ok(())
	}

	fn write(&mut self, data: &[u8]) -> Result<(), SinkError> {
		let file = self
			.current
			.as_mut()
			.and_then(|(temp, _)| temp.file())
			.ok_or_else(|| SinkError("no open transfer".to_owned()))?;
		file.write_all(data).map_err(|e| err("write", &e))
	}

	fn sync(&mut self) -> Result<(), SinkError> {
		let file = self
			.current
			.as_mut()
			.and_then(|(temp, _)| temp.file())
			.ok_or_else(|| SinkError("no open transfer".to_owned()))?;
		file.sync_all().map_err(|e| err("sync", &e))
	}

	fn commit(&mut self) -> Result<String, SinkError> {
		let (mut temp, name) = self
			.current
			.take()
			.ok_or_else(|| SinkError("no open transfer".to_owned()))?;
		temp.close();
		for attempt in 0..MAX_ATTEMPTS {
			let final_name = candidate(&name, attempt);
			match rename_noreplace(temp.path(), &self.dir.join(&final_name)) {
				Ok(()) => {
					temp.keep();
					crate::fs::sync_dir(&self.dir);
					return Ok(final_name);
				}
				Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
				Err(e) => {
					// Dropping `temp` deletes the partial file.
					return Err(err("commit", &e));
				}
			}
		}
		Err(SinkError("no free file name".to_owned()))
	}

	fn abort(&mut self) {
		// Dropping the temp file removes it; idempotent.
		self.current = None;
	}
}
