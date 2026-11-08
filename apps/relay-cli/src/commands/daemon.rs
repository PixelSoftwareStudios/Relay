//! `relay daemon`: start, stop and inspect the background engine.
//!
//! The engine runs as its own process: this executable started with the hidden daemon argument
//! (see `relay_daemon`). The CLI launches it detached, asks it to stop over the API, and
//! otherwise only connects like any other client.

use std::fmt;
use std::path::Path;
use std::process::Command as Process;
use std::time::Duration;

use relay_api::pb::StartupState;
use relay_api::{Endpoint, RelayClient, Revision};
use relay_client::daemon::{self, Launch, Started, Stopped};
use relay_io::proc::RUN_DAEMON_ARG;
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
	/// What a daemon that is not ready waits for or failed on, in plain words.
	#[serde(skip_serializing_if = "Option::is_none")]
	waiting: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	engine_version: Option<String>,
	/// The running daemon's build (git commit) and API revision.
	#[serde(skip_serializing_if = "Option::is_none")]
	build_revision: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	api_revision: Option<u32>,
	/// This CLI's own build, set only when the daemon's differs.
	#[serde(skip_serializing_if = "Option::is_none")]
	out_of_date: Option<String>,
}

impl Report {
	fn new(state: &'static str, endpoint: &Endpoint) -> Self {
		Self {
			state,
			pid: None,
			endpoint: endpoint::describe(endpoint),
			log: None,
			waiting: None,
			engine_version: None,
			build_revision: None,
			api_revision: None,
			out_of_date: None,
		}
	}
}

impl fmt::Display for Report {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "daemon {} ({})", self.state, self.endpoint)?;
		if let Some(pid) = self.pid {
			write!(f, ", pid {pid}")?;
		}
		if let Some(waiting) = &self.waiting {
			write!(f, ": {waiting}")?;
		}
		if let Some(version) = &self.engine_version {
			write!(f, ", engine {version}")?;
		}
		if let (Some(build), Some(api)) = (&self.build_revision, self.api_revision) {
			write!(f, ", build {build} (api {api})")?;
		}
		if let Some(own) = &self.out_of_date {
			write!(
				f,
				"\nwarning: the daemon is out of date; this CLI is {own}. Run `relay daemon restart`."
			)?;
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
		DaemonAction::Start(launch) => start(global, &endpoint, launch, out).await,
		DaemonAction::Stop { wait } => stop(&endpoint, Duration::from_secs(*wait), out).await,
		DaemonAction::Restart { force, launch } => {
			if !force && calls_in_progress(&endpoint).await? {
				return Err(CliError::Usage(
					"a call is in progress and restarting the daemon would end it; end the call or pass --force"
						.into(),
				));
			}
			stop(&endpoint, STOP_WAIT, out).await?;
			start(global, &endpoint, launch, out).await
		}
		DaemonAction::Status => status(&endpoint, out).await,
		DaemonAction::Retry => retry(&endpoint, out).await,
		DaemonAction::UseFileStore { yes } => use_file_store(&endpoint, *yes, out).await,
		DaemonAction::Logs { lines, file } => logs(*lines, file.as_deref(), out),
		DaemonAction::Run(launch) => foreground(global, &endpoint, launch),
	}
}

fn launch_for(global: &GlobalArgs, endpoint: &Endpoint, options: &DaemonLaunch) -> Launch {
	let mut launch = Launch::new(endpoint.clone());
	launch.binary.clone_from(&global.daemon_bin);
	launch.log_file.clone_from(&options.log_file);
	launch.timeout = Duration::from_secs(options.wait);
	launch.extra_args = options.relayd_args.iter().map(Into::into).collect();
	launch
}

