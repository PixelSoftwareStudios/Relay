//! RFC 3550 interarrival jitter, in RTP timestamp units (48 kHz samples).

/// Running jitter estimate `J += (|D| - J) / 16`.
#[derive(Debug, Clone, Copy, Default)]
pub struct InterarrivalJitter {
	last: Option<(i64, i64)>,
	jitter_x16: i64,
}

impl InterarrivalJitter {
	pub const fn new() -> Self {
		Self {
			last: None,
			jitter_x16: 0,
		}
	}

	/// Feeds one packet: arrival in microseconds and RTP timestamp.
	pub fn update(&mut self, arrival_us: u64, timestamp: u64) {
		// 48 samples per millisecond == 48 samples per 1000 microseconds.
		let arrival = (arrival_us.saturating_mul(48) / 1000) as i64;
		let timestamp = timestamp as i64;
		if let Some((last_arrival, last_ts)) = self.last {
			let d = (arrival - last_arrival) - (timestamp - last_ts);
			self.jitter_x16 += d.abs() - (self.jitter_x16 >> 4);
		}
		self.last = Some((arrival, timestamp));
	}

	/// Current estimate in samples.
	pub fn samples(&self) -> u32 {
		(self.jitter_x16 >> 4).clamp(0, i64::from(u32::MAX)) as u32
	}
}
