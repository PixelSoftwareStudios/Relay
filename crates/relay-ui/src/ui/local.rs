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

#[derive(Default)]
pub struct Local {
	pub name_input: String,
	pub name_seeded: bool,
	pub consent: HashMap<String, bool>,
	pub gain_edits: HashMap<String, Option<f32>>,
	/// In-flight drag of the push-to-talk release delay slider.
	pub release_delay_edit: Option<u32>,
	/// In-flight drag of the ringtone volume slider.
	pub ring_volume_edit: Option<f32>,
	pub revoke_confirm: Option<(String, String)>,
	pub leave_confirm: Option<String>,
	/// The "Quit daemon" confirmation is showing.
	pub confirm_quit_daemon: bool,
	/// The "Quit this app" confirmation (calls are live) is showing.
	pub confirm_quit_app: bool,

	pub connection: Option<Result<ConnectionView, String>>,
	pub connection_requested: bool,
	pub qr: Option<(String, TextureHandle)>,
	pub paste: String,

	pub picked: Option<PickedImage>,

	/// A call card asked to show that call's messages (hex call id).
	/// The call whose on-screen talk button is held, and whether the button was
	/// drawn this frame (a held button that vanishes counts as released).
	pub talking: Option<String>,
	pub talk_drawn: bool,
	pub open_messages: Option<String>,

	pub hotkey: HotkeyPanel,

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
