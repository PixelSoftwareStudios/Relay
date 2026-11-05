//! RT-01: a panic in a media worker is contained, reported and recovered
//! from; a hang is detected; repeated failure is final.

mod support_capture;
mod support_media;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use relay_audio::device::Signal;
use relay_audio::dsp::{
	Discontinuity, DspDescriptor, DspError, DspLimits, NoiseSuppressor, OffSuppressor,
	ProcessProgress,
};
use relay_audio::format::BlockTiming;
use relay_audio::priority::WorkerKind;
use relay_core::{AudioGeneration, CallId, SystemEvent};
use relay_engine::media::{
	CaptureCmd, FailureKind, Heartbeat, MediaEvent, Supervised, Supervision, Watched,
	WorkerFailure, WorkerId, supervise, watchdog,
};
use support_capture::{GENERATION, Rig};
use support_media::{open_gate, revision, sender};

const CALL: CallId = CallId::from_u128(1);

/// A third-party suppressor that panics on its Nth frame.
struct Panicking {
	inner: OffSuppressor,
	seen: u32,
	panic_on: u32,
}

impl Panicking {
	fn boxed(panic_on: u32) -> Box<dyn NoiseSuppressor> {
		Box::new(Self {
			inner: OffSuppressor::default(),
			seen: 0,
			panic_on,
		})
	}
}

impl NoiseSuppressor for Panicking {
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
		self.seen += 1;
		assert!(self.seen < self.panic_on, "suppressor exploded");
		self.inner.process_into(input, output, timing)
	}

	fn reset(&mut self, reason: Discontinuity) {
		self.inner.reset(reason);
	}
}

fn feed(rig: &mut Rig, callbacks: usize) {
	for _ in 0..callbacks {
		rig.driver.run(480);
		std::thread::sleep(Duration::from_millis(3));
	}
}

fn failures(rig: &Rig) -> Vec<WorkerFailure> {
	rig.events()
		.into_iter()
		.filter_map(|e| match e {
			MediaEvent::WorkerFailed(f) => Some(f),
			_ => None,
		})
		.collect()
}

#[test]
fn a_panicking_suppressor_closes_the_microphone_and_tells_the_owner() {
	let mut rig = Rig::start(Panicking::boxed(100));
	rig.driver.set_signal(Signal::Sine {
		freq_hz: 440.0,
		amplitude: 0.3,
	});
	let (sender, gate) = sender(CALL, 7, &rig.wire);
	gate.publish(open_gate(CALL, 1, 1), revision(1));
	rig.add_sender(sender);
	for _ in 0..4 {
		feed(&mut rig, 50);
		if !failures(&rig).is_empty() {
			break;
		}
	}
	rig.wait_for("a worker failure", Duration::from_secs(2), |r| {
		!failures(r).is_empty()
	});
	// Contained: the thread lives, the failure is non-final and named.
	assert!(rig.is_alive(), "the worker thread must survive");
	assert_eq!(
		failures(&rig),
		[WorkerFailure {
			worker: WorkerId::Capture,
			kind: FailureKind::Panic,
			permanent: false,
			call: None,
		}]
	);
	// The domain hears the microphone is gone: it closes every gate and the device.
	assert!(
		rig.events()
			.contains(&MediaEvent::System(SystemEvent::CaptureLost {
				generation: GENERATION
			}))
	);
	// The stale level no longer lingers, and nothing more is sent.
	assert_eq!(rig.meters.microphone.get(), None);
	let sent = rig.wire.len();
	feed(&mut rig, 20);
	assert_eq!(rig.wire.len(), sent, "no stream is attached any more");
	// The loop is still beating (idle workers wake at least twice a second).
	let beats = rig.heartbeat.count();
	rig.wait_for("heartbeat", Duration::from_secs(2), |r| {
		r.heartbeat.count() > beats
	});
}

#[test]
fn after_a_contained_panic_a_reopened_microphone_resumes_sending() {
	let mut rig = Rig::start(Panicking::boxed(5));
	let (sender, gate) = sender(CALL, 7, &rig.wire);
	gate.publish(open_gate(CALL, 1, 1), revision(1));
	rig.add_sender(sender);
	feed(&mut rig, 30);
	rig.wait_for("failure", Duration::from_secs(2), |r| {
		!failures(r).is_empty()
	});
	let before = rig.wire.len();
	// The user resumes capture: a new generation, the same call, the replaced
	// (pass-through) suppressor.
	rig.reattach(AudioGeneration::new(2));
	rig.driver.set_signal(Signal::Sine {
		freq_hz: 300.0,
		amplitude: 0.3,
	});
	// The domain also re-publishes the gate for the new capture generation.
	gate.publish(open_gate(CALL, 1, 2), revision(2));
	feed(&mut rig, 60);
	rig.wait_for("sending again", Duration::from_secs(2), |r| {
		r.wire.len() > before + 5
	});
	assert!(!rig.failed.load(Ordering::Acquire));
}

