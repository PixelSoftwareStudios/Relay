//! A raw peer that a daemon trusts: it can say anything on the session lane.
//! Needs `service_support` and `support_channels` in the including test.
#![allow(dead_code)]

use std::time::{Duration, SystemTime};

use relay_engine::peers::{PeerOutput, SessionEvent};
use relay_net::identity::{Fingerprint, TrustRecord, TrustState};
use relay_net::proto::session_message::Body;
use relay_net::proto::{ChatAck, ChatMessage, SessionMessage, chat_message};

use crate::support_channels::Side;

pub use crate::support_daemon::e2e::peers_support::TestPeer;
use crate::support_daemon::e2e::peers_support::{fast_config, peer_with};

pub fn text(seq: u64, body: &str) -> SessionMessage {
	SessionMessage {
		body: Some(Body::Message(ChatMessage {
			seq,
			sent_at_unix_ms: 0,
			content: Some(chat_message::Content::Text(body.to_owned())),
		})),
	}
}

/// A raw peer that the daemon `target` trusts, linked to it.
pub async fn hostile(target: &mut Side) -> TestPeer {
	let identity = std::sync::Arc::new(relay_net::identity::Identity::generate().unwrap());
	let evil = peer_with(fast_config("evil"), identity).await;
	let target_fp = Fingerprint::from_bytes(*target.daemon.local_peer().as_bytes());
	evil.trust
		.lock()
		.put(TrustRecord {
			fingerprint: target_fp,
			state: TrustState::UnverifiedFirstUse,
			display_name: "target".into(),
			trusted_at: SystemTime::now(),
		})
		.unwrap();
	let target_id = relay_engine::peers::peer_id(&target_fp);
	evil.handle
		.add_address(target_id, target.daemon.listen_addr().unwrap())
		.unwrap();
	evil.handle.ensure_link(target_id).unwrap();
	crate::support_channels::pin_unknown(&mut target.cli, 200).await;
	let evil_id = evil.id().as_bytes().to_vec();
	eventually!(
		target
			.cli
			.snapshot()
			.await
			.unwrap()
			.peers
			.iter()
			.find(|p| p.peer_id == evil_id && p.reachable)
			.map(|_| ())
	);
	evil
}

/// The acks the daemon sent back, until `quiet` passes without one.
pub async fn acks(evil: &mut TestPeer, quiet: Duration) -> Vec<(u64, i32)> {
	let mut out = Vec::new();
	while let Ok(Some(item)) = tokio::time::timeout(quiet, evil.output.recv()).await {
		if let PeerOutput::Session(SessionEvent::Message { message, .. }) = item
			&& let Some(Body::Ack(ChatAck { seq, status })) = message.body
		{
			out.push((seq, status));
		}
	}
	out
}
