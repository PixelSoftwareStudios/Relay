//! Hotkey startup, honest status and PTT supervision with fake keys and engine.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;
use common::FakeControl;
use common::engine::EngineHarness;
use common::fixtures;
use relay_api::pb::{CallState, InputAction, InputControlMessage};
use relay_cli::cli::{Cli, Command, HotkeyCommand, exit};
use relay_cli::hotkey::{
	HotkeyReport, HotkeyUpdate, TargetStatus, configured_binding, ptt_label, start_with, supervise,
	validate_accelerator,
};
use relay_client::ptt::PttState;
use relay_client::wire::connect_registered;
use relay_config::{Paths, Storage};
use relay_platform::hotkey::{BackendStatus, HotkeyEvent, PermissionKind};
use relay_platform::testing::{FakeBackend, FakeKeyboard};

#[test]
fn an_available_backend_reports_active_and_delivers_edges() {
	let (mut backend, keyboard) = FakeBackend::new(BackendStatus::Available);
	let feed = start_with(&mut backend, "Ctrl+Alt+Space");
	assert_eq!(
		feed.report,
		HotkeyReport::Active {
			backend: "fake".into(),
			binding: "Ctrl+Alt+Space".into()
		}
	);
	assert!(feed.report.is_active());
	let mut keys = feed.stream.expect("stream when active");
	assert!(keyboard.send(HotkeyEvent::Pressed));
	assert_eq!(keys.try_recv().unwrap(), HotkeyEvent::Pressed);
}

#[test]
fn missing_permission_is_reported_with_what_to_do_and_no_stream() {
	let (mut backend, _) = FakeBackend::new(BackendStatus::NeedsPermission(
		PermissionKind::InputMonitoring,
	));
	let feed = start_with(&mut backend, "Ctrl+Alt+Space");
	assert!(feed.stream.is_none());
	let HotkeyReport::NeedsPermission {
		permission, hint, ..
	} = &feed.report
	else {
		panic!("{:?}", feed.report);
	};
	assert_eq!(*permission, "Input Monitoring");
	assert!(hint.contains("terminal app"), "{hint}");
	assert!(!feed.report.is_active());
}

#[test]
fn unsupported_sessions_and_disabled_bindings_never_claim_to_work() {
	let (mut backend, _) = FakeBackend::new(BackendStatus::Unsupported("Wayland session".into()));
	let feed = start_with(&mut backend, "Ctrl+Alt+Space");
	assert!(
		matches!(&feed.report, HotkeyReport::Unsupported { reason, .. } if reason.contains("Wayland"))
	);
	assert!(feed.stream.is_none());

	let (mut backend, _) = FakeBackend::new(BackendStatus::Available);
	let off = start_with(&mut backend, "");
	assert!(matches!(off.report, HotkeyReport::Disabled(_)));
	assert!(off.stream.is_none());
}

#[test]
fn reports_serialize_with_a_state_tag_for_scripts() {
	let json = serde_json::to_value(HotkeyReport::Unsupported {
		backend: "global-hotkey".into(),
		reason: "no X11 display".into(),
	})
	.unwrap();
	assert_eq!(json["state"], "unsupported");
	assert_eq!(json["reason"], "no X11 display");
}

#[test]
fn accelerators_are_checked_with_the_platform_parser() {
	for good in ["Ctrl+Alt+Space", "Ctrl+Shift+F9", ""] {
		assert!(validate_accelerator(good).is_ok(), "{good}");
	}
	for bad in ["Ctrl+", "Ctrl+Alt+Banana", "+++"] {
		assert!(validate_accelerator(bad).is_err(), "{bad}");
	}
	assert!(validate_accelerator(&configured_binding(Some("Ctrl+Shift+F8"))).is_ok());
	assert_eq!(configured_binding(Some("Ctrl+Shift+F8")), "Ctrl+Shift+F8");
	assert!(validate_accelerator(&relay_config::prefs::Hotkeys::default().push_to_talk).is_ok());
}

