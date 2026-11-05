//! Options only a desktop shell can offer: notifying about incoming calls while the window
//! is out of sight, and starting at login. The shell owns the state (the notification
//! switch is stored by the app, the login item lives in the OS); the UI only draws the
//! switches and the honest status text.

/// Whether this app starts when the user logs in, as the OS reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginStatus {
	/// This platform or build cannot register a login item (the reason is shown).
	Unsupported(String),
	/// Not registered.
	Off,
	/// Registered with the OS.
	On,
	/// The OS could not be asked or refused the change (the reason is shown).
	Failed(String),
}

/// Implemented by the desktop shell. Methods must be cheap (the UI calls them per frame) and
/// never block.
pub trait HostOptions: Send + Sync {
	/// The "Notify on incoming calls" switch (default on).
	fn notify_incoming(&self) -> bool;
	/// Change the switch; the shell stores it.
	fn set_notify_incoming(&self, on: bool);
	/// The last notification the system refused, if any, as the OS worded it.
	fn notify_failure(&self) -> Option<String>;
	/// The login item as last read from the OS.
	fn login_status(&self) -> LoginStatus;
	/// Register or remove the login item. `Err` carries the OS's reason.
	fn set_launch_at_login(&self, on: bool) -> Result<(), String>;
}

/// What the "Notify on incoming calls" switch really does.
pub fn notify_text(on: bool, failure: Option<&str>) -> String {
	match (on, failure) {
		(false, _) => "Off: a call that rings while this window is hidden or in the background \
			is only shown in the tray tooltip and heard from the daemon's ringtone."
			.to_owned(),
		(true, None) => "Shows the caller's name (never message text) when a call rings while \
			this window is hidden or not in front."
			.to_owned(),
		(true, Some(reason)) => format!(
			"The system refused the last notification ({reason}). Calls still ring through the \
			 daemon and show in this window and the tray tooltip."
		),
	}
}

/// What the "Start at login" switch really does.
pub fn login_text(status: &LoginStatus) -> String {
	match status {
		LoginStatus::Unsupported(reason) => format!("Not available here: {reason}"),
		LoginStatus::Off => {
			"Off. When on, this app is registered with the system and opens its window at login. \
			 The daemon is not started until the app needs it."
				.to_owned()
		}
		LoginStatus::On => {
			"On: the system starts this app when you log in (it opens its window). Turn it off \
			 here to remove the entry."
				.to_owned()
		}
		LoginStatus::Failed(reason) => format!("Could not read or change the login item: {reason}"),
	}
}
