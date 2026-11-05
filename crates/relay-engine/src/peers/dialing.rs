//! Dialling: candidate addresses, outbound connects and reconnect backoff.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use relay_net::identity::Fingerprint;
use relay_net::session::Role;
use tokio::net::TcpStream;
use tokio::time::timeout;

use super::handle::Command;
use super::link::{LinkEvent, LinkId, establish};
use super::manager::Actor;

const DIAL_TIMEOUT: Duration = Duration::from_secs(3);

/// A peer the user wants a link to, with its reconnect bookkeeping.
#[derive(Default)]
pub(super) struct Want {
	pub(super) attempt: u32,
	pub(super) dialling: bool,
	pub(super) retry_scheduled: bool,
}

impl Actor {
	pub(super) fn remember_address(&mut self, peer: Fingerprint, addr: SocketAddr) {
		let known = self.addresses.entry(peer).or_default();
		if !known.contains(&addr) && known.len() < 8 {
			known.push(addr);
		}
	}

	pub(super) fn candidate_addresses(&self, peer: &Fingerprint) -> Vec<SocketAddr> {
		let mut out = self.addresses.get(peer).cloned().unwrap_or_default();
		for found in self.table.iter() {
			if found
				.fingerprint_hint
				.is_some_and(|hint| hint.matches(peer))
			{
				out.extend(&found.addresses);
			}
		}
		out.dedup();
		out
	}

	/// Starts a dial for a wanted peer without a link, if addresses are known.
	pub(super) fn try_dial(&mut self, peer: Fingerprint) {
		let Some(want) = self.wanted.get(&peer) else {
			return;
		};
		if want.dialling || self.canonical.contains_key(&peer) {
			return;
		}
		let addresses = self.candidate_addresses(&peer);
		if addresses.is_empty() {
			return;
		}
		let id = self.dial(addresses, Some(peer));
		if let Some(want) = self.wanted.get_mut(&peer) {
			want.dialling = true;
		}
		self.dialling.insert(id, Some(peer));
	}

	pub(super) fn dial(
		&mut self,
		addresses: Vec<SocketAddr>,
		expected: Option<Fingerprint>,
	) -> LinkId {
		let id = self.ids.fetch_add(1, Ordering::Relaxed);
		self.dialling.insert(id, expected);
		let (params, events) = (Arc::clone(&self.params), self.link_tx.clone());
		tokio::spawn(async move {
			let mut last = String::from("no address");
			for addr in addresses {
				match timeout(DIAL_TIMEOUT, TcpStream::connect(addr)).await {
					Ok(Ok(stream)) => {
						match establish(
							stream,
							Role::Initiator,
							expected,
							&params,
							id,
							events.clone(),
						)
						.await
						{
							Ok(()) => return,
							Err(error) => last = error.to_string(),
						}
					}
					Ok(Err(error)) => last = error.to_string(),
					Err(_) => last = "connect timed out".to_owned(),
				}
			}
			let _ = events
				.send(LinkEvent::Failed {
					link: id,
					error: last,
				})
				.await;
		});
		id
	}

	pub(super) fn dial_ended(&mut self, peer: Fingerprint) {
		if let Some(want) = self.wanted.get_mut(&peer) {
			want.dialling = false;
		}
		if !self.canonical.contains_key(&peer) {
			self.schedule_redial(peer);
		}
	}

	pub(super) fn schedule_redial(&mut self, peer: Fingerprint) {
		let Some(want) = self.wanted.get_mut(&peer) else {
			return;
		};
		if want.retry_scheduled {
			return;
		}
		want.retry_scheduled = true;
		let delay = self
			.config
			.backoff
			.delay(want.attempt, rand::random::<f64>());
		want.attempt = want.attempt.saturating_add(1);
		let commands = self.commands.clone();
		tokio::spawn(async move {
			tokio::time::sleep(delay).await;
			let _ = commands.send(Command::Redial(peer)).await;
		});
	}
}
