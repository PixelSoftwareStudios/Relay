//! Live levels and counters, shared with readers as plain atomics.
//!
//! The audio workers store into these without locking or allocating; the
//! control side reads them at its own pace.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};

use relay_audio::FRAME_SAMPLES;
use rtrb::{Consumer, Producer};

/// Level reported for silence.
pub const FLOOR_DBFS: f32 = -100.0;
/// Frames (10 ms each) a remote voice keeps counting as speaking after its
/// last audible frame.
pub const SPEAKING_HOLD_FRAMES: u8 = 30;
/// Level above which a frame counts as audible speech.
pub const SPEAKING_DBFS: f32 = -50.0;

/// Root-mean-square level of a frame in dBFS, with a finite silence floor.
pub fn level_dbfs(samples: &[f32]) -> f32 {
	if samples.is_empty() {
		return FLOOR_DBFS;
	}
	let energy: f32 = samples.iter().map(|s| s * s).sum();
	let rms = (energy / samples.len() as f32).sqrt();
	if rms <= 1e-5 {
		FLOOR_DBFS
	} else {
		(20.0 * rms.log10()).max(FLOOR_DBFS)
	}
}

/// A dBFS value readable from any thread. Default is "no signal".
#[derive(Debug)]
pub struct Level(AtomicU32);

impl Default for Level {
	fn default() -> Self {
		Self(AtomicU32::new(f32::NEG_INFINITY.to_bits()))
	}
}

impl Level {
	/// Publishes a level.
	pub fn set(&self, dbfs: f32) {
		self.0.store(dbfs.to_bits(), Ordering::Relaxed);
	}

	/// Publishes "no signal".
	pub fn clear(&self) {
		self.set(f32::NEG_INFINITY);
	}

	/// The latest level, if there is a signal source.
	pub fn get(&self) -> Option<f32> {
		let value = f32::from_bits(self.0.load(Ordering::Relaxed));
		value.is_finite().then_some(value)
	}
}

/// Engine-wide meters written by the capture and render workers.
#[derive(Debug, Default)]
pub struct Meters {
	/// Microphone level after gain and suppression.
	pub microphone: Level,
	/// Capture callback frames lost to a full ring.
	pub capture_overruns: AtomicU64,
	/// Playback callback frames filled with silence.
	pub render_underruns: AtomicU64,
	/// Capture blocks excised because the noise suppressor returned an error.
	pub suppressor_errors: AtomicU64,
	/// The microphone test wants capture mixed into the local output.
	pub monitor: AtomicBool,
	/// Clock-drift correction applied to playback, in thousandths of a ppm.
	pub drift_ppm_milli: AtomicI64,
	/// Audio waiting in the playback FIFO, in microseconds.
	pub playout_fifo_us: AtomicU64,
	/// Output latency the audio backend reports, in microseconds (0 when it reports none).
	pub output_latency_us: AtomicU64,
}

/// One 10 ms frame of monitored microphone audio.
pub type MonitorFrame = [f32; FRAME_SAMPLES];
/// Capture side of the local monitor path.
pub type MonitorTx = Producer<MonitorFrame>;
/// Render side of the local monitor path.
pub type MonitorRx = Consumer<MonitorFrame>;

/// Frames the monitor path may hold; older audio is dropped, never queued up.
pub const MONITOR_FRAMES: usize = 8;

/// The queue between the capture and render workers for the local monitor.
pub fn monitor_pair() -> (MonitorTx, MonitorRx) {
	rtrb::RingBuffer::new(MONITOR_FRAMES)
}
