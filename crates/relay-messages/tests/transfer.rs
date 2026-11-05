mod common;

use common::*;
use relay_messages::TransferError;
use relay_messages::message::*;
use relay_messages::transfer::*;

const FREE: u64 = 10 * 1024 * 1024 * 1024;

#[test]
fn happy_path_across_several_windows() {
	let data = pseudo_data(3 * 1024 * 1024 + 17);
	let o = offer(1, "big.bin", &data);
	let mut sink = MemSink::new(FREE);
	let mut h = Harness::new(&o, data.clone(), &mut sink);
	assert_eq!(h.r.state(), TransferState::Offered);
	h.start();
	assert_eq!(h.s.state(), TransferState::Completed);
	assert_eq!(h.r.state(), TransferState::Completed);
	assert_eq!(h.r.received(), data.len() as u64);
	assert_eq!(h.r.committed_as(), Some("big.bin"));
	let chunks: Vec<_> = h
		.log
		.iter()
		.filter_map(|m| {
			if let TransferMsg::Chunk { data, .. } = m {
				Some(data.len())
			} else {
				None
			}
		})
		.collect();
	assert!(
		chunks
			.iter()
			.all(|&n| n <= relay_messages::limits::MAX_CHUNK_BYTES)
	);
	assert_eq!(chunks.iter().sum::<usize>(), data.len());
	assert!(
		h.log
			.iter()
			.filter(|m| matches!(m, TransferMsg::Credit(_)))
			.count() >= 3
	);
	assert!(h.log.contains(&TransferMsg::Result(Verdict::Ok)));
	assert_eq!(sink.committed.len(), 1);
	assert_eq!(sink.committed[0].1, data);
	assert!(!sink.aborted);
}

#[test]
fn zero_byte_file_completes() {
	let o = offer(1, "empty", b"");
	let mut sink = MemSink::new(FREE);
	let mut h = Harness::new(&o, Vec::new(), &mut sink);
	h.start();
	assert_eq!(h.r.state(), TransferState::Completed);
	assert_eq!(h.s.state(), TransferState::Completed);
	assert_eq!(sink.committed[0].1, Vec::<u8>::new());
}

#[test]
fn hash_mismatch_aborts_and_never_exposes_the_file() {
	let data = pseudo_data(200_000);
	let o = offer(1, "x.bin", &data);
	let mut sink = MemSink::new(FREE);
	let mut h = Harness::new(&o, data, &mut sink);
	h.corrupt_at = Some(123_456);
	h.start();
	assert_eq!(
		h.r.state(),
		TransferState::Failed(FailureReason::HashMismatch)
	);
	assert_eq!(
		h.s.state(),
		TransferState::Failed(FailureReason::PeerReported)
	);
	assert!(sink.committed.is_empty());
	assert!(sink.aborted && sink.temp.is_none());
}

#[test]
fn source_changed_at_start_aborts_sender() {
	let data = pseudo_data(100_000);
	let o = offer(1, "x.bin", &data);
	let mut sink = MemSink::new(FREE);
	let mut h = Harness::new(&o, data, &mut sink);
	h.checks.push_back(false);
	h.start();
	assert_eq!(
		h.s.state(),
		TransferState::Failed(FailureReason::SourceChanged)
	);
	assert_eq!(h.r.state(), TransferState::Cancelled(CancelBy::Peer));
	assert!(sink.committed.is_empty() && sink.aborted);
}

#[test]
fn source_changed_before_finishing_never_sends_done() {
	let data = pseudo_data(100_000);
	let o = offer(1, "x.bin", &data);
	let mut sink = MemSink::new(FREE);
	let mut h = Harness::new(&o, data, &mut sink);
	h.checks.extend([true, false]);
	h.start();
	assert_eq!(
		h.s.state(),
		TransferState::Failed(FailureReason::SourceChanged)
	);
	assert!(!h.log.contains(&TransferMsg::Done));
	assert!(sink.committed.is_empty() && sink.aborted);
}

