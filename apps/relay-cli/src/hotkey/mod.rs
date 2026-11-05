//! Global push-to-talk for the CLI: start the platform backend, report its
//! state honestly and supervise a `PttDriver` against the running engine.
//!
//! The backend must be started on the process main thread (macOS), so
//! [`start`] is synchronous and called from `main` before tokio takes over;
//! everything after that is async and runs on worker threads.

mod binding;
mod report;
mod session;

use relay_platform::hotkey::{HotkeyBackend, HotkeyBinding, HotkeyError, HotkeyStream};

pub use binding::{configured_binding, validate_accelerator};
pub use report::{HotkeyReport, HotkeyUpdate, TargetStatus, ptt_label};
pub use session::{run_against_engine, supervise};

/// What a started (or refused) backend hands to the async side.
pub struct HotkeyFeed {
	/// Honest state of the backend.
	pub report: HotkeyReport,
	/// Key edges, present only when the backend is active.
	pub stream: Option<HotkeyStream>,
}

impl HotkeyFeed {
	/// A feed for commands that run without any hotkey backend.
	pub fn off(reason: &str) -> Self {
		Self {
			report: HotkeyReport::Disabled(reason.to_owned()),
			stream: None,
		}
	}
}

/// Probe `backend` and register `accelerator`. An empty accelerator disables.
/// Call on the main thread; keep the backend alive until the program ends.
pub fn start_with(backend: &mut dyn HotkeyBackend, accelerator: &str) -> HotkeyFeed {
	if accelerator.trim().is_empty() {
		return HotkeyFeed::off("no push-to-talk key is configured");
	}
	let name = backend.name();
	let status = backend.probe();
	if !status.is_available() {
		return HotkeyFeed {
			report: HotkeyReport::from_status(name, accelerator, &status),
			stream: None,
		};
	}
	match backend.start(&HotkeyBinding::push_to_talk(accelerator)) {
		Ok(stream) => HotkeyFeed {
			report: HotkeyReport::Active {
				backend: name.to_owned(),
				binding: accelerator.to_owned(),
			},
			stream: Some(stream),
		},
		Err(err) => HotkeyFeed {
			report: HotkeyReport::Failed {
				backend: name.to_owned(),
				reason: error_text(&err),
			},
			stream: None,
		},
	}
}

fn error_text(err: &HotkeyError) -> String {
	match err {
		HotkeyError::Unavailable(status) => format!("{status:?}"),
		other => other.to_string(),
	}
}
