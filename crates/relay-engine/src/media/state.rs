//! Bookkeeping of prepared calls and per-peer sockets.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use relay_core::{CallId, PeerId, SessionId};

use super::net::PeerSocket;
use super::recv::RecvStats;
use super::send::{CallSender, SendStats};

/// Where a call is in its media lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
	/// Resources are being created.
	Preparing,
	/// Ready locally; waiting for the remote's stream parameters and candidates.
	Prepared,
	/// Streams are installed and the path is being validated.
	Activating,
	/// Streams run; the path was validated.
	Active,
}

/// The remote side's announced stream.
#[derive(Debug, Clone, Copy)]
pub struct RemoteReady {
	pub ssrc: u32,
	pub generation: u32,
}

/// Everything the control side keeps for one call.
pub struct CallMedia {
	pub peer: PeerId,
	pub session: SessionId,
	pub phase: Phase,
	pub ssrc: u32,
	pub generation: u32,
	pub remote: Option<RemoteReady>,
	/// Held until activation hands it to the capture worker.
	pub sender: Option<Box<CallSender>>,
	pub send_stats: Option<Arc<SendStats>>,
	pub recv_stats: Option<Arc<RecvStats>>,
}

/// Everything the control side keeps for one peer.
pub struct PeerMedia {
	pub socket: PeerSocket,
	pub epoch: [u8; 16],
	pub remote_candidates: Option<Vec<SocketAddr>>,
}

/// Mutable state of the media system, behind one lock.
#[derive(Default)]
pub struct State {
	pub calls: HashMap<CallId, CallMedia>,
	pub peers: HashMap<PeerId, PeerMedia>,
	/// Candidates and readiness that arrived before the local side was ready.
	pub early_offers: HashMap<PeerId, ([u8; 16], Vec<SocketAddr>)>,
	pub early_ready: HashMap<CallId, ([u8; 16], RemoteReady)>,
	pub playback_open: bool,
	pub next_source: u64,
}

/// A point-in-time view for tests and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallReport {
	pub phase: Phase,
	pub sent: u64,
	pub gate_closed: u64,
	pub stale: u64,
	pub received_voice: u64,
	pub received_concealed: u64,
	pub misbound: u64,
}
