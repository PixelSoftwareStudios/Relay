//! The Android implementation of `relay_ui::shell::Shell`.
//!
//! Everything platform-specific is a call into the Kotlin [`HostBridge`]; the
//! state Kotlin pushes (visibility, insets, keyboard) is held in atomics so the
//! UI thread reads it without blocking.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use relay_io::LockExt;
use relay_ui::imaging::avatar_check;
use relay_ui::qr_scan::ScanError;
use relay_ui::shell::{BoxFuture, FilePicker, Insets, PickedFile, Shell};
use tokio::sync::oneshot;

use crate::back::{BackGate, WAIT};
use crate::bridge::{BackgroundStatus, HostBridge, InsetsPx, MicrophoneState, PickKind, PickOutcome, ScanOutcome};
use relay_ui::microphone::MicrophoneAccess;

type Pending = HashMap<u64, oneshot::Sender<PickOutcome>>;

/// Floats that a thread may read while another writes.
#[derive(Default)]
struct AtomicF32(AtomicU32);

impl AtomicF32 {
	fn set(&self, value: f32) {
		self.0.store(value.to_bits(), Ordering::Relaxed);
	}

	fn get(&self) -> f32 {
		f32::from_bits(self.0.load(Ordering::Relaxed))
	}
}

/// 0 unknown, 1 yes, 2 no.
fn tri(value: u8) -> Option<bool> {
	match value {
		1 => Some(true),
		2 => Some(false),
		_ => None,
	}
}

pub struct AndroidShell {
	bridge: Arc<dyn HostBridge>,
	ctx: Mutex<Option<egui::Context>>,
	visible: AtomicBool,
	top: AtomicF32,
	bottom: AtomicF32,
	left: AtomicF32,
	right: AtomicF32,
	keyboard: AtomicF32,
	microphone: AtomicU8,
	/// 0 unknown, 1 exempt, 2 restricted.
	battery: AtomicU8,
	/// 0 unknown, 1 allowed, 2 blocked.
	notifications: AtomicU8,
	vibrator: AtomicBool,
	/// A [`BackgroundStatus`] discriminant.
	background: AtomicU8,
	/// One-time questions already answered (cached from Kotlin on first ask).
	questions: Mutex<HashMap<String, bool>>,
	next_pick: AtomicU64,
	pending: Mutex<Pending>,
	scanner: AtomicBool,
	scans: Mutex<HashMap<u64, oneshot::Sender<ScanOutcome>>>,
	/// Back requests waiting for the render thread.
	pub back: BackGate,
}

impl AndroidShell {
	pub fn new(bridge: Arc<dyn HostBridge>) -> Self {
		Self {
			bridge,
			ctx: Mutex::new(None),
			visible: AtomicBool::new(false),
			top: AtomicF32::default(),
			bottom: AtomicF32::default(),
			left: AtomicF32::default(),
			right: AtomicF32::default(),
			keyboard: AtomicF32::default(),
			microphone: AtomicU8::new(MicrophoneState::Ready as u8),
			battery: AtomicU8::new(0),
			notifications: AtomicU8::new(0),
			vibrator: AtomicBool::new(false),
			background: AtomicU8::new(BackgroundStatus::None as u8),
			questions: Mutex::new(HashMap::new()),
			next_pick: AtomicU64::new(1),
			pending: Mutex::new(Pending::new()),
			scanner: AtomicBool::new(false),
			scans: Mutex::new(HashMap::new()),
			back: BackGate::default(),
		}
	}

	/// The activity's egui context exists only while the activity does.
	pub fn set_context(&self, ctx: Option<egui::Context>) {
		*self.ctx.lock_unpoisoned() = ctx;
	}

	/// The system Back action: true when the UI used it (closed something or
	/// went up a level), false when the app should leave to the background.
	pub fn handle_back(&self) -> bool {
		self.back.ask(|| self.request_repaint(), WAIT)
	}

	pub fn set_visible(&self, visible: bool) {
		self.visible.store(visible, Ordering::Release);
		self.request_repaint();
	}

	pub fn set_insets(&self, insets: InsetsPx) {
		self.top.set(insets.top);
		self.bottom.set(insets.bottom);
		self.left.set(insets.left);
		self.right.set(insets.right);
		self.keyboard.set(insets.keyboard);
		self.request_repaint();
	}

	pub fn set_microphone(&self, state: MicrophoneState) {
		self.microphone.store(state as u8, Ordering::Release);
		self.request_repaint();
	}