#[test]
fn the_fourth_panic_inside_the_window_is_final() {
	let mut rig = Rig::start(Panicking::boxed(2));
	let (sender, gate) = sender(CALL, 7, &rig.wire);
	gate.publish(open_gate(CALL, 1, 1), revision(1));
	rig.add_sender(sender);
	feed(&mut rig, 10);
	for n in 2..=5_u64 {
		rig.wait_for("failure", Duration::from_secs(3), |r| {
			failures(r).len() >= (n - 1) as usize
		});
		rig.push(CaptureCmd::SetSuppressor(Panicking::boxed(2)));
		rig.reattach(AudioGeneration::new(n));
		feed(&mut rig, 10);
	}
	rig.wait_for("a final failure", Duration::from_secs(3), |r| {
		failures(r).last().is_some_and(|f| f.permanent)
	});
	assert!(rig.failed.load(Ordering::Acquire));
	assert!(
		rig.is_alive(),
		"even a failed worker keeps draining its queue"
	);
}

/// A stand-in worker for the supervisor's own contract.
struct Fake {
	steps: Arc<AtomicU32>,
	recovered: Arc<Mutex<Vec<()>>>,
	disabled: Arc<AtomicBool>,
	culprit: Option<CallId>,
}

impl Supervised for Fake {
	fn step(&mut self) {
		self.steps.fetch_add(1, Ordering::Relaxed);
		panic!("boom");
	}

	fn wait(&self) -> Duration {
		Duration::from_millis(1)
	}

	fn recover(&mut self) -> Option<CallId> {
		self.recovered.lock().unwrap().push(());
		self.culprit
	}

	fn disable(&mut self) {
		self.disabled.store(true, Ordering::Release);
	}
}

#[test]
fn the_supervisor_names_the_culprit_call_and_stops_stepping_after_the_limit() {
	let steps = Arc::new(AtomicU32::new(0));
	let recovered = Arc::new(Mutex::new(Vec::new()));
	let disabled = Arc::new(AtomicBool::new(false));
	let reports = Arc::new(Mutex::new(Vec::new()));
	let stop = Arc::new(AtomicBool::new(false));
	let supervision = Supervision {
		id: WorkerId::Render,
		heartbeat: Arc::new(Heartbeat::default()),
		failed: Arc::new(AtomicBool::new(false)),
		stop: Arc::clone(&stop),
		report: {
			let reports = Arc::clone(&reports);
			Arc::new(move |f| reports.lock().unwrap().push(f))
		},
		promote: false,
	};
	let worker = Fake {
		steps: Arc::clone(&steps),
		recovered,
		disabled: Arc::clone(&disabled),
		culprit: Some(CALL),
	};
	let handle = std::thread::spawn(move || supervise(worker, &supervision, WorkerKind::Render));
	std::thread::sleep(Duration::from_millis(300));
	stop.store(true, Ordering::Release);
	handle.thread().unpark();
	handle.join().unwrap();
	let reports = reports.lock().unwrap();
	assert_eq!(reports.len(), 4, "three restarts, then the final failure");
	assert!(reports.iter().all(|f| f.call == Some(CALL)));
	assert!(reports[..3].iter().all(|f| !f.permanent) && reports[3].permanent);
	assert_eq!(
		steps.load(Ordering::Relaxed),
		4,
		"no step after the final one"
	);
	assert!(disabled.load(Ordering::Acquire));
}

#[tokio::test]
async fn a_worker_that_stops_beating_is_reported_once_as_hung() {
	let heartbeat = Arc::new(Heartbeat::default());
	let reports = Arc::new(Mutex::new(Vec::new()));
	let stop = Arc::new(AtomicBool::new(false));
	let task = tokio::spawn(watchdog(
		vec![Watched {
			id: WorkerId::Capture,
			heartbeat: Arc::clone(&heartbeat),
			failed: Arc::new(AtomicBool::new(false)),
		}],
		Arc::clone(&stop),
		{
			let reports = Arc::clone(&reports);
			Arc::new(move |f| reports.lock().unwrap().push(f))
		},
		Duration::from_millis(10),
		Duration::from_millis(100),
	));
	// Beating: no report.
	for _ in 0..15 {
		heartbeat.beat();
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	assert!(reports.lock().unwrap().is_empty());
	// Silence: exactly one report, however long the hang lasts.
	tokio::time::sleep(Duration::from_millis(400)).await;
	assert_eq!(
		*reports.lock().unwrap(),
		[WorkerFailure {
			worker: WorkerId::Capture,
			kind: FailureKind::Hang,
			permanent: false,
			call: None,
		}]
	);
	stop.store(true, Ordering::Release);
	task.await.unwrap();
}
