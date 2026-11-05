//! Carrying out [`TuiCommand`]s through a [`Control`].

use relay_api::pb::DeviceDirection;

use super::keys::TuiCommand;
use crate::cli::CliError;
use crate::client::{CallTarget, Control, GlobalControls};
use crate::commands::outcome;

/// Run one command and return a short status line for the footer.
pub async fn execute<C: Control>(client: &mut C, command: TuiCommand) -> Result<String, CliError> {
	let op = match command {
		TuiCommand::StartCall(peer) => client.start_call(CallTarget::Peer(peer)).await?,
		TuiCommand::CallAction(action, call_id) => client.call_action(action, &call_id).await?,
		TuiCommand::Route {
			call_id,
			mode,
			consent,
		} => client.set_route(&call_id, mode, consent).await?,
		TuiCommand::SetMuted(on) => {
			let controls = GlobalControls {
				muted: Some(on),
				..GlobalControls::default()
			};
			client.set_global_controls(controls).await?
		}
		TuiCommand::SetDeafened(on) => {
			let controls = GlobalControls {
				deafened: Some(on),
				..GlobalControls::default()
			};
			client.set_global_controls(controls).await?
		}
		TuiCommand::MicGain(v) => {
			let controls = GlobalControls {
				microphone_gain: Some(v),
				..GlobalControls::default()
			};
			client.set_global_controls(controls).await?
		}
		TuiCommand::PlaybackGain(v) => {
			let controls = GlobalControls {
				playback_gain: Some(v),
				..GlobalControls::default()
			};
			client.set_global_controls(controls).await?
		}
		TuiCommand::SelectDevice {
			direction,
			device_id,
		} => {
			let snapshot = client.snapshot().await?;
			let current = snapshot.settings.unwrap_or_default();
			let (input, output) = if direction == DeviceDirection::Input {
				(device_id, current.output_device_id)
			} else {
				(current.input_device_id, device_id)
			};
			client.select_devices(&input, &output).await?
		}
		TuiCommand::Ptt(request) => {
			client.ptt(request).await?;
			return Ok(String::new());
		}
	};
	outcome(&op)?;
	Ok("ok".into())
}
