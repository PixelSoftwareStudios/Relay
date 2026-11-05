//! Differential tests against the `rtp` crate used purely as an oracle.

use bytes::Bytes;
use proptest::prelude::*;
use relay_protocol::rtp::{HEADER_LEN, RtpHeader, RtpProfile};
use rtp::header::Header;
use rtp::packet::Packet;
use webrtc_util::marshal::{Marshal, Unmarshal};

const PROFILE: RtpProfile = RtpProfile::V1;

proptest! {
	/// Our serializer is byte-identical to the oracle's.
	#[test]
	fn serialization_matches_oracle(
		seq in any::<u16>(), ts in any::<u32>(), ssrc in any::<u32>(), marker in any::<bool>(),
		payload in proptest::collection::vec(any::<u8>(), 1..1000),
	) {
		let ours = RtpHeader { marker, payload_type: 111, sequence: seq, timestamp: ts, ssrc };
		let mut out = vec![0; HEADER_LEN + payload.len()];
		PROFILE.write(&ours, &payload, &mut out).unwrap();

		let oracle = Packet {
			header: Header {
				version: 2, marker, payload_type: 111, sequence_number: seq,
				timestamp: ts, ssrc, ..Default::default()
			},
			payload: Bytes::from(payload),
		};
		prop_assert_eq!(&out[..], &oracle.marshal().unwrap()[..]);
	}

	/// Whenever we accept bytes the oracle agrees on every field; whenever
	/// the oracle parses a packet inside our profile we accept it too.
	#[test]
	fn parsing_agrees_with_oracle(data in proptest::collection::vec(any::<u8>(), 0..200)) {
		let ours = PROFILE.parse(&data);
		let mut buf = &data[..];
		let theirs = Packet::unmarshal(&mut buf);
		match (&ours, &theirs) {
			(Ok(a), Ok(b)) => {
				prop_assert_eq!(a.header.marker, b.header.marker);
				prop_assert_eq!(a.header.payload_type, b.header.payload_type);
				prop_assert_eq!(a.header.sequence, b.header.sequence_number);
				prop_assert_eq!(a.header.timestamp, b.header.timestamp);
				prop_assert_eq!(a.header.ssrc, b.header.ssrc);
				prop_assert_eq!(a.payload, &b.payload[..]);
			}
			(Ok(_), Err(_)) => prop_assert!(false, "we accepted what the oracle rejects"),
			(Err(_), Ok(b)) => {
				let inside_profile = b.header.version == 2
					&& !b.header.padding && !b.header.extension && b.header.csrc.is_empty()
					&& b.header.payload_type == 111
					&& !b.payload.is_empty() && b.payload.len() <= 1000;
				prop_assert!(!inside_profile, "we rejected an in-profile packet");
			}
			(Err(_), Err(_)) => {}
		}
	}
}

// Seeds the generator with mostly well-formed packets so the `(Ok, Ok)` arm runs.
proptest! {
	#[test]
	fn valid_prefix_agrees(
		seq in any::<u16>(), ts in any::<u32>(), ssrc in any::<u32>(),
		tail in proptest::collection::vec(any::<u8>(), 1..64),
	) {
		let mut data = vec![0x80, 111];
		data.extend_from_slice(&seq.to_be_bytes());
		data.extend_from_slice(&ts.to_be_bytes());
		data.extend_from_slice(&ssrc.to_be_bytes());
		data.extend_from_slice(&tail);
		let ours = PROFILE.parse(&data).unwrap();
		let mut buf = &data[..];
		let theirs = Packet::unmarshal(&mut buf).unwrap();
		prop_assert_eq!(ours.header.sequence, theirs.header.sequence_number);
		prop_assert_eq!(ours.payload, &theirs.payload[..]);
	}
}
