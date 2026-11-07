//! Background availability on the desktop: what closing the window does and what the tray
//! shows. Pure decisions; the engine setting is `background.available` (default on).
//!
//! The tray keeps a watch on the daemon while the window is hidden, and a daemon with a
//! watch attached advertises, listens and rings and never idles out. With the setting off a
//! closed window quits this app, the watch ends and an app-started daemon leaves after its
//! idle timeout.

use std::sync::atomic::{AtomicBool, Ordering};

use relay_ui::emitter::Sink;
use relay_ui::emitter::TraySummary;
use relay_ui::model::{AppState, Link, Meters};
use std::sync::Arc;

/// What a window close request does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
	/// Hide the window; the tray, the watch and the daemon stay.
	HideToTray,
	/// Let the window close, which ends this app.
	Quit,
}

/// The decision. Hiding needs a tray to bring the window back, the setting on, and no quit
/// already under way (the tray menu's Quit and "Quit app" set that).
pub fn close_action(has_tray: bool, background_available: bool, quitting: bool) -> CloseAction {
	if has_tray && background_available && !quitting {
		CloseAction::HideToTray
	} else {
		CloseAction::Quit
	}
}

/// What the tray menu shows beyond the tooltip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayFacts {
	/// This device can be reached now: the app holds its watch on a running daemon.
	pub available: bool,
	/// The "Stay available in the background" check item.
	pub background: bool,
}

impl Default for TrayFacts {
	fn default() -> Self {
		Self {
			available: false,
			background: true,
		}
	}
}

impl TrayFacts {
	pub fn from_state(state: &AppState) -> Self {
		Self {
			available: state.link == Link::Connected,
			background: state
				.settings
				.as_ref()
				.is_none_or(|s| s.background_available),
		}
	}
}

/// The setting as the shell last saw it, shared by the window (close), the shell
/// (`hides_to_tray`) and the tray.
pub struct Background {
	enabled: AtomicBool,
	available: AtomicBool,
	/// The tray menu must be redrawn from the facts (set on a change and after a click).
	stale: AtomicBool,
}

impl Default for Background {
	fn default() -> Self {
		Self {
			enabled: AtomicBool::new(true),
			available: AtomicBool::new(false),
			stale: AtomicBool::new(true),
		}
	}
}

impl Background {
	pub fn enabled(&self) -> bool {
		self.enabled.load(Ordering::Acquire)
	}

	pub fn facts(&self) -> TrayFacts {
		TrayFacts {
			available: self.available.load(Ordering::Acquire),
			background: self.enabled(),
		}
	}

	/// Store new facts; true when they changed.
	pub fn set(&self, facts: TrayFacts) -> bool {
		let before = self.facts();
		self.enabled.store(facts.background, Ordering::Release);
		self.available.store(facts.available, Ordering::Release);
		let changed = before != facts;
		if changed {
			self.stale.store(true, Ordering::Release);
		}
		changed
	}

	/// Ask for the tray menu to be redrawn from the facts, even if they did not change
	/// (a click flips the check mark before the engine has answered).
	pub fn mark_stale(&self) {
		self.stale.store(true, Ordering::Release);
	}

	/// The facts to draw, once per change.
	pub fn take_stale(&self) -> Option<TrayFacts> {
		self.stale
			.swap(false, Ordering::AcqRel)
			.then(|| self.facts())
	}
}

/// Wraps the sink chain: forwards everything and keeps [`Background`] current.
pub struct BackgroundSink {
	inner: Arc<dyn Sink>,
	background: Arc<Background>,
	repaint: Arc<dyn Fn() + Send + Sync>,
}

impl BackgroundSink {
	pub fn new(
		inner: Arc<dyn Sink>,
		background: Arc<Background>,
		repaint: Arc<dyn Fn() + Send + Sync>,
	) -> Self {
		Self {
			inner,
			background,
			repaint,
		}
	}
}

impl Sink for BackgroundSink {
	fn state(&self, state: &AppState) {
		self.inner.state(state);
		if self.background.set(TrayFacts::from_state(state)) {
			// The window may be hidden and not repainting; the tray is updated in `logic`.
			(self.repaint)();
		}
	}

	fn meters(&self, meters: &Meters) {
		self.inner.meters(meters);
	}

	fn tray(&self, summary: &TraySummary) {
		self.inner.tray(summary);
	}
}
