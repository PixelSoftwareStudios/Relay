//! Transmit consent, call controls and push-to-talk leases.

use super::State;
use crate::call::{CallState, Gain, InputLease, RouteMode, TransmitRoute};
use crate::command::PttUpdate;
use crate::error::Reject;
use crate::event::{Event, InputFault};
use crate::id::{CallId, ClientId, ControllerLeaseId};
use crate::outcome::{OpResult, Reply};
use crate::time::MonoTime;

impl State {
	pub(super) fn set_route(
		&mut self,
		client: ClientId,
		call: CallId,
		lease: ControllerLeaseId,
		route: TransmitRoute,
	) -> OpResult {
		if !route.is_valid() {
			return Err(Reject::InvalidRoute);
		}
		let id = self.authorize(client, call, lease)?;
		let entry = self.calls.get(&id).ok_or(Reject::UnknownCall(call))?;
		if route == entry.route {
			return Ok(Reply::Done);
		}
		if route.is_authorized() {
			if entry.route_exhausted {
				return Err(Reject::GenerationExhausted);
			}
			if !matches!(entry.state, CallState::Connecting | CallState::Active) {
				return Err(Reject::CallNotActive);
			}
		}
		self.close_leases(id, InputFault::RouteChanged);
		if let Some(entry) = self.calls.get_mut(&id) {
			entry.route = route;
			entry.advance_route_generation();
		}
		self.mark_dirty(id);
		Ok(Reply::Done)
	}

	/// Withdraws transmit consent and closes leases; a new route generation
	/// invalidates anything issued under the old one.
	pub(crate) fn reset_route(&mut self, call: CallId, fault: InputFault) {
		self.close_leases(call, fault);
		let Some(entry) = self.calls.get_mut(&call) else {
			return;
		};
		if entry.route != TransmitRoute::RECEIVE_ONLY {
			entry.route = TransmitRoute::RECEIVE_ONLY;
			entry.advance_route_generation();
			self.mark_dirty(call);
		}
	}

	pub(super) fn set_call_controls(
		&mut self,
		client: ClientId,
		call: CallId,
		lease: ControllerLeaseId,
		muted: Option<bool>,
		deafened: Option<bool>,
		gain: Option<Gain>,
	) -> OpResult {
		let id = self.authorize(client, call, lease)?;
		let entry = self.calls.get_mut(&id).ok_or(Reject::UnknownCall(call))?;
		let before = entry.controls;
		entry.controls.muted = muted.unwrap_or(before.muted);
		entry.controls.deafened = deafened.unwrap_or(before.deafened);
		entry.controls.gain = gain.unwrap_or(before.gain);
		if entry.controls != before {
			self.mark_dirty(id);
		}
		Ok(Reply::Done)
	}

	pub(super) fn ptt(&mut self, client: ClientId, update: PttUpdate, now: MonoTime) -> OpResult {
		let session = self.clients.get(&client).ok_or(Reject::UnknownClient)?;
		let last = session.sources.get(&update.source).copied();
		if let Some(last) = last
			&& update.sequence <= last
		{
			return Err(Reject::StaleSequence { last });
		}
		if last.is_none() && session.sources.len() >= self.limits.input_sources_per_client {
			return Err(Reject::TooManyInputSources);
		}
		if update.pressed {
			self.ptt_press(client, update, now)
		} else {
			self.record_sequence(client, &update);
			self.ptt_release(client, &update);
			Ok(Reply::Done)
		}
	}

	fn record_sequence(&mut self, client: ClientId, update: &PttUpdate) {
		if let Some(session) = self.clients.get_mut(&client) {
			session.sources.insert(update.source, update.sequence);
		}
	}

	/// Release always takes effect, even if the call or lease is stale.
	fn ptt_release(&mut self, client: ClientId, update: &PttUpdate) {
		let id = self.resolve(update.call);
		let removed = self
			.calls
			.get_mut(&id)
			.is_some_and(|call| call.input_leases.remove(&(client, update.source)).is_some());
		if removed {
			self.mark_dirty(id);
		}
	}

	fn ptt_press(&mut self, client: ClientId, update: PttUpdate, now: MonoTime) -> OpResult {
		let id = self.authorize(client, update.call, update.lease)?;
		let ttl = match update.ttl {
			None => self.limits.ptt_lease_default,
			Some(ttl) if ttl.is_zero() => return Err(Reject::InvalidTtl),
			Some(ttl) => ttl.min(self.limits.ptt_lease_max),
		};
		if !self.session.allowed() {
			return Err(Reject::SessionBlocked);
		}
		let entry = self
			.calls
			.get(&id)
			.ok_or(Reject::UnknownCall(update.call))?;
		if entry.state != CallState::Active {
			return Err(Reject::CallNotActive);
		}
		if entry.route.mode != RouteMode::PushToTalk {
			return Err(Reject::RouteNotAuthorized);
		}
		let (route_generation, key) = (entry.route_generation, (client, update.source));
		let existing = entry.input_leases.get(&key).map(|lease| lease.id);
		let lease_id = existing.unwrap_or(self.next_input);
		if existing.is_none() {
			self.next_input = lease_id.next().unwrap_or(lease_id);
		}
		let expires_at = now.saturating_add(ttl);
		self.record_sequence(client, &update);
		let entry = self
			.calls
			.get_mut(&id)
			.ok_or(Reject::UnknownCall(update.call))?;
		entry.input_leases.insert(
			key,
			InputLease {
				id: lease_id,
				expires_at,
				controller: update.lease,
				route_generation,
			},
		);
		if existing.is_none() {
			self.mark_dirty(id);
		}
		Ok(Reply::Input {
			lease: lease_id,
			expires_at,
		})
	}

	/// Kills every lease whose expiry has passed. A missed refresh is an early
	/// mute, never a stuck-open microphone.
	pub(super) fn expire_leases(&mut self, now: MonoTime) {
		let mut expired = Vec::new();
		for call in self.calls.values_mut() {
			let before = call.input_leases.len();
			call.input_leases.retain(|_, lease| lease.expires_at > now);
			if call.input_leases.len() != before {
				expired.push(call.id);
			}
		}
		for id in expired {
			self.mark_dirty(id);
			self.push_event(Event::InputFault {
				call: id,
				fault: InputFault::Expired,
			});
		}
	}
}
