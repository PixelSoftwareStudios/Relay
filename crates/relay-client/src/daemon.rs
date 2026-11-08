//! Starting and stopping the engine process.
//!
//! Clients never run the engine in their own process. This module launches the engine as a
//! separate process fully detached (it keeps running after the launcher exits): the running
//! executable itself, started with [`RUN_DAEMON_ARG`], which every client binary honours by
//! running the `relay-daemon` library. There is no search for a daemon file, so the daemon is
//! always the same binary, with the same code identity, as the client that started it. Then
//! it waits until the endpoint accepts connections, and asks the daemon to stop through the
//! `Shutdown` RPC.
//!
//! [`ensure_running`] is what clients call before they connect: it starts the
//! daemon when none answers, under a per-endpoint file lock so that several
//! clients launching at once start exactly one. A process that asked the
//! daemon to stop ([`stop`]) is never allowed to restart it implicitly.
//!
//! The daemon serves its endpoint before it opens the system's secure storage, which can
//! wait minutes for the user (a keychain password prompt). A daemon whose endpoint answers is
//! running, whatever it reports: the launcher never kills or restarts it, waits while it is
//! `Starting` (up to [`StartOptions::startup_timeout`], reporting progress) and gives up only
//! when it reports `Failed` or disappears.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use relay_api::pb::{Startup, StartupReason, StartupState};
use relay_api::{ApiError, Endpoint, RelayClient};
use relay_io::IoError;
use relay_io::lock::ProcessLock;
use relay_io::proc::{
	RUN_DAEMON_ARG, detached_command, open_rotating_log, read_tail, reap_in_background,
};
use tokio::time::{Instant, sleep, timeout};

const POLL: Duration = Duration::from_millis(50);
const PROBE: Duration = Duration::from_millis(500);
const LOG_ROTATE_BYTES: u64 = 5 * 1024 * 1024;
const TAIL_BYTES: u64 = 4096;
const TAIL_LINES: usize = 20;

/// Seconds of no attached client after which an auto-started daemon stops by
/// itself; `0` turns that off.
pub const IDLE_EXIT_ENV: &str = "RELAY_IDLE_EXIT_SECS";

/// How long an auto-started daemon lingers with nobody attached.
pub const DEFAULT_IDLE_EXIT: Duration = Duration::from_secs(300);

/// Overrides [`default_log_path`].
pub const LOG_ENV: &str = "RELAY_DAEMON_LOG";

/// Where a detached daemon's output goes by default (per-user state directory).
pub fn default_log_path() -> PathBuf {
	if let Some(path) = std::env::var_os(LOG_ENV) {
		return PathBuf::from(path);
	}
	relay_io::paths::log_dir().join("relayd.log")
}

