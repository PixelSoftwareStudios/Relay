//! The published transmit gate, read lock-free by the media thread.

use std::collections::BTreeMap;
use std::sync::Arc;

use arc_swap::{ArcSwap, Guard};
use relay_core::{CallId, GateSnapshot, Revision};

/// All current gates as of one revision. Replaced atomically as a whole.
#[derive(Debug, Clone, Default)]
pub struct GateTable {
	revision: Revision,
	gates: BTreeMap<CallId, GateSnapshot>,
}

impl GateTable {
	/// Revision at which the newest entry became current.
	pub fn revision(&self) -> Revision {
		self.revision
	}

	/// The gate of a call, if one was published.
	pub fn get(&self, call: CallId) -> Option<&GateSnapshot> {
		self.gates.get(&call)
	}

	/// Whether `call` may send right now. Unknown calls are closed.
	pub fn is_open(&self, call: CallId) -> bool {
		self.get(call).is_some_and(|gate| gate.open)
	}

	/// Number of published gates.
	pub fn len(&self) -> usize {
		self.gates.len()
	}

	/// Whether no gate is published.
	pub fn is_empty(&self) -> bool {
		self.gates.is_empty()
	}
}

/// Writer side, owned by the control actor.
#[derive(Debug, Clone, Default)]
pub struct GatePublisher {
	table: Arc<ArcSwap<GateTable>>,
}

impl GatePublisher {
	/// A reader for the media side.
	pub fn reader(&self) -> GateReader {
		GateReader {
			table: Arc::clone(&self.table),
		}
	}

	/// Publishes one gate atomically.
	pub fn publish(&self, gate: GateSnapshot, revision: Revision) {
		let mut next = GateTable::clone(&self.table.load());
		next.gates.insert(gate.call, gate);
		next.revision = revision;
		self.table.store(Arc::new(next));
	}

	/// Drops the gate of a forgotten call.
	pub fn forget(&self, call: CallId) {
		let mut next = GateTable::clone(&self.table.load());
		if next.gates.remove(&call).is_some() {
			self.table.store(Arc::new(next));
		}
	}
}

/// Reader side. Loading never blocks and never allocates.
#[derive(Debug, Clone)]
pub struct GateReader {
	table: Arc<ArcSwap<GateTable>>,
}

impl GateReader {
	/// The current table.
	pub fn load(&self) -> Guard<Arc<GateTable>> {
		self.table.load()
	}
}
