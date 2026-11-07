#![cfg(unix)]
//! Presence and ownership on real engines: a daemon nobody watches stays off
//! the LAN and declines calls, a call belongs to the client that controls it,
//! and an unused daemon stops by itself.

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_media;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use relay_api::pb::{CallState, StartCallRequest, start_call_request::Target};
use relay_api::{Endpoint, RelayClient, WatchOptions};
use relay_engine::daemon::RunOptions;
use relay_engine::peers::{AdvertSink, DiscoverySetup, SinkHandle};
use relay_net::discovery::Advertisement;
use service_support::*;
use support_channels::{Side, answer, pair, ring, side_tuned};
use support_daemon::engine_options;

const TONE_A: f32 = 440.0;
const TONE_B: f32 = 1_700.0;
const GRACE: Duration = Duration::from_millis(800);

/// Stands in for mDNS and remembers whether the device is visible.
#[derive(Default)]
struct Recorder {
	current: Mutex<Option<(String, bool)>>,
}

impl Recorder {
	fn visible(&self) -> bool {
		self.current.lock().unwrap().is_some()
	}

	/// Whether what goes out is the hidden (pseudonymous) kind.
	fn hidden(&self) -> bool {
		self.current
			.lock()
			.unwrap()
			.as_ref()
			.is_some_and(|(_, h)| *h)
	}
}

impl AdvertSink for Recorder {
	fn publish(&self, advert: Option<&Advertisement>) {
		*self.current.lock().unwrap() = advert.map(|a| {
			(
				a.display_name.clone(),
				a.mode == relay_net::discovery::AdvertMode::Hidden,
			)
		});
	}
}

fn tuned(recorder: Option<&Arc<Recorder>>) -> impl FnOnce(&mut RunOptions) + use<> {
	let sink = recorder.map(|r| SinkHandle(Arc::clone(r) as Arc<dyn AdvertSink>));
	move |options: &mut RunOptions| {
		options.limits.owner_grace = GRACE;
		options.presence_debounce = Duration::from_millis(300);
		if let Some(sink) = sink {
			options.network.discovery = Some(DiscoverySetup {
				advertise: true,
				sink: Some(sink),
				..DiscoverySetup::default()
			});
		}
	}
}

async fn state_of(cli: &mut RelayClient, call: &[u8]) -> Option<i32> {
	call_in(cli, call).await.map(|c| c.state)
}

