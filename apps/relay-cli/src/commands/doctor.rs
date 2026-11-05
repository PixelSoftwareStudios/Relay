//! `relay doctor`: environment checks and an optional redacted support bundle.

use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::path::Path;

use relay_config::Paths;
use relay_config::diagnostics::{BundleInput, build_bundle};
use relay_config::prefs::{PreferenceStore, Preferences};
use serde::Serialize;

use super::Out;
use crate::cli::{CliError, DoctorArgs, GlobalArgs, endpoint};
use crate::client::{self, Control};

/// One line of the report.
#[derive(Debug, Serialize)]
pub struct Check {
	pub name: &'static str,
	pub ok: bool,
	pub detail: String,
}

/// All checks, in order.
#[derive(Debug, Serialize)]
pub struct Report {
	pub checks: Vec<Check>,
	pub bundle: Option<String>,
}

impl Report {
	/// True when every check passed.
	pub fn healthy(&self) -> bool {
		self.checks.iter().all(|c| c.ok)
	}
}

impl Display for Report {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		for (i, c) in self.checks.iter().enumerate() {
			if i > 0 {
				writeln!(f)?;
			}
			write!(
				f,
				"[{}] {}: {}",
				if c.ok { " ok " } else { "FAIL" },
				c.name,
				c.detail
			)?;
		}
		if let Some(path) = &self.bundle {
			write!(f, "\nsupport bundle written to {path}")?;
		}
		Ok(())
	}
}

pub async fn run(
	global: &GlobalArgs,
	args: &DoctorArgs,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let mut checks = Vec::new();
	let mut counters = BTreeMap::new();
	let target = endpoint::resolve(global)?;
	checks.push(Check {
		name: "endpoint",
		ok: true,
		detail: endpoint::describe(&target),
	});

	match client::connect_to(&target, global).await {
		Ok(mut engine) => {
			let info = engine.info().clone();
			checks.push(Check {
				name: "engine",
				ok: true,
				detail: format!(
					"version {} api {}.{}",
					info.application_version, info.api_major, info.api_minor
				),
			});
			match engine.snapshot().await {
				Ok(snap) => {
					counters.insert("peers".to_owned(), snap.peers.len() as u64);
					counters.insert("calls".to_owned(), snap.calls.len() as u64);
					let has = |dir| {
						snap.devices
							.iter()
							.any(|d| d.available && d.direction == dir)
					};
					checks.push(Check {
						name: "input device",
						ok: has(1),
						detail: if has(1) {
							"available".into()
						} else {
							"none available".into()
						},
					});
					checks.push(Check {
						name: "output device",
						ok: has(2),
						detail: if has(2) {
							"available".into()
						} else {
							"none available".into()
						},
					});
					for w in &snap.warnings {
						checks.push(Check {
							name: "engine warning",
							ok: false,
							detail: format!("{}: {}", w.code, w.message),
						});
					}
				}
				Err(e) => checks.push(Check {
					name: "snapshot",
					ok: false,
					detail: e.to_string(),
				}),
			}
		}
		Err(e) => checks.push(Check {
			name: "engine",
			ok: false,
			detail: e.to_string(),
		}),
	}

	let prefs = load_preferences(&mut checks);
	let bundle = match &args.bundle {
		Some(path) => {
			write_bundle(path, &prefs, &counters)?;
			Some(path.display().to_string())
		}
		None => None,
	};
	let report = Report { checks, bundle };
	let healthy = report.healthy();
	out.emit(&report)?;
	if healthy {
		Ok(())
	} else {
		Err(CliError::Other(anyhow::anyhow!(
			"one or more checks failed"
		)))
	}
}

/// Read preferences without taking the writer lock (the engine may hold it).
fn load_preferences(checks: &mut Vec<Check>) -> Preferences {
	let paths = match Paths::platform() {
		Ok(paths) => paths,
		Err(e) => {
			checks.push(Check {
				name: "config",
				ok: false,
				detail: e.to_string(),
			});
			return Preferences::default();
		}
	};
	match PreferenceStore::read_only(&paths) {
		Ok(Some(prefs)) => {
			checks.push(Check {
				name: "preferences",
				ok: true,
				detail: "readable".into(),
			});
			prefs
		}
		Ok(None) => {
			checks.push(Check {
				name: "preferences",
				ok: true,
				detail: "not created yet (defaults)".into(),
			});
			Preferences::default()
		}
		Err(e) => {
			checks.push(Check {
				name: "preferences",
				ok: false,
				detail: format!("unreadable: {e}"),
			});
			Preferences::default()
		}
	}
}

fn write_bundle(
	path: &Path,
	prefs: &Preferences,
	counters: &BTreeMap<String, u64>,
) -> Result<(), CliError> {
	let text = build_bundle(&BundleInput {
		app_version: env!("CARGO_PKG_VERSION"),
		os: std::env::consts::OS,
		preferences: prefs,
		notices: &[],
		counters,
		log_lines: &[],
	});
	write_private(path, text.as_bytes())?;
	Ok(())
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
	use std::io::Write;
	use std::os::unix::fs::OpenOptionsExt;
	let mut file = std::fs::OpenOptions::new()
		.write(true)
		.create(true)
		.truncate(true)
		.mode(0o600)
		.open(path)?;
	file.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
	std::fs::write(path, bytes)
}