#[test]
fn hotkey_subcommands_parse_and_only_the_bare_form_needs_the_main_thread() {
	let run = Cli::try_parse_from(["relay", "hotkey", "--binding", "Ctrl+F9"]).unwrap();
	assert!(run.command.uses_hotkeys());
	assert_eq!(run.command.binding_override(), Some("Ctrl+F9"));
	for args in [&["hotkey", "status"][..], &["hotkey", "set", "Ctrl+F9"]] {
		let cli =
			Cli::try_parse_from(std::iter::once("relay").chain(args.iter().copied())).unwrap();
		assert!(!cli.command.uses_hotkeys());
	}
	let tui = Cli::try_parse_from(["relay", "tui"]).unwrap();
	assert!(tui.command.uses_hotkeys());
	assert!(
		!Cli::try_parse_from(["relay", "tui", "--no-hotkey"])
			.unwrap()
			.command
			.uses_hotkeys()
	);
	assert!(Cli::try_parse_from(["relay", "tui", "--no-hotkey", "--binding", "F9"]).is_err());
	let Command::Hotkey(args) = Cli::try_parse_from(["relay", "hotkey", "set", "F9"])
		.unwrap()
		.command
	else {
		panic!()
	};
	assert!(matches!(args.command, Some(HotkeyCommand::Set { .. })));
}

#[test]
fn set_saves_a_valid_key_and_rejects_bad_or_locked() {
	let dir = tempfile::tempdir().unwrap();
	let paths = Paths::under(dir.path());
	let mut buffer = Vec::new();
	let mut out = relay_cli::commands::Out::new(false, &mut buffer);
	let err = relay_cli::commands::hotkey_set(&paths, "Ctrl+Alt+Banana", &mut out).unwrap_err();
	assert_eq!(err.exit_code(), exit::USAGE);
	relay_cli::commands::hotkey_set(&paths, "Ctrl+Shift+F9", &mut out).unwrap();
	let saved = relay_config::prefs::PreferenceStore::read_only(&paths)
		.unwrap()
		.unwrap();
	assert_eq!(saved.hotkeys.push_to_talk, "Ctrl+Shift+F9");

	let _holder = Storage::open(&paths).unwrap();
	let err = relay_cli::commands::hotkey_set(&paths, "Ctrl+Shift+F8", &mut out).unwrap_err();
	assert!(err.to_string().contains("holds the settings lock"), "{err}");
}

