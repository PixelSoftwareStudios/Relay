//! Criterion microbenchmarks for individual audio stages (warm state, 10 ms frames).

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use relay_audio::codec::{CallDecoder, CallEncoder, EncoderConfig, MAX_PACKET_BYTES};
use relay_audio::convert::StreamResampler;
use relay_audio::dsp::{Agc, AgcConfig, HighPass, NoiseSuppressor, OffSuppressor};
use relay_audio::format::BlockTiming;
use relay_audio::mixer::Mixer;

fn tone(n: usize) -> Vec<f32> {
	(0..n).map(|i| (i as f32 * 0.07).sin() * 0.3).collect()
}

fn stages(c: &mut Criterion) {
	let frame = tone(480);

	let mut enc = CallEncoder::new(EncoderConfig::default()).unwrap();
	let mut packet = [0u8; MAX_PACKET_BYTES];
	c.bench_function("opus_encode_10ms", |b| {
		b.iter(|| enc.encode(black_box(&frame), &mut packet).unwrap())
	});

	let len = enc.encode(&frame, &mut packet).unwrap();
	let mut dec = CallDecoder::new().unwrap();
	let mut out = [0.0_f32; 480];
	c.bench_function("opus_decode_10ms", |b| {
		b.iter(|| dec.decode(black_box(&packet[..len]), &mut out).unwrap())
	});

	let mut resampler = StreamResampler::new(44_100, 48_000, 441).unwrap();
	let input = tone(441);
	c.bench_function("resample_44k1_to_48k_10ms", |b| {
		b.iter(|| {
			resampler
				.push(black_box(&input), |o| {
					black_box(o);
				})
				.unwrap()
		})
	});

	let mut mixer = Mixer::new(480);
	let slots: Vec<_> = (0..4).map(|_| mixer.open_slot().unwrap()).collect();
	let mut mixed = [0.0_f32; 480];
	c.bench_function("mixer_4_calls_10ms", |b| {
		b.iter(|| {
			mixer.begin();
			for slot in &slots {
				mixer.add(*slot, black_box(&frame)).unwrap();
			}
			mixer.finish(&mut mixed).unwrap();
		})
	});

	let mut hp = HighPass::new(80.0);
	let mut work = frame.clone();
	c.bench_function("high_pass_10ms", |b| {
		b.iter(|| hp.process(black_box(&mut work)))
	});

	let mut agc = Agc::new(AgcConfig::default());
	c.bench_function("agc_10ms", |b| b.iter(|| agc.process(black_box(&mut work))));

	let mut off = OffSuppressor::default();
	let mut scratch = [0.0_f32; 480];
	c.bench_function("suppressor_off_10ms", |b| {
		b.iter(|| {
			off.process_into(black_box(&frame), &mut scratch, BlockTiming::default())
				.unwrap()
		})
	});
}

criterion_group!(benches, stages);
criterion_main!(benches);
