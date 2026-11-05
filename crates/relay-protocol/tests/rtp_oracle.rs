//! Differential tests against an independent RFC 3550 reference model.
//!
//! The model below is written from section 5.1 of the RFC and shares no code
//! with `relay_protocol`. Fixed known-answer vectors pin both implementations
//! to bytes worked out by hand.

use proptest::prelude::*;
use relay_protocol::rtp::{HEADER_LEN, RtpHeader, RtpProfile};

const PROFILE: RtpProfile = RtpProfile::V1;

/// A fully general RTP packet as RFC 3550 section 5.1 defines it.
#[derive(Debug, PartialEq)]
struct RefPacket {
	padding: bool,
	extension: bool,
	csrc_count: usize,
	marker: bool,
	payload_type: u8,
	sequence: u16,
	timestamp: u32,
	ssrc: u32,
	payload: Vec<u8>,
}

/// Serializes a version 2 packet with no CSRC, extension or padding.
fn ref_serialize(marker: bool, pt: u8, seq: u16, ts: u32, ssrc: u32, payload: &[u8]) -> Vec<u8> {
	let mut out = vec![0x80, (u8::from(marker) << 7) | (pt & 0x7f)];
	out.extend_from_slice(&seq.to_be_bytes());
	out.extend_from_slice(&ts.to_be_bytes());
	out.extend_from_slice(&ssrc.to_be_bytes());
	out.extend_from_slice(payload);
	out
}

/// Parses any valid version 2 packet, including CSRC list, header extension and padding.
fn ref_parse(d: &[u8]) -> Option<RefPacket> {
	if d.len() < 12 || d[0] >> 6 != 2 {
		return None;
	}
	let padding = d[0] & 0x20 != 0;
	let extension = d[0] & 0x10 != 0;
	let csrc_count = usize::from(d[0] & 0x0f);
	let mut at = 12 + 4 * csrc_count;
	if d.len() < at {
		return None;
	}
	if extension {
		if d.len() < at + 4 {
			return None;
		}
		let words = usize::from(u16::from_be_bytes([d[at + 2], d[at + 3]]));
		at += 4 + 4 * words;
		if d.len() < at {
			return None;
		}
	}
	let mut end = d.len();
	if padding {
		let pad = usize::from(*d.last()?);
		if pad == 0 || at + pad > end {
			return None;
		}
		end -= pad;
	}
	Some(RefPacket {
		padding,
		extension,
		csrc_count,
		marker: d[1] & 0x80 != 0,
		payload_type: d[1] & 0x7f,
		sequence: u16::from_be_bytes([d[2], d[3]]),
		timestamp: u32::from_be_bytes([d[4], d[5], d[6], d[7]]),
		ssrc: u32::from_be_bytes([d[8], d[9], d[10], d[11]]),
		payload: d[at..end].to_vec(),
	})
}

#[test]
fn known_answer_serialization() {
	// Marker set, PT 111, seq 0x1234, ts 0x01020304, ssrc 0xdeadbeef, payload "ab".
	let want = [
		0x80, 0xef, 0x12, 0x34, 0x01, 0x02, 0x03, 0x04, 0xde, 0xad, 0xbe, 0xef, b'a', b'b',
	];
	let h = RtpHeader {
		marker: true,
		payload_type: 111,
		sequence: 0x1234,
		timestamp: 0x0102_0304,
		ssrc: 0xdead_beef,
	};
	let mut out = vec![0; HEADER_LEN + 2];
	PROFILE.write(&h, b"ab", &mut out).unwrap();
	assert_eq!(out, want);
	assert_eq!(
		ref_serialize(true, 111, 0x1234, 0x0102_0304, 0xdead_beef, b"ab"),
		want
	);
	// Wrap-around values and a clear marker.
	let want = [
		0x80, 0x6f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 7,
	];
	let h = RtpHeader {
		marker: false,
		payload_type: 111,
		sequence: u16::MAX,
		timestamp: u32::MAX,
		ssrc: 0,
	};
	let mut out = vec![0; HEADER_LEN + 1];
	PROFILE.write(&h, &[7], &mut out).unwrap();
	assert_eq!(out, want);
}

