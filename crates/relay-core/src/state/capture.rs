//! Shared capture path and device/session events.

use super::State;
use crate::call::CallState;
use crate::command::{SessionEvent, SystemEvent};
use crate::effect::Effect;
use crate::error::Reject;
use crate::event::{CaptureState, InputFault};
use crate::id::CallId;
use crate::outcome::{OpResult, Reply};

impl State {
	pub(super) fn on_system(&mut self, event: SystemEvent) -> OpResult {
		match event {
			SystemEvent::Session(session) => self.on_session(session),
			SystemEvent::CaptureReady { generation } => {
				self.complete_open(generation, CaptureState::Ready)
			}
			SystemEvent::CaptureFailed { generation } => {
				self.complete_open(generation, CaptureState::Lost)
			}
			SystemEvent::CaptureLost { generation } => self.capture_lost(generation),
			SystemEvent::PresenceChanged { attached } => {
				self.attached = attached;
				Ok(Reply::Done)
			}
			SystemEvent::MediaPrepared { call, session } => {
				let id = self.checked_session(call, session)?;
				let entry = self.calls.get_mut(&id).ok_or(Reject::UnknownCall(call))?;
				if entry.state != CallState::Connecting {
					return Err(Reject::InvalidState { state: entry.state });
				}
				entry.media_prepared = true;
				self.try_activate(id);
				Ok(Reply::Done)
			}
			SystemEvent::MediaStopped { call, session } => self.media_stopped(call, session),
		}
	}

	fn on_session(&mut self, event: SessionEvent) -> OpResult {
		match event {
			SessionEvent::Locked => self.session.locked = true,
			SessionEvent::Suspended => self.session.suspended = true,
			SessionEvent::Unlocked => self.session.locked = false,
			SessionEvent::Resumed => self.session.suspended = false,
		}
		self.scratch.dirty_global = true;
		if !self.session.allowed() {
			self.close_all_leases(InputFault::SessionBlocked);
		}
		Ok(Reply::Done)
	}

	fn complete_open(
		&mut self,
		generation: crate::id::AudioGeneration,
		outcome: CaptureState,
	) -> OpResult {
		if generation != self.capture.generation || self.capture.state != CaptureState::Opening {
			return Err(Reject::StaleGeneration);
		}
		if outcome == CaptureState::Lost {
			self.invalidate_capture(generation);
		} else {
			self.capture.state = outcome;
		}
		self.scratch.dirty_global = true;
		Ok(Reply::Done)
	}

	fn capture_lost(&mut self, generation: crate::id::AudioGeneration) -> OpResult {
		let open = matches!(
			self.capture.state,
			CaptureState::Opening | CaptureState::Ready
		);
		if generation != self.capture.generation || !open {
			return Err(Reject::StaleGeneration);
		}
		self.invalidate_capture(generation);
		self.close_all_leases(InputFault::DeviceInvalid);
		self.scratch.dirty_global = true;
		Ok(Reply::Done)
	}

	/// Advances the generation and waits for an explicit resume. No other
	/// device is ever picked automatically.
	fn invalidate_capture(&mut self, generation: crate::id::AudioGeneration) {
		self.push_effect(Effect::CloseCapture { generation });
		self.capture.state = CaptureState::Lost;
		match self.capture.generation.next() {
			Some(next) => self.capture.generation = next,
			None => self.capture.exhausted = true,
		}
	}

	pub(super) fn resume_capture(&mut self) -> OpResult {
		if self.capture.state != CaptureState::Lost {
			return Err(Reject::NothingToResume);
		}
		if self.capture.exhausted {
			return Err(Reject::GenerationExhausted);
		}
		self.capture.state = CaptureState::Closed;
		self.scratch.dirty_global = true;
		Ok(Reply::Done)
	}

	/// Opens capture only while some call has explicit transmit consent and
	/// the session allows it; closes it as soon as that stops being true.
	pub(super) fn reconcile_capture(&mut self) {
		let needed = self.session.allowed()
			&& self.calls.values().any(|call| {
				matches!(call.state, CallState::Connecting | CallState::Active)
					&& call.route.is_authorized()
			});
		match (needed, self.capture.state) {
			(true, CaptureState::Closed) => {
				let Some(next) = self.capture.generation.next() else {
					self.capture.exhausted = true;
					self.capture.state = CaptureState::Lost;
					self.scratch.dirty_global = true;
					return;
				};
				self.capture.generation = next;
				self.capture.state = CaptureState::Opening;
				self.push_effect(Effect::OpenCapture { generation: next });
				self.scratch.dirty_global = true;
			}
			(false, CaptureState::Opening | CaptureState::Ready) => {
				let generation = self.capture.generation;
				self.capture.state = CaptureState::Closed;
				self.push_effect(Effect::CloseCapture { generation });
				self.scratch.dirty_global = true;
			}
			_ => {}
		}
	}

	pub(crate) fn close_all_leases(&mut self, fault: InputFault) {
		let ids: Vec<CallId> = self.calls.keys().copied().collect();
		for id in ids {
			self.close_leases(id, fault);
		}
	}
}
