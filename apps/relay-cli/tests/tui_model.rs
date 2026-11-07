//! TUI state transitions and key mapping, without a terminal.

mod common;

use common::fixtures::{self, ALICE, CALL};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use relay_api::WatchItem;
use relay_api::pb::*;
use relay_cli::client::{CallAction, ConnectionState, PttAction, Update};
use relay_cli::hotkey::{HotkeyReport, HotkeyUpdate, TargetStatus};
use relay_cli::tui::keys::{TuiCommand, handle_key, on_tick};
use relay_cli::tui::{Model, Tab};
use relay_client::ptt::PttState;

fn key(c: char) -> KeyEvent {
	KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn model() -> Model {
	let mut m = Model::new();
	m.apply(Update::State(Box::new(WatchItem::Snapshot(
		fixtures::snapshot(),
	))));
	m
}

fn event(revision: u64, e: state_event::Event) -> Update {
	Update::State(Box::new(WatchItem::Event {
		revision,
		event: StateEvent { event: Some(e) },
	}))
}

#[test]
fn snapshot_then_events_update_the_cache() {
	let mut m = model();
	assert!(m.synced && m.snapshot.revision == 10);

	let mut bob = fixtures::peer(fixtures::BOB, "Bob", true, TrustState::Verified);
	bob.display_name = "Bobby".into();
	m.apply(event(11, state_event::Event::Peer(bob)));
	assert_eq!(m.snapshot.peers.len(), 2, "upsert, not append");
	assert_eq!(m.peer_name(&fixtures::BOB), "Bobby");

	let carol = fixtures::peer([0xcc; 32], "Carol", true, TrustState::Unknown);
	m.apply(event(12, state_event::Event::Peer(carol)));
	assert_eq!(m.snapshot.peers.len(), 3);

	let mut call = fixtures::call(CallState::Connected, TransmitMode::PushToTalk);
	call.transmit.as_mut().unwrap().effective_transmitting = true;
	m.apply(event(13, state_event::Event::Call(call)));
	assert_eq!(m.snapshot.calls.len(), 1);
	assert!(
		m.snapshot.calls[0]
			.transmit
			.as_ref()
			.unwrap()
			.effective_transmitting
	);

	m.apply(event(
		14,
		state_event::Event::Removed(Removed {
			id: ALICE.to_vec(),
			kind: "peer".into(),
		}),
	));
	assert_eq!(m.snapshot.peers.len(), 2);
	assert_eq!(m.snapshot.revision, 14);
}

#[test]
fn ended_calls_leave_the_live_list_and_selection_stays_in_range() {
	let mut m = model();
	m.tab = Tab::Calls;
	let mut ended = fixtures::call(CallState::Ended, TransmitMode::Muted);
	ended.revision = 20;
	m.apply(event(11, state_event::Event::Call(ended)));
	assert!(m.live_calls().is_empty());
	m.move_selection(5);
	assert_eq!(m.selection(Tab::Calls), 0);
}

#[test]
fn failed_operation_events_surface_in_the_status_line() {
	let mut m = model();
	let op = Operation {
		state: OperationState::Failed as i32,
		problem: Some(Problem {
			message: "no microphone".into(),
			..Problem::default()
		}),
		..Operation::default()
	};
	m.apply(event(11, state_event::Event::Operation(op)));
	assert_eq!(m.status, "failed: no microphone");
}

#[test]
fn connection_and_hotkey_status_are_tracked() {
	let mut m = model();
	assert!(
		!m.hotkey.is_active(),
		"nothing claimed before the backend reports"
	);
	let active = HotkeyReport::Active {
		backend: "global-hotkey".into(),
		binding: "Ctrl+Alt+Space".into(),
	};
	m.apply(Update::Hotkey(HotkeyUpdate::Report(active.clone())));
	assert_eq!(m.hotkey, active);
	m.apply(Update::Hotkey(HotkeyUpdate::Ptt(PttState::Transmitting)));
	m.apply(Update::Hotkey(HotkeyUpdate::Target(TargetStatus::NoCall)));
	assert_eq!(m.ptt, PttState::Transmitting);
	assert_eq!(m.ptt_target, Some(TargetStatus::NoCall));
	m.apply(Update::Connection(ConnectionState::Resyncing("gap".into())));
	assert_eq!(m.connection, ConnectionState::Resyncing("gap".into()));
}

#[test]
fn navigation_keys_switch_tabs_and_move_selection() {
	let mut m = model();
	assert_eq!(m.tab, Tab::Peers);
	handle_key(&mut m, key('3'));
	assert_eq!(m.tab, Tab::Devices);
	handle_key(&mut m, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
	assert_eq!(m.tab, Tab::Settings);
	handle_key(&mut m, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
	assert_eq!(m.tab, Tab::Messages);
	handle_key(&mut m, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
	assert_eq!(m.tab, Tab::Peers, "wraps around");
	handle_key(&mut m, key('j'));
	assert_eq!(m.selection(Tab::Peers), 1);
	handle_key(&mut m, key('j'));
	assert_eq!(m.selection(Tab::Peers), 1, "clamped at the end");
	assert_eq!(handle_key(&mut m, key('q')), None);
	assert!(m.quit);
}

#[test]
fn enter_on_a_peer_calls_it_and_on_a_device_selects_it() {
	let mut m = model();
	assert_eq!(
		handle_key(&mut m, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
		Some(TuiCommand::StartCall(ALICE.to_vec()))
	);
	m.tab = Tab::Devices;
	m.move_selection(1);
	let cmd = handle_key(&mut m, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
	assert_eq!(
		cmd,
		Some(TuiCommand::SelectDevice {
			direction: DeviceDirection::Input,
			device_id: "mic2".into()
		})
	);
}

#[test]
fn choosing_a_mode_is_the_consent_and_needs_no_question() {
	let mut m = model();
	// muted -> push-to-talk applies at once
	assert_eq!(
		handle_key(&mut m, key('t')),
		Some(TuiCommand::Route {
			call_id: CALL.to_vec(),
			mode: TransmitMode::PushToTalk,
		})
	);
	m.snapshot.calls[0].transmit.as_mut().unwrap().mode = TransmitMode::PushToTalk as i32;
	assert_eq!(
		handle_key(&mut m, key('t')),
		Some(TuiCommand::Route {
			call_id: CALL.to_vec(),
			mode: TransmitMode::Unmuted,
		})
	);
}

#[test]
fn closing_the_microphone_needs_no_prompt_and_drops_a_held_ptt() {
	let mut m = model();
	m.snapshot.calls[0].transmit.as_mut().unwrap().mode = TransmitMode::Unmuted as i32;
	m.ptt_latched = Some(CALL.to_vec());
	let cmd = handle_key(&mut m, key('t'));
	assert_eq!(
		cmd,
		Some(TuiCommand::Route {
			call_id: CALL.to_vec(),
			mode: TransmitMode::Muted,
		})
	);
	assert!(m.ptt_latched.is_none());
}

#[test]
fn ptt_latch_presses_renews_on_tick_and_releases() {
	let mut m = model();
	assert_eq!(
		handle_key(&mut m, key(' ')),
		None,
		"not in push-to-talk yet"
	);
	assert!(m.status.contains("push-to-talk"));
	assert!(on_tick(&m).is_none());

	m.snapshot.calls[0].transmit.as_mut().unwrap().mode = TransmitMode::PushToTalk as i32;
	let press = handle_key(&mut m, key(' ')).unwrap();
	let TuiCommand::Ptt(p) = press else {
		panic!("{press:?}")
	};
	assert_eq!((p.action, p.route_generation), (PttAction::Press, 7));
	let TuiCommand::Ptt(r) = on_tick(&m).unwrap() else {
		panic!()
	};
	assert_eq!(r.action, PttAction::Renew);
	let TuiCommand::Ptt(rel) = handle_key(&mut m, key(' ')).unwrap() else {
		panic!()
	};
	assert_eq!(rel.action, PttAction::Release);
	assert!(on_tick(&m).is_none());
}

#[test]
fn call_keys_act_on_the_highlighted_call() {
	let mut m = model();
	assert_eq!(
		handle_key(&mut m, key('e')),
		Some(TuiCommand::CallAction(CallAction::End, CALL.to_vec()))
	);
	assert_eq!(
		handle_key(&mut m, key('x')),
		Some(TuiCommand::CallAction(CallAction::Decline, CALL.to_vec()))
	);
	m.snapshot.calls.clear();
	assert_eq!(handle_key(&mut m, key('a')), None);
	assert_eq!(m.status, "select a call first");
}

#[test]
fn global_toggles_and_gain_steps() {
	let mut m = model();
	assert_eq!(
		handle_key(&mut m, key('m')),
		Some(TuiCommand::SetMuted(true))
	);
	assert_eq!(
		handle_key(&mut m, key('d')),
		Some(TuiCommand::SetDeafened(true))
	);
	m.tab = Tab::Settings;
	let up = handle_key(&mut m, key('+'));
	let Some(TuiCommand::MicGain(v)) = up else {
		panic!("{up:?}")
	};
	assert!((v - 1.1).abs() < 1e-6);
	m.move_selection(1);
	let down = handle_key(&mut m, key('-'));
	let Some(TuiCommand::PlaybackGain(v)) = down else {
		panic!("{down:?}")
	};
	assert!((v - 0.9).abs() < 1e-6);
}

#[test]
fn release_delay_row_steps_by_50_ms_within_0_to_2000() {
	let mut m = model();
	m.tab = Tab::Settings;
	for _ in 0..4 {
		m.move_selection(1);
	}
	let at = |m: &mut Model, ms: u32| {
		m.snapshot.settings.as_mut().unwrap().ptt_release_delay_ms = ms;
	};
	at(&mut m, 200);
	assert_eq!(
		handle_key(&mut m, key('+')),
		Some(TuiCommand::PttReleaseDelay(250))
	);
	assert_eq!(
		handle_key(&mut m, key('-')),
		Some(TuiCommand::PttReleaseDelay(150))
	);
	at(&mut m, 0);
	assert_eq!(
		handle_key(&mut m, key('-')),
		Some(TuiCommand::PttReleaseDelay(0))
	);
	at(&mut m, 2000);
	assert_eq!(
		handle_key(&mut m, key('+')),
		Some(TuiCommand::PttReleaseDelay(2000))
	);
}
