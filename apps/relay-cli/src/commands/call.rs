//! `call`, `accept`, `decline` and `end`.

use relay_net::AddressPolicy;
use relay_net::connect::ConnectionString;

use super::resolve::{parse_address, resolve_call, resolve_peer};
use super::{Out, report};
use crate::cli::{CallTarget as CallArg, CliError};
use crate::client::{CallAction, CallTarget, Control, Manage};

/// Start a call. This never enables the microphone; use `relay talk` for that.
pub async fn start<C: Control + Manage>(
	client: &mut C,
	peer: &str,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	if peer
		.trim_start()
		.to_ascii_lowercase()
		.starts_with("relay://")
	{
		return start_string(client, peer, out).await;
	}
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

/// Call a pasted `relay://ip:port#fingerprint` string. It is checked with the
/// connection-string parser first, so typos fail here and not as an engine
/// error. The overlay opt-in is the engine's call (it holds the setting), so
/// the local check admits such addresses and the engine decides.
async fn start_string<C: Manage>(
	client: &mut C,
	text: &str,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let policy = AddressPolicy::lan_only().with_overlay_manual(true);
	let parsed = ConnectionString::parse(text, &policy)
		.map_err(|e| CliError::Usage(format!("not a usable connection string: {e}")))?;
	let op = client.start_call_string(&parsed.to_string()).await?;
	report(&op, out)
}
