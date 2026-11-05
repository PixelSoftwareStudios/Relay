//! The transmit gate as seen by the media send path.
//!
//! The domain publishes a [`GateSnapshot`] per call. The send path reads the
//! latest one with a single atomic load immediately before every packet, so a
//! deny takes effect without waiting for any queue.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;
use relay_core::{AudioGeneration, CallId, GateSnapshot, Revision, RouteGeneration};

use crate::service::GateReader;

const PENDING_MAX: usize = 64;

#[derive(Debug, Clone, Copy)]
struct Versioned {
	gate: GateSnapshot,
	revision: Revision,
}

/// The latest gate of one call. Cheap to read from a worker thread.
#[derive(Debug)]
pub struct GateCell {
	call: CallId,
	latest: ArcSwap<Versioned>,
	/// When set, the domain's published table is the source of truth and
	/// `latest` is ignored.
	external: Option<GateReader>,
	retired: AtomicBool,
}

/// What a frame was captured under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameStamp {
	/// Capture generation the frame came from.
	pub capture: AudioGeneration,
	/// Route generation current when the frame left the capture reader.
	pub route: RouteGeneration,
	/// The gate was open when this poll began, was already open at the previous
	/// poll, and the worker was not stalled: only then can the audio read by
	/// this poll have been captured after the gate opened.
	pub clean: bool,
}

impl GateCell {
	/// A closed cell for `call`: nothing is sent until a snapshot opens it.
	pub fn closed(call: CallId) -> Self {
		let gate = GateSnapshot {
			call,
			open: false,
			deny: None,
			route: RouteGeneration::ZERO,
			capture: AudioGeneration::ZERO,
		};
		Self::with(gate, Revision::ZERO, None)
	}

	fn with(gate: GateSnapshot, revision: Revision, external: Option<GateReader>) -> Self {
		Self {
			call: gate.call,
			latest: ArcSwap::from_pointee(Versioned { gate, revision }),
			external,
			retired: AtomicBool::new(false),
		}
	}

	fn deny_all(&self) -> GateSnapshot {
		GateSnapshot {
			call: self.call,
			open: false,
			deny: None,
			route: RouteGeneration::ZERO,
			capture: AudioGeneration::ZERO,
		}
	}

	/// Permanently closes the cell.
	pub fn retire(&self) {
		self.retired.store(true, Ordering::Release);
	}

	/// Installs `gate` unless a newer revision is already current.
	pub fn publish(&self, gate: GateSnapshot, revision: Revision) {
		self.latest.rcu(|current| {
			if revision >= current.revision {
				Arc::new(Versioned { gate, revision })
			} else {
				Arc::clone(current)
			}
		});
	}

	/// The latest snapshot (one atomic load, no allocation).
	pub fn load(&self) -> GateSnapshot {
		if self.retired.load(Ordering::Acquire) {
			return self.deny_all();
		}
		match &self.external {
			Some(reader) => reader
				.load()
				.get(self.call)
				.copied()
				.unwrap_or_else(|| self.deny_all()),
			None => self.latest.load().gate,
		}
	}

	/// The route generation a frame read now would be stamped with.
	pub fn route(&self) -> RouteGeneration {
		self.load().route
	}

	/// Whether a frame with `stamp` may be emitted right now: the gate is open
	/// and both generations still match the latest update.
	pub fn permits(&self, stamp: FrameStamp) -> bool {
		let gate = self.load();
		gate.open && stamp.clean && gate.route == stamp.route && gate.capture == stamp.capture
	}
}

/// All calls' gate cells. The control side publishes; workers hold the cells.
#[derive(Debug, Default)]
pub struct GateBoard {
	inner: Mutex<BoardInner>,
	external: Option<GateReader>,
}

#[derive(Debug, Default)]
struct BoardInner {
	cells: HashMap<CallId, Arc<GateCell>>,
	pending: HashMap<CallId, Versioned>,
	order: VecDeque<CallId>,
}

impl GateBoard {
	/// A shared, empty board fed by `publish`.
	pub fn new() -> Arc<Self> {
		Arc::new(Self::default())
	}

	/// A board that reads the control actor's published table instead of
	/// being published to: every packet sees the actor's latest gate.
	pub fn following(reader: GateReader) -> Arc<Self> {
		Arc::new(Self {
			inner: Mutex::default(),
			external: Some(reader),
		})
	}

	fn lock(&self) -> std::sync::MutexGuard<'_, BoardInner> {
		self.inner.lock().unwrap_or_else(|p| p.into_inner())
	}

	/// Applies a `GateUpdate` effect. Updates that arrive before the call's
	/// media is prepared are kept (bounded) and applied at preparation.
	pub fn publish(&self, gate: GateSnapshot, revision: Revision) {
		let mut inner = self.lock();
		if let Some(cell) = inner.cells.get(&gate.call) {
			cell.publish(gate, revision);
			return;
		}
		let call = gate.call;
		let newer = inner
			.pending
			.get(&call)
			.is_none_or(|p| revision >= p.revision);
		if newer {
			if inner
				.pending
				.insert(call, Versioned { gate, revision })
				.is_none()
			{
				inner.order.push_back(call);
			}
			if inner.order.len() > PENDING_MAX
				&& let Some(oldest) = inner.order.pop_front()
			{
				inner.pending.remove(&oldest);
			}
		}
	}

	/// The call's cell, created on first use from any pending update.
	pub fn cell(&self, call: CallId) -> Arc<GateCell> {
		let mut inner = self.lock();
		if let Some(cell) = inner.cells.get(&call) {
			return Arc::clone(cell);
		}
		let cell = Arc::new(match inner.pending.remove(&call) {
			Some(v) => GateCell::with(v.gate, v.revision, self.external.clone()),
			None => {
				let mut cell = GateCell::closed(call);
				cell.external = self.external.clone();
				cell
			}
		});
		inner.order.retain(|c| *c != call);
		inner.cells.insert(call, Arc::clone(&cell));
		cell
	}

	/// Forgets a call. Its cell stays valid for any worker still holding it,
	/// but it is closed first so a late frame cannot pass.
	pub fn remove(&self, call: CallId) {
		let mut inner = self.lock();
		if let Some(cell) = inner.cells.remove(&call) {
			cell.retire();
		}
		inner.pending.remove(&call);
		inner.order.retain(|c| *c != call);
	}
}
