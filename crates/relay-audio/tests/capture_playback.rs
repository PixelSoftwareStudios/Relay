use relay_audio::capture::{CaptureSetup, capture_pair};
use relay_audio::convert::ChannelSelect;
use relay_audio::format::Discontinuity;
use relay_audio::playback::playback_pair;

fn setup(ring: usize) -> CaptureSetup {
	CaptureSetup {
		channels: 1,
		select: ChannelSelect::default(),
		ring_samples: ring,
	}
}

fn read_all(reader: &mut relay_audio::capture::CaptureReader) -> Vec<(u64, Vec<f32>, Option<Discontinuity>)> {
	let mut out = Vec::new();
	let mut buf = [0.0; 64];
	loop {
		let r = reader.read(&mut buf);
		if r.count == 0 && r.discontinuity.is_none() {
			break;
		}
		out.push((r.first_index, buf[..r.count].to_vec(), r.discontinuity));
		if r.count == 0 {
			break;
		}
	}
	out
}

#[test]
fn indices_are_contiguous_without_gaps() {
	let (mut cb, mut reader, _) = capture_pair(setup(1024));
	let data: Vec<f32> = (0..100).map(|i| i as f32).collect();
	cb.process(&data[..40]);
	cb.process(&data[40..]);
	let blocks = read_all(&mut reader);
	let total: usize = blocks.iter().map(|b| b.1.len()).sum();
	assert_eq!(total, 100);
	assert_eq!(blocks[0].0, 0);
	assert!(blocks.iter().all(|b| b.2.is_none()));
}

#[test]
fn overrun_never_writes_past_capacity_and_is_accounted() {
	let (mut cb, mut reader, shared) = capture_pair(setup(100));
	let data: Vec<f32> = (0..250).map(|i| i as f32).collect();
	cb.process(&data);
	assert_eq!(shared.overrun_frames(), 150);
	let first = read_all(&mut reader);
	assert_eq!(first.iter().map(|b| b.1.len()).sum::<usize>(), 100);
	// The next callback publishes the gap, so indices skip the lost span.
	cb.process(&[7.0; 10]);
	let after = read_all(&mut reader);
	let b = after.iter().find(|b| !b.1.is_empty()).unwrap();
	assert_eq!(b.0, 250);
	assert_eq!(b.2, Some(Discontinuity::Overrun));
}

#[test]
fn mute_discards_queued_audio_and_advances_the_timeline() {
	let (mut cb, mut reader, shared) = capture_pair(setup(1024));
	cb.process(&[0.1; 50]);
	shared.set_muted(true);
	// Queued-before-mute audio must never surface after unmute.
	let r = reader.read(&mut [0.0; 64]);
	assert_eq!(r.count, 0);
	assert_eq!(r.discontinuity, Some(Discontinuity::Mute));
	cb.process(&[0.2; 30]);
	shared.set_muted(false);
	cb.process(&[0.3; 20]);
	let blocks = read_all(&mut reader);
	let samples: Vec<f32> = blocks.iter().flat_map(|b| b.1.clone()).collect();
	assert_eq!(samples, vec![0.3; 20]);
	let first = blocks.iter().find(|b| !b.1.is_empty()).unwrap();
	assert_eq!(first.0, 50 + 30);
	assert_eq!(first.2, Some(Discontinuity::Mute));
}

#[test]
fn playback_underflow_is_silence_not_a_replay() {
	let (mut cb, mut writer, shared) = playback_pair(2, 1024);
	writer.write(&[0.5; 10]);
	let mut out = [1.0_f32; 40]; // 20 stereo frames
	cb.process(&mut out);
	assert!(out[..20].iter().all(|v| *v == 0.5));
	assert!(out[20..].iter().all(|v| *v == 0.0));
	assert_eq!(shared.underrun_frames(), 10);
	let mut again = [1.0_f32; 8];
	cb.process(&mut again);
	assert!(again.iter().all(|v| *v == 0.0));
}

#[test]
fn playback_mute_and_flush_drop_stale_audio() {
	let (mut cb, mut writer, shared) = playback_pair(1, 1024);
	writer.write(&[0.5; 100]);
	shared.set_muted(true);
	let mut out = [1.0_f32; 40];
	cb.process(&mut out);
	assert!(out.iter().all(|v| *v == 0.0));
	shared.set_muted(false);
	shared.request_flush();
	let mut out = [1.0_f32; 10];
	cb.process(&mut out);
	assert!(out.iter().all(|v| *v == 0.0), "flushed audio must not play");
}

#[test]
fn writer_counts_overflow() {
	let (_cb, mut writer, shared) = playback_pair(1, 16);
	assert_eq!(writer.write(&[0.1; 40]), 16);
	assert_eq!(shared.overflow_frames(), 24);
}