	pub fn set_battery_exempt(&self, exempt: bool) {
		self.battery.store(if exempt { 1 } else { 2 }, Ordering::Release);
		self.request_repaint();
	}

	pub fn set_notifications_allowed(&self, allowed: bool) {
		self.notifications
			.store(if allowed { 1 } else { 2 }, Ordering::Release);
		self.request_repaint();
	}

	pub fn set_background_status(&self, status: BackgroundStatus) {
		self.background.store(status as u8, Ordering::Release);
		self.request_repaint();
	}

	pub fn set_can_vibrate(&self, can: bool) {
		self.vibrator.store(can, Ordering::Release);
		self.request_repaint();
	}

	/// Kotlin's answer to [`HostBridge::pick`]. Unknown ids are ignored.
	pub fn complete_pick(&self, request_id: u64, outcome: PickOutcome) {
		if let Some(waiting) = self.pending.lock_unpoisoned().remove(&request_id) {
			let _ = waiting.send(outcome);
		}
	}

	pub fn set_scanner(&self, available: bool) {
		self.scanner.store(available, Ordering::Release);
		self.request_repaint();
	}

	/// Kotlin's answer to [`HostBridge::scan_qr`]. Unknown ids are ignored.
	pub fn complete_scan(&self, request_id: u64, outcome: ScanOutcome) {
		if let Some(waiting) = self.scans.lock_unpoisoned().remove(&request_id) {
			let _ = waiting.send(outcome);
		}
	}

	fn points(&self, pixels: f32) -> f32 {
		let scale = self
			.ctx
			.lock_unpoisoned()
			.as_ref()
			.map_or(1.0, egui::Context::pixels_per_point);
		pixels / scale.max(0.1)
	}

	async fn pick(&self, kind: PickKind) -> PickOutcome {
		let id = self.next_pick.fetch_add(1, Ordering::Relaxed);
		let (tx, rx) = oneshot::channel();
		self.pending.lock_unpoisoned().insert(id, tx);
		self.bridge.pick(id, kind);
		match rx.await {
			Ok(outcome) => outcome,
			Err(_) => PickOutcome::Cancelled,
		}
	}
}

impl FilePicker for AndroidShell {
	fn pick_image(&self) -> BoxFuture<'_, Result<Option<PickedFile>, String>> {
		Box::pin(async {
			match self.pick(PickKind::Image).await {
				PickOutcome::Image { name, bytes } => {
					// Kotlin checked the size before reading; the format comes from the bytes.
					avatar_check(&bytes)?;
					Ok(Some(PickedFile { name, bytes }))
				}
				PickOutcome::Cancelled => Ok(None),
				PickOutcome::Failed { reason } => Err(reason),
				PickOutcome::File { .. } | PickOutcome::Audio { .. } => {
					Err("unexpected file result".to_owned())
				}
			}
		})
	}

	/// The document picker returns content URIs, not paths. Kotlin copies the
	/// chosen file into the app cache and the engine reads that copy.
	fn pick_file(&self, _title: &str) -> BoxFuture<'_, Result<Option<std::path::PathBuf>, String>> {
		Box::pin(async {
			match self.pick(PickKind::File).await {
				PickOutcome::File { path } => Ok(Some(path.into())),
				PickOutcome::Cancelled => Ok(None),
				PickOutcome::Failed { reason } => Err(reason),
				PickOutcome::Image { .. } | PickOutcome::Audio { .. } => {
					Err("unexpected image result".to_owned())
				}
			}
		})
	}

	/// The system document picker for an audio file (`ACTION_OPEN_DOCUMENT`, no storage
	/// permission). Kotlin reads at most `ringtone_pick::PICK_MAX_BYTES`; the engine decodes.
	fn pick_audio(&self) -> BoxFuture<'_, Result<Option<PickedFile>, String>> {
		Box::pin(async {
			match self.pick(PickKind::Audio).await {
				PickOutcome::Audio { name, bytes } => Ok(Some(PickedFile { name, bytes })),
				PickOutcome::Cancelled => Ok(None),
				PickOutcome::Failed { reason } => Err(reason),
				PickOutcome::Image { .. } | PickOutcome::File { .. } => {
					Err("unexpected picker result".to_owned())
				}
			}
		})
	}
}

impl Shell for AndroidShell {
	fn open_url(&self, url: &str) {
		self.bridge.open_url(url.to_owned());
	}

	fn copy_text(&self, text: &str) {
		self.bridge.copy_text(text.to_owned());
	}

	fn request_repaint(&self) {
		if let Some(ctx) = self.ctx.lock_unpoisoned().as_ref() {
			ctx.request_repaint();
		}
	}

