//! User-facing notices raised when persistence recovered from a problem.
//!
//! Recovery never happens silently: callers must show these to the user.

use std::path::PathBuf;

/// Something the user needs to know about after loading persisted state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
	/// The preferences database was unreadable; the backup was used instead.
	/// Microphone selection was cleared and calls start receive-only.
	PreferencesRecoveredFromBackup,
	/// Neither preferences file nor backup was readable; defaults are in use
	/// (no microphone selected, receive-only). The bad file was kept at `preserved`.
	PreferencesReset { preserved: Option<PathBuf> },
	/// The trust store was unreadable; the backup was used. Entries written after
	/// the backup may be missing and need re-verification.
	TrustRecoveredFromBackup,
	/// The trust store and its backup were unreadable. All peers are unknown and
	/// must be re-verified. The bad file was kept at `preserved`.
	TrustStoreLost { preserved: Option<PathBuf> },
}
