//! Live call measurements: the raw numbers the media layer keeps, and the windows that turn
//! them into what `WatchCallStats` reports (loss over 5 s, round trip min and max over 10 s,
//! the mouth-to-ear estimate).
//!
//! Everything here is sans-IO: the caller passes the time, so tests use a fake clock.

use std::collections::VecDeque;

use relay_api::pb::{CallStats, MediaPathState};
use relay_core::CallId;

/// Window of the short loss figure.
pub const LOSS_WINDOW_MS: u64 = 5_000;
/// Window of the round-trip minimum and maximum.
pub const RTT_WINDOW_MS: u64 = 10_000;
/// A packet seen within this long counts as "sending" or "receiving".
pub const ACTIVE_WITHIN_MS: u64 = 500;
/// Encode delay in the mouth-to-ear estimate: one 10 ms frame plus about 2.5 ms of codec
/// lookahead.
pub const ENCODE_MS: f64 = 12.5;
/// Decode delay: the render worker produces audio in 10 ms ticks, so on average half a tick
/// passes between a packet being due and its samples being mixed.
pub const DECODE_MS: f64 = 5.0;

/// Where the media path of a call stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PathKind {
	/// Waiting for the remote's candidates or the challenge echo.
	#[default]
	Waiting,
	/// A path was validated.
	Connected,
	/// Gave up; see the reason.
	Failed,
}

/// One instant's raw numbers for a call, read from the media layer's atomics.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CallRaw {
	/// Path state.
	pub path: PathKind,
	/// Why the path waits or failed.
	pub path_reason: String,
	/// Local media socket address.
	pub local_addr: String,
	/// Remote media address.
	pub remote_addr: String,
	/// The link runs over an overlay network.
	pub overlay: bool,
	/// Packets sent.
	pub packets_sent: u64,
	/// Packets received (stored or refused as late), without the ones for other calls.
	pub packets_received: u64,
	/// Packets the sender's sequence numbers say it sent so far.
	pub packets_expected: u64,
	/// Milliseconds since the last packet was sent, if any was.
	pub since_sent_ms: Option<u64>,
	/// Milliseconds since the last packet arrived, if any did.
	pub since_received_ms: Option<u64>,
	/// RFC 3550 interarrival jitter.
	pub jitter_ms: Option<f64>,
	/// Playout margin the jitter buffer aims for.
	pub jitter_buffer_target_ms: f64,
	/// Audio queued in the jitter buffer.
	pub jitter_buffer_depth_ms: f64,
	/// Audio waiting in the playback FIFO plus the latency the backend reports.
	pub playout_delay_ms: f64,
	/// Microphone level.
	pub mic_dbfs: Option<f32>,
	/// Smoothed round trip of the authenticated control link.
	pub rtt_ms: Option<f64>,
	/// Clock drift correction applied to playback.
	pub drift_ppm: Option<f64>,
	/// Encoder bitrate.
	pub bitrate_bps: u32,
	/// Size of the last sealed datagram.
	pub packet_bytes: u32,
	/// Microphone buffer size in frames and the stream's sample rate.
	pub capture_frames: u32,
	/// Microphone sample rate.
	pub capture_rate: u32,
	/// Playback buffer size in frames.
	pub playback_frames: u32,
	/// Playback sample rate.
	pub playback_rate: u32,
}

#[derive(Debug, Clone, Copy)]
struct Sample {
	at_ms: u64,
	expected: u64,
	received: u64,
	rtt_ms: Option<f64>,
}

/// The windows over one call's samples; one per open stream.
#[derive(Debug, Default)]
pub struct StatsWindow {
	samples: VecDeque<Sample>,
}

/// What the windows add to a raw reading.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Windowed {
	/// Loss over the last 5 s, percent.
	pub loss_5s: f64,
	/// Loss since the first packet, percent.
	pub loss_total: f64,
	/// Smallest smoothed round trip in the last 10 s.
	pub rtt_min_ms: Option<f64>,
	/// Largest smoothed round trip in the last 10 s.
	pub rtt_max_ms: Option<f64>,
}

fn loss(expected: u64, received: u64) -> f64 {
	if expected == 0 {
		return 0.0;
	}
	let lost = expected.saturating_sub(received);
	(lost as f64 * 100.0 / expected as f64).min(100.0)
}

