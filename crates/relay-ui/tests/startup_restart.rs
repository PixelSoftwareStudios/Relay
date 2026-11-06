//! The desktop's launch-time "replace an idle service" never touches a service that is still
//! opening its secure storage (the real engine, held at a fake keychain prompt).
#![cfg(unix)]

#[path = "../../relay-engine/tests/support_prompt/mod.rs"]
mod support_prompt;

use relay_api::Endpoint;
use relay_config::Paths;
use relay_engine::daemon::{DaemonHandle, NetworkOptions, RunOptions, Transport};
use relay_engine::service::fakes::FakePorts;
use relay_ui::actions::Shared;
use relay_ui::autostart::AutoStart;
use relay_ui::daemon_control::{LaunchRefresh, restart_idle_at_launch};
use relay_ui::hub::Hub;
use support_prompt::{Handle, Mode, OpenOnDrop, Prompt};

#[tokio::test]
async fn a_service_waiting_for_the_keychain_is_left_alone_at_launch() {
	let dir = tempfile::Builder::new()
		.prefix("rlyr")
		.tempdir_in("/tmp")
		.expect("dir");
	let prompt = Prompt::new(Mode::Blocked);
	let _open = OpenOnDrop(prompt.clone());
	let mut options = RunOptions::new(Paths::under(dir.path().join("cfg")));
	options.transport = Transport::Unix(Some(dir.path().join("e.sock")));
	options.identity_store = Some(Box::new(Handle(prompt.clone())));
	options.network = NetworkOptions::loopback();
	let ports = FakePorts::default();
	options.adapters = Some(Box::new(move |_| ports.ports()));
	let booting = DaemonHandle::begin(options).await.expect("begin");
	let endpoint = Endpoint::Unix(dir.path().join("e.sock"));
	let ready = tokio::spawn(booting.ready());

	let (hub, shared, autostart) = (Hub::default(), Shared::default(), AutoStart::new(false));
	for _ in 0..2 {
		let result = restart_idle_at_launch(&endpoint, &autostart, &hub, &shared).await;
		assert_eq!(result, LaunchRefresh::KeptStarting);
	}
	// Still the same service, still waiting: nothing was stopped or started.
	assert!(relay_client::daemon::reachable(&endpoint).await);
	assert!(!ready.is_finished());
	assert!(!autostart.launch_restarted());

	prompt.set(Mode::Open);
	let daemon = ready.await.expect("join").expect("ready");
	daemon.shutdown().await.expect("shutdown");
}
