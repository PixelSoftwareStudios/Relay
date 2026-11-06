//! Peers found by the platform's own service browser (Android `NsdManager`).
//!
//! `mdns-sd` stays the engine's discovery. Some Android devices and Wi-Fi
//! stacks deliver multicast badly, so the shell may also push what the
//! platform browser resolved. Hints go through the same address policy and the
//! same table as mDNS results, so they are exactly as untrusted.

use std::sync::{Arc, Mutex};

use relay_io::LockExt;
use relay_net::discovery::PlatformRecord;
use tokio::sync::mpsc;

/// What the platform browser reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hint {
	Found(PlatformRecord),
	/// The service with this full name went away.
	Lost(String),
}

/// The engine's end; handed to [`super::DiscoverySetup`].
#[derive(Clone, Default)]
pub struct HintSource(Arc<Mutex<Option<mpsc::UnboundedReceiver<Hint>>>>);

impl std::fmt::Debug for HintSource {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("HintSource")
	}
}

impl HintSource {
	/// Takes the receiver; the discovery task owns it from then on.
	pub(super) fn take(&self) -> Option<mpsc::UnboundedReceiver<Hint>> {
		self.0.lock_unpoisoned().take()
	}
}

/// The shell's end.
#[derive(Clone)]
pub struct HintSender(mpsc::UnboundedSender<Hint>);

impl HintSender {
	/// Returns false once the engine has stopped listening.
	pub fn send(&self, hint: Hint) -> bool {
		self.0.send(hint).is_ok()
	}
}

/// A connected pair.
pub fn hint_channel() -> (HintSender, HintSource) {
	let (tx, rx) = mpsc::unbounded_channel();
	(HintSender(tx), HintSource(Arc::new(Mutex::new(Some(rx)))))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn hints_reach_the_taker_once() {
		let (sender, source) = hint_channel();
		let mut rx = source.take().expect("first take");
		assert!(source.take().is_none(), "the receiver is handed out once");
		assert!(sender.send(Hint::Lost("x".into())));
		assert_eq!(rx.recv().await, Some(Hint::Lost("x".into())));
	}
}
