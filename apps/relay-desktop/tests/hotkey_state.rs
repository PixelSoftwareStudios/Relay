//! What Settings is told when the global push-to-talk key cannot be
//! registered: plain words, and the on-screen button stays offered.

use std::sync::Arc;

use relay_desktop::hotkey::{HotkeyHost, HotkeyShared};
use relay_desktop::ptt::PttControl;
use relay_platform::hotkey::{BackendStatus, PermissionKind};
use relay_platform::testing::FakeBackend;
use relay_ui::hotkey_settings::HotkeyState;
use relay_ui::hub::Hub;

fn host(status: BackendStatus) -> (HotkeyHost, Arc<HotkeyShared>) {
	let control = Arc::new(PttControl::new(
		Arc::new(Hub::default()),
		tokio::runtime::Handle::current(),
		Box::new(|| Err("no engine".to_owned())),
	));
	let shared = Arc::new(HotkeyShared::default());
	let (backend, _keyboard) = FakeBackend::new(status);
	let host = HotkeyHost::new(
		Box::new(backend),
		shared.clone(),
		control,
		tokio::runtime::Handle::current(),
	);
	(host, shared)
}

fn failed(shared: &HotkeyShared) -> String {
	match shared.state() {
		HotkeyState::Failed(reason) => reason,
		other => panic!("{other:?}"),
	}
}

#[tokio::test]
async fn nothing_is_registered_before_the_engine_has_been_asked() {
	let (mut host, shared) = host(BackendStatus::Available);
	host.sync();
	assert_eq!(shared.state(), HotkeyState::Off);
	shared.set_accelerator("Ctrl+Alt+Space");
	host.sync();
	assert_eq!(
		shared.state(),
		HotkeyState::Registered("Ctrl+Alt+Space".into())
	);
}

#[tokio::test]
async fn a_session_without_global_shortcuts_says_so() {
	let (mut host, shared) = host(BackendStatus::Unsupported("Wayland".into()));
	shared.set_accelerator("Ctrl+Alt+Space");
	host.sync();
	assert!(
		failed(&shared).contains("global shortcuts"),
		"{:?}",
		shared.state()
	);
}

#[tokio::test]
async fn a_missing_permission_is_not_called_unsupported() {
	let (mut host, shared) = host(BackendStatus::NeedsPermission(
		PermissionKind::InputMonitoring,
	));
	shared.set_accelerator("Ctrl+Alt+Space");
	host.sync();
	assert!(failed(&shared).contains("allowed"), "{:?}", shared.state());
}

#[tokio::test]
async fn a_refused_key_is_not_retried_every_frame_but_a_new_key_is() {
	let (mut host, shared) = host(BackendStatus::Unsupported("x".into()));
	shared.set_accelerator("Ctrl+Alt+Space");
	host.sync();
	let first = shared.state();
	host.sync();
	assert_eq!(shared.state(), first);
	shared.set_accelerator("");
	host.sync();
	assert_eq!(shared.state(), HotkeyState::Off);
}
