//! What the hotkey is doing, in terms a user can act on.

use std::fmt::{self, Display, Formatter};

use relay_platform::hotkey::{BackendStatus, PermissionKind};
use relay_platform::ptt::{BlockReason, PttState};
use serde::Serialize;

/// Honest state of the hotkey backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum HotkeyReport {
	/// Registered and delivering key edges.
	Active { backend: String, binding: String },
	/// Not started on purpose.
	Disabled(String),
	/// The OS needs a permission first.
	NeedsPermission {
		backend: String,
		binding: String,
		permission: &'static str,
		hint: String,
	},
	/// This session cannot deliver global keys.
	Unsupported { backend: String, reason: String },
	/// Starting failed (bad key, key taken, thread).
	Failed { backend: String, reason: String },
}

impl HotkeyReport {
	/// Translate a backend probe that did not say `Available`.
	pub fn from_status(backend: &str, binding: &str, status: &BackendStatus) -> Self {
		match status {
			BackendStatus::Available => Self::Active {
				backend: backend.to_owned(),
				binding: binding.to_owned(),
			},
			BackendStatus::NeedsPermission(kind) => Self::NeedsPermission {
				backend: backend.to_owned(),
				binding: binding.to_owned(),
				permission: permission_name(*kind),
				hint: permission_hint(*kind),
			},
			BackendStatus::Unsupported(reason) => Self::Unsupported {
				backend: backend.to_owned(),
				reason: reason.clone(),
			},
		}
	}

	/// True only when keys will be delivered.
	pub fn is_active(&self) -> bool {
		matches!(self, Self::Active { .. })
	}
}

fn permission_name(kind: PermissionKind) -> &'static str {
	match kind {
		PermissionKind::Accessibility => "Accessibility",
		PermissionKind::InputMonitoring => "Input Monitoring",
	}
}

fn permission_hint(kind: PermissionKind) -> String {
	format!(
		"grant {} to the terminal app that runs relay (System Settings, Privacy & Security), then start it again",
		permission_name(kind)
	)
}

impl Display for HotkeyReport {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		match self {
			Self::Active { backend, binding } => write!(f, "{binding} active via {backend}"),
			Self::Disabled(reason) => write!(f, "off ({reason})"),
			Self::NeedsPermission {
				permission, hint, ..
			} => write!(f, "needs {permission}: {hint}"),
			Self::Unsupported { reason, .. } => write!(f, "unsupported: {reason}"),
			Self::Failed { reason, .. } => write!(f, "failed: {reason}"),
		}
	}
}

/// Which call the push-to-talk key controls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum TargetStatus {
	/// No connected call.
	NoCall,
	/// Bound to this call (hex id).
	Call(String),
	/// Several calls are connected; the key will not guess.
	Ambiguous,
	/// The engine refused control of the call.
	Denied(String),
}

impl Display for TargetStatus {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		match self {
			Self::NoCall => f.write_str("no connected call"),
			Self::Call(id) => write!(f, "call {}", id.chars().take(8).collect::<String>()),
			Self::Ambiguous => f.write_str("several calls; end all but one"),
			Self::Denied(why) => write!(f, "control denied: {why}"),
		}
	}
}

/// Something the UI or `relay hotkey` should show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyUpdate {
	/// Backend state (sent once at start).
	Report(HotkeyReport),
	/// The driver's transmit state changed.
	Ptt(PttState),
	/// The controlled call changed.
	Target(TargetStatus),
}

/// Short label for a driver state.
pub fn ptt_label(state: &PttState) -> String {
	match state {
		PttState::Idle => "idle".into(),
		PttState::Transmitting => "transmitting".into(),
		PttState::Gated => "held, engine not transmitting (muted or no consent)".into(),
		PttState::Blocked(BlockReason::NoTarget) => "blocked: no controlled call".into(),
		PttState::Blocked(BlockReason::SessionLocked) => "blocked: session locked".into(),
		PttState::Blocked(BlockReason::NeedsRepress) => "blocked: release and press again".into(),
		PttState::Rejected { code } => format!("rejected by engine ({code})"),
		PttState::Expired => "lease expired".into(),
		PttState::Failed(why) => format!("failed: {why}"),
	}
}
