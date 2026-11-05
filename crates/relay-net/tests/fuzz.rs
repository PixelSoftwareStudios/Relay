//! Property and fuzz-style tests over every parser that sees untrusted bytes.

mod common;

use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Instant;

use proptest::prelude::*;
use prost::Message;
use relay_net::control::{control_channel, validate_envelope};
use relay_net::discovery::sanitize_display_name;
use relay_net::identity::Fingerprint;
use relay_net::media::{MediaReceiver, ReplayVerdict, ReplayWindow, seal_audio};
use relay_net::proto::Envelope;
use relay_net::record::{Lane, RecordHeader};
use relay_net::session::{LinkEpoch, SecureSession};
use relay_net::{AddressPolicy, is_lan_ip};
use tokio::io::AsyncWriteExt;

fn runtime() -> tokio::runtime::Runtime {
	tokio::runtime::Builder::new_current_thread()
		.enable_all()
		.build()
		.unwrap()
}

fn fixed_pair() -> &'static (Arc<SecureSession>, Arc<SecureSession>) {
	use std::sync::OnceLock;
	static PAIR: OnceLock<(Arc<SecureSession>, Arc<SecureSession>)> = OnceLock::new();
	PAIR.get_or_init(|| runtime().block_on(common::sessions()))
}

proptest! {
	#[test]
	fn header_parse_never_panics_and_roundtrips_when_valid(bytes in proptest::collection::vec(any::<u8>(), 0..80)) {
		if let Ok(header) = RecordHeader::parse(&bytes) {
			prop_assert_eq!(&header.encode()[..], &bytes[..32]);
		}
	}

	#[test]
	fn header_encode_parse_roundtrip(epoch in any::<[u8; 16]>(), seq in any::<u64>(), control in any::<bool>()) {
		let lane = if control { Lane::Control } else { Lane::Media };
		let header = RecordHeader::new(lane, LinkEpoch::from_bytes(epoch), seq);
		let parsed = RecordHeader::parse(&header.encode()).unwrap();
		prop_assert_eq!(parsed, header);
		prop_assert_eq!(parsed.lane(), lane);
		prop_assert_eq!(parsed.sequence(), seq & !(1u64 << 63));
	}

	#[test]
	fn media_receiver_rejects_arbitrary_datagrams(bytes in proptest::collection::vec(any::<u8>(), 0..1300)) {
		let (_, rx_session) = fixed_pair();
		let mut rx = MediaReceiver::new(Arc::clone(rx_session));
		prop_assert!(rx.receive(&bytes, Instant::now()).is_err());
	}

	#[test]
	fn media_receiver_rejects_valid_header_with_random_ciphertext(
		seq in 0u64..(1 << 40),
		body in proptest::collection::vec(any::<u8>(), 50..400),
	) {
		let (tx, rx_session) = fixed_pair();
		let mut rx = MediaReceiver::new(Arc::clone(rx_session));
		let mut datagram = RecordHeader::new(Lane::Media, *tx.epoch(), seq).encode().to_vec();
		datagram.extend_from_slice(&body);
		prop_assert!(rx.receive(&datagram, Instant::now()).is_err());
	}

	#[test]
	fn multi_byte_corruption_of_a_valid_datagram_is_always_rejected(
		payload in proptest::collection::vec(any::<u8>(), 0..300),
		edits in proptest::collection::vec((any::<prop::sample::Index>(), 1u8..=255), 1..6),
	) {
		let (tx, rx_session) = fixed_pair();
		let mut rx = MediaReceiver::new(Arc::clone(rx_session));
		rx.bind_stream([1; 16], 1);
		let mut datagram = seal_audio(tx, &[1; 16], 1, &payload).unwrap();
		let original = datagram.clone();
		for (index, xor) in edits {
			let i = index.index(datagram.len());
			datagram[i] ^= xor;
		}
		if datagram != original {
			prop_assert!(rx.receive(&datagram, Instant::now()).is_err());
		}
	}

	#[test]
	fn replay_window_matches_a_reference_model(ops in proptest::collection::vec(0u64..3000, 1..400)) {
		let mut window = ReplayWindow::new();
		let mut accepted: HashSet<u64> = HashSet::new();
		let mut highest: Option<u64> = None;
		for seq in ops {
			let expected = match highest {
				Some(h) if seq <= h && h - seq >= 1024 => ReplayVerdict::TooOld,
				_ if accepted.contains(&seq) => ReplayVerdict::Duplicate,
				_ => ReplayVerdict::Fresh,
			};
			let verdict = window.check(seq);
			prop_assert_eq!(verdict, expected);
			// Checking is read-only: asking twice gives the same answer.
			prop_assert_eq!(window.check(seq), verdict);
			if verdict == ReplayVerdict::Fresh {
				window.commit(seq);
				accepted.insert(seq);
				highest = Some(highest.map_or(seq, |h| h.max(seq)));
			}
		}
	}

	#[test]
	fn envelope_decode_and_validate_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2000)) {
		if let Ok(envelope) = Envelope::decode(bytes.as_slice()) {
			let _ = validate_envelope(&envelope);
		}
	}

	#[test]
	fn control_receiver_survives_arbitrary_stream_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..2000)) {
		runtime().block_on(async {
			let (init, resp) = fixed_pair().clone();
			let _ = init;
			let (mut raw, near) = tokio::io::duplex(1 << 16);
			let (_tx, mut rx) = control_channel(near, resp);
			raw.write_all(&bytes).await.unwrap();
			drop(raw);
			prop_assert!(rx.recv().await.is_err());
			Ok(())
		})?;
	}

	#[test]
	fn fingerprint_parse_never_panics_and_roundtrips(bytes in any::<[u8; 32]>(), junk in ".{0,100}") {
		let _ = Fingerprint::parse(&junk);
		let fp = Fingerprint::from_bytes(bytes);
		prop_assert_eq!(Fingerprint::parse(&fp.full()).unwrap(), fp);
	}

	#[test]
	fn sanitized_names_are_bounded_and_clean(raw in ".{0,400}") {
		let name = sanitize_display_name(&raw);
		prop_assert!(name.chars().count() <= 64 && name.len() <= 256 && !name.is_empty());
		let clean = name.chars().all(|c| !c.is_control() && !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '<' | '>'));
		prop_assert!(clean);
	}

	#[test]
	fn public_ipv4_is_never_accepted(a in any::<u8>(), b in any::<u8>(), c in any::<u8>(), d in any::<u8>()) {
		let ip = Ipv4Addr::new(a, b, c, d);
		let lan = ip.is_private() || ip.is_link_local();
		prop_assert_eq!(is_lan_ip(ip.into()), lan);
		let result = AddressPolicy::lan_only().check_ip(ip.into());
		prop_assert_eq!(result.is_ok(), lan);
	}

	#[test]
	fn manual_endpoint_parser_never_panics(input in ".{0,80}") {
		let _ = AddressPolicy::lan_only().parse_manual(&input);
	}
}
