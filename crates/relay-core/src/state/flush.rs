//! End-of-apply reconciliation: capture need, published gates and mixes,
//! revisioned events, and effect ordering.

use super::State;
use crate::call::{Call, CallState};
use crate::effect::{Effect, GateSnapshot, ReceiveMix};
use crate::event::{CallView, Event, Revisioned};
use crate::gate::{self, GateInputs};
use crate::id::CallId;
use crate::outcome::{OpResult, Outcome};

impl State {
	pub(super) fn finish(&mut self, result: OpResult, duplicate: bool) -> Outcome {
		self.reconcile_capture();
		let (gates, mixes) = self.refresh_published();
		self.emit_events();
		let revision = self.revision;
		let mut effects: Vec<Effect> = gates
			.into_iter()
			.map(|gate| Effect::GateUpdate { gate, revision })
			.collect();
		effects.extend(mixes.into_iter().map(Effect::ReceiveMix));
		effects.append(&mut self.scratch.effects);
		// Deny gates are published before anything else is acknowledged.
		effects.sort_by_key(|effect| !effect.is_gate());
		Outcome {
			result,
			revision,
			duplicate,
			effects,
		}
	}

	fn append_event(&mut self, event: Event) {
		self.revision = self.revision.next().unwrap_or(self.revision);
		self.log.push(Revisioned {
			revision: self.revision,
			event,
		});
	}

	fn emit_events(&mut self) {
		for event in std::mem::take(&mut self.scratch.events) {
			self.append_event(event);
		}
		if std::mem::take(&mut self.scratch.dirty_global) {
			let view = self.global_view();
			self.append_event(Event::GlobalUpdated(view));
		}
		for id in std::mem::take(&mut self.scratch.dirty_calls) {
			if let Some(call) = self.calls.get(&id) {
				let view = CallView::of(call);
				self.append_event(Event::CallUpdated(view));
			}
		}
	}

	/// Recomputes every call's gate and receive mix; returns the changes.
	fn refresh_published(&mut self) -> (Vec<GateSnapshot>, Vec<ReceiveMix>) {
		let ids: Vec<CallId> = self.calls.keys().copied().collect();
		let mut gates = Vec::new();
		let mut mixes = Vec::new();
		for id in ids {
			let Some(call) = self.calls.get(&id) else {
				continue;
			};
			let gate = self.gate_snapshot(call);
			let mix = self.receive_mix(call);
			let Some(call) = self.calls.get_mut(&id) else {
				continue;
			};
			if call.published.gate != Some(gate) {
				call.published.gate = Some(gate);
				gates.push(gate);
				self.scratch.dirty_calls.insert(id);
			}
			if call.published.mix != Some(mix) {
				call.published.mix = Some(mix);
				mixes.push(mix);
			}
		}
		(gates, mixes)
	}

	pub(crate) fn gate_snapshot(&self, call: &Call) -> GateSnapshot {
		let inputs = GateInputs {
			global_muted: self.globals.muted,
			global_deafened: self.globals.deafened,
			session_allowed: self.session.allowed(),
			capture_ready: self.capture.state == crate::event::CaptureState::Ready,
			call_active: call.state == CallState::Active,
			link_up: self.links.contains(&call.peer),
			route: call.route.mode,
			authority_valid: call.controller.is_some() || call.route.background,
			call_muted: call.controls.muted,
			call_deafened: call.controls.deafened,
			ptt_held: !call.input_leases.is_empty(),
		};
		let decision = gate::evaluate(&inputs);
		GateSnapshot {
			call: call.id,
			open: decision.is_open(),
			deny: decision.deny(),
			route: call.route_generation,
			capture: self.capture.generation,
		}
	}

	fn receive_mix(&self, call: &Call) -> ReceiveMix {
		ReceiveMix {
			call: call.id,
			audible: call.state == CallState::Active
				&& !self.globals.deafened
				&& !call.controls.deafened,
			gain: call.controls.gain,
			master: self.globals.output_gain,
		}
	}
}
