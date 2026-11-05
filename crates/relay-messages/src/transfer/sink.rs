//! Destination port supplied by the engine (real filesystem) or by tests (memory).

/// A destination failure (message for diagnostics, never shown with file contents).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SinkError(pub String);

/// One incoming file's destination.
pub trait FileSink {
	/// Free bytes in the destination directory.
	fn free_space(&mut self) -> Result<u64, SinkError>;
	/// Exclusively creates a temporary file in the destination directory (never an existing path).
	fn create_temp(&mut self, name: &crate::SafeFileName, size: u64) -> Result<(), SinkError>;
	/// Appends bytes to the temporary file.
	fn write(&mut self, data: &[u8]) -> Result<(), SinkError>;
	/// fsync the temporary file.
	fn sync(&mut self) -> Result<(), SinkError>;
	/// Atomically renames to the final name without overwriting (picks a unique name if
	/// taken) and returns that name.
	fn commit(&mut self) -> Result<String, SinkError>;
	/// Deletes the temporary file; idempotent.
	fn abort(&mut self);
}
