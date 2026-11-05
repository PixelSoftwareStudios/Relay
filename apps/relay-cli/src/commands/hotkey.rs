//! `relay hotkey [status|set]`.

use std::fmt::{self, Display, Formatter};

use relay_config::prefs::PreferenceStore;
use relay_config::{ConfigError, Paths, Storage};
use relay_platform::hotkey::detect_backend;
use relay_platform::session::{LockDetection, lock_detection};
use serde::Serialize;

use super::Out;
use super::config::Message;
use crate::cli::{CliError, GlobalArgs, HotkeyArgs, HotkeyCommand};
use crate::client;
use crate::hotkey::{
	HotkeyFeed, HotkeyReport, HotkeyUpdate, configured_binding, ptt_label, run_against_engine,
	validate_accelerator,
};

/// Backend, permissions and binding, without starting anything.
#[derive(Debug, Serialize)]
pub struct StatusView {
	pub binding: String,
	#[serde(flatten)]
	pub report: HotkeyReport,
	pub lock_detection: String,
	pub permissions: Vec<String>,
}

impl Display for StatusView {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		writeln!(f, "hotkey   {}", self.report)?;
		writeln!(
			f,
			"binding  {}",
			if self.binding.is_empty() {
				"(none)"
			} else {
				&self.binding
			}
		)?;
		write!(f, "lock     {}", self.lock_detection)?;
		for line in &self.permissions {
			write!(f, "\npermission  {line}")?;
		}
		Ok(())
	}
}

/// One line of the running hotkey's output.
#[derive(Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Line {
	Hotkey(HotkeyReport),
	Ptt { state: String },
	Call { target: String },
}

impl Display for Line {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		match self {
			Self::Hotkey(report) => write!(f, "hotkey: {report}"),
			Self::Ptt { state } => write!(f, "ptt: {state}"),
			Self::Call { target } => write!(f, "controlling: {target}"),
		}
	}
}

fn lock_text() -> String {
	match lock_detection() {
		LockDetection::Detected => "screen lock releases push-to-talk".into(),
		LockDetection::Unsupported(why) => why.to_owned(),
	}
}

#[cfg(target_os = "macos")]
fn permission_lines() -> Vec<String> {
	use relay_platform::hotkey::macos::{PermissionState, permissions};
	let p = permissions();
	let show = |s: PermissionState| match s {
		PermissionState::Granted => "granted",
		PermissionState::Denied => "not granted",
	};
	vec![
		format!("Accessibility: {}", show(p.accessibility)),
		format!("Input Monitoring: {}", show(p.input_monitoring)),
	]
}

#[cfg(not(target_os = "macos"))]
fn permission_lines() -> Vec<String> {
	Vec::new()
}

/// Probe the backend for the binding that would be used.
pub fn status_view(binding: &str) -> StatusView {
	let backend = detect_backend();
	let report = if binding.is_empty() {
		HotkeyReport::Disabled("no push-to-talk key is configured".into())
	} else {
		HotkeyReport::from_status(backend.name(), binding, &backend.probe())
	};
	StatusView {
		binding: binding.to_owned(),
		report,
		lock_detection: lock_text(),
		permissions: permission_lines(),
	}
}

pub async fn run(
	global: &GlobalArgs,
	args: &HotkeyArgs,
	feed: HotkeyFeed,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	match &args.command {
		Some(HotkeyCommand::Status) => {
			out.emit(&status_view(&configured_binding(args.binding.as_deref())))
		}
		Some(HotkeyCommand::Set { accelerator }) => {
			let paths = Paths::platform().map_err(|e| CliError::Other(e.into()))?;
			set(&paths, accelerator, out)
		}
		None => run_hotkey(global, feed, out).await,
	}
}

/// Save the accelerator. Needs the settings lock, which a running engine holds.
pub fn set(paths: &Paths, accelerator: &str, out: &mut Out<'_>) -> Result<(), CliError> {
	validate_accelerator(accelerator).map_err(CliError::Usage)?;
	let storage = Storage::open(paths).map_err(|e| match e {
		ConfigError::Locked(_) => CliError::Other(anyhow::anyhow!(
			"the engine holds the settings lock, so the key cannot be saved now; \
			 stop it and run this again, or use `relay hotkey --binding KEYS` for one run"
		)),
		other => CliError::Other(other.into()),
	})?;
	let store = PreferenceStore::open(&storage.database).map_err(|e| CliError::Other(e.into()))?;
	store
		.set("hotkeys.push_to_talk", accelerator)
		.map_err(|e| CliError::Other(e.into()))?;
	out.emit(&Message {
		message: format!("push-to-talk key saved: {accelerator}"),
	})
}

async fn run_hotkey(
	global: &GlobalArgs,
	feed: HotkeyFeed,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	out.emit(&Line::Hotkey(feed.report.clone()))?;
	let Some(keys) = feed.stream else {
		return Err(CliError::Other(anyhow::anyhow!(
			"the hotkey is not running: {}",
			feed.report
		)));
	};
	let mut engine = client::connect(global).await?;
	let stop = async {
		let _ = tokio::signal::ctrl_c().await;
	};
	// Output errors (a closed pipe) must not stop push-to-talk from releasing.
	run_against_engine(
		&mut engine,
		keys,
		|update| {
			let line = match update {
				HotkeyUpdate::Ptt(state) => Line::Ptt {
					state: ptt_label(&state),
				},
				HotkeyUpdate::Target(target) => Line::Call {
					target: target.to_string(),
				},
				HotkeyUpdate::Report(report) => Line::Hotkey(report),
			};
			let _ = out.emit(&line);
		},
		stop,
	)
	.await
}