async fn start(
	global: &GlobalArgs,
	endpoint: &Endpoint,
	options: &DaemonLaunch,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	match daemon::start(&launch_for(global, endpoint, options)).await {
		Started::Launched { pid, log, .. } => {
			let mut report = Report::new("started", endpoint);
			report.pid = Some(pid);
			report.log = Some(log.display().to_string());
			out.emit(&report)
		}
		Started::AlreadyRunning => out.emit(&Report::new("already running", endpoint)),
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
	let startup = client.startup();
	let mut report = Report::new(
		match startup.state() {
			StartupState::Ready => "running",
			StartupState::Starting => "starting",
			StartupState::Failed => "failed",
		},
		endpoint,
	);
	report.waiting = match startup.state() {
		StartupState::Ready => None,
		StartupState::Starting => Some(
			"waiting for permission to use secure storage; answer the system prompt".to_owned(),
		),
		StartupState::Failed => Some(format!(
			"{} Run `relay daemon retry`.",
			daemon::describe_failure(&startup)
		)),
	};
	report.engine_version = Some(client.info().application_version.clone());
	let revision = client.daemon_revision();
	if !revision.is_current() {
		report.out_of_date = Some(Revision::local().to_string());
	}
	report.build_revision = Some(if revision.build.is_empty() {
		"unknown".to_owned()
	} else {
		revision.build
	});
	report.api_revision = Some(revision.api);
	out.emit(&report)
}

/// Asks a daemon that failed to open the secure storage to read it again.
async fn retry(endpoint: &Endpoint, out: &mut Out<'_>) -> Result<(), CliError> {
	let mut client = connect_running(endpoint).await?;
	let info = client.retry_keychain().await?;
	let mut report = Report::new("retrying", endpoint);
	report.waiting = Some(describe_startup(info.startup.as_ref()));
	out.emit(&report)
}

/// Chooses the file-backed store, after the user confirmed.
async fn use_file_store(endpoint: &Endpoint, yes: bool, out: &mut Out<'_>) -> Result<(), CliError> {
	if !yes {
		return Err(CliError::Usage(
			"the file store keeps your identity key and channel secrets in a plain file that is only \
			 protected by file permissions; the system's secure storage is safer. Pass --yes to use it anyway"
				.into(),
		));
	}
	let mut client = connect_running(endpoint).await?;
	let info = client.use_file_store().await?;
	let mut report = Report::new("using the file store", endpoint);
	report.waiting = Some(describe_startup(info.startup.as_ref()));
	out.emit(&report)
}

async fn connect_running(endpoint: &Endpoint) -> Result<RelayClient, CliError> {
	if !daemon::reachable(endpoint).await {
		return Err(CliError::NotRunning {
			endpoint: endpoint::describe(endpoint),
		});
	}
	Ok(RelayClient::connect(endpoint).await?)
}

fn describe_startup(startup: Option<&relay_api::pb::Startup>) -> String {
	match startup.map(|s| (s.state(), s)) {
		Some((StartupState::Failed, s)) => daemon::describe_failure(s),
		Some((StartupState::Starting, _)) => {
			"waiting for permission to use secure storage".to_owned()
		}
		_ => "ready".to_owned(),
	}
}

/// Whether a call is in progress on the running daemon (false when none runs).
async fn calls_in_progress(endpoint: &Endpoint) -> Result<bool, CliError> {
	if !daemon::reachable(endpoint).await {
		return Ok(false);
	}
	let mut client = RelayClient::connect(endpoint).await?;
	client
		.register(CLIENT_NAME, env!("CARGO_PKG_VERSION"), &["observe"])
		.await?;
	let snapshot = client.snapshot().await?;
	Ok(relay_client::refresh::any_call_in_progress(&snapshot.calls))
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

/// Runs the daemon as a child of this terminal (this executable's own, unless `--daemon-bin`
/// names another program) and returns its exit status.
fn foreground(
	global: &GlobalArgs,
	endpoint: &Endpoint,
	options: &DaemonLaunch,
) -> Result<(), CliError> {
	let mut command = match &global.daemon_bin {
		Some(binary) => Process::new(binary),
		None => {
			let mut command = Process::new(std::env::current_exe()?);
			command.arg(RUN_DAEMON_ARG);
			command
		}
	};
	let status = command
		.args(daemon::endpoint_args(endpoint))
		.args(&options.relayd_args)
		.status()?;
	if status.success() {
		Ok(())
	} else {
		Err(CliError::Other(anyhow::anyhow!("relayd stopped: {status}")))
	}
}