async fn connect(a: &mut Side, b: &mut Side) -> Vec<u8> {
	pair(a, b, 1).await;
	let call = ring(a, b, 10).await;
	answer(&mut b.cli, &call, 11).await;
	wait_state(&mut a.cli, &call, CallState::Connected).await;
	wait_state(&mut b.cli, &call, CallState::Connected).await;
	call
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unwatched_engine_is_invisible_and_declines_calls_until_a_client_attaches() {
	let seen = Arc::new(Recorder::default());
	let mut a = side_tuned(TONE_A, None, true, tuned(None)).await;
	let mut b = side_tuned(TONE_B, None, false, tuned(Some(&seen))).await;

	// Nobody is watching B: it advertises nothing, now or after a while.
	tokio::time::sleep(Duration::from_millis(400)).await;
	assert!(!seen.visible());

	// Pairing is possible (a link and a trust decision are not user-visible)...
	pair(&mut a, &mut b, 1).await;
	// ...but a call is declined as busy: it never rings and nothing opens.
	let refused = ring(&mut a, &b, 10).await;
	wait_state(&mut a.cli, &refused, CallState::Declined).await;
	assert!(b.cli.snapshot().await.expect("snapshot").calls.is_empty());
	assert!(!seen.visible(), "a one-shot command does not attach");

	// A watch attaches: B shows up and rings.
	b.attach().await;
	eventually!(seen.visible().then_some(()));
	let ringing = ring(&mut a, &b, 12).await;
	wait_state(&mut b.cli, &ringing, CallState::IncomingRinging).await;

	// The last client leaving withdraws the advertisement after the debounce
	// and declines the call nobody answered.
	b.detach();
	assert!(
		seen.visible(),
		"the debounce keeps a reconnect from flickering"
	);
	eventually!((!seen.visible()).then_some(()));
	eventually!(
		(state_of(&mut a.cli, &ringing).await == Some(CallState::Declined as i32)).then_some(())
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_call_ends_with_its_owner_and_survives_a_quick_reconnect() {
	let mut a = side_tuned(TONE_A, None, true, tuned(None)).await;
	let mut b = side_tuned(TONE_B, None, true, tuned(None)).await;
	let call = connect(&mut a, &mut b).await;

	// B's window closes and comes back inside the grace: nothing happens.
	b.detach();
	tokio::time::sleep(GRACE / 3).await;
	b.attach().await;
	tokio::time::sleep(GRACE * 2).await;
	assert_eq!(
		state_of(&mut a.cli, &call).await,
		Some(CallState::Connected as i32)
	);

	// Closing for good hangs up: the peer sees the end and B's call is over.
	b.detach();
	wait_state(&mut a.cli, &call, CallState::Ended).await;
	eventually!(
		matches!(
			state_of(&mut b.cli, &call).await,
			Some(s) if s == CallState::Ended as i32 || s == CallState::Ending as i32
		)
		.then_some(())
	);
}

#[tokio::test]
async fn closing_one_client_ends_only_its_own_calls() {
	let rig = Rig::with(|o| o.limits.owner_grace = Duration::from_millis(400)).await;
	let (mut x, mut y) = (rig.full_client("x").await, rig.full_client("y").await);
	let mut x_watch = Some(x.watch(WatchOptions::default()).await.expect("watch"));
	let _y_watch = y.watch(WatchOptions::default()).await.expect("watch");
	let first = start_call(&mut x, 1).await.call_id;
	let second = {
		let request = StartCallRequest {
			context: Some(ctx(&y, 1)),
			target: Some(Target::PeerId(vec![8; 32])),
			allow_parallel_same_peer: false,
		};
		y.raw()
			.start_call(request)
			.await
			.expect("call")
			.into_inner()
			.call_id
	};

	// An observer with no calls coming and going changes nothing.
	let mut observer = rig.client("observer", &["observe"]).await;
	drop(
		observer
			.watch(WatchOptions::default())
			.await
			.expect("watch"),
	);
	tokio::time::sleep(Duration::from_millis(700)).await;
	assert!(
		call_in(&mut y, &first)
			.await
			.is_some_and(|c| c.state != CallState::Ended as i32)
	);

	x_watch.take();
	eventually!((state_of(&mut y, &first).await == Some(CallState::Ended as i32)).then_some(()));
	let kept = state_of(&mut y, &second).await.expect("Y's call");
	assert_ne!(kept, CallState::Ended as i32);
	assert_ne!(kept, CallState::Failed as i32);
}

fn idle_options(home: &std::path::Path) -> RunOptions {
	let (mut options, sim) = engine_options(home, 440.0);
	std::mem::forget(sim);
	options.idle_exit = Some(Duration::from_millis(500));
	options.presence_debounce = Duration::from_millis(100);
	options
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_engine_nobody_attaches_to_stops_by_itself() {
	let home = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.expect("home");
	let socket = home.path().join("e.sock");
	let host = tokio::spawn(relay_engine::daemon::run(idle_options(home.path())));
	let finished = tokio::time::timeout(Duration::from_secs(10), host)
		.await
		.expect("it stops on its own")
		.expect("task");
	finished.expect("clean stop");
	assert!(!socket.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_attached_client_keeps_the_engine_alive_until_it_leaves() {
	let home = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.expect("home");
	let socket = home.path().join("e.sock");
	let mut host = tokio::spawn(relay_engine::daemon::run(idle_options(home.path())));
	eventually!(socket.exists().then_some(()));
	wait_ready(&Endpoint::Unix(socket.clone())).await;
	let mut cli = RelayClient::connect(&Endpoint::Unix(socket.clone()))
		.await
		.expect("connect");
	cli.register("desktop", "test", &["observe"])
		.await
		.expect("register");
	let watch = cli.watch(WatchOptions::default()).await.expect("watch");

	let early = tokio::time::timeout(Duration::from_millis(1_500), &mut host).await;
	assert!(early.is_err(), "still running while attached");
	drop(watch);
	let finished = tokio::time::timeout(Duration::from_secs(10), host)
		.await
		.expect("stops once nobody is attached")
		.expect("task");
	finished.expect("clean stop");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_background_on_an_unwatched_engine_advertises_and_never_idle_exits() {
	let seen = Arc::new(Recorder::default());
	let home = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.expect("home");
	let socket = home.path().join("e.sock");
	let (mut options, sim) = engine_options(home.path(), 440.0);
	std::mem::forget(sim);
	support_daemon::set_background(&options.paths, true);
	tuned(Some(&seen))(&mut options);
	options.idle_exit = Some(Duration::from_millis(500));
	let mut host = tokio::spawn(relay_engine::daemon::run(options));
	eventually!(socket.exists().then_some(()));
	wait_ready(&Endpoint::Unix(socket.clone())).await;

	// No client ever attaches: it is visible and outlives the idle-exit time.
	eventually!(seen.visible().then_some(()));
	let early = tokio::time::timeout(Duration::from_millis(1_500), &mut host).await;
	assert!(early.is_err(), "background availability keeps it running");
	assert!(seen.visible());

	// Switching the setting off withdraws the advertisement, then the idle exit applies.
	let mut cli = RelayClient::connect(&Endpoint::Unix(socket))
		.await
		.expect("connect");
	cli.register("status", "test", &[]).await.expect("register");
	cli.set_settings([("background.available", "false")])
		.await
		.expect("settings");
	eventually!((!seen.visible()).then_some(()));
	let finished = tokio::time::timeout(Duration::from_secs(10), host)
		.await
		.expect("stops once background is off")
		.expect("task");
	finished.expect("clean stop");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hidden_mode_applies_to_an_unwatched_background_engine() {
	let seen = Arc::new(Recorder::default());
	let home = tempfile::Builder::new()
		.prefix("rly")
		.tempdir_in("/tmp")
		.expect("home");
	let socket = home.path().join("e.sock");
	let (mut options, sim) = engine_options(home.path(), 440.0);
	std::mem::forget(sim);
	support_daemon::set_background(&options.paths, true);
	{
		let storage = relay_config::Storage::open(&options.paths).expect("storage");
		relay_config::prefs::PreferenceStore::open(&storage.database)
			.expect("prefs")
			.set("privacy.advert_mode", "hidden")
			.expect("set");
	}
	tuned(Some(&seen))(&mut options);
	let host = tokio::spawn(relay_engine::daemon::run(options));
	eventually!(socket.exists().then_some(()));
	wait_ready(&Endpoint::Unix(socket)).await;
	eventually!(seen.visible().then_some(()));
	assert!(seen.hidden(), "the unwatched engine keeps the hidden kind");
	host.abort();
}