#[test]
fn truncated_source_is_detected_by_the_sender() {
	let o = offer(1, "x", &pseudo_data(100));
	let mut s = Sender::new(&o);
	s.on_message(TransferMsg::Accept);
	s.source_checked(true);
	let fx = s.on_message(TransferMsg::Credit(100));
	assert!(matches!(
		fx[0],
		Effect::ReadChunk {
			offset: 0,
			len: 100
		}
	));
	s.chunk_read(vec![0; 40]);
	assert_eq!(
		s.state(),
		TransferState::Failed(FailureReason::SourceChanged)
	);
}

#[test]
fn receiver_cancel_stops_everything() {
	let data = pseudo_data(2_000_000);
	let o = offer(1, "x.bin", &data);
	let mut sink = MemSink::new(FREE);
	let mut r = Receiver::new(&o);
	let mut s = Sender::new(&o);
	for e in r.accept(&mut sink).unwrap() {
		if let Effect::Send(m) = e {
			s.on_message(m);
		}
	}
	let fx = r.cancel(&mut sink);
	assert_eq!(fx, vec![Effect::Send(TransferMsg::Cancel)]);
	assert_eq!(r.state(), TransferState::Cancelled(CancelBy::Local));
	assert!(sink.aborted);
	s.on_message(TransferMsg::Cancel);
	assert_eq!(s.state(), TransferState::Cancelled(CancelBy::Peer));
	// Terminal states ignore everything.
	assert!(r.on_message(TransferMsg::Done, &mut sink).is_empty());
	assert!(r.cancel(&mut sink).is_empty());
	assert!(s.on_message(TransferMsg::Credit(5)).is_empty());
}

#[test]
fn sender_cancel_mid_transfer_discards_partial_file() {
	let data = pseudo_data(300_000);
	let o = offer(1, "x.bin", &data);
	let mut sink = MemSink::new(FREE);
	let mut r = Receiver::new(&o);
	r.accept(&mut sink).unwrap();
	r.on_message(
		TransferMsg::Chunk {
			offset: 0,
			data: data[..1000].to_vec(),
		},
		&mut sink,
	);
	assert_eq!(r.state(), TransferState::Transferring);
	r.on_message(TransferMsg::Cancel, &mut sink);
	assert_eq!(r.state(), TransferState::Cancelled(CancelBy::Peer));
	assert!(sink.committed.is_empty() && sink.aborted);
}

#[test]
fn reject_and_no_space() {
	let o = offer(1, "x.bin", &pseudo_data(1000));
	let mut sink = MemSink::new(FREE);
	let mut r = Receiver::new(&o);
	assert_eq!(r.reject().unwrap(), vec![Effect::Send(TransferMsg::Reject)]);
	assert_eq!(r.state(), TransferState::Rejected);
	assert_eq!(r.accept(&mut sink).unwrap_err(), TransferError::WrongState);
	let mut s = Sender::new(&o);
	s.on_message(TransferMsg::Reject);
	assert_eq!(s.state(), TransferState::Rejected);

	let mut small = MemSink::new(1000);
	let mut r = Receiver::new(&o);
	assert!(matches!(
		r.accept(&mut small),
		Err(TransferError::NoSpace { .. })
	));
	assert_eq!(
		r.state(),
		TransferState::Offered,
		"a failed accept leaves the offer open"
	);
	assert!(small.temp.is_none());
}

#[test]
fn free_space_includes_margin() {
	assert_eq!(required_free_space(0), 16 * 1024 * 1024);
	assert_eq!(required_free_space(1_000_000_000), 1_100_000_000);
}

#[test]
fn protocol_violations_fail_the_transfer() {
	let data = pseudo_data(300_000);
	let o = offer(1, "x.bin", &data);
	let run = |msgs: Vec<TransferMsg>| {
		let mut sink = MemSink::new(FREE);
		let mut r = Receiver::new(&o);
		r.accept(&mut sink).unwrap();
		for m in msgs {
			r.on_message(m, &mut sink);
		}
		(r.state(), sink.aborted, sink.committed.len())
	};
	let c = |off: u64, n: usize| TransferMsg::Chunk {
		offset: off,
		data: data[..n].to_vec(),
	};
	assert_eq!(
		run(vec![c(10, 100)]),
		(TransferState::Failed(FailureReason::Protocol), true, 0)
	);
	assert_eq!(
		run(vec![c(0, 100), c(0, 100)]).0,
		TransferState::Failed(FailureReason::Protocol)
	);
	assert_eq!(
		run(vec![c(0, 70_000)]).0,
		TransferState::Failed(FailureReason::Protocol),
		"chunk over 64 KiB"
	);
	assert_eq!(
		run(vec![TransferMsg::Chunk {
			offset: 0,
			data: vec![]
		}])
		.0,
		TransferState::Failed(FailureReason::Protocol)
	);
	assert_eq!(
		run(vec![TransferMsg::Accept]).0,
		TransferState::Failed(FailureReason::Protocol)
	);
	assert_eq!(
		run(vec![TransferMsg::Done]).0,
		TransferState::Failed(FailureReason::SizeMismatch),
		"done before all bytes"
	);
}

