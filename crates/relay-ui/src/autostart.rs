//! When the app starts the daemon by itself, and when it stops trying.
//!
//! It starts one at launch if none answers, and restarts one that died. It
//! never restarts one the user quit on purpose, it backs off between
//! attempts, and a daemon that keeps dying is left alone (three starts within
//! a minute) until the user presses "Start daemon".

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The window in which starts are counted.
pub const CRASH_WINDOW: Duration = Duration::from_secs(60);
/// Starts allowed within [`CRASH_WINDOW`].
pub const MAX_STARTS: usize = 3;
const BACKOFF_BASE: Duration = Duration::from_secs(1);

/// What the supervisor should do about a daemon that does not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
	/// Do nothing: auto-start is off, or the user quit the daemon.
	Idle,
	/// Start it now.
	Start,
	/// Start it after this long (exponential backoff).
	Wait(Duration),
	/// Too many starts in a short time: stop and show the failure.
	GiveUp,
}

#[derive(Debug)]
struct Inner {
	setting: bool,
	env_off: bool,
	user_quit: bool,
	gave_up: bool,
	starts: VecDeque<Instant>,
	last_failure: Option<String>,
}

/// The auto-start policy of one app session.
#[derive(Debug)]
pub struct AutoStart {
	inner: Mutex<Inner>,
}

/// Whether `RELAY_NO_AUTOSTART` switches auto-start off for every client.
pub fn env_switch_off(value: Option<&str>) -> bool {
	value.is_some_and(|v| {
		let v = v.trim().to_ascii_lowercase();
		!(v.is_empty() || v == "0" || v == "false")
	})
}

impl Default for AutoStart {
	fn default() -> Self {
		Self::new(env_switch_off(
			std::env::var("RELAY_NO_AUTOSTART").ok().as_deref(),
		))
	}
}

impl AutoStart {
	/// The setting starts on (the default); `env_off` is the environment switch.
	pub fn new(env_off: bool) -> Self {
		Self {
			inner: Mutex::new(Inner {
				setting: true,
				env_off,
				user_quit: false,
				gave_up: false,
				starts: VecDeque::new(),
				last_failure: None,
			}),
		}
	}

	fn lock(&self) -> MutexGuard<'_, Inner> {
		self.inner.lock().unwrap_or_else(|e| e.into_inner())
	}

	/// The stored "Start daemon automatically" choice, read from the daemon.
	pub fn set_setting(&self, on: bool) {
		self.lock().setting = on;
	}

	/// The stored choice (what the checkbox shows).
	pub fn setting(&self) -> bool {
		self.lock().setting
	}

	/// The user quit the daemon: never start it again in this session by itself.
	pub fn user_quit(&self) {
		self.lock().user_quit = true;
	}

	/// The user asked for a start: forget quits and failures.
	pub fn user_start(&self) {
		let mut inner = self.lock();
		inner.user_quit = false;
		inner.gave_up = false;
		inner.starts.clear();
	}

	/// Whether automatic starts are currently allowed at all.
	pub fn enabled(&self) -> bool {
		let inner = self.lock();
		inner.setting && !inner.env_off
	}

	/// Whether the app is holding back (quit, or crash loop) and the UI should
	/// offer a manual button.
	pub fn paused(&self) -> bool {
		let inner = self.lock();
		inner.user_quit || inner.gave_up
	}

	/// Whether the crash-loop cap stopped automatic starts.
	pub fn gave_up(&self) -> bool {
		self.lock().gave_up
	}

	/// The last start failure, for the "gave up" message.
	pub fn last_failure(&self) -> Option<String> {
		self.lock().last_failure.clone()
	}

	/// What to do now about an unreachable daemon.
	pub fn decide(&self, now: Instant) -> Decision {
		let mut inner = self.lock();
		if !inner.setting || inner.env_off || inner.user_quit || inner.gave_up {
			return Decision::Idle;
		}
		while inner
			.starts
			.front()
			.is_some_and(|at| now.saturating_duration_since(*at) >= CRASH_WINDOW)
		{
			inner.starts.pop_front();
		}
		if inner.starts.len() >= MAX_STARTS {
			inner.gave_up = true;
			return Decision::GiveUp;
		}
		let Some(last) = inner.starts.back().copied() else {
			return Decision::Start;
		};
		let exponent = u32::try_from(inner.starts.len() - 1).unwrap_or(0);
		let due = last + BACKOFF_BASE.saturating_mul(1u32 << exponent);
		if now >= due {
			Decision::Start
		} else {
			Decision::Wait(due - now)
		}
	}

	/// Records a start attempt and how it went.
	pub fn record_start(&self, now: Instant, outcome: &Result<String, String>) {
		let mut inner = self.lock();
		inner.starts.push_back(now);
		inner.last_failure = outcome.as_ref().err().cloned();
	}
}
