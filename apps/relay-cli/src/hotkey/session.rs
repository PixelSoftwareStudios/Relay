//! Keeping the shared `PttWorker` pointed at the right call.

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use relay_api::Endpoint;
use relay_api::pb::{Call, CallState, Snapshot};
use relay_client::ptt::{PttTarget, PttWorker};
use relay_io::LockExt;
use relay_platform::hotkey::{HotkeyEvent, HotkeyStream};

use super::report::{HotkeyUpdate, TargetStatus};
use crate::cli::CliError;
use crate::client::{Control, RealClient};
use crate::commands::names;

/// How often the controlled call and lease are re-evaluated.
const REFRESH: Duration = Duration::from_secs(1);

/// The call the key controls: its hex id and the target a press is sent for.
type Bound = Option<(String, PttTarget)>;

fn connected(snapshot: &Snapshot) -> Vec<&Call> {
	snapshot
		.calls
		.iter()
		.filter(|c| c.state == CallState::Connected as i32)
		.collect()
}

/// Work out which call the key controls, taking its controller lease.
async fn find_target<C: Control>(
	control: &mut C,
	client_id: &[u8],
) -> (Option<PttTarget>, TargetStatus) {
	let snapshot = match control.snapshot().await {
		Ok(snapshot) => snapshot,
		Err(err) => return (None, TargetStatus::Denied(err.to_string())),
	};
	// Followed live: with push-to-talk off no call is bound, so the key sends nothing and a
	// held lease is released. (The OS registration itself is dropped at the next start.)
	if snapshot.settings.as_ref().is_some_and(|s| !s.ptt_enabled) {
		return (None, TargetStatus::PttOff);
	}
	let call = match connected(&snapshot).as_slice() {
		[] => return (None, TargetStatus::NoCall),
		[one] => (*one).clone(),
		_ => return (None, TargetStatus::Ambiguous),
	};
	match control.controller_generation(&call.call_id).await {
		Ok(generation) => {
			let route = call.transmit.as_ref().map_or(0, |t| t.generation);
			let target = PttTarget {
				call_id: call.call_id.clone(),
				controller_generation: generation,
				transmit_route_generation: route,
				client_id: client_id.to_vec(),
			};
			(
				Some(target),
				TargetStatus::Call(names::hex_id(&call.call_id)),
			)
		}
		Err(err) => (None, TargetStatus::Denied(err.to_string())),
	}
}

/// Press and release the worker with the key until `stop` resolves. Every
/// change of worker state and controlled call is reported through `notify`.
/// On return the stream is closed, which releases at once.
pub async fn supervise<C: Control>(
	control: &mut C,
	endpoint: Endpoint,
	client_id: Vec<u8>,
	mut keys: HotkeyStream,
	mut notify: impl FnMut(HotkeyUpdate),
	stop: impl Future<Output = ()>,
) -> Result<(), CliError> {
	let bound: Arc<Mutex<Bound>> = Arc::default();
	let slot = bound.clone();
	let resolve: relay_client::ptt::Resolve = Arc::new(move |call| {
		let bound = slot.lock_unpoisoned();
		bound
			.as_ref()
			.filter(|(hex, _)| hex == call)
			.map(|(_, target)| target.clone())
	});
	let worker = PttWorker::spawn(&tokio::runtime::Handle::current(), endpoint, resolve);
	let mut states = worker.states();
	let mut tick = tokio::time::interval(REFRESH);
	let mut shown: Option<TargetStatus> = None;
	let mut stop = std::pin::pin!(stop);
	let result = loop {
		tokio::select! {
			edge = keys.recv() => match edge {
				Some(HotkeyEvent::Pressed) => {
					let call = bound.lock_unpoisoned().as_ref().map(|(hex, _)| hex.clone());
					worker.press(call.unwrap_or_default());
				}
				Some(HotkeyEvent::Released) => worker.release(),
				None => break Err(CliError::Other(anyhow::anyhow!("hotkey backend stopped"))),
			},
			() = &mut stop => break Ok(()),
			changed = states.changed() => {
				if changed.is_ok() {
					notify(HotkeyUpdate::Ptt(states.borrow_and_update().clone()));
				}
			}
			_ = tick.tick() => {
				let (target, status) = find_target(control, &client_id).await;
				let hex = match &status {
					TargetStatus::Call(hex) => Some(hex.clone()),
					_ => None,
				};
				*bound.lock_unpoisoned() = hex.zip(target);
				if shown.as_ref() != Some(&status) {
					notify(HotkeyUpdate::Target(status.clone()));
					shown = Some(status);
				}
			}
		}
	};
	worker.cancel();
	result
}

/// Supervise the key against a connected engine.
pub async fn run_against_engine(
	engine: &mut RealClient,
	keys: HotkeyStream,
	notify: impl FnMut(HotkeyUpdate),
	stop: impl Future<Output = ()>,
) -> Result<(), CliError> {
	let context = engine
		.client_mut()
		.context()
		.ok_or(relay_api::ApiError::NotRegistered)?;
	let endpoint = engine.endpoint().clone();
	supervise(engine, endpoint, context.client_id, keys, notify, stop).await
}
