//! Screen snapshots on ratatui's `TestBackend`, plus a scripted drive loop.

mod common;

use common::FakeControl;
use common::fixtures::{self, CALL};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use relay_api::WatchItem;
use relay_api::pb::*;
use relay_cli::client::{ConnectionState, Update};
use relay_cli::tui::view::{draw, meter};
use relay_cli::tui::{Model, Tab, drive};
use tokio::sync::mpsc;

fn screen(model: &Model, width: u16, height: u16) -> String {
	let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
	terminal.draw(|f| draw(f, model)).unwrap();
	let buffer = terminal.backend().buffer().clone();
	buffer
		.content()
		.chunks(usize::from(width))
		.map(|row| {
			row.iter()
				.map(|c| c.symbol())
				.collect::<String>()
				.trim_end()
				.to_owned()
		})
		.collect::<Vec<_>>()
		.join("\n")
}

fn model() -> Model {
	let mut m = Model::new();
	m.apply(Update::State(Box::new(WatchItem::Snapshot(
		fixtures::snapshot(),
	))));
	m.apply(Update::Connection(ConnectionState::Live));
	m
}

#[test]
fn peers_screen_shows_status_header_and_peer_rows() {
	let text = screen(&model(), 90, 13);
	assert!(text.contains("relay live"), "{text}");
	assert!(text.contains("rev 10"));
	assert!(text.contains("hotkey: off (not started)"), "{text}");
	assert!(
		text.contains("Peers")
			&& text.contains("Calls")
			&& text.contains("Devices")
			&& text.contains("Settings")
	);
	assert!(
		text.contains("> * Alice"),
		"selected online peer row: {text}"
	);
	assert!(text.contains("verified") && text.contains("unverified"));
	assert!(text.contains("192.168.1.20:7000"));
}

#[test]
fn header_states_the_hotkey_truthfully_for_every_backend_outcome() {
	use relay_cli::hotkey::{HotkeyReport, HotkeyUpdate, TargetStatus};
	use relay_client::ptt::PttState;
	let mut m = model();
	let show = |m: &Model| screen(m, 120, 13);
	let set = |m: &mut Model, r| m.apply(Update::Hotkey(HotkeyUpdate::Report(r)));

	set(
		&mut m,
		HotkeyReport::NeedsPermission {
			backend: "global-hotkey".into(),
			binding: "Ctrl+Alt+Space".into(),
			permission: "Input Monitoring",
			hint: "grant Input Monitoring to the terminal app".into(),
		},
	);
	let text = show(&m);
	assert!(text.contains("hotkey: needs Input Monitoring"), "{text}");
	assert!(
		!text.contains("ptt:"),
		"no push-to-talk claims without a working backend"
	);

	set(
		&mut m,
		HotkeyReport::Unsupported {
			backend: "global-hotkey".into(),
			reason: "Wayland session".into(),
		},
	);
	assert!(show(&m).contains("hotkey: unsupported: Wayland session"));

	set(
		&mut m,
		HotkeyReport::Active {
			backend: "global-hotkey".into(),
			binding: "Ctrl+Alt+Space".into(),
		},
	);
	m.apply(Update::Hotkey(HotkeyUpdate::Target(TargetStatus::NoCall)));
	let text = show(&m);
	assert!(
		text.contains("hotkey: Ctrl+Alt+Space active via global-hotkey"),
		"{text}"
	);
	assert!(text.contains("ptt: idle  (no connected call)"), "{text}");
	m.apply(Update::Hotkey(HotkeyUpdate::Ptt(PttState::Transmitting)));
	assert!(show(&m).contains("ptt: transmitting"));
}

#[test]
fn header_flags_resyncing_and_offline_links() {
	let mut m = model();
	m.apply(Update::Connection(ConnectionState::Resyncing(
		"revision gap".into(),
	)));
	assert!(screen(&m, 100, 8).contains("resyncing (revision gap)"));
	m.apply(Update::Connection(ConnectionState::Lost(
		"engine closed the stream".into(),
	)));
	assert!(screen(&m, 100, 8).contains("offline (engine closed the stream)"));
}

