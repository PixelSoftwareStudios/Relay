//! UDP path validation: a fresh authenticated challenge must be echoed from the candidate
//! endpoint before media is sent there.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use rand::RngExt;

use super::datagram::CHALLENGE_BYTES;
use crate::limits::UDP_PATH_TIMEOUT;
use crate::session::LinkEpoch;

const OUTSTANDING_MAX: usize = 8;

#[derive(Debug)]
struct Pending {
	challenge: [u8; CHALLENGE_BYTES],
	addr: SocketAddr,
	epoch: LinkEpoch,
	issued: Instant,
}

#[derive(Debug, Default)]
pub struct PathValidator {
	outstanding: Vec<Pending>,
	validated: Option<SocketAddr>,
}

impl PathValidator {
	pub fn new() -> Self {
		Self::default()
	}

	/// The destination media may be sent to, if any has been validated.
	pub fn validated(&self) -> Option<SocketAddr> {
		self.validated
	}

	/// Forgets the validated destination, e.g. after an interface or address change.
	pub fn invalidate(&mut self) {
		self.validated = None;
		self.outstanding.clear();
	}

	/// Starts validation of `addr` under `epoch`. Returns the fresh challenge to send.
	pub fn begin(
		&mut self,
		addr: SocketAddr,
		epoch: LinkEpoch,
		now: Instant,
	) -> [u8; CHALLENGE_BYTES] {
		self.outstanding
			.retain(|p| now.duration_since(p.issued) < UDP_PATH_TIMEOUT);
		if self.outstanding.len() >= OUTSTANDING_MAX {
			self.outstanding.remove(0);
		}
		let mut challenge = [0u8; CHALLENGE_BYTES];
		rand::rng().fill(&mut challenge[..]);
		self.outstanding.push(Pending {
			challenge,
			addr,
			epoch,
			issued: now,
		});
		challenge
	}

	/// Handles an authenticated response. It validates a path only when the echoed value
	/// matches an unexpired challenge, arrives from the challenged endpoint, and was
	/// authenticated under the epoch the challenge was issued for.
	pub fn on_response(
		&mut self,
		challenge: &[u8; CHALLENGE_BYTES],
		from: SocketAddr,
		epoch: &LinkEpoch,
		now: Instant,
	) -> Option<SocketAddr> {
		let index = self.outstanding.iter().position(|p| {
			&p.challenge == challenge
				&& p.addr == from
				&& &p.epoch == epoch
				&& now.duration_since(p.issued) < UDP_PATH_TIMEOUT
		})?;
		self.outstanding.remove(index);
		self.validated = Some(from);
		Some(from)
	}
}

/// Caps how many path responses are sent per second so a flood of authenticated challenges
/// (or replays routed to us) cannot turn this endpoint into a reflector.
#[derive(Debug)]
pub struct ResponseLimiter {
	window_start: Instant,
	sent: u32,
	per_second: u32,
}

impl ResponseLimiter {
	pub fn new(per_second: u32, now: Instant) -> Self {
		Self {
			window_start: now,
			sent: 0,
			per_second,
		}
	}

	pub fn allow(&mut self, now: Instant) -> bool {
		if now.duration_since(self.window_start) >= Duration::from_secs(1) {
			self.window_start = now;
			self.sent = 0;
		}
		if self.sent < self.per_second {
			self.sent += 1;
			true
		} else {
			false
		}
	}
}
