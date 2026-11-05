//! Finding, starting and stopping the `relayd` engine process.
//!
//! Clients never embed the engine. This module launches the separate `relayd`
//! binary fully detached (it keeps running after the launcher exits), waits
//! until its endpoint accepts connections, and asks it to stop through the
//! `Shutdown` RPC. Nothing here starts a daemon implicitly: callers decide.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use relay_api::{ApiError, Endpoint, RelayClient};
use tokio::time::{Instant, sleep, timeout};

/// Names an explicit `relayd` binary.
pub const BINARY_ENV: &str = "RELAY_DAEMON_BIN";

const POLL: Duration = Duration::from_millis(50);
const PROBE: Duration = Duration::from_millis(500);
const LOG_ROTATE_BYTES: u64 = 5 * 1024 * 1024;
const TAIL_BYTES: u64 = 4096;
const TAIL_LINES: usize = 20;

/// The daemon binary's file name on this platform.
pub const fn binary_name() -> &'static str {
	if cfg!(windows) {
		"relayd.exe"
	} else {
		"relayd"
	}
}

/// Where `relayd` is looked for, in order: an explicit path, `RELAY_DAEMON_BIN`,
/// next to the running executable, then `PATH`. Pure, so it can be tested.
pub fn locate_in(
	explicit: Option<&Path>,
	env_binary: Option<&Path>,
	exe_dir: Option<&Path>,
	path_var: Option<&OsString>,
) -> Result<PathBuf, Vec<PathBuf>> {
	let mut searched = Vec::new();
	let mut candidates: Vec<PathBuf> = Vec::new();
	candidates.extend(explicit.map(Path::to_path_buf));
	candidates.extend(env_binary.map(Path::to_path_buf));
	candidates.extend(exe_dir.map(|dir| dir.join(binary_name())));
	if let Some(path) = path_var {
		candidates.extend(std::env::split_paths(path).map(|dir| dir.join(binary_name())));
	}
	for candidate in candidates {
		if candidate.is_file() {
			return Ok(candidate);
		}
		searched.push(candidate);
	}
	Err(searched)
}

/// [`locate_in`] with the real environment.
pub fn locate(explicit: Option<&Path>) -> Result<PathBuf, Vec<PathBuf>> {
	let env_binary = std::env::var_os(BINARY_ENV).map(PathBuf::from);
	let exe_dir = std::env::current_exe()
		.ok()
		.and_then(|exe| exe.parent().map(Path::to_path_buf));
	locate_in(
		explicit,
		env_binary.as_deref(),
		exe_dir.as_deref(),
		std::env::var_os("PATH").as_ref(),
	)
}

/// Where a detached daemon's output goes by default (per-user state directory).
pub fn default_log_path() -> PathBuf {
	let home = |relative: &str| {
		std::env::var_os("HOME")
			.map_or_else(std::env::temp_dir, PathBuf::from)
			.join(relative)
	};
	if cfg!(windows) {
		let base = std::env::var_os("LOCALAPPDATA").map_or_else(std::env::temp_dir, PathBuf::from);
		base.join("Relay").join("relayd.log")
	} else if cfg!(target_os = "macos") {
		home("Library/Logs/Relay/relayd.log")
	} else if let Some(state) = std::env::var_os("XDG_STATE_HOME") {
		PathBuf::from(state).join("relay").join("relayd.log")
	} else {
		home(".local/state/relay/relayd.log")
	}
}

/// How to start the daemon.
#[derive(Debug, Clone)]
pub struct Launch {
	/// The endpoint the daemon must serve (passed to it as `--socket` or
	/// `--tcp-profile`) and that readiness is judged on.
	pub endpoint: Endpoint,
	/// An explicit `relayd` binary; otherwise see [`locate`].
	pub binary: Option<PathBuf>,
	/// Where the daemon's output is appended; `None` uses [`default_log_path`].
	pub log_file: Option<PathBuf>,
	/// Extra arguments for `relayd`.
	pub extra_args: Vec<OsString>,
	/// How long to wait for the endpoint to come up.
	pub timeout: Duration,
}