	fn window_visible(&self) -> bool {
		self.visible.load(Ordering::Acquire)
	}

	fn show_window(&self) {}

	fn hide_window(&self) {}

	fn quit(&self) {
		self.bridge.quit_app();
	}

	fn hides_to_tray(&self) -> bool {
		false
	}

	fn safe_area_insets(&self) -> Insets {
		Insets {
			top: self.points(self.top.get()),
			bottom: self.points(self.bottom.get()),
			left: self.points(self.left.get()),
			right: self.points(self.right.get()),
		}
	}

	fn keyboard_height(&self) -> f32 {
		self.points(self.keyboard.get())
	}

	fn has_global_hotkey(&self) -> bool {
		false
	}

	/// The foreground service keeps a call alive without the activity.
	fn runs_in_background(&self) -> bool {
		true
	}

	fn has_talk_button(&self) -> bool {
		true
	}

	fn microphone_access(&self) -> MicrophoneAccess {
		match self.microphone.load(Ordering::Acquire) {
			x if x == MicrophoneState::NotAsked as u8 => MicrophoneAccess::NotAsked,
			x if x == MicrophoneState::Denied as u8 => MicrophoneAccess::Denied,
			x if x == MicrophoneState::Blocked as u8 => MicrophoneAccess::Blocked,
			_ => MicrophoneAccess::Ready,
		}
	}

	fn request_microphone(&self) {
		self.bridge.request_microphone();
	}

	fn open_microphone_settings(&self) {
		self.bridge.open_app_settings();
	}

	fn battery_optimization_exempt(&self) -> Option<bool> {
		tri(self.battery.load(Ordering::Acquire))
	}

	fn request_battery_exemption(&self) {
		self.bridge.request_battery_exemption();
	}

	fn background_state(&self) -> Option<relay_ui::background_settings::BackgroundState> {
		use relay_ui::background_settings::BackgroundState as State;
		match self.background.load(Ordering::Acquire) {
			x if x == BackgroundStatus::Running as u8 => Some(State::Running),
			x if x == BackgroundStatus::WaitingForNetwork as u8 => Some(State::WaitingForNetwork),
			x if x == BackgroundStatus::Paused as u8 => Some(State::Paused),
			_ => None,
		}
	}

	fn question_answered(&self, id: &str) -> bool {
		if let Some(known) = self.questions.lock_unpoisoned().get(id) {
			return *known;
		}
		// Asked once per id; Kotlin reads SharedPreferences, the answer is cached.
		let answered = self.bridge.question_answered(id.to_owned());
		self.questions.lock_unpoisoned().insert(id.to_owned(), answered);
		answered
	}

	fn set_question_answered(&self, id: &str) {
		self.questions.lock_unpoisoned().insert(id.to_owned(), true);
		self.bridge.set_question_answered(id.to_owned());
	}

	fn can_vibrate(&self) -> bool {
		self.vibrator.load(Ordering::Acquire)
	}

	fn notifications_allowed(&self) -> Option<bool> {
		tri(self.notifications.load(Ordering::Acquire))
	}

	fn request_notifications(&self) {
		self.bridge.request_notifications();
	}

	fn can_scan_qr(&self) -> bool {
		self.scanner.load(Ordering::Acquire)
	}

	fn scan_qr(&self) -> BoxFuture<'_, Result<Option<String>, ScanError>> {
		Box::pin(async {
			let id = self.next_pick.fetch_add(1, Ordering::Relaxed);
			let (tx, rx) = oneshot::channel();
			self.scans.lock_unpoisoned().insert(id, tx);
			self.bridge.scan_qr(id);
			match rx.await {
				Ok(ScanOutcome::Text { text }) => Ok(Some(text)),
				Ok(ScanOutcome::Cancelled) | Err(_) => Ok(None),
				Ok(ScanOutcome::CameraDenied) => Err(ScanError::CameraDenied),
				Ok(ScanOutcome::CameraBlocked) => Err(ScanError::CameraBlocked),
				Ok(ScanOutcome::Unavailable) => Err(ScanError::Unavailable),
				Ok(ScanOutcome::Failed { reason }) => {
					tracing::warn!(%reason, "QR scan failed");
					Err(ScanError::Failed)
				}
			}
		})
	}

	fn open_camera_settings(&self) {
		self.bridge.open_app_settings();
	}

	fn push_to_talk(&self, call_id: &str, pressed: bool) {
		if let Some(host) = crate::host::host() {
			host.talk(Some(call_id.to_owned()), pressed);
		}
	}
}
