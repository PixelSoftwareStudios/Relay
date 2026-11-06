//! A fake identity store standing in for the system keychain prompt: it blocks while the
//! "prompt" is open, refuses like a denied one, or reports a damaged item.
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use relay_config::identity::{IdentityStore, MemoryStore, StoredIdentity};
use relay_config::{ConfigError, Result};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
	/// `load` waits (the prompt is open).
	Blocked,
	/// `load` fails the way a denied prompt does.
	Denied,
	/// `load` fails the way a damaged item does.
	Corrupt,
	Open,
}

/// An identity store behind a switch the test flips.
pub struct Prompt {
	inner: MemoryStore,
	mode: Mutex<Mode>,
	changed: Condvar,
	pub loads: AtomicUsize,
	pub saves: AtomicUsize,
}

impl Prompt {
	pub fn new(mode: Mode) -> Arc<Self> {
		Arc::new(Self {
			inner: MemoryStore::default(),
			mode: Mutex::new(mode),
			changed: Condvar::new(),
			loads: AtomicUsize::new(0),
			saves: AtomicUsize::new(0),
		})
	}

	pub fn set(&self, mode: Mode) {
		*self.mode.lock().unwrap() = mode;
		self.changed.notify_all();
	}
}

pub struct Handle(pub Arc<Prompt>);

impl IdentityStore for Handle {
	fn describe(&self) -> &'static str {
		"fake prompt"
	}

	fn load(&self) -> Result<Option<StoredIdentity>> {
		self.0.loads.fetch_add(1, Ordering::SeqCst);
		let mut mode = self.0.mode.lock().unwrap();
		while *mode == Mode::Blocked {
			mode = self.0.changed.wait(mode).unwrap();
		}
		match *mode {
			Mode::Denied => Err(ConfigError::CredentialStore("user denied".into())),
			Mode::Corrupt => Err(ConfigError::BundleUnreadable("malformed".into())),
			_ => self.0.inner.load(),
		}
	}

	fn save(&self, identity: &StoredIdentity) -> Result<()> {
		self.0.saves.fetch_add(1, Ordering::SeqCst);
		self.0.inner.save(identity)
	}

	fn delete(&self) -> Result<()> {
		self.0.inner.delete()
	}
}

/// A failing assertion must not leave the fake prompt open: the runtime would wait for it.
pub struct OpenOnDrop(pub Arc<Prompt>);

impl Drop for OpenOnDrop {
	fn drop(&mut self) {
		self.0.set(Mode::Open);
	}
}