impl Launch {
	/// Defaults for an endpoint: 10 s to come up, default log file.
	pub fn new(endpoint: Endpoint) -> Self {
		Self {
			endpoint,
			binary: None,
			log_file: None,
			extra_args: Vec::new(),
			timeout: Duration::from_secs(10),
		}
	}

	fn log_path(&self) -> PathBuf {
		self.log_file.clone().unwrap_or_else(default_log_path)
	}
}

/// What [`start`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Started {
	/// A new daemon was launched and its endpoint accepts connections.
	Launched {
		/// Process id of the daemon.
		pid: u32,
		/// The binary that was run.
		binary: PathBuf,
		/// The file its output goes to.
		log: PathBuf,
	},
	/// Something already serves the endpoint; nothing was started.
	AlreadyRunning,
	/// No `relayd` binary was found in any of these places.
	BinaryNotFound {
		/// Everything that was checked.
		searched: Vec<PathBuf>,
	},
	/// The daemon could not be started or never became ready.
	FailedToStart {
		/// What went wrong.
		reason: String,
		/// The last lines the daemon wrote to its log.
		log_tail: String,
	},
}

/// Whether something accepts connections at `endpoint`. This is a plain
/// connect, not an API call, so it works while the daemon is still starting.
pub async fn reachable(endpoint: &Endpoint) -> bool {
	let attempt = async {
		match endpoint {
			#[cfg(unix)]
			Endpoint::Unix(path) => tokio::net::UnixStream::connect(path).await.is_ok(),
			Endpoint::LoopbackTcp { profile_path } => {
				match relay_api::TcpProfile::load(profile_path) {
					Ok(profile) => tokio::net::TcpStream::connect(profile.addr).await.is_ok(),
					Err(_) => false,
				}
			}
		}
	};
	timeout(PROBE, attempt).await.unwrap_or(false)
}

/// The tail of a log file: at most [`TAIL_LINES`] lines from the last
/// [`TAIL_BYTES`], starting no earlier than byte `from`.
pub fn log_tail(path: &Path, from: u64) -> String {
	let Ok(mut file) = File::open(path) else {
		return String::new();
	};
	let len = file.metadata().map_or(0, |meta| meta.len());
	let start = len.saturating_sub(TAIL_BYTES).max(from.min(len));
	let mut bytes = Vec::new();
	if file.seek(SeekFrom::Start(start)).is_err() || file.read_to_end(&mut bytes).is_err() {
		return String::new();
	}
	let text = String::from_utf8_lossy(&bytes);
	let lines: Vec<&str> = text.lines().collect();
	lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n")
}

fn endpoint_args(endpoint: &Endpoint) -> [OsString; 2] {
	match endpoint {
		#[cfg(unix)]
		Endpoint::Unix(path) => ["--socket".into(), path.clone().into_os_string()],
		Endpoint::LoopbackTcp { profile_path } => [
			"--tcp-profile".into(),
			profile_path.clone().into_os_string(),
		],
	}
}

fn open_log(path: &Path) -> std::io::Result<File> {
	if let Some(dir) = path.parent() {
		fs::create_dir_all(dir)?;
	}
	if fs::metadata(path).is_ok_and(|meta| meta.len() > LOG_ROTATE_BYTES) {
		let _ = fs::rename(path, path.with_extension("log.1"));
	}
	OpenOptions::new().create(true).append(true).open(path)
}

/// A command for the daemon that survives this process: its own session or
/// process group, no terminal, no inherited standard input.
fn detached(binary: &Path, args: &[OsString], log: &File) -> std::io::Result<Command> {
	let mut command = Command::new(binary);
	command
		.args(args)
		.stdin(Stdio::null())
		.stdout(log.try_clone()?)
		.stderr(log.try_clone()?);
	#[cfg(unix)]
	{
		use std::os::unix::process::CommandExt;
		command.process_group(0);
	}
	#[cfg(windows)]
	{
		use std::os::windows::process::CommandExt;
		const DETACHED_PROCESS: u32 = 0x0000_0008;
		const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
		command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
	}
	Ok(command)
}

