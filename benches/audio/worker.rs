//! Real-time pacing test: does a candidate keep up on a dedicated worker
//! thread fed one 10 ms frame every 10 ms of wall-clock time?

use std::sync::mpsc::sync_channel;
use std::time::{Duration, Instant};

use relay_audio::dsp::DspLimits;
use relay_audio::format::{BlockTiming, FRAME_SAMPLES};
use serde::Serialize;

use super::runner::make_candidate;
use super::synth::{NoiseKind, mix_at_snr, noise, speech_like};
use super::{BenchError, Result};

const PERIOD: Duration = Duration::from_millis(10);

#[derive(Debug, Serialize)]
pub struct WorkerReport {
	pub backend: String,
	pub frames: usize,
	pub wall_s: f64,
	/// Time from the frame becoming available to its output being ready.
	pub latency_p50_us: f64,
	pub latency_p99_us: f64,
	pub latency_max_us: f64,
	pub frames_over_5ms: usize,
	pub frames_over_10ms: usize,
	pub interactive_qos: bool,
	pub note: &'static str,
}

/// Ask macOS to schedule the calling thread at user-interactive QoS. Returns
/// whether the request was accepted; a no-op elsewhere.
pub fn request_interactive_qos() -> bool {
	#[cfg(target_os = "macos")]
	{
		unsafe extern "C" {
			fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
		}
		const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
		// SAFETY: plain libSystem call with value arguments; it only changes
		// the scheduling class of the calling thread.
		unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) == 0 }
	}
	#[cfg(not(target_os = "macos"))]
	{
		false
	}
}

pub fn run(backend: &str, seconds: f64, interactive_qos: bool) -> Result<WorkerReport> {
	let frames = (seconds * 100.0) as usize;
	let clean = speech_like(8.0, 48_000, 1);
	let signal = mix_at_snr(&clean, &noise(NoiseKind::White, 8.0, 48_000, 2), 5.0);
	let mut candidate = make_candidate(backend)?;
	candidate
		.ns
		.prepare(&DspLimits {
			max_block_samples: FRAME_SAMPLES,
			max_scratch_bytes: 0,
		})
		.map_err(|e| BenchError::Dsp(e.to_string()))?;

	let (tx, rx) = sync_channel::<(Instant, usize)>(16);
	let frames_in_signal = signal.len() / FRAME_SAMPLES;
	let worker = std::thread::spawn(move || -> Result<Vec<f64>> {
		if interactive_qos {
			request_interactive_qos();
		}
		let mut out = vec![0.0_f32; FRAME_SAMPLES * 2];
		let silence = vec![0.0_f32; FRAME_SAMPLES];
		let mut latencies = Vec::with_capacity(frames);
		while let Ok((enqueued, n)) = rx.recv() {
			let frame = &signal[(n % frames_in_signal) * FRAME_SAMPLES..][..FRAME_SAMPLES];
			if candidate.feed_render {
				candidate
					.ns
					.push_render(&silence)
					.map_err(|e| BenchError::Dsp(e.to_string()))?;
			}
			let timing = BlockTiming {
				generation: 0,
				first_sample: (n * FRAME_SAMPLES) as u64,
			};
			candidate
				.ns
				.process_into(frame, &mut out, timing)
				.map_err(|e| BenchError::Dsp(e.to_string()))?;
			latencies.push(enqueued.elapsed().as_secs_f64() * 1e6);
		}
		Ok(latencies)
	});

	if interactive_qos {
		request_interactive_qos();
	}
	let start = Instant::now();
	for n in 0..frames {
		let deadline = start + PERIOD * n as u32;
		while Instant::now() < deadline {
			std::thread::sleep(Duration::from_micros(200));
		}
		tx.send((Instant::now(), n))
			.map_err(|_| BenchError::Dsp("worker stopped".into()))?;
	}
	drop(tx);
	let wall_s = start.elapsed().as_secs_f64();
	let mut latencies = worker
		.join()
		.map_err(|_| BenchError::Dsp("worker panicked".into()))??;
	latencies.sort_by(f64::total_cmp);
	let at = |q: f64| latencies[((latencies.len() - 1) as f64 * q).round() as usize];
	Ok(WorkerReport {
		backend: backend.to_owned(),
		frames: latencies.len(),
		wall_s,
		latency_p50_us: at(0.5),
		latency_p99_us: at(0.99),
		latency_max_us: latencies[latencies.len() - 1],
		frames_over_5ms: latencies.iter().filter(|&&l| l > 5_000.0).count(),
		frames_over_10ms: latencies.iter().filter(|&&l| l > 10_000.0).count(),
		interactive_qos,
		note: "idle machine, synthetic 5 dB white-noise mix, single worker thread; AEC fed silent render",
	})
}

/// Run one file through a candidate three times in one process and report
/// where the slowest frames fall, to separate first-touch/initialisation
/// costs from steady-state scheduling noise.
pub fn outliers(backend: &str, wav: &std::path::Path, interactive_qos: bool) -> Result<()> {
	if interactive_qos {
		request_interactive_qos();
	}
	let signal = super::wav::read_mono_48k(wav)?;
	let mut candidate = make_candidate(backend)?;
	candidate
		.ns
		.prepare(&DspLimits {
			max_block_samples: FRAME_SAMPLES,
			max_scratch_bytes: 0,
		})
		.map_err(|e| BenchError::Dsp(e.to_string()))?;
	let mut out = vec![0.0_f32; FRAME_SAMPLES * 2];
	let silence = vec![0.0_f32; FRAME_SAMPLES];
	for pass in 0..3 {
		let mut times: Vec<(f64, usize)> = Vec::new();
		for (n, frame) in signal.as_chunks::<FRAME_SAMPLES>().0.iter().enumerate() {
			if candidate.feed_render {
				candidate
					.ns
					.push_render(&silence)
					.map_err(|e| BenchError::Dsp(e.to_string()))?;
			}
			let timing = BlockTiming {
				generation: 0,
				first_sample: (n * FRAME_SAMPLES) as u64,
			};
			let started = Instant::now();
			candidate
				.ns
				.process_into(frame, &mut out, timing)
				.map_err(|e| BenchError::Dsp(e.to_string()))?;
			times.push((started.elapsed().as_secs_f64() * 1e6, n));
		}
		let over_1ms = times.iter().filter(|t| t.0 > 1_000.0).count();
		times.sort_by(|a, b| b.0.total_cmp(&a.0));
		let top: Vec<String> = times
			.iter()
			.take(5)
			.map(|(t, n)| format!("{t:.0}us@{n}"))
			.collect();
		println!(
			"{backend} pass {pass}: frames {} over1ms {over_1ms} top5 [{}]",
			times.len(),
			top.join(", ")
		);
	}
	Ok(())
}
