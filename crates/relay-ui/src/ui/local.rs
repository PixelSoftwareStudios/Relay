//! UI-only state that never leaves the window. Deliberately not `Debug`: it
//! can hold passwords and invite tokens while a form is open.

use std::collections::HashMap;

use egui::TextureHandle;

use crate::hotkey_settings::HotkeyPanel;
use crate::inbox::Invite;
use crate::model::ConnectionView;
use crate::tr;

#[derive(Default)]
pub struct NewChannel {
	pub name: String,
	pub listed: bool,
	pub password_required: bool,
	pub invite_only: bool,
	pub password: String,
}

/// A chosen profile image waiting for the user to confirm the upload.
pub struct PickedImage {
	pub bytes: Vec<u8>,
	pub media_type: &'static str,
	pub texture: TextureHandle,
}

/// The call whose chat is open.
#[derive(Clone)]
pub struct ChatTarget {
	/// Hex call id.
	pub call_id: String,
	/// Who the call was with, for the heading.
	pub name: String,
	/// The call is over and comes from the history: the chat is read only.
	pub over: bool,
}

#[derive(Default)]
pub struct Local {
	pub name_input: String,
	pub name_seeded: bool,
	pub gain_edits: HashMap<String, Option<f32>>,
	/// In-flight drag of the push-to-talk release delay slider.
	pub release_delay_edit: Option<u32>,
	/// In-flight drag of the ringtone volume slider.
	pub ring_volume_edit: Option<f32>,
	/// A ringtone file is being chosen or prepared (shared with the task that clears it).
	pub ringtone_busy: std::sync::Arc<std::sync::atomic::AtomicBool>,
	pub revoke_confirm: Option<(String, String)>,
	pub leave_confirm: Option<String>,
	/// The "Quit daemon" confirmation is showing.
	pub confirm_quit_daemon: bool,
	/// The "Reset Relay data" confirmation is showing.
	pub confirm_reset_data: bool,
	/// The "Quit this app" confirmation (calls are live) is showing.
	pub confirm_quit_app: bool,

	pub connection: Option<Result<ConnectionView, String>>,
	pub connection_requested: bool,
	pub qr: Option<(String, TextureHandle)>,
	pub paste: String,
	/// A QR scan is open (the Scan button waits until it ends).
	pub scanning: bool,
	/// What the last scan said; cleared when the section is left.
	pub scan_note: Option<crate::qr_scan::ScanNote>,
	/// Where "connect and trust" after a scan stands; cleared by the next scan.
	pub scan_trust: Option<crate::qr_scan::ScanTrust>,
	/// The user's choice for the "Connect a device" section; `None` follows the default.
	pub connect_open: Option<bool>,

	pub picked: Option<PickedImage>,

	/// The call whose on-screen talk button is held, and whether the button was
	/// drawn this frame (a held button that vanishes counts as released).
	pub talking: Option<String>,
	pub talk_drawn: bool,
	/// The chat shown beside the call it belongs to; at most one.
	pub chat: Option<ChatTarget>,
	/// Calls placed here that have not been answered yet; the one that connects pulls the
	/// user to Calls, once.
	pub awaiting_answer: std::collections::HashSet<String>,
	/// A page asked to go to another page (an empty state's button).
	pub goto: Option<crate::layout::Page>,
	/// A control asked to open this Settings category.
	pub goto_settings: Option<crate::capabilities::SettingsCategory>,
	/// The first-run background question was answered during this run.
	pub first_run_closed: bool,
	/// The contact (hex peer id) the Contacts page should scroll to and show.
	pub focus_contact: Option<String>,
	/// The microphone test of Settings, Audio.
	pub mic_test: crate::mic_test::MicTest,
	/// The device (hex peer id) whose "Forget" is waiting for confirmation.
	pub forget_confirm: Option<String>,
	/// The device (hex peer id) whose "Block" is waiting for confirmation.
	pub block_confirm: Option<String>,
	/// The contact (hex peer id) whose "Answer automatically" awaits confirmation.
	pub auto_answer_confirm: Option<String>,
	/// The key-changed contact (hex peer id) whose "Compare and trust new key" step is open,
	/// and the fingerprint typed or pasted so far.
	pub verify_key: Option<String>,
	pub verify_text: String,
	/// The found device (hex peer id) whose "Trust this device" awaits confirmation.
	pub trust_confirm: Option<String>,
	/// "Remove picture" is waiting for confirmation.
	pub confirm_remove_avatar: bool,

	pub hotkey: HotkeyPanel,

	/// The microphone card was dismissed for the current access state.
	pub microphone: crate::microphone::Dismissal,
	/// Dismissed and fading notices of the strip above the pages.
	pub notices: crate::notices::Board,

	/// The call whose connection details are open, and whether they were drawn this frame
	/// (a panel that is not drawn closes its statistics stream).
	pub details_call: Option<String>,
	pub details_drawn: bool,
	/// The call whose volume popup was asked for from the menu.
	pub volume_ask: Option<String>,
	/// Single-key saves in flight and the engine's last answer for each key.
	pub setting_saving: std::collections::HashSet<&'static str>,
	pub setting_reports: HashMap<&'static str, Result<Vec<crate::engine::SettingResult>, String>>,
	/// How many call history rows the Calls page shows.
	pub history_shown: usize,
	/// "Clear history" is waiting for confirmation.
	pub history_confirm_clear: bool,
	/// The engine's answer to the last "Keep call history" change, or why it failed.
	pub history_report: Option<Result<Vec<crate::engine::SettingResult>, String>>,
	/// A "Keep call history" change is in flight.
	pub history_saving: bool,

	pub new_channel: NewChannel,
	pub join_target: String,
	pub join_password: String,
	pub invite_channel: Option<String>,
	pub invite_ttl: usize,
	pub invite_uses: usize,
	pub invite: Option<Invite>,
}

/// Invite lifetimes offered in the UI: label and seconds.
pub fn invite_ttls() -> [(&'static str, u32); 3] {
	[
		(tr!("invite-ttl-hour"), 3600),
		(tr!("invite-ttl-day"), 86_400),
		(tr!("invite-ttl-week"), 604_800),
	]
}
/// Invite use counts offered in the UI.
pub const INVITE_USES: [u32; 3] = [1, 5, 25];
