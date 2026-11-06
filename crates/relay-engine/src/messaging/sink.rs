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

/// The storage limits the messaging actor enforces, from `limits.*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessagingLimits {
	/// Budget of one live session's transcript.
	pub session_bytes: u64,
	/// Budget of all ended transcripts together.
	pub retained_bytes: u64,
	/// File transfers running at once in one session.
	pub transfers: usize,
}

impl MessagingLimits {
	/// The limits stored in the preferences.
	pub fn from_prefs(limits: &relay_config::prefs::Limits) -> Self {
		Self {
			session_bytes: limits.session_bytes(),
			retained_bytes: limits.retained_bytes(),
			transfers: limits.transfer_concurrency as usize,
		}
	}
}

impl Default for MessagingLimits {
	fn default() -> Self {
		Self::from_prefs(&relay_config::prefs::Limits::default())
	}
}

/// What a daemon can tune about messaging beyond the stored preferences.
#[derive(Clone)]
pub struct MessagingOptions {
	/// Where received files are written.
	pub sinks: SinkFactory,
	/// Replaces the `limits.*` preferences (tests need budgets below the allowed minimum).
	pub limits: Option<MessagingLimits>,
}

impl Default for MessagingOptions {
	fn default() -> Self {
		Self {
			sinks: directory_sinks(),
			limits: None,
		}
	}
}
