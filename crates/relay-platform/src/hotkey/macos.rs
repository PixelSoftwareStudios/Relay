//! macOS specifics: main-thread check, run-loop pumping and permission status.

use std::ffi::c_void;
use std::time::Duration;

type CfStringRef = *const c_void;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
	static kCFRunLoopDefaultMode: CfStringRef;
	fn CFRunLoopRunInMode(mode: CfStringRef, seconds: f64, return_after_source: u8) -> i32;
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
	fn AXIsProcessTrusted() -> u8;
	fn CGPreflightListenEventAccess() -> u8;
}

unsafe extern "C" {
	fn pthread_main_np() -> i32;
}

/// State of one macOS privacy permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
	/// Granted to this process (or its parent terminal).
	Granted,
	/// Not granted; the user must enable it in System Settings, Privacy & Security.
	Denied,
}

/// Snapshot of the permissions relevant to key capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacPermissions {
	/// Accessibility (`AXIsProcessTrusted`).
	pub accessibility: PermissionState,
	/// Input Monitoring (`CGPreflightListenEventAccess`).
	pub input_monitoring: PermissionState,
}

fn state(granted: bool) -> PermissionState {
	if granted {
		PermissionState::Granted
	} else {
		PermissionState::Denied
	}
}

/// Query both permissions without prompting the user.
pub fn permissions() -> MacPermissions {
	MacPermissions {
		accessibility: state(accessibility_trusted()),
		input_monitoring: state(input_monitoring_granted()),
	}
}

/// Whether Accessibility is granted.
pub fn accessibility_trusted() -> bool {
	// SAFETY: plain query function with no arguments or side effects.
	unsafe { AXIsProcessTrusted() != 0 }
}

/// Whether Input Monitoring is granted. Does not show the system prompt.
pub fn input_monitoring_granted() -> bool {
	// SAFETY: plain query function with no arguments or side effects.
	unsafe { CGPreflightListenEventAccess() != 0 }
}

/// Whether the caller runs on the process main thread.
pub fn is_main_thread() -> bool {
	// SAFETY: plain query function with no arguments.
	unsafe { pthread_main_np() != 0 }
}

/// Service the current thread's run loop for up to `limit`.
pub fn pump_run_loop(limit: Duration) {
	// SAFETY: kCFRunLoopDefaultMode is a valid constant CFString for the process lifetime.
	unsafe {
		CFRunLoopRunInMode(kCFRunLoopDefaultMode, limit.as_secs_f64(), 1);
	}
}
