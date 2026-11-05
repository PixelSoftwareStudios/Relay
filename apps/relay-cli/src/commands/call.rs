//! `call`, `accept`, `decline` and `end`.

use super::resolve::{parse_address, resolve_call, resolve_peer};
use super::{Out, report};
use crate::cli::{CallTarget as CallArg, CliError};
use crate::client::{CallAction, CallTarget, Control};

/// Start a call. This never enables the microphone; use `relay talk` for that.
pub async fn start<C: Control>(
	client: &mut C,
	peer: &str,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let target = match parse_address(peer) {
		Some(address) => CallTarget::Address(address),
		None => {
			let snapshot = client.snapshot().await?;
			CallTarget::Peer(resolve_peer(&snapshot, peer)?.peer_id.clone())
		}
	};
	let op = client.start_call(target).await?;
	report(&op, out)
}

pub async fn lifecycle<C: Control>(
	client: &mut C,
	action: CallAction,
	target: CallArg,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let snapshot = client.snapshot().await?;
	let call = resolve_call(&snapshot, target.call.as_deref())?;
	let op = client.call_action(action, &call.call_id).await?;
	report(&op, out)
}
