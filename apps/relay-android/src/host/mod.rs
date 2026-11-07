//! The in-process engine and the client side that talks to it.
//!
//! The Android process is the daemon and the client at once: there is no
//! separate `relayd`. [`Host`] starts relay-engine's `DaemonHandle` on a
//! private tokio runtime, serves it on a Unix socket in `filesDir`, and runs
//! the same supervisors the desktop app runs against that socket. The Activity
//! only draws what [`Host`] already keeps up to date, so a recreated activity
//! (rotation, theme) does not disturb a call.
//!
//! Presence: the engine advertises and accepts calls only while a client holds
//! an open watch stream. The supervisors are attached while the app is in the
//! foreground or a call exists, and aborted otherwise, so a backgrounded app
//! without a call leaves the LAN. A call is owned by this attachment: closing
//! the app ends it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use relay_api::Endpoint;
use relay_client::ptt::PttWorker;
use relay_config::Paths;
use relay_engine::daemon::{AudioSetup, DaemonHandle, RunOptions, Transport};
use relay_engine::peers::{DiscoverySetup, Hint, HintSender, hint_channel};
use relay_io::LockExt;
use relay_net::discovery::PlatformRecord;
use relay_ui::actions::{Actions, Shared, ShellSink};
use relay_ui::controller::{Controller, SessionSlot};
use relay_ui::emitter::{self, Cadence, Sink, TraySummary};
use relay_ui::hub::Hub;
use relay_ui::messaging::link::MsgLink;
use relay_ui::model::{AppState, Link, Meters};
use relay_ui::relay_link::RelayLink;
use relay_ui::shell::Shell;
use relay_ui::{messaging, supervisor};
use tokio::runtime::{Handle, Runtime};
use tokio::task::JoinHandle;

mod quiet;

use crate::audio_session::AudioSession;
use crate::bridge::{CallSummary, EngineConfig, EngineError, FoundService, HostBridge};
use crate::sealed::{KeystoreSealer, SealedIdentityStore, SealedVault};
use crate::shell::AndroidShell;


static HOST: Mutex<Option<Arc<Host>>> = Mutex::new(None);

/// The running host, if the engine was started.
pub fn host() -> Option<Arc<Host>> {
	HOST.lock_unpoisoned().clone()
}

/// Test seam: adjusts the options before the engine starts.
pub type Tweak = Box<dyn FnOnce(&mut RunOptions)>;

pub struct Host {
	runtime: Mutex<Option<Runtime>>,
	handle: Handle,
	pub shell: Arc<AndroidShell>,
	pub hub: Arc<Hub>,
	pub shared: Arc<Shared>,
	pub actions: Actions,
	slot: Arc<SessionSlot>,
	bridge: Arc<dyn HostBridge>,
	hints: HintSender,
	endpoint: Endpoint,
	daemon: tokio::sync::Mutex<Option<DaemonHandle>>,
	attached: Mutex<Vec<JoinHandle<()>>>,
	ptt: PttWorker,
	foreground: AtomicBool,
	in_call: AtomicBool,
	/// `background.available` as the engine last said; the engine holds presence itself then.
	background: AtomicBool,
	/// The shell alerts incoming calls itself (`SetClientQuiet`).
	quiet_wanted: AtomicBool,
	quiet_task: Mutex<Option<JoinHandle<()>>>,
}

impl Host {
	pub fn endpoint(&self) -> &Endpoint {
		&self.endpoint
	}

