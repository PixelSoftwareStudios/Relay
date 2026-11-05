//! Screen-lock polling through the console session dictionary.

use std::ffi::{CStr, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::mpsc::UnboundedSender;

use super::{LockMonitor, SessionEvent};

type CfRef = *const c_void;
const UTF8: u32 = 0x0800_0100;
const POLL: Duration = Duration::from_millis(250);

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
	fn CFStringCreateWithCString(alloc: CfRef, s: *const std::ffi::c_char, encoding: u32) -> CfRef;
	fn CFDictionaryGetValue(dict: CfRef, key: CfRef) -> CfRef;
	fn CFBooleanGetValue(b: CfRef) -> u8;
	fn CFRelease(cf: CfRef);
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
	fn CGSessionCopyCurrentDictionary() -> CfRef;
}

/// True while the screen is locked. A missing key means unlocked.
fn screen_locked() -> bool {
	let key_name: &CStr = c"CGSSessionScreenIsLocked";
	// SAFETY: every CF object created here is released before returning, the key is a
	// valid NUL-terminated string, and the dictionary value is only read as a CFBoolean
	// (the documented type of this key) while the dictionary is still alive.
	unsafe {
		let dict = CGSessionCopyCurrentDictionary();
		if dict.is_null() {
			return false;
		}
		let key = CFStringCreateWithCString(std::ptr::null(), key_name.as_ptr(), UTF8);
		let value = CFDictionaryGetValue(dict, key);
		let locked = !value.is_null() && CFBooleanGetValue(value) != 0;
		CFRelease(key);
		CFRelease(dict);
		locked
	}
}

pub(super) fn spawn(tx: UnboundedSender<SessionEvent>) -> LockMonitor {
	let stop = Arc::new(AtomicBool::new(false));
	let flag = stop.clone();
	let spawned = std::thread::Builder::new()
		.name("relay-lock-monitor".into())
		.spawn(move || {
			let mut locked = screen_locked();
			if locked {
				let _ = tx.send(SessionEvent::Locked);
			}
			while !flag.load(Ordering::SeqCst) {
				std::thread::sleep(POLL);
				let now = screen_locked();
				if now != locked {
					locked = now;
					let event = if now {
						SessionEvent::Locked
					} else {
						SessionEvent::Unlocked
					};
					if tx.send(event).is_err() {
						break;
					}
				}
			}
		});
	if let Err(e) = spawned {
		tracing::warn!("lock monitor thread failed: {e}");
	}
	LockMonitor { stop }
}
