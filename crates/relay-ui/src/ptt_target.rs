//! Which call a push-to-talk press is for: the live push-to-talk call this
//! app's own client controls. Shells hand [`resolver`] to
//! `relay_client::ptt::PttWorker`, so a press is only ever sent for a call the
//! UI client can actually talk in, and a call that ended, left push-to-talk or
//! changed controller ends the press, and so does a client that is no longer
//! attached to the engine.

use std::sync::Arc;

use relay_client::ptt::{PttTarget, Resolve};

use crate::hub::Hub;
use crate::model::{AppState, Link, Mode};

/// The push-to-talk target for `call_hex`, if the UI client is attached and
/// controls it.
pub fn target(state: &AppState, call_hex: &str) -> Option<PttTarget> {
	if state.link != Link::Connected {
		return None;
	}
	let call = state.calls.iter().find(|c| c.call_id == call_hex)?;
	let usable = !call.phase.is_terminal()
		&& call.controlled_by_me
		&& call.transmit.mode == Mode::PushToTalk;
	if !usable {
		return None;
	}
	Some(PttTarget {
		call_id: hex::decode(&call.call_id).ok()?,
		controller_generation: call.controller_generation,
		transmit_route_generation: 0,
		client_id: state.client_id.clone(),
	})
}

/// The one call a global key presses for: live, controlled here and in
/// push-to-talk. With several such calls nothing is guessed (`None`), the
/// on-screen button names its call.
pub fn current_call(state: &AppState) -> Option<String> {
	let mut calls = state
		.calls
		.iter()
		.filter(|c| target(state, &c.call_id).is_some());
	let first = calls.next()?;
	calls.next().is_none().then(|| first.call_id.clone())
}

/// The resolver a push-to-talk worker asks on every press and renewal.
pub fn resolver(hub: Arc<Hub>) -> Resolve {
	Arc::new(move |call| hub.read(|state| target(state, call)))
}
