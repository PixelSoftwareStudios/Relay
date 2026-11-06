//! The raw diagnostics of one call, read from the atomics the workers keep.

use std::sync::atomic::Ordering::Relaxed;

use relay_core::CallId;

use super::diag::{PathState, now_ms};
use super::meters::FLOOR_DBFS;
use super::system::MediaSystem;
use crate::service::call_stats::{CallRaw, PathKind};

fn since(now: u64, stamp: u64) -> Option<u64> {
	(stamp != 0).then(|| now.saturating_sub(stamp))
}

impl MediaSystem {
	/// Packet counters, latency figures and the path of `call`; `None` without media.
	pub fn call_raw(&self, call: CallId) -> Option<CallRaw> {
		let inner = &self.inner;
		let now = now_ms();
		let state = self.state();
		let media = state.calls.get(&call)?;
		let mut raw = CallRaw {
			path: match media.path.state {
				PathState::Waiting => PathKind::Waiting,
				PathState::Connected => PathKind::Connected,
				PathState::Failed => PathKind::Failed,
			},
			path_reason: media.path.reason.clone(),
			local_addr: media.path.local.map(|a| a.to_string()).unwrap_or_default(),
			remote_addr: media.path.remote.map(|a| a.to_string()).unwrap_or_default(),
			overlay: state.peers.get(&media.peer).is_some_and(|p| p.overlay),
			bitrate_bps: u32::try_from(inner.config.encoder.bitrate_bps).unwrap_or(0),
			rtt_ms: inner
				.peers
				.rtt(media.peer)
				.map(|d| d.as_secs_f64() * 1000.0),
			..CallRaw::default()
		};
		if let Some(send) = &media.send_stats {
			raw.packets_sent = send.sent.load(Relaxed);
			raw.since_sent_ms = since(now, send.last_sent_ms.load(Relaxed));
			raw.packet_bytes = u32::try_from(send.packet_bytes.load(Relaxed)).unwrap_or(0);
		}
		if let Some(recv) = &media.recv_stats {
			raw.packets_received = recv.accepted.load(Relaxed) + recv.refused.load(Relaxed);
			raw.packets_expected = recv.expected.load(Relaxed);
			raw.since_received_ms = since(now, recv.last_packet_ms.load(Relaxed));
			if raw.packets_received > 0 {
				raw.jitter_ms = Some(recv.jitter_us.load(Relaxed) as f64 / 1000.0);
			}
			raw.jitter_buffer_depth_ms = f64::from(recv.buffered_samples.load(Relaxed)) / 48.0;
			raw.jitter_buffer_target_ms = f64::from(recv.target_margin_frames.load(Relaxed)) * 10.0;
		}
		let meters = &inner.meters;
		raw.mic_dbfs = meters.microphone.get().filter(|l| *l > FLOOR_DBFS);
		raw.playout_delay_ms =
			(meters.playout_fifo_us.load(Relaxed) + meters.output_latency_us.load(Relaxed)) as f64
				/ 1000.0;
		raw.drift_ppm = Some(meters.drift_ppm_milli.load(Relaxed) as f64 / 1000.0);
		let (frames, rate) = inner.buffers.capture();
		(raw.capture_frames, raw.capture_rate) = (frames, rate);
		let (frames, rate) = inner.buffers.playback();
		(raw.playback_frames, raw.playback_rate) = (frames, rate);
		Some(raw)
	}
}