#[test]
fn chunks_beyond_the_granted_window_are_refused() {
	let data = pseudo_data(3 * 1024 * 1024);
	let o = offer(1, "x.bin", &data);
	let mut sink = MemSink::new(FREE);
	let mut r = Receiver::new(&o);
	r.accept(&mut sink).unwrap();
	let mut off = 0usize;
	while off < 1024 * 1024 {
		r.on_message(
			TransferMsg::Chunk {
				offset: off as u64,
				data: data[off..off + 65536].to_vec(),
			},
			&mut sink,
		);
		off += 65536;
		if off == 1024 * 1024 {
			break;
		}
	}
	// The window was extended as data arrived, but never beyond received + 1 MiB.
	assert_ne!(r.state(), TransferState::Failed(FailureReason::Protocol));
	let far = 5 * 1024 * 1024;
	let mut r2 = Receiver::new(&offer(2, "y", &data));
	let mut sink2 = MemSink::new(FREE);
	r2.accept(&mut sink2).unwrap();
	r2.on_message(
		TransferMsg::Chunk {
			offset: far,
			data: vec![1; 10],
		},
		&mut sink2,
	);
	assert_eq!(r2.state(), TransferState::Failed(FailureReason::Protocol));
}

#[test]
fn sink_write_failure_fails_and_cleans_up() {
	let data = pseudo_data(10_000);
	let o = offer(1, "x.bin", &data);
	let mut sink = MemSink::new(FREE);
	sink.fail_write = true;
	let mut h = Harness::new(&o, data, &mut sink);
	h.start();
	assert_eq!(h.r.state(), TransferState::Failed(FailureReason::Sink));
	assert!(sink.aborted && sink.committed.is_empty());
}

#[test]
fn existing_files_are_never_overwritten() {
	let data = pseudo_data(5000);
	let o = offer(1, "report.pdf", &data);
	let mut sink = MemSink::new(FREE);
	sink.existing.push("report.pdf".into());
	let mut h = Harness::new(&o, data, &mut sink);
	h.start();
	assert_eq!(h.r.committed_as(), Some("report.pdf (1)"));
	assert_eq!(
		sink.existing,
		vec!["report.pdf".to_owned(), "report.pdf (1)".to_owned()]
	);
}

#[test]
fn hostile_offer_names_are_sanitized_before_reaching_the_sink() {
	let data = b"x".to_vec();
	let o = offer(1, "..\\..\\Windows\\System32\\con.exe", &data);
	assert_eq!(o.name.as_str(), "_con.exe");
	let mut sink = MemSink::new(FREE);
	let mut h = Harness::new(&o, data, &mut sink);
	h.start();
	assert_eq!(sink.committed[0].0, "_con.exe");
}

