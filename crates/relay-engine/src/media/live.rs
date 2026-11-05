//! Live views of the media system: the microphone test and telemetry.
//!
//! The microphone test is local only. It meters the capture path and, if
//! asked, plays the microphone on the user's own output. It adds no sender
//! and touches no gate, so nothing it does can transmit; it ends by itself and
//! releases the microphone when it ends.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use relay_audio::FRAME_SAMPLES;
use relay_core::{AudioGeneration, SystemEvent};

use super::devices::DeviceCmd;
use super::events::{EventSink, MediaEvent};
use super::meters::{FLOOR_DBFS, Meters};
use super::system::MediaSystem;
use crate::service::ports::{CallMeter, MediaTelemetry, PortError};

/// The capture generation the test opens the microphone under. The domain
/// never uses it, and events carrying it are consumed here.
pub(super) const TEST_GENERATION: AudioGeneration = AudioGeneration::new(u64::MAX - 1);

/// Longest a microphone test runs before it stops on its own.
pub const MIC_TEST_MAX: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy)]
struct Active {
	id: u64,
	local_playback: bool,
}

/// The state of the microphone test, shared with the event filter.
#[derive(Debug, Default)]
pub(super) struct MicTest {
	active: Mutex<Option<Active>>,
	sessions: AtomicU64,
}

impl MicTest {
	fn lock(&self) -> std::sync::MutexGuard<'_, Option<Active>> {
		self.active.lock().unwrap_or_else(|p| p.into_inner())
	}

	pub(super) fn is_active(&self) -> bool {
		self.lock().is_some()
	}

	pub(super) fn wants_playback(&self) -> bool {
		self.lock().is_some_and(|a| a.local_playback)
	}

	fn begin(&self, local_playback: bool) -> Result<u64, PortError> {
		let mut active = self.lock();
		if active.is_some() {
			return Err(PortError::Invalid(
				"a microphone test is already running".to_owned(),
			));
		}
		let id = self.sessions.fetch_add(1, Ordering::Relaxed) + 1;
		*active = Some(Active { id, local_playback });
		Ok(id)
	}

	/// Ends the test if `id` (or any test, for `None`) is running.
	fn end(&self, id: Option<u64>) -> bool {
		let mut active = self.lock();
		if active.is_some_and(|a| id.is_none_or(|wanted| wanted == a.id)) {
			*active = None;
			return true;
		}
		false
	}
}

/// Wraps the sink so the test's own capture events never reach the domain,
/// and a lost or failed test microphone ends the test.
pub(super) fn filter_events(sink: EventSink, test: Arc<MicTest>, meters: Arc<Meters>) -> EventSink {
	Arc::new(move |event| match event {
		MediaEvent::System(
			SystemEvent::CaptureReady { generation }
			| SystemEvent::CaptureFailed { generation }
			| SystemEvent::CaptureLost { generation },
		) if generation == TEST_GENERATION => {
			if !matches!(event, MediaEvent::System(SystemEvent::CaptureReady { .. })) {
				test.end(None);
				meters.monitor.store(false, Ordering::Relaxed);
			}
		}
		other => sink(other),
	})
}

impl MediaSystem {
	/// Starts a local microphone test (see the module notes).
	pub fn start_mic_test(&self, local_playback: bool) -> Result<(), PortError> {
		let inner = &self.inner;
		if !inner.input_selected.load(Ordering::Relaxed) {
			return Err(PortError::Unavailable(
				"no microphone is selected".to_owned(),
			));
		}
		let id = inner.mic_test.begin(local_playback)?;
		inner
			.meters
			.monitor
			.store(local_playback, Ordering::Relaxed);
		if !inner.domain_capture.load(Ordering::Acquire) {
			let _ = inner
				.devices
				.commands
				.send(DeviceCmd::OpenCapture(TEST_GENERATION));
		}
		if local_playback {
			let this = self.clone();
			inner.runtime.spawn(async move {
				if let Err(error) = this.open_playback().await {
					tracing::warn!(%error, "microphone test could not open the output");
				}
			});
		}
		let this = self.clone();
		inner.runtime.spawn(async move {
			tokio::time::sleep(this.inner.config.mic_test_max).await;
			this.end_mic_test(Some(id));
		});
		Ok(())
	}

	/// Stops the microphone test, if one runs, and releases the microphone
	/// unless a call is using it.
	pub fn stop_mic_test(&self) {
		self.end_mic_test(None);
	}

	fn end_mic_test(&self, id: Option<u64>) {
		let inner = &self.inner;
		if !inner.mic_test.end(id) {
			return;
		}
		inner.meters.monitor.store(false, Ordering::Relaxed);
		if !inner.domain_capture.load(Ordering::Acquire) {
			let _ = inner
				.devices
				.commands
				.send(DeviceCmd::CloseCapture(TEST_GENERATION));
		}
		if self.state().calls.is_empty() && !self.tone_wanted() {
			self.state().playback_open = false;
			let _ = inner.devices.commands.send(DeviceCmd::ClosePlayback);
		}
	}

	/// Whether a microphone test is running.
	pub fn mic_test_active(&self) -> bool {
		self.inner.mic_test.is_active()
	}

	/// Levels and counters, read from atomics without waiting for any worker.
	pub fn telemetry(&self) -> MediaTelemetry {
		let inner = &self.inner;
		let frame_samples = FRAME_SAMPLES as u64;
		let calls = self
			.state()
			.calls
			.iter()
			.map(|(call, media)| {
				let stats = media.recv_stats.as_ref();
				let load = |pick: fn(&super::recv::RecvStats) -> &AtomicU64| {
					stats.map_or(0, |s| pick(s).load(Ordering::Relaxed))
				};
				let concealed = load(|s| &s.concealed);
				CallMeter {
					call: Some(*call),
					speaking: stats.is_some_and(|s| s.speaking.load(Ordering::Relaxed)),
					receive_dbfs: stats
						.and_then(|s| s.level.get())
						.filter(|level| *level > FLOOR_DBFS),
					packets_received: load(|s| &s.accepted),
					packets_late: load(|s| &s.refused),
					packets_missing: concealed,
					concealed_samples: concealed * frame_samples,
					playout_target_ms: f64::from(inner.config.playout_target_ms),
					rtt_ms: inner
						.peers
						.rtt(media.peer)
						.map(|d| d.as_secs_f64() * 1000.0),
				}
			})
			.collect();
		MediaTelemetry {
			microphone_dbfs: inner.meters.microphone.get().filter(|l| *l > FLOOR_DBFS),
			capture_overruns: inner.meters.capture_overruns.load(Ordering::Relaxed),
			render_underruns: inner.meters.render_underruns.load(Ordering::Relaxed),
			mic_test_active: inner.mic_test.is_active(),
			calls,
		}
	}
}
