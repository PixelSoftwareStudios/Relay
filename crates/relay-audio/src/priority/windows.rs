//! Windows promotion: MMCSS "Pro Audio", else `THREAD_PRIORITY_TIME_CRITICAL`.
//! The only `unsafe` for this platform lives here.
//!
//! MMCSS (`AvSetMmThreadCharacteristicsW`) registers the thread with the Multimedia Class
//! Scheduler Service under a task name whose scheduling parameters come from the registry
//! (`HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks`); the
//! buffer period is not an input. `AvRevertMmThreadCharacteristics` ends the registration.
//!
//! `THREAD_PRIORITY_TIME_CRITICAL` is the fallback: it puts a normal-class thread at level 15,
//! above every normal thread but below the real-time class. A thread that spins can starve other
//! normal threads of the same process and delay their I/O; our workers sleep between ticks, and
//! the previous priority is restored on drop.

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::Threading::{
	AVRT_PRIORITY_HIGH, AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW,
	AvSetMmThreadPriority, GetCurrentThread, GetThreadPriority, SetThreadPriority,
	THREAD_PRIORITY_TIME_CRITICAL,
};

/// `GetThreadPriority` failure value (0x7FFFFFFF; the constant sits behind another feature).
const THREAD_PRIORITY_ERROR_RETURN: i32 = i32::MAX;

use super::PriorityError;
use super::WorkerKind;
use super::policy::{OsPriority, RtTiming};

/// The Windows implementation of [`OsPriority`].
#[derive(Default)]
pub struct System;

/// What to restore.
pub enum Undo {
	/// An MMCSS registration, restored only on the registering thread by PriorityGuard.
	Mmcss(usize),
	/// The thread priority level before it was raised.
	Level(i32),
}

fn os_error(call: &'static str) -> PriorityError {
	// SAFETY: reads the calling thread's last-error value; no arguments.
	let code = unsafe { GetLastError() };
	PriorityError::Os {
		call,
		detail: format!("error {code}"),
	}
}

impl OsPriority for System {
	type Undo = Undo;

	fn realtime(
		&mut self,
		_kind: WorkerKind,
		_timing: RtTiming,
	) -> Result<(&'static str, Undo), PriorityError> {
		const TASK: [u16; 10] = {
			let name = *b"Pro Audio\0";
			let mut wide = [0u16; 10];
			let mut i = 0;
			while i < 10 {
				wide[i] = name[i] as u16;
				i += 1;
			}
			wide
		};
		let mut index: u32 = 0;
		// SAFETY: `TASK` is a NUL-terminated UTF-16 string and `index` a writable u32, both
		// alive for the call.
		let handle = unsafe { AvSetMmThreadCharacteristicsW(TASK.as_ptr(), &raw mut index) };
		if handle.is_null() {
			return Err(os_error("AvSetMmThreadCharacteristicsW"));
		}
		// SAFETY: `handle` was just returned by MMCSS for this thread.
		let raised = unsafe { AvSetMmThreadPriority(handle, AVRT_PRIORITY_HIGH) };
		if raised == 0 {
			// Registered at the task's default level; still useful, so keep the registration.
			tracing::debug!("AvSetMmThreadPriority(HIGH) failed; keeping the MMCSS default level");
		}
		Ok(("MMCSS Pro Audio", Undo::Mmcss(handle as usize)))
	}

	fn elevated(&mut self, _kind: WorkerKind) -> Result<(&'static str, Undo), PriorityError> {
		// SAFETY: the pseudo-handle for the calling thread is always valid and needs no close.
		let thread = unsafe { GetCurrentThread() };
		// SAFETY: `thread` is the valid pseudo-handle above.
		let previous = unsafe { GetThreadPriority(thread) };
		if previous == THREAD_PRIORITY_ERROR_RETURN {
			return Err(os_error("GetThreadPriority"));
		}
		// SAFETY: `thread` is the valid pseudo-handle above.
		if unsafe { SetThreadPriority(thread, THREAD_PRIORITY_TIME_CRITICAL) } == 0 {
			return Err(os_error("SetThreadPriority"));
		}
		Ok((
			"THREAD_PRIORITY_TIME_CRITICAL (MMCSS refused)",
			Undo::Level(previous),
		))
	}

	fn restore(&mut self, undo: Undo) {
		match undo {
			Undo::Mmcss(handle) => {
				// SAFETY: the handle came from `AvSetMmThreadCharacteristicsW` on this thread and
				// is reverted exactly once.
				unsafe { AvRevertMmThreadCharacteristics(handle as *mut core::ffi::c_void) };
			}
			Undo::Level(level) => {
				// SAFETY: the calling thread's pseudo-handle is always valid.
				unsafe { SetThreadPriority(GetCurrentThread(), level) };
			}
		}
	}
}