async fn until(what: &str, mut ready: impl FnMut() -> bool) {
	for _ in 0..300 {
		if ready() {
			return;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	panic!("timed out waiting for {what}");
}

struct Run {
	updates: Arc<Mutex<Vec<HotkeyUpdate>>>,
	engine: EngineHarness,
	/// Keeps the controlling client registered.
	_client: relay_api::RelayClient,
	keyboard: FakeKeyboard,
	stop: tokio::sync::oneshot::Sender<()>,
	task: tokio::task::JoinHandle<(FakeControl, Result<(), relay_cli::cli::CliError>)>,
}

impl Run {
	fn inputs(&self) -> Vec<InputControlMessage> {
		self.engine.engine.inputs.lock().unwrap().clone()
	}

	fn actions(&self) -> Vec<InputAction> {
		self.inputs()
			.iter()
			.map(|m| InputAction::try_from(m.action).unwrap())
			.collect()
	}
}

async fn start(control: FakeControl) -> Run {
	let (mut backend, keyboard) = FakeBackend::new(BackendStatus::Available);
	let keys = start_with(&mut backend, "Ctrl+Alt+Space").stream.unwrap();
	let engine = EngineHarness::start().await;
	let client = connect_registered(&engine.endpoint(), "test", "0", &["observe"])
		.await
		.unwrap();
	let client_id = client.context().unwrap().client_id;
	let endpoint = engine.endpoint();
	let updates: Arc<Mutex<Vec<HotkeyUpdate>>> = Arc::default();
	let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
	let notified = Arc::clone(&updates);
	let task = tokio::spawn(async move {
		let mut control = control;
		let result = supervise(
			&mut control,
			endpoint,
			client_id,
			keys,
			move |u| notified.lock().unwrap().push(u),
			async {
				let _ = stopped.await;
			},
		)
		.await;
		(control, result)
	});
	Run {
		updates,
		engine,
		_client: client,
		keyboard,
		stop,
		task,
	}
}

#[tokio::test]
async fn pressing_the_key_holds_a_lease_on_the_connected_call_and_release_ends_it() {
	let run = start(FakeControl::new()).await;
	until("the call to be bound", || {
		run.updates
			.lock()
			.unwrap()
			.iter()
			.any(|u| matches!(u, HotkeyUpdate::Target(TargetStatus::Call(_))))
	})
	.await;
	// The target reaches the driver on the same turn; give it a moment.
	tokio::time::sleep(Duration::from_millis(50)).await;
	run.keyboard.send(HotkeyEvent::Pressed);
	until("press", || run.actions().contains(&InputAction::Press)).await;
	until("transmitting", || {
		run.updates
			.lock()
			.unwrap()
			.iter()
			.any(|u| matches!(u, HotkeyUpdate::Ptt(PttState::Transmitting)))
	})
	.await;
	run.keyboard.send(HotkeyEvent::Released);
	until("release", || run.actions().contains(&InputAction::Release)).await;

	let press = &run.inputs()[0];
	assert_eq!(press.call_id, fixtures::CALL);
	assert_eq!(
		press.controller_generation, 5,
		"the lease generation the engine granted"
	);
	assert_eq!(
		press.transmit_route_generation, 7,
		"the call's route generation"
	);
	let _ = run.stop.send(());
	let (control, result) = run.task.await.unwrap();
	result.unwrap();
	assert!(control.log.iter().any(|l| l.starts_with("lease ")));
}

#[tokio::test]
async fn without_a_connected_call_the_key_is_blocked_not_sent() {
	let mut control = FakeControl::new();
	control.snapshot.calls[0].state = CallState::OutgoingRinging as i32;
	let run = start(control).await;
	until("no-call status", || {
		run.updates
			.lock()
			.unwrap()
			.iter()
			.any(|u| matches!(u, HotkeyUpdate::Target(TargetStatus::NoCall)))
	})
	.await;
	tokio::time::sleep(Duration::from_millis(50)).await;
	run.keyboard.send(HotkeyEvent::Pressed);
	until("blocked", || {
		run.updates
			.lock()
			.unwrap()
			.iter()
			.any(|u| matches!(u, HotkeyUpdate::Ptt(PttState::NoTarget)))
	})
	.await;
	assert!(
		run.inputs().is_empty(),
		"nothing reaches the engine without a target"
	);
	let _ = run.stop.send(());
	run.task.await.unwrap().1.unwrap();
}

#[tokio::test]
async fn two_connected_calls_are_ambiguous_and_never_guessed() {
	let mut control = FakeControl::new();
	let mut second = control.snapshot.calls[0].clone();
	second.call_id = vec![0xd2; 16];
	control.snapshot.calls.push(second);
	let run = start(control).await;
	until("ambiguous status", || {
		run.updates
			.lock()
			.unwrap()
			.iter()
			.any(|u| matches!(u, HotkeyUpdate::Target(TargetStatus::Ambiguous)))
	})
	.await;
	let _ = run.stop.send(());
	run.task.await.unwrap().1.unwrap();
}

#[test]
fn labels_are_plain_words() {
	assert_eq!(ptt_label(&PttState::Transmitting), "transmitting");
	assert!(ptt_label(&PttState::Gated).contains("not transmitting"));
	assert!(ptt_label(&PttState::Rejected { code: "X".into() }).contains("X"));
}

#[tokio::test]
async fn with_push_to_talk_off_the_key_has_no_target_and_sends_nothing() {
	let mut control = FakeControl::new();
	control.snapshot.settings.as_mut().unwrap().ptt_enabled = false;
	let run = start(control).await;
	until("the off status", || {
		run.updates
			.lock()
			.unwrap()
			.iter()
			.any(|u| matches!(u, HotkeyUpdate::Target(TargetStatus::PttOff)))
	})
	.await;
	tokio::time::sleep(Duration::from_millis(50)).await;
	run.keyboard.send(HotkeyEvent::Pressed);
	tokio::time::sleep(Duration::from_millis(100)).await;
	assert!(
		run.inputs().is_empty(),
		"nothing reaches the engine while push-to-talk is off"
	);
	let _ = run.stop.send(());
	run.task.await.unwrap().1.unwrap();
}
