//! Keeping a `PttDriver` pointed at the right call.

use std::future::Future;
use std::time::Duration;

use relay_api::pb::{Call, CallState, ClientContext, Snapshot};
use relay_platform::hotkey::HotkeyStream;
use relay_platform::ptt::{GrpcLeaseSink, LeaseSink, PttConfig, PttDriver, PttInput, PttTarget};
use tokio::sync::mpsc;

use super::report::{HotkeyUpdate, TargetStatus};
use crate::cli::CliError;
use crate::client::{Control, RealClient};
use crate::commands::names;

/// How often the controlled call and lease are re-evaluated.
const REFRESH: Duration = Duration::from_secs(1);

fn connected(snapshot: &Snapshot) -> Vec<&Call> {
	snapshot
		.calls
		.iter()
		.filter(|c| c.state == CallState::Connected as i32)
		.collect()
}

/// Work out which call the key controls, taking its controller lease.
async fn find_target<C: Control>(control: &mut C) -> (Option<PttTarget>, TargetStatus) {
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
			};
			(
				Some(target),
				TargetStatus::Call(names::hex_id(&call.call_id)),
			)
		}
		Err(err) => (None, TargetStatus::Denied(err.to_string())),
	}
}

/// Run the driver until `stop` resolves or it fails. Every change of driver
/// state and controlled call is reported through `notify`. On return,
/// transmission has been released or has expired on its own.
pub async fn supervise<C, S>(
	control: &mut C,
	sink: S,
	client: ClientContext,
	keys: HotkeyStream,
	mut notify: impl FnMut(HotkeyUpdate),
	stop: impl Future<Output = ()>,
) -> Result<(), CliError>
where
	C: Control,
	S: LeaseSink + 'static,
{
	let (driver, mut states) = PttDriver::new(sink, client, PttConfig::default());
	let (inputs, input_rx) = mpsc::unbounded_channel();
	let mut task = tokio::spawn(driver.run(keys, input_rx));
	let mut tick = tokio::time::interval(REFRESH);
	let mut bound: Option<PttTarget> = None;
	let mut shown: Option<TargetStatus> = None;
	let mut stop = std::pin::pin!(stop);
	loop {
		tokio::select! {
			done = &mut task => {
				return match done {
					Ok(Ok(())) => Ok(()),
					Ok(Err(err)) => Err(CliError::Other(err.into())),
					Err(join) => Err(CliError::Other(join.into())),
				};
			}
			() = &mut stop => {
				let _ = inputs.send(PttInput::Shutdown);
				let _ = task.await;
				return Ok(());
			}
			changed = states.changed() => {
				if changed.is_ok() {
					notify(HotkeyUpdate::Ptt(states.borrow_and_update().clone()));
				}
			}
			_ = tick.tick() => {
				let (target, status) = find_target(control).await;
				if target != bound {
					let _ = inputs.send(PttInput::Target(target.clone()));
					bound = target;
				}
				if shown.as_ref() != Some(&status) {
					notify(HotkeyUpdate::Target(status.clone()));
					shown = Some(status);
				}
			}
		}
	}
}

/// Open an `InputControl` stream on a connected engine client and supervise
/// the driver against it.
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
	let sink = GrpcLeaseSink::open(engine.client_mut()).await?;
	supervise(engine, sink, context, keys, notify, stop).await
}
