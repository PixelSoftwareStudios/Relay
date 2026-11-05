#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use relay_audio::FRAME_SAMPLES;
use relay_audio::codec::EncoderConfig;
use relay_core::{AudioGeneration, CallId, GateSnapshot, Revision, RouteGeneration};
use relay_engine::media::{CallSender, GateCell, Ingress, MediaError, PacketSink, StreamIdentity};

/// A sink that records every packet it is handed.
#[derive(Clone, Default)]
pub struct Wire(pub Arc<Mutex<Vec<Ingress>>>);

impl Wire {
	pub fn take(&self) -> Vec<Ingress> {
		std::mem::take(&mut self.0.lock().unwrap())
	}

	pub fn len(&self) -> usize {
		self.0.lock().unwrap().len()
	}
}

impl PacketSink for Wire {
	fn send(&mut self, call: [u8; 16], generation: u32, rtp: &[u8]) -> Result<(), MediaError> {
		let n = self.0.lock().unwrap().len() as u64;
		self.0
			.lock()
			.unwrap()
			.push(Ingress::new(call, generation, n * 10_000, rtp).unwrap());
		Ok(())
	}
}

pub fn open_gate(call: CallId, route: u64, capture: u64) -> GateSnapshot {
	GateSnapshot {
		call,
		open: true,
		deny: None,
		route: RouteGeneration::new(route),
		capture: AudioGeneration::new(capture),
	}
}

pub fn closed_gate(call: CallId, route: u64, capture: u64) -> GateSnapshot {
	GateSnapshot {
		open: false,
		..open_gate(call, route, capture)
	}
}

pub fn identity(call: CallId, ssrc: u32, generation: u32) -> StreamIdentity {
	StreamIdentity {
		call,
		ssrc,
		generation,
		timestamp_base: 1000,
	}
}

pub fn sender(call: CallId, ssrc: u32, wire: &Wire) -> (CallSender, Arc<GateCell>) {
	let gate = Arc::new(GateCell::closed(call));
	let sender = CallSender::new(
		identity(call, ssrc, 1),
		Arc::clone(&gate),
		EncoderConfig::default(),
		Box::new(wire.clone()),
	)
	.unwrap();
	(sender, gate)
}

pub fn revision(n: u64) -> Revision {
	Revision::new(n)
}

/// The `index`-th 10 ms frame of a sine tone.
pub fn tone(freq: f32, index: u64) -> [f32; FRAME_SAMPLES] {
	let mut frame = [0.0; FRAME_SAMPLES];
	for (i, s) in frame.iter_mut().enumerate() {
		let n = index * FRAME_SAMPLES as u64 + i as u64;
		let phase = (n as f64 * f64::from(freq) / 48_000.0).fract();
		*s = 0.5 * (phase * std::f64::consts::TAU).sin() as f32;
	}
	frame
}

/// Signal power at `freq` (Goertzel), normalized by frame length.
pub fn power_at(samples: &[f32], freq: f32) -> f64 {
	let w = 2.0 * std::f64::consts::PI * f64::from(freq) / 48_000.0;
	let coeff = 2.0 * w.cos();
	let (mut s1, mut s2) = (0.0f64, 0.0f64);
	for &x in samples {
		let s = f64::from(x) + coeff * s1 - s2;
		s2 = s1;
		s1 = s;
	}
	(s1 * s1 + s2 * s2 - coeff * s1 * s2) / (samples.len() as f64).powi(2)
}

pub fn rms(samples: &[f32]) -> f64 {
	(samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / samples.len().max(1) as f64)
		.sqrt()
}
