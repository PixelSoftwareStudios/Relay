use crate::command::{Input, SystemEvent};
use crate::effect::Effect;
use crate::id::AudioGeneration;

/// Fake capture device adapter. It records what it was asked to do and lets a
/// test choose when (and whether) each completion arrives.
#[derive(Debug, Default)]
pub struct FakeDevice {
	/// Generations that were requested to open, in order.
	pub opened: Vec<AudioGeneration>,
	/// Generations that were requested to close, in order.
	pub closed: Vec<AudioGeneration>,
	pending: Vec<AudioGeneration>,
}

impl FakeDevice {
	/// Observes an effect; returns whether it was a device effect.
	pub fn observe(&mut self, effect: &Effect) -> bool {
		match *effect {
			Effect::OpenCapture { generation } => {
				self.opened.push(generation);
				self.pending.push(generation);
				true
			}
			Effect::CloseCapture { generation } => {
				self.closed.push(generation);
				self.pending.retain(|pending| *pending != generation);
				true
			}
			_ => false,
		}
	}

	/// Opens still awaiting completion.
	pub fn pending(&self) -> &[AudioGeneration] {
		&self.pending
	}

	/// Completion for an open request (possibly a stale generation).
	pub fn ready(&self, generation: AudioGeneration) -> Input {
		Input::System(SystemEvent::CaptureReady { generation })
	}

	/// Failure of an open request.
	pub fn failed(&self, generation: AudioGeneration) -> Input {
		Input::System(SystemEvent::CaptureFailed { generation })
	}

	/// The device was unplugged while running.
	pub fn unplugged(&self, generation: AudioGeneration) -> Input {
		Input::System(SystemEvent::CaptureLost { generation })
	}
}
