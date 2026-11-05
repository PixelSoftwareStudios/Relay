#![cfg(unix)]
//! Passwords and invites exist in request bodies (and the invite answer) and
//! nowhere else: not in logs, not in events, snapshots or operation results.
//! This binary installs a global tracing subscriber that captures everything.

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_media;

use std::io::Write;
use std::sync::{Arc, Mutex};

use relay_api::pb::{Operation, Snapshot};
use service_support::*;
use support_channels::*;
use tracing_subscriber::fmt::MakeWriter;

const PASSWORD: &str = "zx9-marker-passphrase-ONE";
const WRONG: &str = "zx9-marker-passphrase-TWO";

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
	fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
		self.0.lock().unwrap().extend_from_slice(buf);
		Ok(buf.len())
	}
	fn flush(&mut self) -> std::io::Result<()> {
		Ok(())
	}
}

impl<'a> MakeWriter<'a> for Capture {
	type Writer = Capture;
	fn make_writer(&'a self) -> Capture {
		self.clone()
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn secrets_never_reach_logs_events_snapshots_or_operations() {
	let capture = Capture::default();
	tracing::subscriber::set_global_default(
		tracing_subscriber::fmt()
			.with_max_level(tracing::Level::TRACE)
			.with_ansi(false)
			.with_writer(capture.clone())
			.finish(),
	)
	.expect("one subscriber per binary");

	let mut a = side(440.0).await;
	let mut b = side(1_700.0).await;
	pair(&mut a, &mut b, 1).await;

	// Watch everything a client can see, from both engines.
	let (first_a, mut watch_a) = a.cli.snapshot_and_watch().await.expect("watch a");
	let (first_b, mut watch_b) = b.cli.snapshot_and_watch().await.expect("watch b");

	let mut seen: Vec<String> = vec![format!("{first_a:?}"), format!("{first_b:?}")];
	let mut operations: Vec<Operation> = Vec::new();

	operations.push(
		a.cli
			.create_channel("Secrets", policy(false, true, true), PASSWORD)
			.await
			.expect("create"),
	);
	let id = channel_of_name(&mut a.cli, "Secrets").await;
	let invite = a.cli.create_invite(&id, 0, 0).await.expect("invite");
	let token = invite.invite.clone();
	assert!(token.starts_with("RLYI1"));
	operations.extend(invite.operation.clone());
	operations.push(
		b.cli
			.join_channel(&id, WRONG, &token)
			.await
			.expect("wrong password"),
	);
	operations.push(
		b.cli
			.join_channel(&id, PASSWORD, &token)
			.await
			.expect("join"),
	);
	assert_applied(operations.last().unwrap());
	wait_shared(&mut a.cli, &id_of(&b), &id).await;
	operations.push(
		a.cli
			.create_invite(&id, 0, 0)
			.await
			.expect("second")
			.operation
			.unwrap(),
	);
	operations.push(b.cli.leave_channel(&id).await.expect("leave"));

	// Drain what the watches saw, then take final snapshots.
	for watch in [&mut watch_a, &mut watch_b] {
		while let Ok(Ok(Some(item))) =
			tokio::time::timeout(std::time::Duration::from_millis(300), watch.next()).await
		{
			seen.push(format!("{item:?}"));
		}
	}
	let snapshots: Vec<Snapshot> = vec![
		a.cli.snapshot().await.expect("snapshot"),
		b.cli.snapshot().await.expect("snapshot"),
	];
	seen.extend(snapshots.iter().map(|s| format!("{s:?}")));
	seen.extend(operations.iter().map(|o| format!("{o:?}")));
	for op in &operations {
		// Polling the stored outcome gives the same, secret-free answer.
		for cli in [&mut a.cli, &mut b.cli] {
			let request = relay_api::pb::GetOperationRequest {
				client: cli.context(),
				operation_id: op.operation_id.clone(),
			};
			if let Ok(polled) = cli.raw().get_operation(request).await {
				seen.push(format!("{:?}", polled.into_inner()));
			}
		}
	}

	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");

	let logs = String::from_utf8_lossy(&capture.0.lock().unwrap()).into_owned();
	assert!(
		logs.len() > 1_000,
		"the capture saw the engines: {} bytes",
		logs.len()
	);
	let secrets = [
		PASSWORD,
		WRONG,
		"marker-passphrase",
		token.as_str(),
		"RLYI1",
	];
	// The token body (base32 after the prefix) must not appear either.
	let body = &token[5..];
	for text in std::iter::once(&logs).chain(seen.iter()) {
		for secret in secrets.iter().copied().chain([body, &body[..16]]) {
			assert!(
				!text.contains(secret),
				"a secret leaked: {}",
				&secret[..secret.len().min(6)]
			);
		}
	}
}

async fn channel_of_name(cli: &mut relay_api::RelayClient, name: &str) -> Vec<u8> {
	cli.list_channels()
		.await
		.expect("list")
		.into_iter()
		.find(|c| c.name == name)
		.expect("channel")
		.channel_id
}
