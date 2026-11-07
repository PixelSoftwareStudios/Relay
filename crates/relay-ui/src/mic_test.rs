//! The microphone test of Settings, Audio: a row of dots that follows the level of the
//! microphone while the test runs. Window-less logic, so it tests without drawing.
//!
//! The engine opens the microphone for the test (`StartMicTest`: a level-only capture that
//! sends nothing, plays nothing, and shares the device with calls) and reports the level in
//! its telemetry stream. The test never outlives the page: [`MicTest::tick`] ends it when the
//! page stops drawing it (a gap between frames), when the engine's own limit passes, and the
//! page stops it when the category or the window is left. The engine also ends it when this
//! client goes away.

use crate::controller::{ControlError, Controller};
use crate::engine::Command;

/// Dots in the row.
pub const DOTS: usize = 12;
/// Level of the empty row, in dBFS.
pub const FLOOR_DBFS: f32 = -60.0;
/// Dots whose upper edge is above this are yellow.
pub const YELLOW_ABOVE_DBFS: f32 = -12.0;
/// Dots whose upper edge is above this are red (close to clipping).
pub const RED_ABOVE_DBFS: f32 = -6.0;
/// How long the peak dot stays before it falls with the level, in seconds.
pub const PEAK_HOLD_SECS: f64 = 1.2;
/// A running test whose page was not drawn for this long is considered left.
pub const MAX_FRAME_GAP_SECS: f64 = 1.0;
/// The engine ends a test after this long; the page follows (seconds).
pub const MAX_RUN_SECS: f64 = 30.0;

/// The colour class of a dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
	Green,
	Yellow,
	Red,
}

/// Why a running test ended by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
	/// The page was not drawn for a while (left, backgrounded).
	Abandoned,
	/// The engine's time limit passed.
	TimedOut,
}

/// The state of the test in the window.
#[derive(Debug, Default)]
pub struct MicTest {
	running: bool,
	started: f64,
	last_frame: f64,
	level: Option<f32>,
	peak: Option<f32>,
	peak_at: f64,
}

impl MicTest {
	pub fn running(&self) -> bool {
		self.running
	}

	/// The test starts at time `now` (seconds).
	pub fn start(&mut self, now: f64) {
		*self = Self {
			running: true,
			started: now,
			last_frame: now,
			..Self::default()
		};
	}

	/// The test is over; the row empties.
	pub fn stop(&mut self) {
		*self = Self::default();
	}

	/// One frame with the newest reading (`None` is silence or no data). Returns why the
	/// test ended, if it did.
	pub fn tick(&mut self, now: f64, dbfs: Option<f32>) -> Option<Ended> {
		if !self.running {
			return None;
		}
		let ended = if now - self.last_frame > MAX_FRAME_GAP_SECS {
			Some(Ended::Abandoned)
		} else if now - self.started > MAX_RUN_SECS {
			Some(Ended::TimedOut)
		} else {
			None
		};
		if ended.is_some() {
			self.stop();
			return ended;
		}
		self.last_frame = now;
		self.level = dbfs.filter(|v| v.is_finite());
		match (self.level, self.peak) {
			(Some(level), Some(peak)) if level < peak && now - self.peak_at <= PEAK_HOLD_SECS => {}
			(Some(level), _) => {
				self.peak = Some(level);
				self.peak_at = now;
			}
			(None, Some(_)) if now - self.peak_at <= PEAK_HOLD_SECS => {}
			(None, _) => self.peak = None,
		}
		None
	}

	/// The reading shown next to the dots.
	pub fn level(&self) -> Option<f32> {
		self.level
	}

	/// The upper edge of dot `i` in dBFS.
	pub fn edge(i: usize) -> f32 {
		FLOOR_DBFS - FLOOR_DBFS * (i as f32 + 1.0) / DOTS as f32
	}

	/// Whether dot `i` is lit for the current level.
	pub fn lit(&self, i: usize) -> bool {
		let half = -FLOOR_DBFS / DOTS as f32 / 2.0;
		self.level.is_some_and(|l| l >= Self::edge(i) - half)
	}

	/// The dot that marks the held peak, when it is above the lit ones.
	pub fn peak_dot(&self) -> Option<usize> {
		let peak = self.peak?;
		(0..DOTS).rev().find(|i| {
			let half = -FLOOR_DBFS / DOTS as f32 / 2.0;
			peak >= Self::edge(*i) - half
		})
	}

	/// The colour class of dot `i`.
	pub fn zone(i: usize) -> Zone {
		let edge = Self::edge(i);
		if edge > RED_ABOVE_DBFS {
			Zone::Red
		} else if edge > YELLOW_ABOVE_DBFS {
			Zone::Yellow
		} else {
			Zone::Green
		}
	}
}

impl Controller {
	/// Opens the microphone for the level-only test.
	pub async fn start_mic_test(&self) -> Result<(), ControlError> {
		self.run(Command::StartMicTest).await.map(drop)
	}

	/// Closes the microphone test.
	pub async fn stop_mic_test(&self) -> Result<(), ControlError> {
		self.run(Command::StopMicTest).await.map(drop)
	}
}
