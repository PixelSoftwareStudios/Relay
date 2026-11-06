//! Transmit consent, call controls and push-to-talk leases.

use super::State;
use crate::call::{CallState, Gain, InputLease, RouteMode, TransmitRoute};
use crate::command::PttUpdate;
use crate::error::Reject;
use crate::event::{Event, InputFault};
use crate::id::{CallId, ClientId, ControllerLeaseId};
use crate::limits::PTT_RELEASE_DELAY_MAX;
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
		if route.mode == RouteMode::PushToTalk && !self.globals.ptt_enabled {
			return Err(Reject::PttDisabled);
		}
		let entry = self.calls.get_mut(&id).ok_or(Reject::UnknownCall(call))?;
		entry.route_chosen = true;
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

	/// The default mode a connecting call gets: push-to-talk becomes muted while it is switched
	/// off. Muted is the fallback because it can never open the microphone by surprise.
	fn effective_default(&self) -> RouteMode {
		match self.globals.default_route {
			RouteMode::PushToTalk if !self.globals.ptt_enabled => RouteMode::Muted,
			mode => mode,
		}
	}

	/// Applies the default mode to a call that just became active, unless the user already
	/// chose one. The route generation advances and the gate is published with the new route,
	/// so audio captured before this point is never sent (RT-02).
	pub(crate) fn apply_default_route(&mut self, id: CallId) {
		let mode = self.effective_default();
		let Some(entry) = self.calls.get(&id) else {
			return;
		};
		if mode == RouteMode::Muted || entry.route_chosen || entry.route_exhausted {
			return;
		}
		self.close_leases(id, InputFault::RouteChanged);
		if let Some(entry) = self.calls.get_mut(&id) {
			entry.route = TransmitRoute {
				mode,
				background: false,
			};
			entry.advance_route_generation();
		}
		self.mark_dirty(id);
	}

	/// Stores the call defaults; switching push-to-talk off mutes the calls that use it.
	pub(super) fn set_call_defaults(&mut self, mode: RouteMode, ptt_enabled: bool) -> OpResult {
		self.globals.default_route = mode;
		self.globals.ptt_enabled = ptt_enabled;
		if !ptt_enabled {
			let ids: Vec<CallId> = self
				.calls
				.values()
				.filter(|c| c.route.mode == RouteMode::PushToTalk)
				.map(|c| c.id)
				.collect();
			for id in ids {
				self.reset_route(id, InputFault::RouteChanged);
			}
		}
		Ok(Reply::Done)
	}

	/// Withdraws transmit consent and closes leases; a new route generation
	/// invalidates anything issued under the old one.
	pub(crate) fn reset_route(&mut self, call: CallId, fault: InputFault) {
		self.close_leases(call, fault);
		let Some(entry) = self.calls.get_mut(&call) else {
			return;
		};
		if entry.route != TransmitRoute::MUTED {
			entry.route = TransmitRoute::MUTED;
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
		let denied = entry.controls.muted || entry.controls.deafened;
		if entry.controls != before {
			self.mark_dirty(id);
		}
		if denied {
			self.cancel_holds(id);
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
			self.ptt_release(client, &update, now);
			Ok(Reply::Done)
		}
	}

	fn record_sequence(&mut self, client: ClientId, update: &PttUpdate) {
		if let Some(session) = self.clients.get_mut(&client) {
			session.sources.insert(update.source, update.sequence);
		}
	}

	/// Release always takes effect, even if the call or lease is stale.
	///
	/// An ordinary key-up on an open gate holds the lease for the release
	/// delay; an immediate release (source retirement) closes at once.
	fn ptt_release(&mut self, client: ClientId, update: &PttUpdate, now: MonoTime) {
		let id = self.resolve(update.call);
		let key = (client, update.source);
		let delay = self.globals.ptt_release_delay.min(PTT_RELEASE_DELAY_MAX);
		let hold = !update.immediate
			&& !delay.is_zero()
			&& self
				.calls
				.get(&id)
				.is_some_and(|call| self.gate_snapshot(call).open);
		let Some(call) = self.calls.get_mut(&id) else {
			return;
		};
		let changed = match call.input_leases.get_mut(&key) {
			None => false,
			Some(lease) if hold && lease.holding => false,
			Some(lease) if hold => {
				lease.holding = true;
				lease.expires_at = now.saturating_add(delay);
				true
			}
			Some(_) => call.input_leases.remove(&key).is_some(),
		};
		if changed {
			self.mark_dirty(id);
		}
	}

	/// Ends pending release holds on a call without touching held keys.
	pub(crate) fn cancel_holds(&mut self, id: CallId) {
		let Some(call) = self.calls.get_mut(&id) else {
			return;
		};
		let before = call.input_leases.len();
		call.input_leases.retain(|_, lease| !lease.holding);
		if call.input_leases.len() != before {
			self.mark_dirty(id);
		}
	}

	pub(crate) fn cancel_all_holds(&mut self) {
		let ids: Vec<CallId> = self.calls.keys().copied().collect();
		for id in ids {
			self.cancel_holds(id);
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
		let was_holding = entry
			.input_leases
			.get(&key)
			.is_some_and(|lease| lease.holding);
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
				holding: false,
			},
		);
		if existing.is_none() || was_holding {
			self.mark_dirty(id);
		}
		Ok(Reply::Input {
			lease: lease_id,
			expires_at,
		})
	}

	/// Kills every lease whose expiry has passed. A missed refresh is an early
	/// mute, never a stuck-open microphone. A hold that runs out is the normal
	/// end of a release delay; a stalled press also cancels pending holds.
	pub(super) fn expire_leases(&mut self, now: MonoTime) {
		let mut stalled = Vec::new();
		let mut changed = Vec::new();
		for call in self.calls.values_mut() {
			let before = call.input_leases.len();
			let mut stale = false;
			call.input_leases.retain(|_, lease| {
				let live = lease.expires_at > now;
				stale |= !live && !lease.holding;
				live
			});
			if stale {
				stalled.push(call.id);
			}
			if call.input_leases.len() != before {
				changed.push(call.id);
			}
		}
		for id in stalled {
			self.cancel_holds(id);
			self.push_event(Event::InputFault {
				call: id,
				fault: InputFault::Expired,
			});
		}
		for id in changed {
			self.mark_dirty(id);
		}
	}
}
