//! Controller lease ownership and takeover.

use super::State;
use crate::call::{CallState, ControllerLease, RouteMode};
use crate::error::Reject;
use crate::event::InputFault;
use crate::id::{CallId, ClientId, ControllerLeaseId};
use crate::outcome::{OpResult, Reply};

impl State {
	/// Resolves a call and checks that `client` holds `lease` on it.
	///
	/// A finished call yields [`Reject::CallTerminal`] so callers can decide
	/// whether the operation is idempotent for terminal calls.
	pub(crate) fn authorize(
		&self,
		client: ClientId,
		call: CallId,
		lease: ControllerLeaseId,
	) -> Result<CallId, Reject> {
		let id = self.resolve(call);
		let entry = self.calls.get(&id).ok_or(Reject::UnknownCall(call))?;
		if entry.state.is_terminal() {
			return Err(Reject::CallTerminal(entry.state));
		}
		match entry.controller {
			Some(held) if held.id == lease && held.holder == client => Ok(id),
			other => Err(Reject::StaleController {
				current: other.map(|held| held.id),
			}),
		}
	}

	pub(crate) fn grant_lease(&mut self, holder: ClientId) -> ControllerLease {
		let id = self.next_controller;
		self.next_controller = id.next().unwrap_or(id);
		ControllerLease { id, holder }
	}

	pub(super) fn acquire_controller(
		&mut self,
		client: ClientId,
		call: CallId,
		takeover: bool,
	) -> OpResult {
		let id = self.resolve(call);
		let entry = self.calls.get(&id).ok_or(Reject::UnknownCall(call))?;
		match entry.state {
			state if state.is_terminal() => return Err(Reject::CallTerminal(state)),
			CallState::Ending => {
				return Err(Reject::InvalidState {
					state: CallState::Ending,
				});
			}
			_ => {}
		}
		match entry.controller {
			Some(held) if held.holder == client => Ok(Reply::Controller(held.id)),
			Some(held) if !takeover && held.holder != ClientId::HOUSE => {
				Err(Reject::ControllerHeld {
					holder: held.holder,
				})
			}
			Some(_) => {
				self.close_leases(id, InputFault::Takeover);
				let lease = self.grant_lease(client);
				self.set_controller(id, Some(lease), InputFault::Takeover);
				Ok(Reply::Controller(lease.id))
			}
			None => {
				let lease = self.grant_lease(client);
				self.set_controller(id, Some(lease), InputFault::ControllerLost);
				Ok(Reply::Controller(lease.id))
			}
		}
	}

	pub(super) fn release_controller(
		&mut self,
		client: ClientId,
		call: CallId,
		lease: ControllerLeaseId,
	) -> OpResult {
		let id = self.authorize(client, call, lease)?;
		self.lose_controller(id, InputFault::ControllerLost);
		Ok(Reply::Done)
	}

	/// Drops the controller of a call, failing closed.
	pub(crate) fn lose_controller(&mut self, call: CallId, fault: InputFault) {
		self.set_controller(call, None, fault);
	}

	/// Installs a controller (or none). Input leases never carry over; the
	/// transmit route is withdrawn unless it is background open-mic.
	pub(crate) fn set_controller(
		&mut self,
		call: CallId,
		controller: Option<ControllerLease>,
		fault: InputFault,
	) {
		self.close_leases(call, fault);
		let Some(entry) = self.calls.get_mut(&call) else {
			return;
		};
		entry.controller = controller;
		let keeps_route = entry.route.mode == RouteMode::Unmuted && entry.route.background;
		if !keeps_route {
			self.reset_route(call, fault);
		}
		self.mark_dirty(call);
	}
}