#[test]
fn at_most_two_concurrent_transfers() {
	let mut t = Transfers::new();
	let offers: Vec<_> = (1..=3u128)
		.map(|i| offer(i, &format!("f{i}.bin"), b"abc"))
		.collect();
	let mut sinks: Vec<_> = (0..3).map(|_| MemSink::new(FREE)).collect();
	for o in &offers {
		t.offer_received(o);
	}
	t.accept(OfferId(1), &mut sinks[0]).unwrap();
	t.accept(OfferId(2), &mut sinks[1]).unwrap();
	assert_eq!(t.active(), 2);
	assert_eq!(
		t.accept(OfferId(3), &mut sinks[2]).unwrap_err(),
		TransferError::TooManyTransfers
	);
	assert_eq!(t.state(OfferId(3)), Some(TransferState::Offered));
	// Cancelling one frees a slot.
	t.cancel(OfferId(1), Some(&mut sinks[0])).unwrap();
	assert_eq!(t.active(), 1);
	t.accept(OfferId(3), &mut sinks[2]).unwrap();
	// Completing one frees a slot too.
	let done = TransferMsg::Chunk {
		offset: 0,
		data: b"abc".to_vec(),
	};
	t.on_message(OfferId(2), done, Some(&mut sinks[1])).unwrap();
	t.on_message(OfferId(2), TransferMsg::Done, Some(&mut sinks[1]))
		.unwrap();
	assert_eq!(t.state(OfferId(2)), Some(TransferState::Completed));
	assert_eq!(t.active(), 1);
	assert_eq!(t.open_ids(), vec![OfferId(3)]);
	assert_eq!(
		t.accept(OfferId(99), &mut sinks[0]).unwrap_err(),
		TransferError::Unknown
	);
	assert_eq!(
		TransferState::Completed.offer_status(),
		OfferStatus::Completed
	);
}

#[test]
fn sender_side_also_refuses_a_third_active_transfer() {
	let mut t = Transfers::new();
	for i in 1..=3u128 {
		t.offer_sent(&offer(i, "f", b"abc"));
	}
	for i in 1..=2u128 {
		t.on_message(OfferId(i), TransferMsg::Accept, None).unwrap();
	}
	let fx = t.on_message(OfferId(3), TransferMsg::Accept, None).unwrap();
	assert_eq!(fx, vec![Effect::Send(TransferMsg::Cancel)]);
	assert_eq!(
		t.state(OfferId(3)),
		Some(TransferState::Cancelled(CancelBy::Local))
	);
}

#[test]
fn preview_rules() {
	assert!(validate_preview("line\nline").is_ok());
	assert!(validate_preview(&"a".repeat(16 * 1024)).is_ok());
	assert!(validate_preview(&"a".repeat(16 * 1024 + 1)).is_err());
	assert!(validate_preview(&"x\n".repeat(100)).is_ok());
	assert!(validate_preview(&"x\n".repeat(101)).is_err());
	assert!(validate_preview("bad \u{202E} text").is_err());
	assert!(validate_preview("nul \0").is_err());
}

#[test]
fn download_is_unavailable_after_the_session_ends() {
	let (mut s, _) = store();
	s.receive(Wire {
		seq: 1,
		sent_at_ms: 0,
		payload: Payload::Offer(meta(5, "a.zip", b"zz")),
	})
	.unwrap();
	s.receive(Wire {
		seq: 2,
		sent_at_ms: 0,
		payload: Payload::Offer(meta(6, "b.zip", b"zz")),
	})
	.unwrap();
	s.set_offer_status(OfferId(6), OfferStatus::InProgress)
		.unwrap();
	let get = |s: &relay_messages::SessionStore, q| match &s
		.get(MessageId {
			origin: Origin::Remote,
			seq: q,
		})
		.unwrap()
		.body
	{
		MessageBody::File(o) => o.clone(),
		_ => unreachable!(),
	};
	assert!(get(&s, 1).download_available(!s.is_ended()));
	s.end();
	assert!(!get(&s, 1).download_available(!s.is_ended()));
	assert_eq!(get(&s, 1).status, OfferStatus::Expired);
	assert_eq!(get(&s, 2).status, OfferStatus::Cancelled);
	assert_eq!(
		s.set_offer_status(OfferId(5), OfferStatus::InProgress)
			.unwrap_err(),
		relay_messages::StoreError::Ended
	);
}

#[test]
fn traffic_classes_order_voice_first_bulk_last() {
	use relay_messages::TrafficClass::*;
	assert!(Voice < Video && Video < Signaling && Signaling < Preview && Preview < BulkFile);
	assert!(BulkFile.is_bulk() && !Signaling.is_bulk());
	assert_eq!(
		relay_messages::TrafficClass::for_chat_payload(100),
		Signaling
	);
	assert_eq!(
		relay_messages::TrafficClass::for_chat_payload(1 << 20),
		Preview
	);
}
