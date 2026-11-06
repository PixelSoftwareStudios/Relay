//! Seed data of the mock messaging engine.

use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use relay_api::pb::{self, transcript_entry};

use super::hub::MsgHub;
use super::state::Feed;

/// Call ids shared with `crate::mock::seed` (`call(100, ..)`).
pub const ADA_CALL: u8 = 100;
pub(super) const GRACE_CALL: u8 = 101;
/// The seeded call that has ended, with a long retained history.
pub const ALAN_CALL: u8 = 90;
pub(super) const NOW_MS: u64 = 1_760_000_000_000;
const DAY_MS: u64 = 86_400_000;
pub(super) const BIG_HISTORY: u64 = 3_000;

pub(super) fn sid(n: u8) -> Vec<u8> {
	vec![n; 16]
}

pub(super) fn summary(n: u8, name: &str, ended: bool, count: u64) -> pb::SessionSummary {
	pb::SessionSummary {
		session_id: sid(n),
		call_id: vec![n; 16],
		peer_id: vec![n; 32],
		peer_name: name.into(),
		state: if ended {
			pb::SessionState::Ended
		} else {
			pb::SessionState::Active
		} as i32,
		permissions: Some(pb::SessionPermissions {
			allow_messages: true,
			allow_files: true,
		}),
		local_is_host: true,
		started_at_unix_ms: NOW_MS - 3_600_000,
		ended_at_unix_ms: if ended { NOW_MS - 600_000 } else { 0 },
		message_count: count,
		next_ordinal: count + 1,
		used_bytes: count * 1100,
		limit_bytes: 64 * 1024 * 1024,
	}
}

pub(super) fn entry(
	ordinal: u64,
	outgoing: bool,
	body: transcript_entry::Body,
) -> pb::TranscriptEntry {
	pb::TranscriptEntry {
		ordinal,
		outgoing,
		seq: ordinal,
		sent_at_unix_ms: NOW_MS + ordinal * 60_000,
		received_at_unix_ms: NOW_MS + ordinal * 60_000 + 40,
		delivery: pb::DeliveryState::Delivered as i32,
		body: Some(body),
		..Default::default()
	}
}

pub(super) fn text(ordinal: u64, outgoing: bool, t: &str) -> pb::TranscriptEntry {
	entry(ordinal, outgoing, transcript_entry::Body::Text(t.into()))
}

pub(super) fn offer(
	n: u8,
	name: &str,
	size: u64,
	state: pb::OfferState,
	outgoing: bool,
) -> pb::FileOfferBody {
	pb::FileOfferBody {
		offer_id: vec![n; 16],
		name: name.into(),
		size,
		media_type: if name.ends_with(".sh") {
			"application/x-sh".into()
		} else {
			"application/octet-stream".into()
		},
		blake3: vec![n.wrapping_mul(37); 32],
		state: state as i32,
		executable_warning: name.ends_with(".sh"),
		outgoing,
		..Default::default()
	}
}

/// A small gradient PNG, well under the 15 KiB inline cap.
pub(super) fn sample_png() -> Vec<u8> {
	let (w, h) = (96u32, 64u32);
	let mut rgba = Vec::with_capacity((w * h * 4) as usize);
	for y in 0..h {
		for x in 0..w {
			rgba.extend_from_slice(&[(x * 255 / w) as u8, (y * 255 / h) as u8, 160, 255]);
		}
	}
	let mut out = Vec::new();
	let _ = PngEncoder::new(&mut out).write_image(&rgba, w, h, ExtendedColorType::Rgba8);
	out
}

pub(super) fn ada_entries() -> Vec<pb::TranscriptEntry> {
	let png = sample_png();
	let mut preview_offer = offer(3, "notes.txt", 2_048, pb::OfferState::Offered, false);
	preview_offer.preview_requested = true;
	let mut failed = text(8, true, "Did that arrive?");
	failed.delivery = pb::DeliveryState::Failed as i32;
	failed.failure = "timeout".into();
	let mut sending = text(9, true, "Retrying in a moment.");
	sending.delivery = pb::DeliveryState::Sending as i32;
	let mut pinned = text(2, true, "Server is `build-07`; use **port 8443**.");
	pinned.pinned = true;
	let mut entries = vec![
		text(
			1,
			false,
			"Hi! Here is the *fix* we talked about:\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```",
		),
		pinned,
		text(
			3,
			false,
			"Docs: https://relay.example/docs and a lookalike https://xn--pple-43d.com/login",
		),
		entry(
			4,
			false,
			transcript_entry::Body::Image(pb::ImageBody {
				media_type: "image/png".into(),
				width: 96,
				height: 64,
				frames: 1,
				data: png,
			}),
		),
		text(5, false, "Checklist:\n- build\n- test\n- ship"),
		entry(
			6,
			false,
			transcript_entry::Body::File(offer(
				1,
				"report-q3.pdf",
				3_400_000,
				pb::OfferState::Offered,
				false,
			)),
		),
		entry(
			7,
			true,
			transcript_entry::Body::File(offer(
				2,
				"diagram.png",
				880_000,
				pb::OfferState::Completed,
				true,
			)),
		),
		failed,
		sending,
		entry(10, false, transcript_entry::Body::File(preview_offer)),
		entry(
			11,
			false,
			transcript_entry::Body::File(offer(
				4,
				"install.sh",
				1_200,
				pb::OfferState::Offered,
				false,
			)),
		),
	];
	// The first two messages are older, so the transcript shows the day
	// separators and the three time formats.
	for (n, days) in [(1, 2), (2, 1)] {
		let at = NOW_MS - days * DAY_MS;
		if let Some(e) = entries.iter_mut().find(|e| e.ordinal == n) {
			e.sent_at_unix_ms = at;
			e.received_at_unix_ms = at + 40;
		}
	}
	entries
}

pub(super) fn long_entries() -> Vec<pb::TranscriptEntry> {
	(1..=BIG_HISTORY)
		.map(|n| {
			let body = format!(
				"Message {n}: the quick brown fox jumps over the lazy dog{}",
				if n % 7 == 0 { "\n- a\n- b" } else { "" }
			);
			text(n, n % 3 == 0, &body)
		})
		.collect()
}

/// Seed sessions and transcripts.
pub fn seed(hub: &MsgHub) {
	let sessions = vec![
		summary(ADA_CALL, "Ada Lovelace", false, 11),
		summary(GRACE_CALL, "Grace Hopper", false, 0),
		summary(ALAN_CALL, "Alan Turing", true, BIG_HISTORY),
	];
	let settings = pb::MessagingSettings {
		enabled: true,
		max_offer_size_mib: 1024,
		transfer_rate_limit_mbps: 16,
		transfer_port: 0,
	};
	hub.update(|s| {
		s.load_list(&sessions, Some(&settings));
		s.set_transcript(&hex::encode(sid(ADA_CALL)), &ada_entries());
		s.set_transcript(&hex::encode(sid(GRACE_CALL)), &[]);
		s.set_transcript(&hex::encode(sid(ALAN_CALL)), &long_entries());
		s.feed = Feed::Live;
	});
}