#[test]
fn known_answer_parsing() {
	let data = [
		0x80, 0x6f, 0x00, 0x2a, 0, 0, 0x03, 0xc0, 0, 0, 0, 9, 1, 2, 3,
	];
	let p = PROFILE.parse(&data).unwrap();
	assert!(!p.header.marker);
	assert_eq!(p.header.sequence, 42);
	assert_eq!(p.header.timestamp, 960);
	assert_eq!(p.header.ssrc, 9);
	assert_eq!(p.payload, &[1, 2, 3]);
	// RFC 3550 features outside the profile: padding, extension, CSRC, version 1.
	for first in [0xa0u8, 0x90, 0x81, 0x40] {
		let mut d = data;
		d[0] = first;
		assert!(PROFILE.parse(&d).is_err(), "first byte {first:#x}");
	}
	// The reference model understands those features.
	let padded = [0xa0, 0x6f, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 5, 0, 0, 3];
	let r = ref_parse(&padded).unwrap();
	assert_eq!(r.payload, [5]);
	let ext = [
		0x90, 0x6f, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0xbe, 0xde, 0, 1, 9, 9, 9, 9, 5,
	];
	assert_eq!(ref_parse(&ext).unwrap().payload, [5]);
	let csrc = [0x81, 0x6f, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 5];
	assert_eq!(ref_parse(&csrc).unwrap().payload, [5]);
	assert!(ref_parse(&[0x80; 11]).is_none());
}

proptest! {
	/// Our serializer is byte-identical to the reference model's.
	#[test]
	fn serialization_matches_reference(
		seq in any::<u16>(), ts in any::<u32>(), ssrc in any::<u32>(), marker in any::<bool>(),
		payload in proptest::collection::vec(any::<u8>(), 1..1000),
	) {
		let ours = RtpHeader { marker, payload_type: 111, sequence: seq, timestamp: ts, ssrc };
		let mut out = vec![0; HEADER_LEN + payload.len()];
		PROFILE.write(&ours, &payload, &mut out).unwrap();
		prop_assert_eq!(out, ref_serialize(marker, 111, seq, ts, ssrc, &payload));
	}

	/// Whenever we accept bytes the reference agrees on every field; whenever
	/// the reference parses a packet inside our profile we accept it too.
	#[test]
	fn parsing_agrees_with_reference(data in proptest::collection::vec(any::<u8>(), 0..200)) {
		let ours = PROFILE.parse(&data);
		let theirs = ref_parse(&data);
		match (&ours, &theirs) {
			(Ok(a), Some(b)) => {
				prop_assert_eq!(a.header.marker, b.marker);
				prop_assert_eq!(a.header.payload_type, b.payload_type);
				prop_assert_eq!(a.header.sequence, b.sequence);
				prop_assert_eq!(a.header.timestamp, b.timestamp);
				prop_assert_eq!(a.header.ssrc, b.ssrc);
				prop_assert_eq!(a.payload, &b.payload[..]);
			}
			(Ok(_), None) => prop_assert!(false, "we accepted what the reference rejects"),
			(Err(_), Some(b)) => {
				let inside_profile = !b.padding && !b.extension && b.csrc_count == 0
					&& b.payload_type == 111
					&& !b.payload.is_empty() && b.payload.len() <= 1000;
				prop_assert!(!inside_profile, "we rejected an in-profile packet");
			}
			(Err(_), None) => {}
		}
	}

	// Seeds the generator with mostly well-formed packets so the `(Ok, Some)` arm runs.
	#[test]
	fn valid_prefix_agrees(
		seq in any::<u16>(), ts in any::<u32>(), ssrc in any::<u32>(),
		tail in proptest::collection::vec(any::<u8>(), 1..64),
	) {
		let data = ref_serialize(false, 111, seq, ts, ssrc, &tail);
		let ours = PROFILE.parse(&data).unwrap();
		let theirs = ref_parse(&data).unwrap();
		prop_assert_eq!(ours.header.sequence, theirs.sequence);
		prop_assert_eq!(ours.payload, &theirs.payload[..]);
	}
}
