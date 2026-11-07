//! Actor-side handling of queries, subscriptions and adapter-executed
//! operations.

use relay_api::pb::{Operation, OperationState, Problem, Snapshot};
use relay_core::{ClientId, Command, OperationId};

use super::actor::Actor;
use super::convert::{problem_of, service_problem, snapshot_to_pb};
use super::error::ServiceError;
use super::handle::{ServiceOp, Subscribed};
use super::ledger::{Fingerprint, Lookup};
use super::ports::{PeerInfo, PeerTrust, PortError, SettingsChange};

/// The warning code raised when a trusted name presents another key.
const KEY_CHANGED_WARNING: &str = "PEER_KEY_CHANGED";

fn operation_pb(id: OperationId, state: OperationState, revision: u64) -> Operation {
	Operation {
		operation_id: id.to_bytes().to_vec(),
		state: state as i32,
		revision,
		call_id: Vec::new(),
		problem: None,
	}
}

impl Actor {
	pub(super) fn snapshot(&self) -> Snapshot {
		snapshot_to_pb(
			&self.engine.state().snapshot(),
			&self.directory,
			self.feed.revision(),
			&self.incarnation,
		)
	}

	pub(super) fn subscribe(&self, after: Option<u64>) -> Subscribed {
		let Some(after) = after else {
			return Subscribed::Fresh {
				snapshot: Box::new(self.snapshot()),
				live: self.feed.subscribe(),
			};
		};
		match self.feed.replay_after(after) {
			Some(replay) => Subscribed::Resume {
				replay,
				live: self.feed.subscribe(),
			},
			None => Subscribed::Resync {
				current: self.feed.revision(),
			},
		}
	}

	/// The recorded outcome of an operation: adapter-executed ones first,
	/// then the domain's own idempotency cache.
	pub(super) fn operation(&self, client: ClientId, id: OperationId) -> Option<Operation> {
		if let Some(operation) = self.ledger.get(client, id) {
			return Some(operation.clone());
		}
		let record = self.engine.state().operation(client, id)?;
		let revision = self.feed.revision();
		let mut operation = operation_pb(id, OperationState::Applied, revision);
		match &record.result {
			Ok(_) => {}
			Err(reject) => {
				operation.state = OperationState::Failed as i32;
				operation.problem = Some(problem_of(reject));
			}
		}
		if let Some(call) = call_of(&record.command) {
			operation.call_id = call.to_bytes().to_vec();
		}
		Some(operation)
	}

	pub(super) fn service_op(
		&mut self,
		client: ClientId,
		id: OperationId,
		print: Fingerprint,
		op: ServiceOp,
	) -> Result<Operation, ServiceError> {
		let now = self.now();
		self.ledger.expire(now);
		match self.ledger.lookup(client, id, &print) {
			Lookup::Replay(stored) => return Ok(stored.clone()),
			Lookup::Conflict => return Err(ServiceError::OperationReused),
			Lookup::Miss => {}
		}
		let starts_test = matches!(op, ServiceOp::StartMicTest { .. });
		let stops_test = matches!(op, ServiceOp::StopMicTest);
		let operation = match self.execute(op) {
			Ok(()) => {
				if starts_test {
					self.mic_test_owner = Some(client);
				} else if stops_test {
					self.mic_test_owner = None;
				}
				operation_pb(id, OperationState::Applied, self.feed.revision())
			}
			Err(PortError::Unsupported(what)) => return Err(ServiceError::Unimplemented(what)),
			Err(error) => failed(id, service_problem(&error), self.feed.revision()),
		};
		self.ledger
			.record(client, id, print, operation.clone(), now);
		Ok(operation)
	}

