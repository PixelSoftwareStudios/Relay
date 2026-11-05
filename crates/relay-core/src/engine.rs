//! The state owner behind its bounded control queue.

use crate::command::Input;
use crate::error::QueueFull;
use crate::outcome::Outcome;
use crate::queue::BoundedQueue;
use crate::state::State;
use crate::time::MonoTime;

/// What happened to a submitted input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Submitted {
	/// A safety deny was applied immediately, ahead of the queue.
	Applied(Box<Outcome>),
	/// Queued for the next [`Engine::drain`].
	Queued,
}

/// A [`State`] fed through a bounded command queue.
///
/// Inputs that only close gates bypass the queue so a full queue can never
/// delay a mute, release, disconnect or fail-closed transition.
#[derive(Debug)]
pub struct Engine {
	state: State,
	queue: BoundedQueue<Input>,
}

impl Engine {
	/// Wraps a state with a queue sized from its limits.
	pub fn new(state: State) -> Self {
		let queue = BoundedQueue::new(state.limits().control_command_queue);
		Self { state, queue }
	}

	/// Read access to the state.
	pub fn state(&self) -> &State {
		&self.state
	}

	/// Number of queued inputs.
	pub fn queued(&self) -> usize {
		self.queue.len()
	}

	/// Submits an input. A full queue yields [`QueueFull`] (resource
	/// exhausted); safety denies are never refused.
	pub fn submit(&mut self, input: Input, now: MonoTime) -> Result<Submitted, QueueFull> {
		if input.is_safety_deny() {
			return Ok(Submitted::Applied(Box::new(self.state.apply(input, now))));
		}
		self.queue.push(input).map(|()| Submitted::Queued)
	}

	/// Applies every queued input in order.
	pub fn drain(&mut self, now: MonoTime) -> Vec<Outcome> {
		let mut outcomes = Vec::with_capacity(self.queue.len());
		while let Some(input) = self.queue.pop() {
			outcomes.push(self.state.apply(input, now));
		}
		outcomes
	}
}
