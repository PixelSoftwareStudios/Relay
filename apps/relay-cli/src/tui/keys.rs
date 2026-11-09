//! Key handling: pure mapping from key events to model changes and commands.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use relay_api::pb::{DeviceDirection, TransmitMode};

use super::messages::{Handled, MsgCommand};
use super::model::{Model, Tab};
use crate::client::{CallAction, PttAction, PttRequest};

/// Gain change per keypress.
pub const GAIN_STEP: f32 = 0.1;
/// Highest gain reachable from the UI.
pub const GAIN_MAX: f32 = 4.0;
/// One `+`/`-` press on the release delay row.
pub const RELEASE_DELAY_STEP_MS: i64 = 50;
/// Longest release delay the engine accepts.
pub const RELEASE_DELAY_MAX_MS: u32 = 2000;
/// Something the app must ask the engine to do.
#[derive(Debug, Clone, PartialEq)]
pub enum TuiCommand {
	StartCall(Vec<u8>),
	CallAction(CallAction, Vec<u8>),
	Route {
		call_id: Vec<u8>,
		mode: TransmitMode,
	},
	SetMuted(bool),
	SetDeafened(bool),
	MicGain(f32),
	PlaybackGain(f32),
	PttReleaseDelay(u32),
	SelectDevice {
		direction: DeviceDirection,
		device_id: String,
	},
	Ptt(PttRequest),
	/// The Messages tab asks the daemon (or the clipboard) for something.
	Messages(MsgCommand),
}

/// Handle one key; returns the engine command it asks for, if any.
pub fn handle_key(model: &mut Model, key: KeyEvent) -> Option<TuiCommand> {
	if key.kind == KeyEventKind::Release {
		return None;
	}
	if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
		model.quit = true;
		return None;
	}
	if model.tab == Tab::Messages {
		match super::messages::keys::handle(&mut model.messages, key) {
			Handled::Pass => {}
			Handled::Done(command, status) => {
				if let Some(status) = status {
					model.status = status;
				}
				return command.map(TuiCommand::Messages);
			}
		}
	}
	match key.code {
		KeyCode::Char('q') | KeyCode::Esc => model.quit = true,
		KeyCode::Tab | KeyCode::Right => switch_tab(model, 1),
		KeyCode::BackTab | KeyCode::Left => switch_tab(model, -1),
		KeyCode::Char(c @ '1'..='5') => {
			model.tab = Tab::ALL[usize::from(c as u8 - b'1')];
		}
		KeyCode::Down | KeyCode::Char('j') => model.move_selection(1),
		KeyCode::Up | KeyCode::Char('k') => model.move_selection(-1),
		KeyCode::Char('m') => return Some(TuiCommand::SetMuted(!muted(model))),
		KeyCode::Char('d') => return Some(TuiCommand::SetDeafened(!deafened(model))),
		KeyCode::Char(' ') => return toggle_ptt(model),
		KeyCode::Char('t') => return cycle_route(model),
		KeyCode::Char(c @ ('+' | '=' | '-')) if model.tab == Tab::Settings => {
			return adjust_gain(model, if c == '-' { -GAIN_STEP } else { GAIN_STEP });
		}
		KeyCode::Enter => return activate(model),
		KeyCode::Char('c') => return call_selected_peer(model),
		KeyCode::Char('a') => return call_action(model, CallAction::Accept),
		KeyCode::Char('x') => return call_action(model, CallAction::Decline),
		KeyCode::Char('e') => return call_action(model, CallAction::End),
		_ => {}
	}
	None
}

/// Called on every UI tick: keeps a latched push-to-talk lease alive.
pub fn on_tick(model: &Model) -> Option<TuiCommand> {
	let call_id = model.ptt_latched.clone()?;
	let route_generation = route_generation(model, &call_id);
	Some(TuiCommand::Ptt(PttRequest {
		call_id,
		route_generation,
		action: PttAction::Renew,
	}))
}

fn switch_tab(model: &mut Model, delta: isize) {
	let len = Tab::ALL.len() as isize;
	let next = (model.tab.index() as isize + delta).rem_euclid(len);
	model.tab = Tab::ALL[next as usize];
}

fn muted(model: &Model) -> bool {
	model
		.snapshot
		.settings
		.as_ref()
		.is_some_and(|s| s.global_muted)
}

fn deafened(model: &Model) -> bool {
	model
		.snapshot
		.settings
		.as_ref()
		.is_some_and(|s| s.global_deafened)
}

