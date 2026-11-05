//! Start at login, through the OS's own mechanism (`auto-launch`: a LaunchAgent on macOS, the
//! `Run` key on Windows, an XDG autostart entry on Linux). Off unless the user turns it on,
//! and the status shown is what the OS says, not what was last requested.

use std::sync::{Arc, Mutex};

use relay_io::LockExt;
use relay_ui::host_options::LoginStatus;
use relay_ui::tr;

/// The OS login item. The real one is `auto-launch`; tests use a fake.
pub trait LoginBackend: Send + Sync {
	fn is_enabled(&self) -> Result<bool, String>;
	fn enable(&self) -> Result<(), String>;
	fn disable(&self) -> Result<(), String>;
}

/// `auto-launch` for this executable.
pub struct SystemLogin(auto_launch::AutoLaunch);

impl SystemLogin {
	/// `Err` with the reason when the platform or the executable path rules it out.
	pub fn new() -> Result<Self, String> {
		if !auto_launch::AutoLaunch::is_support() {
			return Err(tr!("login-no-mechanism").to_owned());
		}
		let exe =
			std::env::current_exe().map_err(|e| tr!("login-no-exe", error = e.to_string()))?;
		let path = exe
			.to_str()
			.ok_or_else(|| tr!("login-bad-path").to_owned())?;
		auto_launch::AutoLaunchBuilder::new()
			.set_app_name("Relay")
			.set_app_path(path)
			.build()
			.map(Self)
			.map_err(|e| e.to_string())
	}
}

impl LoginBackend for SystemLogin {
	fn is_enabled(&self) -> Result<bool, String> {
		self.0.is_enabled().map_err(|e| e.to_string())
	}

	fn enable(&self) -> Result<(), String> {
		self.0.enable().map_err(|e| e.to_string())
	}

	fn disable(&self) -> Result<(), String> {
		self.0.disable().map_err(|e| e.to_string())
	}
}

/// The cached login status plus the way to change it.
pub struct LoginItem {
	backend: Result<Arc<dyn LoginBackend>, String>,
	status: Mutex<LoginStatus>,
}

impl LoginItem {
	/// Read the OS state once; the UI then reads the cache.
	pub fn new(backend: Result<Arc<dyn LoginBackend>, String>) -> Self {
		let item = Self {
			backend,
			status: Mutex::new(LoginStatus::Off),
		};
		item.refresh();
		item
	}

	/// The real backend, or an "unsupported" item saying why not.
	pub fn system() -> Self {
		Self::new(SystemLogin::new().map(|s| Arc::new(s) as Arc<dyn LoginBackend>))
	}

	pub fn status(&self) -> LoginStatus {
		self.status.lock_unpoisoned().clone()
	}

	/// Ask the OS again.
	pub fn refresh(&self) {
		let status = match &self.backend {
			Err(reason) => LoginStatus::Unsupported(reason.clone()),
			Ok(backend) => match backend.is_enabled() {
				Ok(true) => LoginStatus::On,
				Ok(false) => LoginStatus::Off,
				Err(reason) => LoginStatus::Failed(reason),
			},
		};
		*self.status.lock_unpoisoned() = status;
	}

	/// Register or remove the login item, then re-read the OS so the status is its answer.
	pub fn set(&self, on: bool) -> Result<(), String> {
		let backend = self
			.backend
			.as_ref()
			.map_err(|reason| tr!("login-set-unavailable", reason = reason.as_str()))?;
		let result = if on {
			backend.enable()
		} else {
			backend.disable()
		};
		self.refresh();
		result
	}
}
