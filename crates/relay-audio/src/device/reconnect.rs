//! The recovery policy for a stream whose device went away underneath it.
//!
//! On a phone the route changes during a call all the time (the call mode
//! starts, a headset is plugged in, Bluetooth connects) and the platform
//! answers each one by disconnecting the open streams. That is not a broken
//! output: the stream must be closed and opened again on the new route,
//! quickly and without telling the user. Only an output that stays gone is a
//! problem worth a warning, and the warning is raised once.
//!
//! This is a pure state machine: it never touches a device or a clock. The
//! caller passes the time in milliseconds and performs the reopen it asks for.

/// Delays before each quick reopen attempt, in milliseconds. A route change
/// usually settles within a hundred milliseconds.
const QUICK_DELAYS_MS: [u64; 4] = [20, 100, 250, 500];
/// Delay between attempts once the user has been warned: a plugged-in headset
/// or a returning Bluetooth device brings the output back eventually.
const SLOW_DELAY_MS: u64 = 2_000;
/// A stream that fails again sooner than this after a reopen is still the same
/// outage: its failures keep counting toward the warning.
const STABLE_AFTER_MS: u64 = 2_000;

/// What the caller should do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Poll {
	/// Nothing: the stream is healthy or the next attempt is not due yet.
	Wait,
	/// Close the stream and open it again with the same request.
	Reopen,
}

/// What a reopen attempt changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
	/// The stream is back. `was_warned` says the user was told it was gone, so
	/// the warning must be withdrawn.
	Restored { was_warned: bool },
	/// Failed; another attempt follows.
	Retry,
	/// Failed enough times: tell the user now (once).
	Warn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
	Healthy {
		/// When the stream last came back, if it did.
		restored_at: Option<u64>,
		/// Failures of the outage the last restore ended.
		carried: u32,
	},
	Recovering {
		failures: u32,
		due: u64,
		warned: bool,
	},
}

/// Tracks one stream's outage and decides when to reopen and when to warn.
#[derive(Debug, Clone, Copy)]
pub struct Reconnect {
	phase: Phase,
}

impl Default for Reconnect {
	fn default() -> Self {
		Self::new()
	}
}

fn delay_after(failures: u32) -> u64 {
	QUICK_DELAYS_MS
		.get(failures as usize)
		.copied()
		.unwrap_or(SLOW_DELAY_MS)
}

impl Reconnect {
	/// A stream that is, as far as anyone knows, healthy.
	pub const fn new() -> Self {
		Self {
			phase: Phase::Healthy {
				restored_at: None,
				carried: 0,
			},
		}
	}

	/// True while an outage is being worked on (the stream is not delivering).
	pub fn recovering(&self) -> bool {
		matches!(self.phase, Phase::Recovering { .. })
	}

	/// True once the user has been told and not yet told it is back.
	pub fn warned(&self) -> bool {
		matches!(self.phase, Phase::Recovering { warned: true, .. })
	}

	/// Looks at the stream's health at `now` (milliseconds on any monotonic
	/// clock). `healthy` is false once the backend reported the stream lost.
	pub fn poll(&mut self, now: u64, healthy: bool) -> Poll {
		match self.phase {
			Phase::Healthy {
				restored_at,
				carried,
			} => {
				if healthy {
					return Poll::Wait;
				}
				let failures = match restored_at {
					Some(at) if now.saturating_sub(at) < STABLE_AFTER_MS => carried,
					_ => 0,
				};
				self.phase = Phase::Recovering {
					failures,
					due: now + delay_after(failures),
					warned: false,
				};
				self.poll(now, false)
			}
			Phase::Recovering { due, .. } => {
				if now >= due {
					Poll::Reopen
				} else {
					Poll::Wait
				}
			}
		}
	}

