//! Resource limits. Defaults are the design ceilings; runtime configuration
//! may tighten them but never exceed them.

use std::time::Duration;

use crate::error::LimitsError;

/// Hard maximum of the push-to-talk release delay.
pub const PTT_RELEASE_DELAY_MAX: Duration = Duration::from_millis(2000);

/// Bounds enforced by the state owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
	/// Simultaneous non-terminal calls.
	pub active_calls: usize,
	/// Attached local API clients.
	pub attached_clients: usize,
	/// Capacity of the control command queue.
	pub control_command_queue: usize,
	/// Events retained for replay.
	pub lifecycle_replay_events: usize,
	/// Maximum lag a subscriber may have before it must resynchronize.
	pub client_lifecycle_queue: usize,
	/// Retained operation results per client.
	pub operation_cache_per_client: usize,
	/// Retained operation results across all clients.
	pub operation_cache_global: usize,
	/// How long operation results are retained.
	pub operation_retention: Duration,
	/// Default push-to-talk lease length.
	pub ptt_lease_default: Duration,
	/// Hard maximum push-to-talk lease length.
	pub ptt_lease_max: Duration,
	/// Initial push-to-talk release delay (hold after an ordinary key-up).
	pub ptt_release_delay: Duration,
	/// Distinct input sources one client may use.
	pub input_sources_per_client: usize,
	/// Terminal calls kept for idempotent answers.
	pub retained_terminal_calls: usize,
	/// Remembered glare aliases.
	pub glare_aliases: usize,
	/// Time allowed for the secure link before a call fails.
	pub link_setup_timeout: Duration,
	/// Time the user has to answer a ringing call.
	pub ring_timeout: Duration,
	/// Time allowed for UDP path and media readiness.
	pub path_timeout: Duration,
	/// Time allowed for media teardown acknowledgement.
	pub teardown_timeout: Duration,
}

impl Default for Limits {
	fn default() -> Self {
		Self {
			active_calls: 8,
			attached_clients: 32,
			control_command_queue: 256,
			lifecycle_replay_events: 1024,
			client_lifecycle_queue: 256,
			operation_cache_per_client: 4096,
			operation_cache_global: 8192,
			operation_retention: Duration::from_secs(600),
			ptt_lease_default: Duration::from_millis(300),
			ptt_lease_max: Duration::from_millis(500),
			ptt_release_delay: Duration::from_millis(200),
			input_sources_per_client: 8,
			retained_terminal_calls: 64,
			glare_aliases: 64,
			link_setup_timeout: Duration::from_secs(5),
			ring_timeout: Duration::from_secs(30),
			path_timeout: Duration::from_secs(3),
			teardown_timeout: Duration::from_secs(3),
		}
	}
}

impl Limits {
	/// Checks that every bound is non-zero and does not exceed its ceiling.
	pub fn validate(&self) -> Result<(), LimitsError> {
		let ceiling = Self::default();
		let counts = [
			("active_calls", self.active_calls, ceiling.active_calls),
			(
				"attached_clients",
				self.attached_clients,
				ceiling.attached_clients,
			),
			(
				"control_command_queue",
				self.control_command_queue,
				ceiling.control_command_queue,
			),
			(
				"lifecycle_replay_events",
				self.lifecycle_replay_events,
				ceiling.lifecycle_replay_events,
			),
			(
				"client_lifecycle_queue",
				self.client_lifecycle_queue,
				ceiling.client_lifecycle_queue,
			),
			(
				"operation_cache_per_client",
				self.operation_cache_per_client,
				ceiling.operation_cache_per_client,
			),
			(
				"operation_cache_global",
				self.operation_cache_global,
				ceiling.operation_cache_global,
			),
			(
				"input_sources_per_client",
				self.input_sources_per_client,
				ceiling.input_sources_per_client,
			),
			(
				"retained_terminal_calls",
				self.retained_terminal_calls,
				ceiling.retained_terminal_calls,
			),
			("glare_aliases", self.glare_aliases, ceiling.glare_aliases),
		];
		for (name, value, max) in counts {
			if value == 0 || value > max {
				return Err(LimitsError::OutOfRange { name });
			}
		}
		let durations = [
			(
				"operation_retention",
				self.operation_retention,
				ceiling.operation_retention,
			),
			("ptt_lease_max", self.ptt_lease_max, ceiling.ptt_lease_max),
			(
				"ptt_lease_default",
				self.ptt_lease_default,
				self.ptt_lease_max,
			),
		];
		for (name, value, max) in durations {
			if value.is_zero() || value > max {
				return Err(LimitsError::OutOfRange { name });
			}
		}
		if self.ptt_release_delay > PTT_RELEASE_DELAY_MAX {
			return Err(LimitsError::OutOfRange {
				name: "ptt_release_delay",
			});
		}
		let timers = [
			("link_setup_timeout", self.link_setup_timeout),
			("ring_timeout", self.ring_timeout),
			("path_timeout", self.path_timeout),
			("teardown_timeout", self.teardown_timeout),
		];
		match timers.into_iter().find(|(_, value)| value.is_zero()) {
			Some((name, _)) => Err(LimitsError::OutOfRange { name }),
			None => Ok(()),
		}
	}
}
