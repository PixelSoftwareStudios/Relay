//! What the shell can do, read once per frame, and the Settings categories
//! that follow from it. Platform neutral: only capability flags, never a
//! platform name.

use crate::shell::Shell;
use crate::tr;

/// The shell's capability flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
	/// Desktop switches (language, notifications, start at login) exist.
	pub host_options: bool,
	/// Closing the window hides it to a tray.
	pub hides_to_tray: bool,
	/// Leaving the app keeps it and its calls running without a window.
	pub runs_in_background: bool,
	/// The shell registers a system-wide push-to-talk key.
	pub global_hotkey: bool,
	/// The UI shows an on-screen hold-to-talk button.
	pub talk_button: bool,
	/// The shell can start and stop the standalone daemon.
	pub manage_daemon: bool,
	/// The app can update itself (desktop): Settings shows the Updates group.
	pub self_update: bool,
}

impl Capabilities {
	pub fn of(shell: &dyn Shell) -> Self {
		Self {
			host_options: shell.host_options().is_some(),
			hides_to_tray: shell.hides_to_tray(),
			runs_in_background: shell.runs_in_background(),
			global_hotkey: shell.has_global_hotkey(),
			talk_button: shell.has_talk_button(),
			manage_daemon: shell.can_manage_daemon(),
			self_update: shell.can_self_update(),
		}
	}

	/// Whether "Hide window" and "Quit this app" make sense: a window the user
	/// closes (with or without a tray). Where the system keeps the app alive on
	/// its own there is nothing to quit.
	pub fn window_controls(self) -> bool {
		self.hides_to_tray || !self.runs_in_background
	}
}

/// A page of Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsCategory {
	Profile,
	Audio,
	PushToTalk,
	Calls,
	Privacy,
	Limits,
	Language,
	Startup,
	Advanced,
}

impl SettingsCategory {
	pub const ALL: [SettingsCategory; 9] = [
		Self::Profile,
		Self::Audio,
		Self::PushToTalk,
		Self::Calls,
		Self::Privacy,
		Self::Limits,
		Self::Language,
		Self::Startup,
		Self::Advanced,
	];

	pub fn label(self) -> &'static str {
		match self {
			Self::Profile => tr!("settings-profile"),
			Self::Audio => tr!("settings-audio"),
			Self::PushToTalk => tr!("settings-ptt"),
			Self::Calls => tr!("settings-calls"),
			Self::Privacy => tr!("settings-privacy"),
			Self::Limits => tr!("settings-limits"),
			Self::Language => tr!("settings-language"),
			Self::Startup => tr!("settings-startup"),
			Self::Advanced => tr!("settings-advanced"),
		}
	}

	/// One line under the label in the category list.
	pub fn summary(self) -> &'static str {
		match self {
			Self::Profile => tr!("settings-profile-sub"),
			Self::Audio => tr!("settings-audio-sub"),
			Self::PushToTalk => tr!("settings-ptt-sub"),
			Self::Calls => tr!("settings-calls-sub"),
			Self::Privacy => tr!("settings-privacy-sub"),
			Self::Limits => tr!("settings-limits-sub"),
			Self::Language => tr!("settings-language-sub"),
			Self::Startup => tr!("settings-startup-sub"),
			Self::Advanced => tr!("settings-advanced-sub"),
		}
	}

	/// Whether this shell has anything to show in the category.
	pub fn available(self, caps: Capabilities) -> bool {
		match self {
			Self::Language => caps.host_options,
			Self::Startup => caps.host_options || caps.window_controls() || caps.self_update,
			_ => true,
		}
	}

	/// The categories this shell shows, in order.
	pub fn visible(caps: Capabilities) -> Vec<Self> {
		Self::ALL
			.into_iter()
			.filter(|c| c.available(caps))
			.collect()
	}
}
