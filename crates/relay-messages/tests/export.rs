mod common;

use std::sync::Arc;

use common::*;
use relay_messages::message::*;
use relay_messages::store::{EXPORT_SCHEMA, Export, ExportContent, ExportError, ExportOptions};

fn populated() -> relay_messages::SessionStore {
	let (mut s, clock) = store();
	let w = s
		.compose_text("deploy:\n```sh\nkubectl get pods\n```")
		.unwrap();
	s.on_ack(w.seq, AckStatus::Delivered).unwrap();
	clock.advance(1000);
	s.receive(wire_text(1, "ok *thanks*")).unwrap();
	clock.advance(1000);
	s.compose_image(Arc::from(png(64, 32))).unwrap();
	clock.advance(1000);
	s.receive(Wire {
		seq: 2,
		sent_at_ms: 3,
		payload: Payload::Offer(meta(0xabc, "../logs/app.log", b"data")),
	})
	.unwrap();
	s.set_pinned(
		MessageId {
			origin: Origin::Local,
			seq: 1,
		},
		true,
	)
	.unwrap();
	s
}

#[test]
fn export_schema_and_round_trip() {
	let s = populated();
	let json = s.export_json(ExportOptions { embed_images: true });
	let e = Export::from_json(&json).unwrap();
	assert_eq!(e.schema, EXPORT_SCHEMA);
	assert_eq!(e.version, 1);
	assert_eq!(e.session_id, "01".repeat(16));
	assert_eq!(e.peer.fingerprint, "01".repeat(32));
	assert_eq!(e.peer.display_name, "peer-1");
	assert_eq!(e.messages.len(), 4);
	assert_eq!(e, s.export(ExportOptions { embed_images: true }));
	assert!(e.messages[0].pinned);
	assert_eq!(e.messages[0].delivery, "delivered");
	assert_eq!(e.messages[1].delivery, "delivered");
	assert_eq!(e.messages[2].delivery, "sending");
	match &e.messages[0].content {
		ExportContent::Text { code_blocks, .. } => {
			assert_eq!(code_blocks.len(), 1);
			assert_eq!(code_blocks[0].language.as_deref(), Some("sh"));
			assert_eq!(code_blocks[0].text, "kubectl get pods");
		}
		c => panic!("{c:?}"),
	}
	assert_eq!(
		Export::decode_image(&e.messages[2].content).unwrap(),
		png(64, 32)
	);
	match &e.messages[3].content {
		ExportContent::FileOffer {
			name,
			size,
			blake3,
			offer_id,
			..
		} => {
			assert_eq!(name, "app.log");
			assert_eq!(*size, 4);
			assert_eq!(blake3, &blake3::hash(b"data").to_hex().to_string());
			assert_eq!(offer_id.len(), 32);
		}
		c => panic!("{c:?}"),
	}
}

#[test]
fn images_are_references_unless_embedding_is_requested() {
	let s = populated();
	let e = s.export(ExportOptions::default());
	match &e.messages[2].content {
		ExportContent::Image {
			data_base64,
			bytes_len,
			width,
			height,
			media_type,
			..
		} => {
			assert!(data_base64.is_none());
			assert_eq!(
				(*width, *height, media_type.as_str()),
				(64, 32, "image/png")
			);
			assert_eq!(*bytes_len, png(64, 32).len() as u64);
		}
		c => panic!("{c:?}"),
	}
	assert!(
		!s.export_json(ExportOptions::default())
			.contains("data_base64\": \"")
	);
}

#[test]
fn export_is_valid_json_with_stable_keys() {
	let v: serde_json::Value =
		serde_json::from_str(&populated().export_json(ExportOptions::default())).unwrap();
	for k in [
		"schema",
		"version",
		"exported_at_ms",
		"session_id",
		"started_at_ms",
		"peer",
		"messages",
	] {
		assert!(v.get(k).is_some(), "missing {k}");
	}
	let m = &v["messages"][0];
	for k in [
		"seq",
		"direction",
		"sent_at_ms",
		"received_at_ms",
		"delivery",
		"pinned",
		"content",
	] {
		assert!(m.get(k).is_some(), "missing {k}");
	}
	assert_eq!(m["content"]["kind"], "text");
	assert_eq!(v["messages"][1]["direction"], "incoming");
}

#[test]
fn wrong_schema_or_version_is_refused() {
	let s = populated();
	let json = s.export_json(ExportOptions::default());
	let bad = json.replace("\"version\": 1", "\"version\": 2");
	assert!(matches!(Export::from_json(&bad), Err(ExportError::Version)));
	let bad = json.replace(EXPORT_SCHEMA, "other.schema");
	assert!(matches!(Export::from_json(&bad), Err(ExportError::Version)));
	assert!(matches!(Export::from_json("{}"), Err(ExportError::Json(_))));
}

#[test]
fn export_never_contains_downloaded_file_bytes() {
	let s = populated();
	let json = s.export_json(ExportOptions { embed_images: true });
	// The offered content ("data") is only ever present as a hash.
	assert!(!json.contains("\"data\""));
}