impl StatsWindow {
	/// Adds the reading taken at `at_ms` and returns the windowed figures.
	pub fn push(&mut self, at_ms: u64, raw: &CallRaw) -> Windowed {
		self.samples.push_back(Sample {
			at_ms,
			expected: raw.packets_expected,
			received: raw.packets_received,
			rtt_ms: raw.rtt_ms,
		});
		// Keep one sample older than the longest window as the base of the deltas.
		while self.samples.len() > 2
			&& self
				.samples
				.get(1)
				.is_some_and(|s| at_ms.saturating_sub(s.at_ms) >= RTT_WINDOW_MS)
		{
			self.samples.pop_front();
		}
		let base = self
			.samples
			.iter()
			.rev()
			.find(|s| at_ms.saturating_sub(s.at_ms) >= LOSS_WINDOW_MS)
			.or_else(|| self.samples.front())
			.copied()
			.unwrap_or(Sample {
				at_ms,
				expected: 0,
				received: 0,
				rtt_ms: None,
			});
		let rtts = self
			.samples
			.iter()
			.filter(|s| at_ms.saturating_sub(s.at_ms) < RTT_WINDOW_MS)
			.filter_map(|s| s.rtt_ms);
		let (min, max) = rtts.fold((None::<f64>, None::<f64>), |(lo, hi), v| {
			(
				Some(lo.map_or(v, |x| x.min(v))),
				Some(hi.map_or(v, |x| x.max(v))),
			)
		});
		Windowed {
			loss_5s: loss(
				raw.packets_expected.saturating_sub(base.expected),
				raw.packets_received.saturating_sub(base.received),
			),
			loss_total: loss(raw.packets_expected, raw.packets_received),
			rtt_min_ms: min,
			rtt_max_ms: max,
		}
	}
}

fn frames_ms(frames: u32, rate: u32) -> f64 {
	if rate == 0 {
		0.0
	} else {
		f64::from(frames) * 1000.0 / f64::from(rate)
	}
}

/// The mouth-to-ear estimate in milliseconds: microphone buffer, encode, half the round trip,
/// the jitter buffer's depth, decode and the playback delay. It needs a round trip; without
/// one there is no estimate. An estimate, not a measurement: the round trip is the control
/// link's and the one-way delay is assumed symmetric.
pub fn mouth_to_ear_ms(raw: &CallRaw) -> Option<f64> {
	let rtt = raw.rtt_ms?;
	Some(
		frames_ms(raw.capture_frames, raw.capture_rate)
			+ ENCODE_MS
			+ rtt / 2.0
			+ raw.jitter_buffer_depth_ms
			+ DECODE_MS
			+ raw.playout_delay_ms,
	)
}

/// The wire form of one reading.
pub fn to_pb(call: CallId, raw: &CallRaw, window: &Windowed) -> CallStats {
	let active = |since: Option<u64>| since.is_some_and(|ms| ms < ACTIVE_WITHIN_MS);
	CallStats {
		call_id: call.to_bytes().to_vec(),
		path_state: match raw.path {
			PathKind::Waiting => MediaPathState::Waiting,
			PathKind::Connected => MediaPathState::Connected,
			PathKind::Failed => MediaPathState::Failed,
		} as i32,
		path_reason: raw.path_reason.clone(),
		sending: active(raw.since_sent_ms),
		receiving: active(raw.since_received_ms),
		packets_sent: raw.packets_sent,
		packets_received: raw.packets_received,
		mic_level_dbfs: raw.mic_dbfs,
		rtt_ms: raw.rtt_ms,
		rtt_min_ms: window.rtt_min_ms,
		rtt_max_ms: window.rtt_max_ms,
		jitter_ms: raw.jitter_ms,
		loss_percent_5s: window.loss_5s,
		loss_percent_total: window.loss_total,
		jitter_buffer_target_ms: raw.jitter_buffer_target_ms,
		jitter_buffer_depth_ms: raw.jitter_buffer_depth_ms,
		playout_delay_ms: raw.playout_delay_ms,
		mouth_to_ear_ms: mouth_to_ear_ms(raw),
		codec_bitrate_bps: raw.bitrate_bps,
		packet_bytes: raw.packet_bytes,
		local_addr: raw.local_addr.clone(),
		remote_addr: raw.remote_addr.clone(),
		transport: if raw.overlay { "overlay" } else { "lan" }.to_owned(),
		clock_drift_ppm: raw.drift_ppm,
		capture_buffer_frames: raw.capture_frames,
		capture_buffer_ms: frames_ms(raw.capture_frames, raw.capture_rate),
		playback_buffer_frames: raw.playback_frames,
		playback_buffer_ms: frames_ms(raw.playback_frames, raw.playback_rate),
	}
}