	/// Records the result of the reopen [`poll`](Self::poll) asked for.
	pub fn attempted(&mut self, now: u64, ok: bool) -> Outcome {
		let Phase::Recovering {
			failures, warned, ..
		} = self.phase
		else {
			return Outcome::Restored { was_warned: false };
		};
		if ok {
			self.phase = Phase::Healthy {
				restored_at: Some(now),
				carried: failures,
			};
			return Outcome::Restored { was_warned: warned };
		}
		let failures = failures + 1;
		let warn_now = !warned && failures as usize >= QUICK_DELAYS_MS.len();
		self.phase = Phase::Recovering {
			failures,
			due: now + delay_after(failures),
			warned: warned || warn_now,
		};
		if warn_now {
			Outcome::Warn
		} else {
			Outcome::Retry
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Drives the machine the way the device thread does: poll every 10 ms,
	/// reopen when asked, `attempts` says whether each reopen works.
	fn run(machine: &mut Reconnect, start: u64, attempts: &[bool]) -> Vec<(u64, Outcome)> {
		let mut now = start;
		let mut out = Vec::new();
		let mut next = attempts.iter();
		while out.len() < attempts.len() {
			if machine.poll(now, false) == Poll::Reopen {
				let ok = *next.next().unwrap();
				out.push((now, machine.attempted(now, ok)));
			}
			now += 10;
		}
		out
	}

	#[test]
	fn a_healthy_stream_is_left_alone() {
		let mut m = Reconnect::new();
		for t in 0..100 {
			assert_eq!(m.poll(t * 10, true), Poll::Wait);
		}
		assert!(!m.recovering());
	}

	#[test]
	fn a_route_change_reopens_within_a_few_tens_of_milliseconds_silently() {
		let mut m = Reconnect::new();
		let steps = run(&mut m, 1_000, &[true]);
		let (at, outcome) = steps[0];
		assert!(
			at - 1_000 <= 30,
			"reopened {} ms after the loss",
			at - 1_000
		);
		assert_eq!(outcome, Outcome::Restored { was_warned: false });
		assert!(!m.recovering());
	}

	#[test]
	fn a_second_quick_failure_still_recovers_without_a_warning() {
		let mut m = Reconnect::new();
		let steps = run(&mut m, 0, &[false, false, true]);
		assert_eq!(steps[0].1, Outcome::Retry);
		assert_eq!(steps[1].1, Outcome::Retry);
		assert_eq!(steps[2].1, Outcome::Restored { was_warned: false });
	}

	#[test]
	fn repeated_failure_warns_exactly_once_then_keeps_trying_slowly() {
		let mut m = Reconnect::new();
		let steps = run(&mut m, 0, &[false; 8]);
		let warns = steps.iter().filter(|(_, o)| *o == Outcome::Warn).count();
		assert_eq!(warns, 1);
		assert_eq!(steps[3].1, Outcome::Warn);
		assert!(m.warned());
		let gap = steps[7].0 - steps[6].0;
		assert!(gap >= SLOW_DELAY_MS, "slow retry gap was {gap} ms");
	}

	#[test]
	fn the_output_returning_after_a_warning_asks_for_the_warning_to_be_withdrawn() {
		let mut m = Reconnect::new();
		let mut attempts = vec![false; 5];
		attempts.push(true);
		let steps = run(&mut m, 0, &attempts);
		assert_eq!(
			steps.last().unwrap().1,
			Outcome::Restored { was_warned: true }
		);
		assert!(!m.warned());
	}

	#[test]
	fn a_stream_that_drops_again_right_after_a_reopen_counts_as_the_same_outage() {
		let mut m = Reconnect::new();
		let first = run(&mut m, 0, &[false, false, false, true]);
		let restored = first.last().unwrap().0;
		// Lost again 100 ms after coming back: the earlier failures still count.
		let second = run(&mut m, restored + 100, &[false]);
		assert_eq!(second[0].1, Outcome::Warn);
	}

	#[test]
	fn a_stream_that_was_stable_starts_a_fresh_outage() {
		let mut m = Reconnect::new();
		let first = run(&mut m, 0, &[false, false, false, true]);
		let restored = first.last().unwrap().0;
		let second = run(&mut m, restored + 10_000, &[false]);
		assert_eq!(second[0].1, Outcome::Retry);
	}
}
