use proptest::prelude::*;
use relay_protocol::RtpError;
use relay_protocol::rtp::{HEADER_LEN, RtpHeader, RtpProfile};

const PROFILE: RtpProfile = RtpProfile::V1;

fn header() -> RtpHeader {
	RtpHeader {
		marker: true,
		payload_type: 111,
		sequence: 0xfffe,
		timestamp: 0xffff_fe20,
		ssrc: 0xdead_beef,
	}
}

fn encode(header: &RtpHeader, payload: &[u8]) -> Vec<u8> {
	let mut out = vec![0; HEADER_LEN + payload.len()];
	let n = PROFILE.write(header, payload, &mut out).unwrap();
	assert_eq!(n, out.len());
	out
}

#[test]
fn roundtrip_known_vector() {
	let bytes = encode(&header(), &[1, 2, 3]);
	assert_eq!(
		bytes,
		[
			0x80, 0xef, 0xff, 0xfe, 0xff, 0xff, 0xfe, 0x20, 0xde, 0xad, 0xbe, 0xef, 1, 2, 3
		]
	);
	let packet = PROFILE.parse(&bytes).unwrap();
	assert_eq!(packet.header, header());
	assert_eq!(packet.payload, [1, 2, 3]);
}

#[test]
fn rejects_truncated_header() {
	let bytes = encode(&header(), &[9]);
	for len in 0..HEADER_LEN {
		assert_eq!(PROFILE.parse(&bytes[..len]), Err(RtpError::TooShort));
	}
}

#[test]
fn rejects_unsupported_flags() {
	let base = encode(&header(), &[9]);
	let cases = [
		(0x00, 0x40, RtpError::BadVersion(1)),
		(0x00, 0x00, RtpError::BadVersion(0)),
		(0x80, 0xa0, RtpError::Padding),
		(0x80, 0x90, RtpError::Extension),
		(0x80, 0x82, RtpError::Csrc),
	];
	for (clear, set, expected) in cases {
		let mut bytes = base.clone();
		bytes[0] = (bytes[0] & !clear) | set;
		if clear == 0x00 {
			bytes[0] = set;
		}
		assert_eq!(PROFILE.parse(&bytes), Err(expected));
	}
}

#[test]
fn rejects_wrong_payload_type_and_bad_sizes() {
	let mut bytes = encode(&header(), &[9]);
	bytes[1] = 0x80 | 96;
	assert_eq!(PROFILE.parse(&bytes), Err(RtpError::PayloadType(96)));

	let empty = encode_raw_empty();
	assert_eq!(PROFILE.parse(&empty), Err(RtpError::EmptyPayload));

	let mut big = vec![0; HEADER_LEN + 1001];
	big[0] = 0x80;
	big[1] = 111;
	assert_eq!(
		PROFILE.parse(&big),
		Err(RtpError::PayloadTooLarge {
			len: 1001,
			max: 1000
		})
	);
	let mut out = [0; 8];
	assert_eq!(
		PROFILE.write(&header(), &[1], &mut out),
		Err(RtpError::OutputTooSmall { need: 13 })
	);
}

fn encode_raw_empty() -> Vec<u8> {
	let mut out = encode(&header(), &[0]);
	out.truncate(HEADER_LEN);
	out
}

proptest! {
	/// Arbitrary bytes must never panic; accepted packets re-encode exactly.
	#[test]
	fn arbitrary_bytes_never_panic(data in proptest::collection::vec(any::<u8>(), 0..1300)) {
		if let Ok(packet) = PROFILE.parse(&data) {
			let mut out = vec![0; data.len()];
			let n = PROFILE.write(&packet.header, packet.payload, &mut out).unwrap();
			prop_assert_eq!(&out[..n], &data[..]);
		}
	}

	/// Mostly-valid datagrams exercise the deeper validation paths.
	#[test]
	fn mutated_valid_packets_never_panic(
		seq in any::<u16>(), ts in any::<u32>(), ssrc in any::<u32>(),
		payload in proptest::collection::vec(any::<u8>(), 0..1100),
		flips in proptest::collection::vec((0usize..16, any::<u8>()), 0..4),
	) {
		let h = RtpHeader { marker: false, payload_type: 111, sequence: seq, timestamp: ts, ssrc };
		let mut out = vec![0; HEADER_LEN + payload.len()];
		if PROFILE.write(&h, &payload, &mut out).is_ok() {
			for (i, v) in flips {
				if let Some(b) = out.get_mut(i) { *b ^= v; }
			}
			let _ = PROFILE.parse(&out);
		}
	}

	#[test]
	fn roundtrip(seq in any::<u16>(), ts in any::<u32>(), ssrc in any::<u32>(), marker in any::<bool>(),
		payload in proptest::collection::vec(any::<u8>(), 1..1000)) {
		let h = RtpHeader { marker, payload_type: 111, sequence: seq, timestamp: ts, ssrc };
		let bytes = encode(&h, &payload);
		let p = PROFILE.parse(&bytes).unwrap();
		prop_assert_eq!(p.header, h);
		prop_assert_eq!(p.payload, &payload[..]);
	}
}
