//! `relay daemon`: start, stop and inspect the background engine.
//!
//! The engine is the separate `relayd` binary. The CLI launches it detached,
//! asks it to stop over the API, and otherwise only connects like any other
//! client. Nothing starts it implicitly.

use std::fmt;
use std::path::Path;
use std::process::Command as Process;
use std::time::Duration;

use relay_api::{Endpoint, RelayClient};
use relay_client::daemon::{self, Launch, Started, Stopped};
use serde::Serialize;

use super::Out;
use crate::cli::{CliError, DaemonAction, DaemonArgs, DaemonLaunch, GlobalArgs, endpoint};

const STOP_WAIT: Duration = Duration::from_secs(10);
const CLIENT_NAME: &str = "relay-cli";

/// One line of daemon state, as text or JSON.
#[derive(Debug, Serialize)]
struct Report {
	state: &'static str,
	#[serde(skip_serializing_if = "Option::is_none")]
	pid: Option<u32>,
	endpoint: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	log: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	engine_version: Option<String>,
}

impl Report {
	fn new(state: &'static str, endpoint: &Endpoint) -> Self {
		Self {
			state,
			pid: None,
			endpoint: endpoint::describe(endpoint),
			log: None,
			engine_version: None,
		}
	}
}

impl fmt::Display for Report {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "daemon {} ({})", self.state, self.endpoint)?;
		if let Some(pid) = self.pid {
			write!(f, ", pid {pid}")?;
		}
		if let Some(version) = &self.engine_version {
			write!(f, ", engine {version}")?;
		}
		if let Some(log) = &self.log {
			write!(f, ", log {log}")?;
		}
		Ok(())
	}
}

pub async fn run(
	global: &GlobalArgs,
	args: &DaemonArgs,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let endpoint = endpoint::resolve(global)?;
	match &args.action {
		DaemonAction::Start(launch) => start(&endpoint, launch, out).await,
		DaemonAction::Stop { wait } => stop(&endpoint, Duration::from_secs(*wait), out).await,
		DaemonAction::Restart(launch) => {
			stop(&endpoint, STOP_WAIT, out).await?;
			start(&endpoint, launch, out).await
		}
		DaemonAction::Status => status(&endpoint, out).await,
		DaemonAction::Logs { lines, file } => logs(*lines, file.as_deref(), out),
		DaemonAction::Run(launch) => foreground(&endpoint, launch),
	}
}

fn launch_for(endpoint: &Endpoint, options: &DaemonLaunch) -> Launch {
	let mut launch = Launch::new(endpoint.clone());
	launch.binary.clone_from(&options.binary);
	launch.log_file.clone_from(&options.log_file);
	launch.timeout = Duration::from_secs(options.wait);
	launch.extra_args = options.relayd_args.iter().map(Into::into).collect();
	launch
}

async fn start(
	endpoint: &Endpoint,
	options: &DaemonLaunch,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	match daemon::start(&launch_for(endpoint, options)).await {
		Started::Launched { pid, log, .. } => {
			let mut report = Report::new("started", endpoint);
			report.pid = Some(pid);
			report.log = Some(log.display().to_string());
			out.emit(&report)
		}
		Started::AlreadyRunning => out.emit(&Report::new("already running", endpoint)),
		Started::BinaryNotFound { searched } => {
			let places: Vec<String> = searched.iter().map(|p| p.display().to_string()).collect();
			Err(CliError::Other(anyhow::anyhow!(
				"cannot find {}; looked in: {}. Install it next to `relay`, put it on PATH or set {}",
				daemon::binary_name(),
				places.join(", "),
				daemon::BINARY_ENV,
			)))
		}
		Started::FailedToStart { reason, log_tail } => {
			let tail = if log_tail.is_empty() {
				String::new()
			} else {
				format!("\n--- end of the daemon log ---\n{log_tail}")
			};
			Err(CliError::Other(anyhow::anyhow!(
				"the daemon did not start: {reason}{tail}"
			)))
		}
	}
}

async fn stop(endpoint: &Endpoint, wait: Duration, out: &mut Out<'_>) -> Result<(), CliError> {
	match daemon::stop(endpoint, CLIENT_NAME, wait).await? {
		Stopped::Stopped => out.emit(&Report::new("stopped", endpoint)),
		Stopped::NotRunning => out.emit(&Report::new("not running", endpoint)),
	}
}

async fn status(endpoint: &Endpoint, out: &mut Out<'_>) -> Result<(), CliError> {
	if !daemon::reachable(endpoint).await {
		return Err(CliError::NotRunning {
			endpoint: endpoint::describe(endpoint),
		});
	}
	let client = RelayClient::connect(endpoint).await?;
	let mut report = Report::new("running", endpoint);
	report.engine_version = Some(client.info().application_version.clone());
	out.emit(&report)
}

fn logs(lines: usize, file: Option<&Path>, out: &mut Out<'_>) -> Result<(), CliError> {
	let path = file.map_or_else(daemon::default_log_path, Path::to_path_buf);
	let text = daemon::log_tail(&path, 0);
	if text.is_empty() {
		return out.note(&format!("no daemon log yet at {}", path.display()));
	}
	let all: Vec<&str> = text.lines().collect();
	for line in &all[all.len().saturating_sub(lines)..] {
		out.line(line)?;
	}
	Ok(())
}

/// Runs `relayd` as a child of this terminal and returns its exit status.
fn foreground(endpoint: &Endpoint, options: &DaemonLaunch) -> Result<(), CliError> {
	let binary = daemon::locate(options.binary.as_deref()).map_err(|searched| {
		CliError::Other(anyhow::anyhow!(
			"cannot find {} (looked in {} places); set {}",
			daemon::binary_name(),
			searched.len(),
			daemon::BINARY_ENV
		))
	})?;
	let status = Process::new(binary)
		.args(daemon::endpoint_args(endpoint))
		.args(&options.relayd_args)
		.status()?;
	if status.success() {
		Ok(())
	} else {
		Err(CliError::Other(anyhow::anyhow!("relayd stopped: {status}")))
	}
}
