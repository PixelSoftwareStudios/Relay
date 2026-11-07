//! Whether anybody is home.
//!
//! A client is *attached* while it holds an open state watch. One-shot
//! commands never count, so `relay status` cannot make the device appear on
//! the LAN. The engine advertises itself and rings for inbound calls only
//! while somebody is attached, and a call belongs to the attached client that
//! controls it (see `relay_core`'s ownership rules).
//!
//! With `background.available` on the engine itself counts as home
//! ([`relay_core::ClientId::HOUSE`]), so a daemon nobody watches still advertises, listens,
//! rings and may auto-answer, and never exits as idle. A shell that alerts calls on its own
//! marks its client quiet; the engine ringtone then stays silent while every real client is
//! quiet.
//!
//! Attaching takes effect at once; detaching only after a debounce, so a TUI
//! or window that reconnects does not make the device flicker. An engine that
//! stays unattached with no live call for the idle-exit time asks to stop,
//! which reaps daemons nobody uses.

use relay_io::LockExt;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use relay_core::ClientId;
use tokio::sync::Notify;
use tokio::time::Instant;

/// The open state watches per client, shared between the API handlers (which
/// open and close them) and the control actor (which reacts).
#[derive(Debug, Default)]
pub struct PresenceLink {
	watchers: Mutex<BTreeMap<ClientId, usize>>,
	quiet: Mutex<BTreeSet<ClientId>>,
	notify: Notify,
}

impl PresenceLink {
	/// A fresh link with no watcher.
	pub fn new() -> Arc<Self> {
		Arc::new(Self::default())
	}

	fn lock(&self) -> MutexGuard<'_, BTreeMap<ClientId, usize>> {
		self.watchers.lock_unpoisoned()
	}

	/// Registers one open watch of `client` until the guard drops.
	pub fn watch(self: &Arc<Self>, client: ClientId) -> Watcher {
		*self.lock().entry(client).or_default() += 1;
		self.notify.notify_one();
		Watcher {
			link: Arc::clone(self),
			client,
		}
	}

	/// The clients with at least one open watch right now.
	pub fn attached(&self) -> BTreeSet<ClientId> {
		self.lock().keys().copied().collect()
	}

	/// A client says it alerts calls itself (`quiet`) or not. Remembered until the client's last
	/// watch closes, so a shell may set it just before or after it opens its watch.
	pub fn set_quiet(&self, client: ClientId, quiet: bool) {
		let mut set = self.quiet.lock_unpoisoned();
		if quiet {
			set.insert(client);
		} else {
			set.remove(&client);
		}
		drop(set);
		self.notify.notify_one();
	}

	/// The attached clients that alert calls themselves.
	pub fn quiet(&self) -> BTreeSet<ClientId> {
		self.quiet.lock_unpoisoned().clone()
	}

	/// Resolves when a watch opened or closed since the last wait.
	pub async fn changed(&self) {
		self.notify.notified().await;
	}
}

/// One open watch. Dropping it (stream ended, client gone) detaches it.
#[derive(Debug)]
pub struct Watcher {
	link: Arc<PresenceLink>,
	client: ClientId,
}

impl Drop for Watcher {
	fn drop(&mut self) {
		let mut open = self.link.lock();
		if let Some(count) = open.get_mut(&self.client) {
			*count -= 1;
			if *count == 0 {
				open.remove(&self.client);
				self.link.quiet.lock_unpoisoned().remove(&self.client);
			}
		}
		drop(open);
		self.link.notify.notify_one();
	}
}

/// What one look at the presence changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Step {
	/// The effective presence changed to this value.
	pub change: Option<bool>,
	/// The idle time ran out: the engine should stop.
	pub exit: bool,
}

/// The presence rules as a pure state machine over explicit instants.
#[derive(Debug)]
pub struct Presence {
	debounce: Duration,
	idle_exit: Option<Duration>,
	attached: bool,
	leaving_since: Option<Instant>,
	idle_since: Option<Instant>,
}

impl Presence {
	/// Starts unattached at `now`.
	pub fn new(now: Instant, debounce: Duration, idle_exit: Option<Duration>) -> Self {
		Self {
			debounce,
			idle_exit,
			attached: false,
			leaving_since: None,
			idle_since: idle_exit.map(|_| now),
		}
	}

	/// Whether somebody counts as home right now (after the debounce).
	pub fn attached(&self) -> bool {
		self.attached
	}

	/// Looks at the raw facts: any watch or controller (`present`), any live
	/// call (`call_live`).
	pub fn update(&mut self, now: Instant, present: bool, call_live: bool) -> Step {
		let mut step = Step::default();
		if present {
			self.leaving_since = None;
			if !self.attached {
				self.attached = true;
				step.change = Some(true);
			}
		} else if self.attached {
			let since = *self.leaving_since.get_or_insert(now);
			if now.saturating_duration_since(since) >= self.debounce {
				self.attached = false;
				self.leaving_since = None;
				step.change = Some(false);
			}
		}
		if self.attached || call_live || self.idle_exit.is_none() {
			self.idle_since = None;
		} else {
			let since = *self.idle_since.get_or_insert(now);
			step.exit = self
				.idle_exit
				.is_some_and(|limit| now.saturating_duration_since(since) >= limit);
		}
		step
	}

	/// The next instant at which [`Presence::update`] may change something
	/// without any new fact arriving.
	pub fn next_deadline(&self) -> Option<Instant> {
		let leaving = self.leaving_since.map(|since| since + self.debounce);
		let idle = self
			.idle_since
			.zip(self.idle_exit)
			.map(|(since, limit)| since + limit);
		[leaving, idle].into_iter().flatten().min()
	}
}
