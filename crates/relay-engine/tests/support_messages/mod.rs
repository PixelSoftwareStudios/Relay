//! Whole engines in a connected call, each with a client holding the `messages`
//! capability. Needs `service_support`, `support_channels`, `support_daemon` and
//! `support_media` in the including test.
#![allow(dead_code)]

use std::time::Duration;

use relay_api::pb::{
	CallState, OperationState, SessionOpResult, SessionSummary, TranscriptEntry,
	transcript_entry::Body,
};
use relay_api::{RelayClient, entry_text};

use crate::service_support::*;
use crate::support_channels::*;

/// A client holding `messages` (and nothing else) on this side's daemon.
pub async fn messenger(side: &Side) -> RelayClient {
	let mut client = RelayClient::connect(side.daemon.endpoint())
		.await
		.expect("connect");
	client
		.register("messenger", "test", &["messages"])
		.await
		.expect("register");
	client
}

/// A paired call between two daemons plus a messaging client on each.
pub struct Chat {
	pub a: Side,
	pub b: Side,
	pub ma: RelayClient,
	pub mb: RelayClient,
	pub call: Vec<u8>,
	/// Session id on A / on B.
	pub sa: Vec<u8>,
	pub sb: Vec<u8>,
}

/// Rings and answers a call between two paired sides and returns its call id.
pub async fn connect(a: &mut Side, b: &mut Side, op: u8) -> Vec<u8> {
	let call = ring(a, b, op).await;
	answer(&mut b.cli, &call, op + 1).await;
	wait_state(&mut a.cli, &call, CallState::Connected).await;
	wait_state(&mut b.cli, &call, CallState::Connected).await;
	call
}

/// The only active session of a messaging client, once it exists.
pub async fn active_session(client: &mut RelayClient) -> SessionSummary {
	eventually!(
		client
			.list_sessions()
			.await
			.expect("list")
			.sessions
			.into_iter()
			.find(|s| s.state == relay_api::pb::SessionState::Active as i32)
	)
}

pub async fn chatting() -> Chat {
	chatting_tuned(|_| {}, |_| {}).await
}

pub async fn chatting_tuned(
	tune_a: impl FnOnce(&mut relay_engine::daemon::RunOptions),
	tune_b: impl FnOnce(&mut relay_engine::daemon::RunOptions),
) -> Chat {
	let mut a = crate::support_channels::side_tuned(440.0, None, true, tune_a).await;
	let mut b = crate::support_channels::side_tuned(1_700.0, None, true, tune_b).await;
	pair(&mut a, &mut b, 1).await;
	let call = connect(&mut a, &mut b, 10).await;
	let mut ma = messenger(&a).await;
	let mut mb = messenger(&b).await;
	let sa = active_session(&mut ma).await.session_id;
	let sb = active_session(&mut mb).await.session_id;
	Chat {
		a,
		b,
		ma,
		mb,
		call,
		sa,
		sb,
	}
}

pub fn op_code(result: &SessionOpResult) -> &str {
	result
		.operation
		.as_ref()
		.and_then(|o| o.problem.as_ref())
		.map_or("", |p| p.code.as_str())
}

pub fn assert_ok(result: &SessionOpResult) {
	let op = result.operation.as_ref().expect("operation");
	assert_eq!(op.state, OperationState::Applied as i32, "{op:?}");
}

pub fn assert_refused(result: &SessionOpResult, code: &str) {
	let op = result.operation.as_ref().expect("operation");
	assert_eq!(op.state, OperationState::Failed as i32, "{op:?}");
	assert_eq!(op_code(result), code);
}

/// Every entry of a session's transcript.
pub async fn transcript(client: &mut RelayClient, session: &[u8]) -> Vec<TranscriptEntry> {
	client
		.read_transcript(session, 0)
		.await
		.expect("transcript")
}

pub fn texts(entries: &[TranscriptEntry]) -> Vec<String> {
	entries
		.iter()
		.filter_map(|e| entry_text(e).map(str::to_owned))
		.collect()
}

pub fn file_of(entry: &TranscriptEntry) -> &relay_api::pb::FileOfferBody {
	match entry.body.as_ref() {
		Some(Body::File(file)) => file,
		_ => panic!("not a file offer"),
	}
}

/// Waits until `client`'s transcript of `session` satisfies `done`.
pub async fn wait_transcript(
	client: &mut RelayClient,
	session: &[u8],
	done: impl Fn(&[TranscriptEntry]) -> bool,
) -> Vec<TranscriptEntry> {
	for _ in 0..400 {
		let entries = transcript(client, session).await;
		if done(&entries) {
			return entries;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	panic!("transcript never reached the expected state");
}