/// Starts the daemon unless one already serves the endpoint.
pub async fn start(launch: &Launch) -> Started {
	if reachable(&launch.endpoint).await {
		return Started::AlreadyRunning;
	}
	let binary = match locate(launch.binary.as_deref()) {
		Ok(binary) => binary,
		Err(searched) => return Started::BinaryNotFound { searched },
	};
	let log = launch.log_path();
	let failed = |reason: String, from: u64| Started::FailedToStart {
		log_tail: log_tail(&log, from),
		reason,
	};
	let file = match open_log(&log) {
		Ok(file) => file,
		Err(error) => {
			return failed(format!("cannot open the log {}: {error}", log.display()), 0);
		}
	};
	let from = file.metadata().map_or(0, |meta| meta.len());
	let mut args = endpoint_args(&launch.endpoint).to_vec();
	args.extend(launch.extra_args.iter().cloned());
	let child = detached(&binary, &args, &file).and_then(|mut command| command.spawn());
	let mut child = match child {
		Ok(child) => child,
		Err(error) => return failed(format!("cannot run {}: {error}", binary.display()), from),
	};
	let pid = child.id();
	match wait_ready(&mut child, &launch.endpoint, launch.timeout).await {
		Ready::Yes => {
			// Reap it when it eventually exits so a long-lived client leaves no zombie.
			std::thread::spawn(move || {
				let _ = child.wait();
			});
			Started::Launched { pid, binary, log }
		}
		Ready::Exited(status) if reachable(&launch.endpoint).await => {
			let _ = status;
			Started::AlreadyRunning
		}
		Ready::Exited(status) => failed(format!("the daemon exited early ({status})"), from),
		Ready::TimedOut => {
			let _ = child.kill();
			let _ = child.wait();
			failed(
				format!(
					"the daemon did not become ready within {:?}",
					launch.timeout
				),
				from,
			)
		}
	}
}

enum Ready {
	Yes,
	Exited(String),
	TimedOut,
}

async fn wait_ready(child: &mut Child, endpoint: &Endpoint, limit: Duration) -> Ready {
	let deadline = Instant::now() + limit;
	loop {
		if let Ok(Some(status)) = child.try_wait() {
			return Ready::Exited(status.to_string());
		}
		if reachable(endpoint).await {
			return Ready::Yes;
		}
		if Instant::now() >= deadline {
			return Ready::TimedOut;
		}
		sleep(POLL).await;
	}
}

/// What [`stop`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
	/// The daemon acknowledged and its endpoint is gone.
	Stopped,
	/// Nothing was serving the endpoint.
	NotRunning,
}

/// Waits until nothing accepts connections at `endpoint`.
pub async fn wait_stopped(endpoint: &Endpoint, limit: Duration) -> bool {
	let deadline = Instant::now() + limit;
	while reachable(endpoint).await {
		if Instant::now() >= deadline {
			return false;
		}
		sleep(POLL).await;
	}
	true
}

/// Asks the daemon to stop (registering with the `admin` capability under
/// `client_name`) and waits for the endpoint to disappear.
pub async fn stop(
	endpoint: &Endpoint,
	client_name: &str,
	limit: Duration,
) -> Result<Stopped, ApiError> {
	if !reachable(endpoint).await {
		return Ok(Stopped::NotRunning);
	}
	let mut client = RelayClient::connect(endpoint).await?;
	client
		.register(client_name, env!("CARGO_PKG_VERSION"), &["admin"])
		.await?;
	// The acknowledgement can race the endpoint closing; either way the proof
	// of a stop is that the endpoint goes away.
	let _ = client.shutdown().await;
	drop(client);
	if wait_stopped(endpoint, limit).await {
		Ok(Stopped::Stopped)
	} else {
		Err(ApiError::OperationTimeout)
	}
}
