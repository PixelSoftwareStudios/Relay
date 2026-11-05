//! Where received files go.

use std::path::Path;
use std::sync::Arc;

use relay_messages::transfer::FileSink;

/// Builds the sink for one incoming file in a directory the client chose.
pub type SinkFactory = Arc<dyn Fn(&Path) -> Result<Box<dyn FileSink + Send>, String> + Send + Sync>;

/// The real filesystem: temp file in the directory, fsync, verify, rename without replacing.
pub fn directory_sinks() -> SinkFactory {
	Arc::new(|dir| Ok(Box::new(relay_io::sink::DirSink::new(dir))))
}

/// Bounds on transcript memory. The defaults are the design's; tests shrink them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessagingLimits {
	/// Budget of one live session's transcript.
	pub session_bytes: u64,
	/// Budget of all ended transcripts together.
	pub retained_bytes: u64,
}

impl Default for MessagingLimits {
	fn default() -> Self {
		Self {
			session_bytes: relay_messages::limits::SESSION_BUDGET_BYTES,
			retained_bytes: relay_messages::limits::GLOBAL_RETAINED_BYTES,
		}
	}
}

/// What a daemon can tune about messaging beyond the stored preferences.
#[derive(Clone)]
pub struct MessagingOptions {
	/// Where received files are written.
	pub sinks: SinkFactory,
	/// Transcript budgets.
	pub limits: MessagingLimits,
}

impl Default for MessagingOptions {
	fn default() -> Self {
		Self {
			sinks: directory_sinks(),
			limits: MessagingLimits::default(),
		}
	}
}
