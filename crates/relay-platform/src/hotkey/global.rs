//! macOS, Windows and X11 hotkeys through `global-hotkey`.
//!
//! The crate has event-loop rules we must honour:
//! - Windows: a win32 message loop must run on the thread that created the manager,
//!   so a dedicated worker thread creates it and pumps messages.
//! - X11: the crate runs its own grab thread; our worker only forwards events.
//! - macOS: the manager must be created on the main thread and the main run loop
//!   must be pumped by the host, see [`run_main_loop`].

use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tokio::sync::mpsc;

use super::{BackendStatus, HotkeyBackend, HotkeyBinding, HotkeyError, HotkeyEvent, HotkeyStream};

const POLL: Duration = Duration::from_millis(10);

/// Backend over `global-hotkey` for macOS, Windows and X11.
#[derive(Default)]
pub struct GlobalHotkeyBackend {
	running: Option<Running>,
}

struct Running {
	stop: Arc<AtomicBool>,
	thread: Option<JoinHandle<()>>,
	/// macOS keeps the manager on the main thread; elsewhere the worker owns it.
	#[cfg(target_os = "macos")]
	manager: Option<(GlobalHotKeyManager, HotKey)>,
}

impl GlobalHotkeyBackend {
	/// An idle backend.
	pub fn new() -> Self {
		Self::default()
	}
}

/// True when the session is Wayland, where X11-style grabs never see global keys.
#[cfg(target_os = "linux")]
pub(super) fn is_wayland_session() -> bool {
	let set = |name| std::env::var_os(name).is_some_and(|v| !v.is_empty());
	set("WAYLAND_DISPLAY") || std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v == "wayland")
}

impl HotkeyBackend for GlobalHotkeyBackend {
	fn name(&self) -> &'static str {
		"global-hotkey"
	}

	fn probe(&self) -> BackendStatus {
		probe_platform()
	}

	fn start(&mut self, binding: &HotkeyBinding) -> Result<HotkeyStream, HotkeyError> {
		if self.running.is_some() {
			return Err(HotkeyError::AlreadyStarted);
		}
		let status = self.probe();
		if !status.is_available() {
			return Err(HotkeyError::Unavailable(status));
		}
		let hotkey = HotKey::from_str(&binding.accelerator)
			.map_err(|e| HotkeyError::InvalidBinding(binding.accelerator.clone(), e.to_string()))?;
		let (tx, rx) = mpsc::unbounded_channel();
		self.running = Some(spawn(hotkey, tx)?);
		Ok(rx)
	}

	fn stop(&mut self) {
		if let Some(mut running) = self.running.take() {
			running.stop.store(true, Ordering::SeqCst);
			#[cfg(target_os = "macos")]
			if let Some((manager, hotkey)) = running.manager.take() {
				let _ = manager.unregister(hotkey);
			}
			if let Some(thread) = running.thread.take() {
				let _ = thread.join();
			}
		}
	}
}

impl Drop for GlobalHotkeyBackend {
	fn drop(&mut self) {
		self.stop();
	}
}

#[cfg(target_os = "macos")]
fn probe_platform() -> BackendStatus {
	if !super::macos::input_monitoring_granted() {
		return BackendStatus::NeedsPermission(super::PermissionKind::InputMonitoring);
	}
	BackendStatus::Available
}

#[cfg(target_os = "windows")]
fn probe_platform() -> BackendStatus {
	BackendStatus::Available
}

