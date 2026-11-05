//! Finding, starting and stopping the `relayd` engine process.
//!
//! Clients never embed the engine. This module launches the separate `relayd`
//! binary fully detached (it keeps running after the launcher exits), waits
//! until its endpoint accepts connections, and asks it to stop through the
//! `Shutdown` RPC.
//!
//! [`ensure_running`] is what clients call before they connect: it starts the
//! daemon when none answers, under a per-endpoint file lock so that several
//! clients launching at once start exactly one. A process that asked the
//! daemon to stop ([`stop`]) is never allowed to restart it implicitly.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Overrides [`default_log_path`].
pub const LOG_ENV: &str = "RELAY_DAEMON_LOG";

/// Where a detached daemon's output goes by default (per-user state directory).
pub fn default_log_path() -> PathBuf {
	if let Some(path) = std::env::var_os(LOG_ENV) {
		return PathBuf::from(path);
	}
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

/// The `relayd` arguments that make it serve `endpoint`.
pub fn endpoint_args(endpoint: &Endpoint) -> [OsString; 2] {
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

/// Set once this process has asked the daemon to stop; see [`ensure_running`].
static STOPPED_BY_US: AtomicBool = AtomicBool::new(false);

/// Starts the daemon unless one already serves the endpoint. This is an
/// explicit request, so it also lifts the "we stopped it" guard.
pub async fn start(launch: &Launch) -> Started {
	STOPPED_BY_US.store(false, Ordering::Release);
	start_unguarded(launch).await
}

async fn start_unguarded(launch: &Launch) -> Started {
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
		STOPPED_BY_US.store(true, Ordering::Release);
		Ok(Stopped::Stopped)
	} else {
		Err(ApiError::OperationTimeout)
	}
}

/// How [`ensure_running`] should start a missing daemon.
#[derive(Debug, Clone)]
pub struct StartOptions {
	/// An explicit `relayd` binary; otherwise see [`locate`].
	pub binary: Option<PathBuf>,
	/// Where the daemon's output is appended; `None` uses [`default_log_path`].
	pub log_file: Option<PathBuf>,
	/// Extra arguments for `relayd`.
	pub extra_args: Vec<OsString>,
	/// How long to wait for a launched daemon to accept connections.
	pub ready_timeout: Duration,
	/// How long to wait for another client that is already starting it.
	pub lock_timeout: Duration,
}

impl Default for StartOptions {
	fn default() -> Self {
		Self {
			binary: None,
			log_file: None,
			extra_args: Vec::new(),
			ready_timeout: Duration::from_secs(10),
			lock_timeout: Duration::from_secs(15),
		}
	}
}

impl StartOptions {
	/// The launch these options describe for `endpoint`.
	pub fn launch(&self, endpoint: &Endpoint) -> Launch {
		let mut launch = Launch::new(endpoint.clone());
		launch.binary.clone_from(&self.binary);
		launch.log_file.clone_from(&self.log_file);
		launch.extra_args.clone_from(&self.extra_args);
		launch.timeout = self.ready_timeout;
		launch
	}
}

/// What [`ensure_running`] found or did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ensure {
	/// A daemon already answers (possibly started by another client meanwhile).
	AlreadyRunning,
	/// This call launched the daemon.
	Started {
		/// Process id of the daemon.
		pid: u32,
		/// The file its output goes to.
		log: PathBuf,
	},
}

/// Why [`ensure_running`] could not provide a daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnsureError {
	/// This process asked the daemon to stop and does not restart it by itself.
	StoppedByRequest,
	/// Another client held the start lock for the whole wait.
	LockTimeout,
	/// The start lock file could not be used.
	Lock(String),
	/// No `relayd` binary was found.
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

