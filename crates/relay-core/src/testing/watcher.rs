use crate::event::{Cursor, Snapshot, Watch};
use crate::state::State;

/// A fake client view that follows the engine through snapshot + watch.
#[derive(Debug, Clone)]
pub struct FakeWatcher {
	/// The view this client maintains.
	pub view: Snapshot,
	/// How many times it had to resynchronize from a snapshot.
	pub resyncs: usize,
}

impl FakeWatcher {
	/// Starts from the current snapshot.
	pub fn attach(state: &State) -> Self {
		Self {
			view: state.snapshot(),
			resyncs: 0,
		}
	}

	/// Catches up with the engine, resynchronizing if it fell behind.
	pub fn sync(&mut self, state: &State) {
		match state.watch(self.view.cursor()) {
			Watch::Events(events) => {
				for item in &events {
					self.view
						.apply(item)
						.expect("watch must deliver a gap-free sequence");
				}
			}
			Watch::Resync(snapshot) => {
				self.view = *snapshot;
				self.resyncs += 1;
			}
		}
	}

	/// The position the client will continue from.
	pub fn cursor(&self) -> Cursor {
		self.view.cursor()
	}
}
