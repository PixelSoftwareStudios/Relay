//! RT-11 and RT-14: a failing suppressor is counted and leaves the RTP
//! timeline exact; an idle worker forgets its load.

mod support_capture;
mod support_media;

use std::sync::atomic::Ordering;
use std::time::Duration;

use relay_audio::device::Signal;
use relay_audio::dsp::{
	Discontinuity, DspDescriptor, DspError, DspLimits, NoiseSuppressor, OffSuppressor,
	ProcessProgress,
};
use relay_audio::format::BlockTiming;
use relay_core::CallId;
use relay_engine::media::LoadMeter;
use support_capture::Rig;
use support_media::{closed_gate, open_gate, revision, sender};

struct FailsOnce {
	inner: OffSuppressor,
	calls: u32,
}

impl NoiseSuppressor for FailsOnce {
	fn descriptor(&self) -> &DspDescriptor {
		self.inner.descriptor()
	}

	fn prepare(&mut self, limits: &DspLimits) -> Result<(), DspError> {
		self.inner.prepare(limits)
	}

	fn process_into(
		&mut self,
		input: &[f32],
		output: &mut [f32],
		timing: BlockTiming,
	) -> Result<ProcessProgress, DspError> {
		self.calls += 1;
		if self.calls == 40 {
			return Err(DspError::Backend("injected".into()));
		}
		self.inner.process_into(input, output, timing)
	}

	fn reset(&mut self, reason: Discontinuity) {
		self.inner.reset(reason);
	}
}

#[test]
fn a_suppressor_error_is_counted_and_leaves_a_visible_gap_not_a_shift() {
	let mut rig = Rig::start(Box::new(FailsOnce {
		inner: OffSuppressor::default(),
		calls: 0,
	}));
	rig.driver.set_signal(Signal::Sine {
		freq_hz: 440.0,
		amplitude: 0.3,
	});
	let call = CallId::from_u128(1);
	let (sender, gate) = sender(call, 7, &rig.wire);
	gate.publish(closed_gate(call, 1, 1), revision(1));
	rig.add_sender(sender);
	gate.publish(open_gate(call, 1, 1), revision(2));
	for _ in 0..120 {
		rig.driver.run(480);
		std::thread::sleep(Duration::from_millis(3));
	}
	rig.wait_for("frames", Duration::from_secs(2), |r| r.wire.len() > 60);
	assert_eq!(rig.meters.suppressor_errors.load(Ordering::Relaxed), 1);
	let firsts = rig.sent_first_samples();
	let steps: Vec<u64> = firsts.windows(2).map(|w| w[1] - w[0]).collect();
	assert!(
		steps.contains(&960),
		"the excised frame is a hole: {steps:?}"
	);
	assert!(steps.iter().all(|s| *s == 480 || *s == 960), "{steps:?}");
}

#[test]
fn an_idle_worker_forgets_its_load() {
	let meter = LoadMeter::default();
	meter.record(4_900_000);
	assert!(meter.ns_per_frame() > 0);
	meter.reset();
	assert_eq!(meter.ns_per_frame(), 0, "no calls, no load");
}

/// RT-04: a full socket buffer drops the packet, keeps the RTP sequence moving
/// (the receiver sees a loss) and never blocks the worker.
#[test]
fn a_full_socket_buffer_drops_the_packet_and_counts_it() {
	use std::sync::Arc;

	use relay_audio::codec::EncoderConfig;
	use relay_engine::media::{CallSender, Emit, FrameStamp, GateCell, MediaError, PacketSink};

	struct Full(Vec<u16>);
	impl PacketSink for Full {
		fn send(&mut self, _c: [u8; 16], _g: u32, rtp: &[u8]) -> Result<(), MediaError> {
			let seq = u16::from_be_bytes([rtp[2], rtp[3]]);
			self.0.push(seq);
			if self.0.len() == 2 {
				return Err(MediaError::WouldBlock);
			}
			Ok(())
		}
	}
	let call = CallId::from_u128(9);
	let gate = Arc::new(GateCell::closed(call));
	gate.publish(open_gate(call, 1, 1), revision(1));
	let mut sender = CallSender::new(
		support_media::identity(call, 5, 1),
		gate,
		EncoderConfig::default(),
		Box::new(Full(Vec::new())),
	)
	.unwrap();
	let stamp = FrameStamp {
		capture: relay_core::AudioGeneration::new(1),
		route: relay_core::RouteGeneration::new(1),
		clean: true,
	};
	let frame = support_media::tone(440.0, 0);
	let results: Vec<Emit> = (0..3)
		.map(|i| sender.emit(&frame, stamp, i * 480))
		.collect();
	assert_eq!(results, [Emit::Sent, Emit::Dropped, Emit::Sent]);
	assert_eq!(sender.stats().send_dropped.load(Ordering::Relaxed), 1);
	assert_eq!(sender.stats().failed.load(Ordering::Relaxed), 0);
}
