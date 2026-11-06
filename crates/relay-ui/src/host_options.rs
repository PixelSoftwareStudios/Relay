//! Options only a desktop shell can offer: notifying about incoming calls while the window
//! is out of sight, and starting at login. The shell owns the state (the notification
//! switch is stored by the app, the login item lives in the OS); the UI only draws the
//! switches and the honest status text. The language choice is stored the same way.

use crate::i18n::Language;
use crate::tr;

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
	/// The UI language: the stored choice, or the system language matched to
	/// the supported list when none was chosen. Called per frame; the shell
	/// resolves it once and answers from memory.
	fn language(&self) -> Language;
	/// The language the user pinned; `None` while the system language is followed.
	fn language_choice(&self) -> Option<Language>;
	/// The system language matched to the supported list (English where none
	/// matches), as last detected.
	fn system_language(&self) -> Language;
	/// Store the user's pick (`None` = follow the system, detected afresh);
	/// the UI applies it on the next frame.
	fn set_language_choice(&self, choice: Option<Language>);
	/// The login item as last read from the OS.
	fn login_status(&self) -> LoginStatus;
	/// Register or remove the login item. `Err` carries the OS's reason.
	fn set_launch_at_login(&self, on: bool) -> Result<(), String>;
}

/// What the "Notify on incoming calls" switch really does.
pub fn notify_text(on: bool, failure: Option<&str>) -> String {
	match (on, failure) {
		(false, _) => tr!("notify-off").to_owned(),
		(true, None) => tr!("notify-on").to_owned(),
		(true, Some(reason)) => tr!("notify-refused", reason = reason),
	}
}

/// What the "Start at login" switch really does.
pub fn login_text(status: &LoginStatus) -> String {
	match status {
		LoginStatus::Unsupported(reason) => tr!("login-unsupported", reason = reason.as_str()),
		LoginStatus::Off => tr!("login-off").to_owned(),
		LoginStatus::On => tr!("login-on").to_owned(),
		LoginStatus::Failed(reason) => tr!("login-failed", reason = reason.as_str()),
	}
}
