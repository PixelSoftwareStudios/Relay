#![cfg(unix)]
//! The daemon serves its endpoint while the system's secure storage is still being opened.
//! A fake identity store stands in for the keychain prompt: it blocks, or it refuses.

#[macro_use]
mod service_support;
mod support_daemon;
mod support_prompt;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use relay_api::pb::{StartupReason, StartupState};
use relay_api::{ApiError, RelayClient};
use relay_engine::daemon::{Booting, DaemonError, DaemonHandle};
use support_daemon::engine_options;
use support_prompt::{Handle, Mode, OpenOnDrop, Prompt};

async fn begin(dir: &std::path::Path, prompt: &Arc<Prompt>) -> (Booting, Arc<AtomicBool>) {
	let (mut options, _sim) = engine_options(dir, 440.0);
	options.identity_store = Some(Box::new(Handle(prompt.clone())));
	let built = Arc::new(AtomicBool::new(false));
	let spy = built.clone();
	let ports = relay_engine::service::fakes::FakePorts::default();
	options.adapters = Some(Box::new(move |_| {
		spy.store(true, Ordering::SeqCst);
		ports.ports()
	}));
	(DaemonHandle::begin(options).await.expect("begin"), built)
}

fn dir() -> tempfile::TempDir {
	tempfile::Builder::new()
		.prefix("rlyk")
		.tempdir_in("/tmp")
		.expect("dir")
}