#[cfg(target_os = "linux")]
fn probe_platform() -> BackendStatus {
	if is_wayland_session() {
		return BackendStatus::Unsupported(
			"Wayland session: X11 grabs do not see global keys; use the portal backend".into(),
		);
	}
	if std::env::var_os("DISPLAY").is_none_or(|v| v.is_empty()) {
		return BackendStatus::Unsupported("no X11 display (DISPLAY is unset)".into());
	}
	BackendStatus::Available
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn probe_platform() -> BackendStatus {
	BackendStatus::Unsupported("no global hotkey support on this OS".into())
}

/// Forward edges for `hotkey` until `stop` is set or the receiver is gone.
fn forward(hotkey: HotKey, tx: &mpsc::UnboundedSender<HotkeyEvent>, stop: &AtomicBool) {
	let events = GlobalHotKeyEvent::receiver();
	while !stop.load(Ordering::SeqCst) && !tx.is_closed() {
		pump_platform();
		let Ok(event) = events.recv_timeout(POLL) else {
			continue;
		};
		if event.id() != hotkey.id() {
			continue;
		}
		let edge = match event.state() {
			HotKeyState::Pressed => HotkeyEvent::Pressed,
			HotKeyState::Released => HotkeyEvent::Released,
		};
		if tx.send(edge).is_err() {
			break;
		}
	}
}

#[cfg(target_os = "macos")]
fn spawn(hotkey: HotKey, tx: mpsc::UnboundedSender<HotkeyEvent>) -> Result<Running, HotkeyError> {
	if !super::macos::is_main_thread() {
		return Err(HotkeyError::NotMainThread);
	}
	let manager =
		GlobalHotKeyManager::new().map_err(|e| HotkeyError::Registration(e.to_string()))?;
	manager
		.register(hotkey)
		.map_err(|e| HotkeyError::Registration(e.to_string()))?;
	let stop = Arc::new(AtomicBool::new(false));
	let flag = stop.clone();
	let thread = std::thread::Builder::new()
		.name("relay-hotkey".into())
		.spawn(move || forward(hotkey, &tx, &flag))
		.map_err(|e| HotkeyError::Registration(e.to_string()))?;
	Ok(Running {
		stop,
		thread: Some(thread),
		manager: Some((manager, hotkey)),
	})
}

#[cfg(not(target_os = "macos"))]
fn spawn(hotkey: HotKey, tx: mpsc::UnboundedSender<HotkeyEvent>) -> Result<Running, HotkeyError> {
	let stop = Arc::new(AtomicBool::new(false));
	let flag = stop.clone();
	let (ready_tx, ready_rx) = std::sync::mpsc::channel();
	let thread = std::thread::Builder::new()
		.name("relay-hotkey".into())
		.spawn(move || {
			// The manager must live on the thread that pumps the platform loop.
			let registered = GlobalHotKeyManager::new()
				.and_then(|manager| manager.register(hotkey).map(|()| manager));
			match registered {
				Ok(manager) => {
					let _ = ready_tx.send(Ok(()));
					forward(hotkey, &tx, &flag);
					let _ = manager.unregister(hotkey);
				}
				Err(e) => {
					let _ = ready_tx.send(Err(e.to_string()));
				}
			}
		})
		.map_err(|e| HotkeyError::Registration(e.to_string()))?;
	match ready_rx.recv() {
		Ok(Ok(())) => Ok(Running {
			stop,
			thread: Some(thread),
		}),
		Ok(Err(reason)) => Err(HotkeyError::Registration(reason)),
		Err(_) => Err(HotkeyError::Registration("hotkey thread died".into())),
	}
}

#[cfg(target_os = "windows")]
fn pump_platform() {
	use windows_sys::Win32::UI::WindowsAndMessaging::{
		DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
	};
	// SAFETY: MSG is plain data for which all-zero is valid, and the calls only touch
	// the calling thread's own message queue.
	unsafe {
		let mut msg: MSG = std::mem::zeroed();
		while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
			TranslateMessage(&msg);
			DispatchMessageW(&msg);
		}
	}
}

#[cfg(not(target_os = "windows"))]
fn pump_platform() {}

/// Run the host's main-thread loop until `keep_going` returns false.
///
/// On macOS this services the main run loop, which `global-hotkey` needs to deliver
/// events; call it from the process main thread and run async work on other threads.
/// Elsewhere it only sleeps, so it is safe to call unconditionally.
pub fn run_main_loop(mut keep_going: impl FnMut() -> bool) {
	while keep_going() {
		#[cfg(target_os = "macos")]
		super::macos::pump_run_loop(POLL);
		#[cfg(not(target_os = "macos"))]
		std::thread::sleep(POLL);
	}
}