/// How to start the daemon.
#[derive(Debug, Clone)]
pub struct Launch {
	/// The endpoint the daemon must serve (passed to it as `--socket` or
	/// `--tcp-profile`) and that readiness is judged on.
	pub endpoint: Endpoint,
	/// An explicit program to run as `relayd` (tests, `relay --daemon-bin`); `None` runs this
	/// executable with [`RUN_DAEMON_ARG`].
	pub binary: Option<PathBuf>,
	/// Where the daemon's output is appended; `None` uses [`default_log_path`].
	pub log_file: Option<PathBuf>,
	/// Extra arguments for `relayd`.
	pub extra_args: Vec<OsString>,
	/// `relayd --idle-exit`: stop after this long with no client attached.
	pub idle_exit: Option<Duration>,
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
			idle_exit: None,
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
	read_tail(path, from, TAIL_BYTES, TAIL_LINES)
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

/// Set once this process has asked the daemon to stop; see [`ensure_running`].
static STOPPED_BY_US: AtomicBool = AtomicBool::new(false);

/// Starts the daemon unless one already serves the endpoint. This is an
/// explicit request, so it also lifts the "we stopped it" guard.
pub async fn start(launch: &Launch) -> Started {
	STOPPED_BY_US.store(false, Ordering::Release);
	start_unguarded(launch).await
}

/// Starts the daemon for a caller that holds the start lock and just stopped the previous
/// one itself: the "we stopped it" guard is lifted, as for an explicit start.
pub(crate) async fn start_replacement(launch: &Launch) -> Started {
	let outcome = start_unguarded(launch).await;
	STOPPED_BY_US.store(false, Ordering::Release);
	outcome
}

async fn start_unguarded(launch: &Launch) -> Started {
	if reachable(&launch.endpoint).await {
		return Started::AlreadyRunning;
	}
	let log = launch.log_path();
	let failed = |reason: String, from: u64| Started::FailedToStart {
		log_tail: log_tail(&log, from),
		reason,
	};
	let file = match open_rotating_log(&log, LOG_ROTATE_BYTES) {
		Ok(file) => file,
		Err(error) => {
			return failed(format!("cannot open the log {}: {error}", log.display()), 0);
		}
	};
	let from = file.metadata().map_or(0, |meta| meta.len());
	let (binary, mut args) = match &launch.binary {
		Some(binary) => (binary.clone(), Vec::new()),
		None => match std::env::current_exe() {
			Ok(exe) => (exe, vec![OsString::from(RUN_DAEMON_ARG)]),
			Err(error) => {
				return failed(
					format!("cannot find this program's own file: {error}"),
					from,
				);
			}
		},
	};
	args.extend(endpoint_args(&launch.endpoint));
	if let Some(idle) = launch.idle_exit {
		args.push("--idle-exit".into());
		args.push(idle.as_secs().to_string().into());
	}
	args.extend(launch.extra_args.iter().cloned());
	let child = detached_command(&binary, &args, &file).and_then(|mut command| command.spawn());
	let mut child = match child {
		Ok(child) => child,
		Err(error) => return failed(format!("cannot run {}: {error}", binary.display()), from),
	};
	let pid = child.id();
	match wait_serving(&mut child, &launch.endpoint, launch.timeout).await {
		Ready::Yes => {
			// Reap it when it eventually exits so a long-lived client leaves no zombie.
			reap_in_background(child);
			Started::Launched { pid, binary, log }
		}
		Ready::Exited(_) if reachable(&launch.endpoint).await => Started::AlreadyRunning,
		Ready::Exited(status) => failed(format!("the daemon exited early ({status})"), from),
		Ready::TimedOut => {
			// Never killed: it may be slow rather than stuck, and a second attempt finds it.
			reap_in_background(child);
			failed(
				format!(
					"the daemon did not answer within {:?}; it was left running",
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

/// Waits until the endpoint accepts connections. Whether the daemon has finished opening its
/// secure storage is [`wait_ready`]'s question, not this one's.
async fn wait_serving(child: &mut Child, endpoint: &Endpoint, limit: Duration) -> Ready {
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

/// Why a running daemon did not become ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotReady {
	/// It reported a failure and waits for the user (see [`describe_failure`]).
	Failed {
		/// A plain sentence for the user.
		reason: String,
		/// The system's own text.
		detail: String,
	},
	/// It was still starting when the time was up; it was left running.
	TimedOut,
	/// It stopped answering.
	Gone,
}

/// How often readiness is asked.
const READY_POLL: Duration = Duration::from_millis(250);

/// A plain sentence for what a `Failed` start-up means.
pub fn describe_failure(startup: &Startup) -> String {
	match startup.reason() {
		StartupReason::IdentityUnreadable => {
			"The stored identity cannot be read. Relay left it untouched.".to_owned()
		}
		_ => "Relay needs access to your keychain to keep your identity safe. Allow it, or choose \
			the file store in settings"
			.to_owned(),
	}
}

/// Asks the daemon at `endpoint` how far it is. `None` when nothing answers.
pub async fn startup_of(endpoint: &Endpoint) -> Option<Startup> {
	let client = RelayClient::connect(endpoint).await.ok()?;
	Some(client.startup())
}

/// Waits until the daemon at `endpoint` is ready for clients. While it is `Starting`
/// (waiting for the keychain prompt, say) it keeps waiting up to `limit` and calls `progress`
/// whenever what it waits for changes. Never touches the daemon process.
pub async fn wait_ready(
	endpoint: &Endpoint,
	limit: Duration,
	progress: &mut (dyn FnMut(&Startup) + Send),
) -> Result<(), NotReady> {
	let deadline = Instant::now() + limit;
	let mut last: Option<Startup> = None;
	loop {
		let Some(startup) = startup_of(endpoint).await else {
			if !reachable(endpoint).await {
				return Err(NotReady::Gone);
			}
			sleep(READY_POLL).await;
			continue;
		};
		match startup.state() {
			StartupState::Ready => return Ok(()),
			StartupState::Failed => {
				return Err(NotReady::Failed {
					reason: describe_failure(&startup),
					detail: startup.detail,
				});
			}
			StartupState::Starting => {
				if last.as_ref() != Some(&startup) {
					progress(&startup);
					last = Some(startup);
				}
			}
		}
		if Instant::now() >= deadline {
			return Err(NotReady::TimedOut);
		}
		sleep(READY_POLL).await;
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
	/// As [`Launch::binary`].
	pub binary: Option<PathBuf>,
	/// Where the daemon's output is appended; `None` uses [`default_log_path`].
	pub log_file: Option<PathBuf>,
	/// Extra arguments for `relayd`.
	pub extra_args: Vec<OsString>,
	/// Have the daemon stop after this long with nobody attached.
	pub idle_exit: Option<Duration>,
	/// How long to wait for a launched daemon to accept connections.
	pub ready_timeout: Duration,
	/// How long to wait for a daemon that answers but is still opening its secure storage.
	pub startup_timeout: Duration,
	/// How long to wait for another client that is already starting it.
	pub lock_timeout: Duration,
}

impl Default for StartOptions {
	fn default() -> Self {
		Self {
			binary: None,
			log_file: None,
			extra_args: Vec::new(),
			idle_exit: None,
			ready_timeout: Duration::from_secs(10),
			startup_timeout: Duration::from_secs(300),
			lock_timeout: Duration::from_secs(15),
		}
	}
}

/// The idle-exit for a daemon this program starts on its own:
/// [`IDLE_EXIT_ENV`] if set (`0` disables), else [`DEFAULT_IDLE_EXIT`].
pub fn autostart_idle_exit() -> Option<Duration> {
	idle_exit_from(std::env::var(IDLE_EXIT_ENV).ok().as_deref())
}

/// [`autostart_idle_exit`] with the variable's value passed in.
pub fn idle_exit_from(value: Option<&str>) -> Option<Duration> {
	match value.map(|v| v.trim().parse::<u64>()) {
		Some(Ok(0)) => None,
		Some(Ok(secs)) => Some(Duration::from_secs(secs)),
		_ => Some(DEFAULT_IDLE_EXIT),
	}
}

impl StartOptions {
	/// Options for starting a daemon on a client's own initiative: it will
	/// exit by itself when nobody uses it.
	pub fn autostart() -> Self {
		Self {
			idle_exit: autostart_idle_exit(),
			..Self::default()
		}
	}

	/// The launch these options describe for `endpoint`.
	pub fn launch(&self, endpoint: &Endpoint) -> Launch {
		let mut launch = Launch::new(endpoint.clone());
		launch.binary.clone_from(&self.binary);
		launch.log_file.clone_from(&self.log_file);
		launch.extra_args.clone_from(&self.extra_args);
		launch.idle_exit = self.idle_exit;
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
	/// The daemon could not be started or never became ready.
	FailedToStart {
		/// What went wrong.
		reason: String,
		/// The last lines the daemon wrote to its log.
		log_tail: String,
	},
	/// The daemon runs but could not open its secure storage and waits for the user.
	NotReady(NotReady),
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
			Self::NotReady(NotReady::Failed { reason, detail }) => {
				write!(f, "{reason}")?;
				if !detail.is_empty() {
					write!(f, " ({detail})")?;
				}
				write!(f, "\nTry again with `relay daemon retry`.")
			}
			Self::NotReady(NotReady::TimedOut) => write!(
				f,
				"the background service is still waiting for permission to use secure storage; \
				 answer the system prompt, then try again"
			),
			Self::NotReady(NotReady::Gone) => {
				write!(f, "the background service stopped while it was starting")
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

/// Makes sure a daemon answers at `endpoint` and is ready, starting one if needed.
///
/// Several clients may call this at once; they take a file lock, so exactly
/// one launches the daemon and the others attach to it. Starting never grants
/// anything: the daemon comes up receive-only with no microphone open. A daemon that
/// answers but waits for the user's keychain prompt is waited for, never restarted.
pub async fn ensure_running(
	endpoint: &Endpoint,
	options: &StartOptions,
) -> Result<Ensure, EnsureError> {
	ensure_running_with(endpoint, options, &mut |_| {}).await
}

/// [`ensure_running`] reporting what a starting daemon waits for through `progress`.
pub async fn ensure_running_with(
	endpoint: &Endpoint,
	options: &StartOptions,
	progress: &mut (dyn FnMut(&Startup) + Send),
) -> Result<Ensure, EnsureError> {
	if STOPPED_BY_US.load(Ordering::Acquire) {
		return Err(EnsureError::StoppedByRequest);
	}
	let found = serve(endpoint, options).await?;
	wait_ready(endpoint, options.startup_timeout, progress)
		.await
		.map_err(EnsureError::NotReady)?;
	Ok(found)
}

/// Gets an endpoint that answers: attaches to the daemon already there or launches one under
/// the start lock. The lock is released as soon as the endpoint answers, so a daemon that waits
/// for a prompt never holds other clients up.
async fn serve(endpoint: &Endpoint, options: &StartOptions) -> Result<Ensure, EnsureError> {
	if reachable(endpoint).await {
		return Ok(Ensure::AlreadyRunning);
	}
	let lock_file = lock_path(endpoint);
	let deadline = Instant::now() + options.lock_timeout;
	let lock = loop {
		match ProcessLock::acquire_beside(&lock_file) {
			Ok(lock) => break lock,
			Err(IoError::Locked(_)) => {
				// Whoever holds it is starting the daemon: its endpoint is ours too.
				if reachable(endpoint).await {
					return Ok(Ensure::AlreadyRunning);
				}
				if Instant::now() >= deadline {
					return Err(EnsureError::LockTimeout);
				}
				sleep(POLL).await;
			}
			Err(error) => return Err(EnsureError::Lock(error.to_string())),
		}
	};
	// The lock holder before us may have finished the job.
	if reachable(endpoint).await {
		return Ok(Ensure::AlreadyRunning);
	}
	let outcome = start_unguarded(&options.launch(endpoint)).await;
	drop(lock);
	match outcome {
		Started::Launched { pid, log, .. } => Ok(Ensure::Started { pid, log }),
		Started::AlreadyRunning => Ok(Ensure::AlreadyRunning),
		Started::FailedToStart { reason, log_tail } => {
			Err(EnsureError::FailedToStart { reason, log_tail })
		}
	}
}
