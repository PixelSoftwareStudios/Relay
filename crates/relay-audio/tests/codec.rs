use relay_audio::AudioError;
use relay_audio::codec::{CallDecoder, CallEncoder, DecodeKind, EncoderConfig, MAX_PACKET_BYTES};

fn tone(offset: usize) -> Vec<f32> {
	(0..480)
		.map(|i| (std::f32::consts::TAU * 440.0 * (i + offset) as f32 / 48_000.0).sin() * 0.4)
		.collect()
}

#[test]
fn round_trips_a_tone() {
	let mut enc = CallEncoder::new(EncoderConfig::default()).unwrap();
	let mut dec = CallDecoder::new().unwrap();
	let mut packet = [0u8; MAX_PACKET_BYTES];
	let mut out = [0.0; 480];
	let mut energy = 0.0;
	for n in 0..50 {
		let len = enc.encode(&tone(n * 480), &mut packet).unwrap();
		assert!(len > 0 && len < MAX_PACKET_BYTES);
		assert_eq!(
			dec.decode(&packet[..len], &mut out).unwrap(),
			DecodeKind::Normal
		);
		if n > 10 {
			energy += out.iter().map(|v| v * v).sum::<f32>();
		}
	}
	assert!(energy > 1.0, "decoded audio should carry the tone");
	assert_eq!(dec.consecutive_errors(), 0);
}

#[test]
fn rejects_wrong_sizes() {
	let mut enc = CallEncoder::new(EncoderConfig::default()).unwrap();
	let mut packet = [0u8; 100];
	assert!(matches!(
		enc.encode(&[0.0; 960], &mut packet),
		Err(AudioError::BadFrameSize { .. })
	));
	let mut dec = CallDecoder::new().unwrap();
	assert!(dec.decode(&[], &mut [0.0; 480]).is_err());
	assert!(dec.conceal(&mut [0.0; 100]).is_err());
	assert!(dec.consecutive_errors() >= 1);
}

#[test]
fn rejects_twenty_ms_packets() {
	// A 20 ms packet is valid Opus but not valid for the 10 ms profile.
	let mut raw =
		opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::Voip).unwrap();
	let mut packet = [0u8; 400];
	let len = raw.encode_float(&[0.1; 960], &mut packet).unwrap();
	let mut dec = CallDecoder::new().unwrap();
	assert!(matches!(
		dec.decode(&packet[..len], &mut [0.0; 480]),
		Err(AudioError::BadFrameSize { actual: 960, .. })
	));
}

#[test]
fn concealment_and_fec_hooks_produce_frames() {
	let cfg = EncoderConfig {
		inband_fec: true,
		expected_loss_pct: 20,
		..EncoderConfig::default()
	};
	let mut enc = CallEncoder::new(cfg).unwrap();
	let mut dec = CallDecoder::new().unwrap();
	let mut packets = Vec::new();
	let mut buf = [0u8; MAX_PACKET_BYTES];
	for n in 0..20 {
		let len = enc.encode(&tone(n * 480), &mut buf).unwrap();
		packets.push(buf[..len].to_vec());
	}
	let mut out = [0.0; 480];
	for p in &packets[..10] {
		dec.decode(p, &mut out).unwrap();
	}
	// Packet 10 is lost: conceal it.
	assert_eq!(dec.conceal(&mut out).unwrap(), DecodeKind::Concealed);
	// Packet 11 arrives; a different loss would be repaired from its FEC data.
	assert_eq!(
		dec.recover_previous(&packets[11], &mut out).unwrap(),
		DecodeKind::FecRecovered
	);
	assert_eq!(
		dec.decode(&packets[11], &mut out).unwrap(),
		DecodeKind::Normal
	);
}

#[test]
fn calls_have_independent_state() {
	let mut a = CallEncoder::new(EncoderConfig::default()).unwrap();
	let mut b = CallEncoder::new(EncoderConfig::default()).unwrap();
	let (mut pa, mut pb) = ([0u8; MAX_PACKET_BYTES], [0u8; MAX_PACKET_BYTES]);
	for n in 0..5 {
		a.encode(&tone(n * 480), &mut pa).unwrap();
	}
	// b has never seen audio: its first packet equals a fresh encoder's.
	let mut fresh = CallEncoder::new(EncoderConfig::default()).unwrap();
	let mut pf = [0u8; MAX_PACKET_BYTES];
	let lb = b.encode(&tone(0), &mut pb).unwrap();
	let lf = fresh.encode(&tone(0), &mut pf).unwrap();
	assert_eq!(pb[..lb], pf[..lf]);
}

#[test]
fn bitrate_is_bounded() {
	let cfg = EncoderConfig {
		bitrate_bps: 128_000,
		..EncoderConfig::default()
	};
	assert!(CallEncoder::new(cfg).is_err());
}
