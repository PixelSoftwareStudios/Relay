//! Applying engine snapshots and events to the view state.

use relay_api::pb::{self, state_event::Event};

use crate::convert;
use crate::model::{AppState, Link, MAX_ITEMS, MAX_WARNINGS};

/// Insert or replace by key, refusing to grow past [`MAX_ITEMS`].
fn upsert<T>(items: &mut Vec<T>, item: T, same: impl Fn(&T) -> bool) {
	match items.iter().position(same) {
		Some(i) => items[i] = item,
		None if items.len() < MAX_ITEMS => items.push(item),
		None => tracing::warn!("dropping item: collection is at its bound"),
	}
}

impl AppState {
	/// Replace all engine-derived state with an atomic snapshot.
	pub fn load_snapshot(&mut self, snapshot: &pb::Snapshot) {
		self.revision = snapshot.revision;
		self.peers = snapshot
			.peers
			.iter()
			.take(MAX_ITEMS)
			.map(convert::peer)
			.collect();
		self.calls = snapshot
			.calls
			.iter()
			.take(MAX_ITEMS)
			.map(|c| convert::call(c, &self.client_id))
			.collect();
		self.channels = snapshot
			.channels
			.iter()
			.take(MAX_ITEMS)
			.map(convert::channel)
			.collect();
		self.nearby_channels = snapshot
			.nearby_channels
			.iter()
			.take(MAX_ITEMS)
			.map(convert::nearby_channel)
			.collect();
		self.devices = snapshot
			.devices
			.iter()
			.take(MAX_ITEMS)
			.map(convert::device)
			.collect();
		self.settings = snapshot.settings.as_ref().map(convert::settings);
		self.warnings = snapshot
			.warnings
			.iter()
			.take(MAX_WARNINGS)
			.map(convert::problem)
			.collect();
	}

	/// Apply one incremental event that the stream tracker already validated.
	pub fn apply_event(&mut self, revision: u64, event: &pb::StateEvent) {
		self.revision = revision;
		let Some(event) = &event.event else { return };
		match event {
			Event::Peer(p) => {
				let item = convert::peer(p);
				upsert(&mut self.peers, item.clone(), |x| x.peer_id == item.peer_id);
			}
			Event::Call(c) => {
				let item = convert::call(c, &self.client_id);
				upsert(&mut self.calls, item.clone(), |x| x.call_id == item.call_id);
			}
			Event::Device(d) => {
				let item = convert::device(d);
				upsert(&mut self.devices, item.clone(), |x| {
					x.device_id == item.device_id
				});
			}
			Event::Channel(c) => {
				let item = convert::channel(c);
				upsert(&mut self.channels, item.clone(), |x| {
					x.channel_id == item.channel_id
				});
			}
			Event::NearbyChannels(list) => {
				self.nearby_channels = list
					.channels
					.iter()
					.take(MAX_ITEMS)
					.map(convert::nearby_channel)
					.collect();
			}
			Event::Settings(s) => self.settings = Some(convert::settings(s)),
			Event::Removed(r) => self.remove(&r.kind, &r.id),
			Event::Warning(w) => {
				let item = convert::problem(w);
				if !self.warnings.contains(&item) {
					if self.warnings.len() == MAX_WARNINGS {
						self.warnings.remove(0);
					}
					self.warnings.push(item);
				}
			}
			// Outcomes are awaited by the command that issued them.
			// Shells react to AutoAnswered; the call itself carries `auto_answered`.
			Event::Operation(_) | Event::AutoAnswered(_) => {}
		}
	}

	fn remove(&mut self, kind: &str, id: &[u8]) {
		let hex_id = hex::encode(id);
		match kind.to_ascii_lowercase().as_str() {
			"peer" => self.peers.retain(|p| p.peer_id != hex_id),
			"channel" => self.channels.retain(|c| c.channel_id != hex_id),
			"call" => self.calls.retain(|c| c.call_id != hex_id),
			"device" => self.devices.retain(|d| d.device_id.as_bytes() != id),
			// The id is the warning's code: its cause is gone.
			"warning" => self.warnings.retain(|w| w.code.as_bytes() != id),
			other => tracing::debug!(kind = other, "ignoring removal of unknown kind"),
		}
	}

	/// Mark the connection down. Engine data stays visible but stale only
	/// until the next snapshot; the link flag tells the UI to say so.
	pub fn set_link_down(&mut self, link: Link, reason: Option<String>) {
		self.link = link;
		self.link_error = reason;
		self.startup = None;
		self.engine = None;
		self.calls.clear();
		self.peers.clear();
		self.devices.clear();
		self.channels.clear();
		self.nearby_channels.clear();
		self.settings = None;
		self.warnings.clear();
		self.client_id.clear();
	}
}