impl std::fmt::Display for EnsureError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::StoppedByRequest => {
				write!(
					f,
					"this program stopped the daemon and does not restart it by itself"
				)
			}
			Self::LockTimeout => {
				write!(
					f,
					"another program has been starting the daemon for too long"
				)
			}
			Self::Lock(why) => write!(f, "cannot take the daemon start lock: {why}"),
			Self::BinaryNotFound { searched } => {
				let places: Vec<String> =
					searched.iter().map(|p| p.display().to_string()).collect();
				write!(
					f,
					"cannot find {}; looked in: {}. Install it next to this program, put it on PATH or set {BINARY_ENV}",
					binary_name(),
					places.join(", ")
				)
			}
			Self::FailedToStart { reason, log_tail } => {
				write!(f, "the daemon did not start: {reason}")?;
				if !log_tail.is_empty() {
					write!(f, "\n--- end of the daemon log ---\n{log_tail}")?;
				}
				Ok(())
			}
		}
	}
}

impl std::error::Error for EnsureError {}

/// The advisory lock file that serializes starts of the daemon serving
/// `endpoint`: next to the socket or profile, so it is per endpoint and per
/// user. The operating system releases it when its holder dies, so a crashed
/// starter never leaves a stale lock behind.
pub fn lock_path(endpoint: &Endpoint) -> PathBuf {
	let base = match endpoint {
		#[cfg(unix)]
		Endpoint::Unix(path) => path,
		Endpoint::LoopbackTcp { profile_path } => profile_path,
	};
	let name = base.file_name().map_or_else(
		|| "relayd".to_owned(),
		|name| name.to_string_lossy().into_owned(),
	);
	base.with_file_name(format!("{name}.start.lock"))
}

fn open_lock(path: &Path) -> std::io::Result<File> {
	if let Some(dir) = path.parent() {
		#[cfg(unix)]
		{
			use std::os::unix::fs::DirBuilderExt;
			fs::DirBuilder::new()
				.recursive(true)
				.mode(0o700)
				.create(dir)?;
		}
		#[cfg(not(unix))]
		fs::create_dir_all(dir)?;
	}
	OpenOptions::new()
		.create(true)
		.truncate(false)
		.write(true)
		.open(path)
}

/// Makes sure a daemon answers at `endpoint`, starting one if needed.
///
/// Several clients may call this at once; they take a file lock, so exactly
/// one launches the daemon and the others wait for it to become ready.
/// Starting never grants anything: the daemon comes up receive-only with no
/// microphone open.
pub async fn ensure_running(
	endpoint: &Endpoint,
	options: &StartOptions,
) -> Result<Ensure, EnsureError> {
	if STOPPED_BY_US.load(Ordering::Acquire) {
		return Err(EnsureError::StoppedByRequest);
	}
	if reachable(endpoint).await {
		return Ok(Ensure::AlreadyRunning);
	}
	let lock = open_lock(&lock_path(endpoint)).map_err(|e| EnsureError::Lock(e.to_string()))?;
	let deadline = Instant::now() + options.lock_timeout;
	loop {
		match lock.try_lock() {
			Ok(()) => break,
			Err(fs::TryLockError::WouldBlock) => {
				// Whoever holds it is starting the daemon: its readiness is ours too.
				if reachable(endpoint).await {
					return Ok(Ensure::AlreadyRunning);
				}
				if Instant::now() >= deadline {
					return Err(EnsureError::LockTimeout);
				}
				sleep(POLL).await;
			}
			Err(fs::TryLockError::Error(error)) => {
				return Err(EnsureError::Lock(error.to_string()));
			}
		}
	}
	// The lock holder before us may have finished the job.
	if reachable(endpoint).await {
		return Ok(Ensure::AlreadyRunning);
	}
	let outcome = start_unguarded(&options.launch(endpoint)).await;
	drop(lock);
	match outcome {
		Started::Launched { pid, log, .. } => Ok(Ensure::Started { pid, log }),
		Started::AlreadyRunning => Ok(Ensure::AlreadyRunning),
		Started::BinaryNotFound { searched } => Err(EnsureError::BinaryNotFound { searched }),
		Started::FailedToStart { reason, log_tail } => {
			Err(EnsureError::FailedToStart { reason, log_tail })
		}
	}
}