	/// Starts the engine, or returns the running one (a recreated activity
	/// calls this again).
	pub fn start(
		config: &EngineConfig,
		bridge: Arc<dyn HostBridge>,
		tweak: Option<Tweak>,
	) -> Result<Arc<Self>, EngineError> {
		let mut slot = HOST.lock_unpoisoned();
		if let Some(running) = slot.as_ref() {
			return Ok(running.clone());
		}
		let failed = |what: &str, e: &dyn std::fmt::Display| EngineError::Failed {
			reason: format!("{what}: {e}"),
		};
		let root = PathBuf::from(&config.files_dir);
		let runtime = tokio::runtime::Builder::new_multi_thread()
			.worker_threads(3)
			.thread_name("relay-io")
			.enable_all()
			.build()
			.map_err(|e| failed("runtime", &e))?;
		let paths = Paths::under(root.join("relay"));
		let socket = root.join("run").join("engine.sock");
		let sealer = Arc::new(KeystoreSealer(bridge.clone()));
		let (hints, source) = hint_channel();
		let mut options = RunOptions::new(paths.clone());
		options.transport = Transport::Unix(Some(socket));
		options.identity_store = Some(Box::new(SealedIdentityStore::new(
			paths.data_dir().join("identity.sealed"),
			sealer.clone(),
		)));
		options.channel_vault = Some(Arc::new(SealedVault::new(
			paths.data_dir().join("channel-sealed"),
			sealer,
		)));
		options.audio = audio_setup(bridge.clone());
		options.default_display_name.clone_from(&config.default_display_name);
		options.network.discovery = Some(DiscoverySetup {
			advertise: true,
			hints: Some(source),
			..DiscoverySetup::default()
		});
		if let Some(tweak) = tweak {
			tweak(&mut options);
		}
		let daemon = runtime
			.block_on(DaemonHandle::start(options))
			.map_err(|e| failed("start", &e))?;
		tracing::info!(listen = ?daemon.listen_addr(), "engine started in-process");
		let endpoint = daemon.endpoint().clone();

		let handle = runtime.handle().clone();
		let host = Arc::new_cyclic(|weak: &Weak<Host>| {
			let shell = Arc::new(AndroidShell::new(bridge.clone()));
			let dyn_shell: Arc<dyn Shell> = shell.clone();
			let hub = Arc::new(Hub::default());
			let slot = Arc::new(SessionSlot::default());
			let shared = Arc::new(Shared::default());
			let actions = Actions {
				controller: Arc::new(Controller::new(hub.clone(), slot.clone())),
				hub: hub.clone(),
				shared: shared.clone(),
				runtime: handle.clone(),
				shell: dyn_shell.clone(),
			};
			let sink = Arc::new(CallSink {
				inner: ShellSink {
					shared: shared.clone(),
					shell: dyn_shell,
				},
				host: weak.clone(),
				last: Mutex::new(CallSummary::idle()),
				background: Mutex::new(None),
			});
			handle.spawn(emitter::run(hub.clone(), sink, Cadence::default()));
			let ptt = PttWorker::spawn(
				&handle,
				endpoint.clone(),
				relay_ui::ptt_target::resolver(hub.clone()),
			);
			Self {
				runtime: Mutex::new(None),
				handle: handle.clone(),
				shell,
				hub,
				shared,
				actions,
				slot,
				bridge,
				hints,
				endpoint,
				daemon: tokio::sync::Mutex::new(Some(daemon)),
				attached: Mutex::new(Vec::new()),
				ptt,
				foreground: AtomicBool::new(false),
				in_call: AtomicBool::new(false),
				background: AtomicBool::new(false),
				quiet_wanted: AtomicBool::new(false),
				quiet_task: Mutex::new(None),
			}
		});
		*host.runtime.lock_unpoisoned() = Some(runtime);
		*slot = Some(host.clone());
		Ok(host)
	}

	/// Ends any call, withdraws from the LAN and stops the engine.
	pub fn stop(&self) {
		self.detach();
		if let Some(running) = self.quiet_task.lock_unpoisoned().take() {
			running.abort();
		}
		let Some(runtime) = self.runtime.lock_unpoisoned().take() else {
			return;
		};
		let daemon = self.daemon.try_lock().ok().and_then(|mut d| d.take());
		if let Some(daemon) = daemon {
			let stopped = runtime.block_on(async {
				tokio::time::timeout(Duration::from_secs(8), daemon.shutdown()).await
			});
			tracing::info!(clean = matches!(stopped, Ok(Ok(()))), "engine stopped");
		}
		runtime.shutdown_background();
		*HOST.lock_unpoisoned() = None;
	}

	/// The app became visible or hidden. Foreground attaches the client;
	/// background detaches it unless a call needs it.
	pub fn set_foreground(&self, foreground: bool) {
		self.foreground.store(foreground, Ordering::Release);
		self.hub.set_visible(foreground);
		self.shell.set_visible(foreground);
		if !foreground {
			// A process sent to the background never keeps a key down.
			self.cancel_talk();
		}
		if foreground {
			self.attach();
		} else if !self.in_call.load(Ordering::Acquire) {
			self.detach();
		}
	}