#[tokio::test]
async fn getinfo_answers_starting_while_the_prompt_is_open_and_the_start_completes_after() {
	let dir = dir();
	let prompt = Prompt::new(Mode::Blocked);
	let _open = OpenOnDrop(prompt.clone());
	let (booting, built) = begin(dir.path(), &prompt).await;
	let endpoint = booting.endpoint().clone();
	let ready = tokio::spawn(booting.ready());

	// The endpoint answers although the identity is not open.
	let mut client = RelayClient::connect(&endpoint).await.expect("connect");
	let startup = client.startup();
	assert_eq!(startup.state(), StartupState::Starting);
	assert_eq!(startup.reason(), StartupReason::WaitingForKeychain);
	// No client can register, and every other call is refused as unavailable.
	let error = client.register("t", "0", &[]).await.expect_err("starting");
	assert!(matches!(error, ApiError::Starting(_)));
	let refused = client
		.raw()
		.list_channels(relay_api::pb::ListChannelsRequest::default())
		.await;
	// Without a registered client the guard refuses before the engine could answer.
	assert!(refused.is_err());
	// Nothing that touches the network has been built (no peers, discovery or advertising).
	tokio::time::sleep(Duration::from_millis(300)).await;
	assert!(
		!built.load(Ordering::SeqCst),
		"adapters built before the identity"
	);
	assert!(!ready.is_finished());

	prompt.set(Mode::Open);
	let daemon = ready.await.expect("join").expect("ready");
	assert!(built.load(Ordering::SeqCst));
	// The default `create` checks once more; the keychain store overrides it (cached, one read).
	assert!(prompt.loads.load(Ordering::SeqCst) <= 2);
	let mut client = RelayClient::connect(daemon.endpoint())
		.await
		.expect("connect");
	assert_eq!(client.startup().state(), StartupState::Ready);
	client.register("t", "0", &[]).await.expect("register");
	daemon.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn a_denied_keychain_keeps_the_daemon_up_as_failed_and_retry_recovers() {
	let dir = dir();
	let prompt = Prompt::new(Mode::Denied);
	let (booting, built) = begin(dir.path(), &prompt).await;
	let endpoint = booting.endpoint().clone();
	let ready = tokio::spawn(booting.ready());

	let mut client = RelayClient::connect(&endpoint).await.expect("connect");
	let failed = eventually!(
		Some(
			client
				.refresh_info()
				.await
				.expect("info")
				.startup
				.clone()
				.unwrap_or_default()
		)
		.filter(|s| s.state() == StartupState::Failed)
	);
	assert_eq!(failed.reason(), StartupReason::KeychainUnavailable);
	assert!(failed.detail.contains("user denied"), "{}", failed.detail);
	assert!(!built.load(Ordering::SeqCst));
	assert_eq!(
		prompt.saves.load(Ordering::SeqCst),
		0,
		"nothing was written"
	);
	assert!(!ready.is_finished(), "the daemon did not exit");

	// Retrying while it is still denied fails again, and stays up.
	client.retry_keychain().await.expect("retry");
	let again = eventually!(
		Some(
			client
				.refresh_info()
				.await
				.expect("info")
				.startup
				.clone()
				.unwrap_or_default()
		)
		.filter(|s| s.state() == StartupState::Failed)
	);
	assert_eq!(again.reason(), StartupReason::KeychainUnavailable);

	prompt.set(Mode::Open);
	client.retry_keychain().await.expect("retry");
	let daemon = ready.await.expect("join").expect("ready");
	assert_eq!(
		prompt.saves.load(Ordering::SeqCst),
		1,
		"a missing identity is created once"
	);
	daemon.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn an_unreadable_identity_is_reported_and_only_a_reset_replaces_it() {
	let dir = dir();
	let prompt = Prompt::new(Mode::Corrupt);
	let (booting, _built) = begin(dir.path(), &prompt).await;
	let endpoint = booting.endpoint().clone();
	let ready = tokio::spawn(booting.ready());
	let mut client = RelayClient::connect(&endpoint).await.expect("connect");
	let failed = eventually!(
		Some(
			client
				.refresh_info()
				.await
				.expect("info")
				.startup
				.clone()
				.unwrap_or_default()
		)
		.filter(|s| s.state() == StartupState::Failed)
	);
	assert_eq!(failed.reason(), StartupReason::IdentityUnreadable);
	assert_eq!(prompt.saves.load(Ordering::SeqCst), 0);

	prompt.set(Mode::Open);
	client.reset_identity().await.expect("reset");
	let daemon = ready.await.expect("join").expect("ready");
	daemon.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn start_returns_the_error_of_a_denied_store_and_frees_the_endpoint() {
	let dir = dir();
	let prompt = Prompt::new(Mode::Denied);
	let (mut options, _sim) = engine_options(dir.path(), 440.0);
	options.identity_store = Some(Box::new(Handle(prompt.clone())));
	let socket = dir.path().join("e.sock");
	let error = DaemonHandle::start(options).await.err().expect("error");
	assert!(matches!(error, DaemonError::Config(_)), "{error}");
	assert!(!socket.exists(), "the endpoint was removed");
}

#[tokio::test]
async fn the_file_store_is_used_only_when_the_user_chooses_it() {
	let dir = dir();
	let prompt = Prompt::new(Mode::Denied);
	let (mut options, _sim) = engine_options(dir.path(), 440.0);
	options.identity_store = Some(Box::new(Handle(prompt.clone())));
	let booting = DaemonHandle::begin(options).await.expect("begin");
	let endpoint = booting.endpoint().clone();
	let ready = tokio::spawn(booting.ready());
	let mut client = RelayClient::connect(&endpoint).await.expect("connect");
	eventually!(
		Some(
			client
				.refresh_info()
				.await
				.expect("info")
				.startup
				.clone()
				.unwrap_or_default()
		)
		.filter(|s| s.state() == StartupState::Failed)
	);
	// Still failed: nothing fell back by itself.
	tokio::time::sleep(Duration::from_millis(200)).await;
	assert!(!ready.is_finished());

	client.use_file_store().await.expect("file store");
	let daemon = ready.await.expect("join").expect("ready");
	assert_eq!(
		prompt.saves.load(Ordering::SeqCst),
		0,
		"the fake keychain was left alone"
	);
	daemon.shutdown().await.expect("shutdown");
}