#[test]
fn call_card_shows_route_effective_ptt_meter_and_speaking() {
	let mut m = model();
	m.tab = Tab::Calls;
	let mut call = fixtures::call(CallState::Connected, TransmitMode::PushToTalk);
	let t = call.transmit.as_mut().unwrap();
	t.consent_granted = true;
	t.effective_transmitting = true;
	m.apply(Update::State(Box::new(WatchItem::Event {
		revision: 11,
		event: StateEvent {
			event: Some(state_event::Event::Call(call)),
		},
	})));
	m.apply(Update::Telemetry(TelemetrySnapshot {
		microphone_dbfs: Some(-6.0),
		calls: vec![CallTelemetry {
			call_id: CALL.to_vec(),
			speaking: true,
			receive_dbfs: Some(-30.0),
			rtt_ms: Some(2.5),
			..CallTelemetry::default()
		}],
		..TelemetrySnapshot::default()
	}));
	let text = screen(&m, 90, 14);
	assert!(text.contains("Alice connected (speaking)"), "{text}");
	assert!(
		text.contains("mode: push-to-talk  TRANSMITTING"),
		"{text}"
	);
	assert!(text.contains("rtt 2.5ms"));
	assert!(
		text.contains("mic [##################--]"),
		"mic meter in header: {text}"
	);
	assert!(
		text.contains("in  [##########----------]"),
		"receive meter: {text}"
	);
}

#[test]
fn blocked_transmit_reason_and_closed_mic_are_visible() {
	let mut m = model();
	m.tab = Tab::Calls;
	m.snapshot.calls[0]
		.transmit
		.as_mut()
		.unwrap()
		.blocked_reason = "no input device".into();
	let text = screen(&m, 90, 14);
	assert!(text.contains("mic closed"), "{text}");
	assert!(text.contains("blocked: no input device"));
}

#[test]
fn devices_and_settings_screens_mark_the_selection() {
	let mut m = model();
	m.tab = Tab::Devices;
	let devices = screen(&m, 80, 14);
	assert!(
		devices.contains("* input   Built-in Mic (default)"),
		"{devices}"
	);
	assert!(devices.contains("  input   USB Mic"));
	assert!(devices.contains("* output  Speakers"));
	m.tab = Tab::Settings;
	let settings = screen(&m, 80, 14);
	assert!(settings.contains("Microphone gain") && settings.contains("x1.00"));
	assert!(settings.contains("Mute microphone   off"));
	assert!(settings.contains("PTT release delay 0 ms"), "{settings}");
}

#[test]
fn small_terminals_do_not_panic() {
	let mut m = model();
	for tab in Tab::ALL {
		m.tab = tab;
		let _ = screen(&m, 20, 5);
		let _ = screen(&m, 1, 1);
	}
}

#[test]
fn meter_maps_dbfs_to_bar_fill() {
	assert_eq!(meter::bar(None, 10), "----------");
	assert_eq!(meter::bar(Some(-60.0), 10), "----------");
	assert_eq!(meter::bar(Some(0.0), 10), "##########");
	assert_eq!(meter::bar(Some(-30.0), 10), "#####-----");
	assert_eq!(meter::bar(Some(f32::NAN), 4), "----");
	assert_eq!(
		meter::bar(Some(12.0), 4),
		"####",
		"clipping stays inside the bar"
	);
}

#[tokio::test]
async fn drive_applies_updates_executes_commands_and_quits() {
	let mut terminal = Terminal::new(TestBackend::new(90, 12)).unwrap();
	let mut client = FakeControl::new();
	let mut model = Model::new();
	let (utx, mut updates) = mpsc::channel(8);
	let (etx, mut events) = mpsc::channel(8);
	utx.send(Update::State(Box::new(WatchItem::Snapshot(
		fixtures::snapshot(),
	))))
	.await
	.unwrap();
	let key = |c| Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
	// Let the snapshot land first, then act and quit.
	let feeder = tokio::spawn(async move {
		tokio::time::sleep(std::time::Duration::from_millis(50)).await;
		etx.send(key('m')).await.unwrap();
		etx.send(Event::Key(KeyEvent::new(
			KeyCode::Enter,
			KeyModifiers::NONE,
		)))
		.await
		.unwrap();
		etx.send(key('q')).await.unwrap();
	});
	drive(
		&mut terminal,
		&mut client,
		&mut model,
		&mut updates,
		&mut events,
	)
	.await
	.unwrap();
	feeder.await.unwrap();
	assert!(model.quit && model.synced);
	assert_eq!(client.log.len(), 2, "{:?}", client.log);
	assert!(client.log[0].starts_with("global_controls"));
	assert!(client.log[1].starts_with("start_call"));
}