	/// The shell alerts incoming calls itself (`true`) or leaves the ring to the engine.
	/// The engine stays silent while only quiet clients are attached.
	pub fn set_quiet(&self, quiet: bool) {
		self.quiet_wanted.store(quiet, Ordering::Release);
		self.sync_quiet();
	}

	/// `background.available` changed. With it on the engine holds presence itself, so the
	/// quiet client is what keeps the flag alive while no window is open; with it off no
	/// client may stay attached for the flag.
	fn set_background(&self, on: bool) {
		self.background.store(on, Ordering::Release);
		self.sync_quiet();
	}

	fn sync_quiet(&self) {
		let wanted = self.quiet_wanted.load(Ordering::Acquire)
			&& self.background.load(Ordering::Acquire);
		let mut task = self.quiet_task.lock_unpoisoned();
		match (wanted, task.is_some()) {
			(true, false) => {
				*task = Some(self.handle.spawn(quiet::hold(self.endpoint.clone())));
			}
			(false, true) => {
				if let Some(running) = task.take() {
					running.abort();
				}
			}
			_ => {}
		}
	}

	/// The microphone permission is (or may now be) granted: a capture that
	/// failed to open while it was missing is tried again, in a running call
	/// too.
	pub fn microphone_available(self: &Arc<Self>) {
		let host = self.clone();
		self.handle.spawn(async move {
			if let Some(daemon) = host.daemon.lock().await.as_ref() {
				daemon.input_access_changed().await;
			}
		});
	}

	fn attach(&self) {
		let mut tasks = self.attached.lock_unpoisoned();
		if !tasks.is_empty() {
			return;
		}
		let shell: Arc<dyn Shell> = self.shell.clone();
		let endpoint = self.endpoint.clone();
		let engine = Arc::new(RelayLink::new({
			let endpoint = endpoint.clone();
			move || Ok(endpoint.clone())
		}));
		let msg_engine = Arc::new(MsgLink::new(move || Ok(endpoint.clone())));
		tasks.push(self.handle.spawn(messaging::supervisor::supervise(
			msg_engine,
			self.shared.messages.clone(),
			shell.clone(),
		)));
		tasks.push(self.handle.spawn(supervisor::supervise(
			engine,
			self.hub.clone(),
			self.slot.clone(),
			shell,
			self.shared.autostart.clone(),
		)));
		tracing::info!("client attached: the engine now advertises");
	}

	fn detach(&self) {
		let tasks: Vec<_> = self.attached.lock_unpoisoned().drain(..).collect();
		if tasks.is_empty() {
			return;
		}
		// The stream closes first: a held press must not outlive the client.
		self.ptt.cancel();
		tasks.iter().for_each(JoinHandle::abort);
		self.slot.clear();
		self.hub.clear_meters();
		self.hub.update(|s| s.set_link_down(Link::Offline, None));
		tracing::info!("client detached: the engine leaves the LAN after its debounce");
	}

	fn on_call(&self, summary: CallSummary) {
		self.in_call.store(summary.active, Ordering::Release);
		self.bridge.call_changed(summary.clone());
		if !summary.active {
			self.ptt.cancel();
		}
		if !summary.active && !self.foreground.load(Ordering::Acquire) {
			self.detach();
		}
	}

	pub fn mute(&self, muted: bool) {
		self.actions
			.run(|c| async move { c.set_global_controls(Some(muted), None, None, None).await });
	}

	pub fn hang_up(&self) {
		let Some(id) = self.active_call_id() else {
			return;
		};
		self.actions.run(|c| async move { c.end_call(&id).await });
	}

	pub fn answer(&self) {
		let Some(id) = self.active_call_id() else {
			return;
		};
		self.actions
			.run(|c| async move { c.accept_call(&id).await });
	}

	/// Hold-to-talk from the on-screen button.
	pub fn talk(&self, call_id: Option<String>, pressed: bool) {
		let Some(id) = call_id else {
			return;
		};
		if pressed {
			self.ptt.press(id);
		} else {
			self.ptt.release();
		}
	}