	fn execute(&mut self, op: ServiceOp) -> Result<(), PortError> {
		match op {
			ServiceOp::SelectDevices { input, output } => {
				self.ports.devices.select(&input, &output)?;
				self.ports.settings.save(&SettingsChange::Devices {
					input: input.clone(),
					output: output.clone(),
				})?;
				self.directory.aux.input_device = input;
				self.directory.aux.output_device = output;
				self.publish_settings();
			}
			ServiceOp::StartMicTest { local_playback } => {
				self.ports.media.start_mic_test(local_playback)?;
			}
			ServiceOp::StopMicTest => self.ports.media.stop_mic_test()?,
			ServiceOp::PreviewTone => self.tones.preview(tokio::time::Instant::now()),
			ServiceOp::Ringtone { name, data } => {
				let samples = self.ports.settings.store_ringtone(&name, &data)?;
				self.ports.media.set_ringtone(Some(samples));
				self.directory.aux.ringtone_name = name;
				self.publish_settings();
			}
			ServiceOp::ResetRingtone => {
				self.ports.settings.clear_ringtone()?;
				self.ports.media.set_ringtone(None);
				self.directory.aux.ringtone_name.clear();
				self.publish_settings();
			}
			ServiceOp::Profile(name) => {
				self.ports
					.settings
					.save(&SettingsChange::DisplayName(name.clone()))?;
				self.directory.aux.display_name = name;
				self.advertise_profile();
			}
			ServiceOp::Avatar(bytes) => {
				let hash = self.ports.settings.store_avatar(&bytes)?;
				self.directory.aux.avatar = Some(hash);
				self.advertise_profile();
			}
			ServiceOp::ClearAvatar => {
				self.ports.settings.clear_avatar()?;
				self.directory.aux.avatar = None;
				self.advertise_profile();
			}
			ServiceOp::ForgetPeer(selector) => {
				let gone = self.ports.peers.forget_peer(&selector)?;
				let mut events = self.directory.forget_peer(gone.id);
				for offending in &gone.key_changed {
					events.extend(self.directory.forget_peer(*offending));
				}
				if !gone.key_changed.is_empty() {
					events.extend(self.directory.clear_warning(KEY_CHANGED_WARNING));
				}
				events.extend(self.directory.set_peers(self.ports.peers.peers()));
				events.extend(
					self.directory
						.set_favorites(self.ports.settings.favorites()),
				);
				events.extend(
					self.directory
						.set_auto_answer(self.ports.settings.auto_answer()),
				);
				self.publish(events);
			}
			ServiceOp::ReplaceKey {
				peer,
				fingerprint,
				verified,
				via,
			} => {
				let done = self.ports.peers.replace_pinned_key(
					peer,
					&fingerprint,
					verified,
					via.as_deref(),
				)?;
				let mut events = self.directory.forget_peer(done.previous);
				events.extend(self.directory.flag(PeerInfo {
					display_name: done.name,
					trust: PeerTrust::Verified,
					..PeerInfo::bare(done.id)
				}));
				events.extend(self.directory.clear_warning(KEY_CHANGED_WARNING));
				events.extend(self.directory.set_peers(self.ports.peers.peers()));
				events.extend(
					self.directory
						.set_favorites(self.ports.settings.favorites()),
				);
				events.extend(
					self.directory
						.set_auto_answer(self.ports.settings.auto_answer()),
				);
				self.publish(events);
			}
			ServiceOp::SetFavorites(ordered) => {
				self.ports.settings.set_favorites(&ordered)?;
				let events = self
					.directory
					.set_favorites(self.ports.settings.favorites());
				self.publish(events);
			}
			ServiceOp::SetAutoAnswer { peer, enabled } => {
				self.ports.settings.set_auto_answer(peer, enabled)?;
				let events = self
					.directory
					.set_auto_answer(self.ports.settings.auto_answer());
				self.publish(events);
			}
			ServiceOp::Trust(request) => {
				self.ports.peers.decide_trust(request)?;
				let mut events = self.directory.set_peers(self.ports.peers.peers());
				events.extend(
					self.directory
						.set_favorites(self.ports.settings.favorites()),
				);
				events.extend(
					self.directory
						.set_auto_answer(self.ports.settings.auto_answer()),
				);
				self.publish(events);
			}
			ServiceOp::Privacy { inbound, advert } => {
				let aux = &self.directory.aux;
				let (inbound, advert) = (
					inbound.unwrap_or(aux.inbound_policy),
					advert.unwrap_or(aux.advert_mode),
				);
				self.ports
					.settings
					.save(&SettingsChange::Privacy { inbound, advert })?;
				self.directory.aux.inbound_policy = inbound;
				self.directory.aux.advert_mode = advert;
				self.advertise_profile();
			}
			ServiceOp::Controls {
				microphone_gain,
				release_delay_ms,
				autostart_daemon,
				ring,
			} => {
				if !ring.is_empty() {
					let next = ring.apply(self.directory.aux.ring);
					self.ports.settings.save(&SettingsChange::Ring(next))?;
					self.directory.aux.ring = next;
				}
				if let Some(on) = autostart_daemon {
					self.ports
						.settings
						.save(&SettingsChange::AutostartDaemon(on))?;
					self.directory.aux.autostart_daemon = on;
				}
				if let Some(delay) = release_delay_ms {
					self.ports
						.settings
						.save(&SettingsChange::PttReleaseDelay(delay))?;
				}
				if let Some(gain) = microphone_gain {
					self.ports.media.set_microphone_gain(gain)?;
					self.directory.aux.microphone_gain = gain;
				}
				self.publish_settings();
			}
		}
		Ok(())
	}

	fn advertise_profile(&mut self) {
		self.ports
			.peers
			.profile_changed(&self.directory.aux.profile());
		self.publish_settings();
	}
}

fn failed(id: OperationId, problem: Problem, revision: u64) -> Operation {
	let mut operation = operation_pb(id, OperationState::Failed, revision);
	operation.problem = Some(problem);
	operation
}

fn call_of(command: &Command) -> Option<relay_core::CallId> {
	match command {
		Command::StartCall { call, .. }
		| Command::AcceptCall { call, .. }
		| Command::DeclineCall { call, .. }
		| Command::EndCall { call, .. }
		| Command::AcquireController { call, .. }
		| Command::ReleaseController { call, .. }
		| Command::SetTransmitMode { call, .. }
		| Command::SetCallControls { call, .. } => Some(*call),
		_ => None,
	}
}