fn route_generation(model: &Model, call_id: &[u8]) -> u64 {
	model
		.snapshot
		.calls
		.iter()
		.find(|c| c.call_id == call_id)
		.and_then(|c| c.transmit.as_ref())
		.map_or(0, |t| t.generation)
}

fn call_selected_peer(model: &mut Model) -> Option<TuiCommand> {
	if model.tab != Tab::Peers {
		return None;
	}
	model
		.selected_peer()
		.map(|p| TuiCommand::StartCall(p.peer_id.clone()))
}

fn call_action(model: &mut Model, action: CallAction) -> Option<TuiCommand> {
	let Some(call) = model.target_call() else {
		model.status = "select a call first".into();
		return None;
	};
	Some(TuiCommand::CallAction(action, call.call_id.clone()))
}

fn cycle_route(model: &mut Model) -> Option<TuiCommand> {
	let Some(call) = model.target_call() else {
		model.status = "select a call first".into();
		return None;
	};
	let call_id = call.call_id.clone();
	let current = call.transmit.as_ref().map_or(0, |t| t.mode);
	let next = match TransmitMode::try_from(current) {
		Ok(TransmitMode::Muted) => TransmitMode::PushToTalk,
		Ok(TransmitMode::PushToTalk) => TransmitMode::Unmuted,
		_ => TransmitMode::Muted,
	};
	if next != TransmitMode::PushToTalk {
		model.ptt_latched = None;
	}
	// The mode is the consent: unmuted and push-to-talk apply at once.
	model.status = format!(
		"mic: {}",
		crate::commands::names::transmit_mode(next as i32)
	);
	Some(TuiCommand::Route {
		call_id,
		mode: next,
	})
}

fn toggle_ptt(model: &mut Model) -> Option<TuiCommand> {
	if let Some(call_id) = model.ptt_latched.take() {
		let route_generation = route_generation(model, &call_id);
		return Some(TuiCommand::Ptt(PttRequest {
			call_id,
			route_generation,
			action: PttAction::Release,
		}));
	}
	let call = model.target_call()?;
	let mode = call.transmit.as_ref().map_or(0, |t| t.mode);
	if mode != TransmitMode::PushToTalk as i32 {
		model.status = "set push-to-talk first (t)".into();
		return None;
	}
	let call_id = call.call_id.clone();
	let route_generation = route_generation(model, &call_id);
	model.ptt_latched = Some(call_id.clone());
	Some(TuiCommand::Ptt(PttRequest {
		call_id,
		route_generation,
		action: PttAction::Press,
	}))
}

fn adjust_gain(model: &Model, delta: f32) -> Option<TuiCommand> {
	let delay_step = if delta < 0.0 {
		-RELEASE_DELAY_STEP_MS
	} else {
		RELEASE_DELAY_STEP_MS
	};
	let defaults = Default::default();
	let settings = model.snapshot.settings.as_ref().unwrap_or(&defaults);
	let step = |v: f32| (v + delta).clamp(0.0, GAIN_MAX);
	match model.selection(Tab::Settings) {
		0 => Some(TuiCommand::MicGain(step(settings.microphone_gain))),
		1 => Some(TuiCommand::PlaybackGain(step(settings.playback_gain))),
		4 => {
			let current = i64::from(settings.ptt_release_delay_ms);
			let next = (current + delay_step).clamp(0, i64::from(RELEASE_DELAY_MAX_MS));
			Some(TuiCommand::PttReleaseDelay(
				u32::try_from(next).unwrap_or(0),
			))
		}
		_ => None,
	}
}

fn activate(model: &mut Model) -> Option<TuiCommand> {
	match model.tab {
		Tab::Peers => call_selected_peer(model),
		Tab::Devices => {
			let device = model.selected_device()?;
			if !device.available {
				model.status = "that device is unavailable".into();
				return None;
			}
			let direction = DeviceDirection::try_from(device.direction).ok()?;
			Some(TuiCommand::SelectDevice {
				direction,
				device_id: device.device_id.clone(),
			})
		}
		Tab::Settings => match model.selection(Tab::Settings) {
			2 => Some(TuiCommand::SetMuted(!muted(model))),
			3 => Some(TuiCommand::SetDeafened(!deafened(model))),
			_ => None,
		},
		Tab::Calls | Tab::Messages => None,
	}
}