	/// The button went away without an up event (pause, focus or
	/// surface loss): release whatever is held.
	pub fn cancel_talk(&self) {
		self.ptt.cancel();
	}

	fn active_call_id(&self) -> Option<String> {
		self.hub.read(|s| {
			s.calls
				.iter()
				.find(|c| !c.phase.is_terminal())
				.map(|c| c.call_id.clone())
		})
	}

	/// Pushes a service the platform browser found; the engine applies the same
	/// address policy as for mDNS and treats it as untrusted.
	pub fn found(&self, service: FoundService) {
		let addresses = service
			.addresses
			.iter()
			.filter_map(|a| a.parse().ok())
			.collect();
		let record = PlatformRecord {
			fullname: service.fullname,
			port: service.port,
			addresses,
			txt: service.txt.into_iter().collect(),
		};
		self.hints.send(Hint::Found(record));
	}

	pub fn lost(&self, fullname: String) {
		self.hints.send(Hint::Lost(fullname));
	}
}

/// Tells Kotlin when audio streams are wanted (see `audio_session`).
struct BridgeSession(Arc<dyn HostBridge>);

impl AudioSession for BridgeSession {
	fn streams_wanted(&self, wanted: bool) {
		self.0.audio_streams_wanted(wanted);
	}
}

#[cfg(target_os = "android")]
fn audio_setup(bridge: Arc<dyn HostBridge>) -> AudioSetup {
	AudioSetup::Backend(Box::new(move || {
		let mut backend = crate::audio_session::SessionBackend::new(
			relay_audio::device::AAudioBackend::new(),
			Arc::new(BridgeSession(bridge)),
		);
		// The first output of a process is the slow one; pay for it now, not at the first ring.
		backend.warm();
		Box::new(backend)
	}))
}

#[cfg(not(target_os = "android"))]
fn audio_setup(_bridge: Arc<dyn HostBridge>) -> AudioSetup {
	AudioSetup::Cpal
}

/// Wraps the UI sink and reports call changes to Kotlin (foreground service).
struct CallSink {
	inner: ShellSink,
	host: Weak<Host>,
	last: Mutex<CallSummary>,
	background: Mutex<Option<(bool, &'static str)>>,
}

/// What the service notification needs from the state.
pub fn summarize(state: &AppState) -> CallSummary {
	let Some(call) = state.calls.iter().find(|c| !c.phase.is_terminal()) else {
		return CallSummary::idle();
	};
	let global = state.settings.as_ref().is_some_and(|s| s.global_muted);
	CallSummary {
		active: true,
		incoming: call.phase == relay_ui::model::Phase::IncomingRinging,
		muted: call.user_muted || global,
		peer: state
			.peers
			.iter()
			.find(|p| p.peer_id == call.peer_id)
			.map(|p| p.display_name.clone())
			.unwrap_or_default(),
		auto_answered: call.auto_answered,
	}
}

/// The two settings Kotlin acts on: `background.available` and `calls.alert`.
pub fn background_of(state: &AppState) -> Option<(bool, &'static str)> {
	state
		.settings
		.as_ref()
		.map(|s| (s.background_available, s.call_alert.key()))
}

impl Sink for CallSink {
	fn state(&self, state: &AppState) {
		self.inner.state(state);
		let summary = summarize(state);
		let changed = {
			let mut last = self.last.lock_unpoisoned();
			let changed = *last != summary;
			if changed {
				*last = summary.clone();
			}
			changed
		};
		let host = self.host.upgrade();
		if let (true, Some(host)) = (changed, host.as_ref()) {
			host.on_call(summary);
		}
		let background = background_of(state);
		if let (Some((available, alert)), Some(host)) = (background, host) {
			let changed = {
				let mut last = self.background.lock_unpoisoned();
				let changed = *last != background;
				*last = background;
				changed
			};
			if changed {
				host.set_background(available);
				host.bridge.background_changed(available, alert.to_owned());
			}
		}
	}

	fn meters(&self, meters: &Meters) {
		self.inner.meters(meters);
	}

	fn tray(&self, summary: &TraySummary) {
		self.inner.tray(summary);
	}
}

#[cfg(test)]
mod ptt_tests;
#[cfg(test)]
mod tests;
