//! Snapshot and no-gap watch.

use std::collections::BTreeSet;

use super::State;
use crate::event::{CallView, Cursor, Snapshot, Watch};

impl State {
	/// A consistent full view as of the current revision.
	pub fn snapshot(&self) -> Snapshot {
		Snapshot {
			incarnation: self.incarnation,
			revision: self.revision,
			global: self.global_view(),
			clients: self.clients.keys().copied().collect::<BTreeSet<_>>(),
			links: self.links.clone(),
			calls: self
				.calls
				.values()
				.map(|call| (call.id, CallView::of(call)))
				.collect(),
		}
	}

	/// Events after `cursor` with no gap, or a snapshot to resynchronize from
	/// when the cursor is from another incarnation, ahead of us, outside the
	/// replay window, or lagging by more than the per-client queue.
	pub fn watch(&self, cursor: Cursor) -> Watch {
		let usable = cursor.incarnation == self.incarnation && cursor.revision <= self.revision;
		let events = usable
			.then(|| {
				self.log
					.after(cursor.revision, self.limits.client_lifecycle_queue)
			})
			.flatten();
		match events {
			Some(events) => Watch::Events(events),
			None => Watch::Resync(Box::new(self.snapshot())),
		}
	}
}
