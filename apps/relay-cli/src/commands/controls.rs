//! Audio and call controls: mute, deafen, gain, talk, devices, ns, profile.

use relay_api::pb::{DeviceDirection, TransmitMode};

use super::resolve::{resolve_call, resolve_device};
use super::views::DeviceList;
use super::{Out, report};
use crate::cli::{
	CliError, DevicesCommand, GainArgs, GainKind, ProfileCommand, SwitchArgs, TalkArgs, TalkMode,
};
use crate::client::{CallControls, Control, GlobalControls};

pub async fn mute<C: Control>(
	client: &mut C,
	args: &SwitchArgs,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let on = Some(args.state.enabled());
	let call = CallControls {
		muted: on,
		..CallControls::default()
	};
	let global = GlobalControls {
		muted: on,
		..GlobalControls::default()
	};
	switch(client, args, call, global, out).await
}

pub async fn deafen<C: Control>(
	client: &mut C,
	args: &SwitchArgs,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let on = Some(args.state.enabled());
	let call = CallControls {
		deafened: on,
		..CallControls::default()
	};
	let global = GlobalControls {
		deafened: on,
		..GlobalControls::default()
	};
	switch(client, args, call, global, out).await
}

async fn switch<C: Control>(
	client: &mut C,
	args: &SwitchArgs,
	call: CallControls,
	global: GlobalControls,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let op = match &args.call {
		Some(query) => {
			let snapshot = client.snapshot().await?;
			let id = resolve_call(&snapshot, Some(query))?.call_id.clone();
			client.set_call_controls(&id, call).await?
		}
		None => client.set_global_controls(global).await?,
	};
	report(&op, out)
}

fn check_gain(value: f32) -> Result<f32, CliError> {
	if value.is_finite() && value >= 0.0 {
		Ok(value)
	} else {
		Err(CliError::Usage(
			"gain must be a finite number, 0 or greater".into(),
		))
	}
}

pub async fn gain<C: Control>(
	client: &mut C,
	args: &GainArgs,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let value = Some(check_gain(args.value)?);
	let op = match args.kind {
		GainKind::Mic => {
			let controls = GlobalControls {
				microphone_gain: value,
				..GlobalControls::default()
			};
			client.set_global_controls(controls).await?
		}
		GainKind::Playback => {
			let controls = GlobalControls {
				playback_gain: value,
				..GlobalControls::default()
			};
			client.set_global_controls(controls).await?
		}
		GainKind::Receive => {
			let snapshot = client.snapshot().await?;
			let id = resolve_call(&snapshot, args.call.as_deref())?
				.call_id
				.clone();
			let controls = CallControls {
				receive_gain: value,
				..CallControls::default()
			};
			client.set_call_controls(&id, controls).await?
		}
	};
	report(&op, out)
}

/// Set the call's microphone mode. `unmuted` and `ptt` are the consent; there is no flag.
pub async fn talk<C: Control>(
	client: &mut C,
	args: &TalkArgs,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let mode = match args.mode {
		TalkMode::Unmuted => TransmitMode::Unmuted,
		TalkMode::Ptt => TransmitMode::PushToTalk,
		TalkMode::Muted => TransmitMode::Muted,
	};
	let snapshot = client.snapshot().await?;
	let id = resolve_call(&snapshot, args.call.as_deref())?
		.call_id
		.clone();
	let op = client.set_transmit_mode(&id, mode).await?;
	report(&op, out)
}

pub async fn devices<C: Control>(
	client: &mut C,
	cmd: DevicesCommand,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let snapshot = client.snapshot().await?;
	let (input, output) = match cmd {
		DevicesCommand::List => return out.emit(&DeviceList::from_snapshot(&snapshot)),
		DevicesCommand::Select { input, output } => (input, output),
	};
	if input.is_none() && output.is_none() {
		return Err(CliError::Usage("pass --input and/or --output".into()));
	}
	let current = snapshot.settings.clone().unwrap_or_default();
	let input_id = match input {
		Some(q) => resolve_device(&snapshot, &q, DeviceDirection::Input)?
			.device_id
			.clone(),
		None => current.input_device_id,
	};
	let output_id = match output {
		Some(q) => resolve_device(&snapshot, &q, DeviceDirection::Output)?
			.device_id
			.clone(),
		None => current.output_device_id,
	};
	let op = client.select_devices(&input_id, &output_id).await?;
	report(&op, out)
}

pub async fn profile<C: Control>(
	client: &mut C,
	cmd: ProfileCommand,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let ProfileCommand::Set { name } = cmd;
	if name.trim().is_empty() {
		return Err(CliError::Usage("the display name cannot be empty".into()));
	}
	let op = client.set_profile(&name).await?;
	report(&op, out)
}
