//! UI-only state that never leaves the window. Deliberately not `Debug`: it
//! can hold passwords and invite tokens while a form is open.

use std::collections::HashMap;

use eframe::egui::TextureHandle;

use crate::inbox::Invite;
use crate::model::ConnectionView;

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
	pub revoke_confirm: Option<(String, String)>,
	pub leave_confirm: Option<String>,

	pub connection: Option<Result<ConnectionView, String>>,
	pub connection_requested: bool,
	pub qr: Option<(String, TextureHandle)>,
	pub paste: String,

	pub picked: Option<PickedImage>,

	pub new_channel: NewChannel,
	pub join_target: String,
	pub join_password: String,
	pub invite_channel: Option<String>,
	pub invite_ttl: usize,
	pub invite_uses: usize,
	pub invite: Option<Invite>,
}

/// Invite lifetimes offered in the UI: label and seconds.
pub const INVITE_TTLS: [(&str, u32); 3] =
	[("1 hour", 3600), ("1 day", 86_400), ("7 days", 604_800)];
/// Invite use counts offered in the UI.
pub const INVITE_USES: [u32; 3] = [1, 5, 25];
