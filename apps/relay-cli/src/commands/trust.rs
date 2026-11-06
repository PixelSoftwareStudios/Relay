//! `trust list|verify|replace|forget`.

use relay_api::pb::TrustState;

use super::resolve::resolve_peer;
use super::views::PeerList;
use super::{Out, report};
use crate::cli::{CliError, ConnectArgs, TrustCommand};
use crate::client::{Control, Manage, TrustDecision};

/// Longest `forget` query the engine accepts.
const MAX_QUERY_BYTES: usize = 256;

pub async fn run<C: Control + Manage>(
	client: &mut C,
	cmd: TrustCommand,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let decision = match cmd {
		TrustCommand::List => {
			let snapshot = client.snapshot().await?;
			return out.emit(&PeerList::from_snapshot(&snapshot));
		}
		TrustCommand::Verify { peer, fingerprint } => {
			let hash = parse_fingerprint(&fingerprint)?;
			let snapshot = client.snapshot().await?;
			TrustDecision {
				peer_id: resolve_peer(&snapshot, &peer)?.peer_id.clone(),
				public_key_sha256: hash,
				requested: TrustState::Verified,
				out_of_band_verified: true,
			}
		}
		TrustCommand::Forget { peer } => {
			let query = peer.trim();
			if query.is_empty() || query.len() > MAX_QUERY_BYTES {
				return Err(CliError::Usage(
					"give a fingerprint prefix (at least 8 hex digits) or a peer name".into(),
				));
			}
			// The engine matches a fingerprint prefix or an exact name; an ambiguous query is
			// answered with the candidates and their short fingerprints, never a guess.
			let op = client.forget_peer(&[], query).await?;
			return report(&op, out);
		}
		TrustCommand::Replace { peer, fingerprint } => {
			let digits = parse_fingerprint(&fingerprint)?;
			let snapshot = client.snapshot().await?;
			let found = resolve_peer(&snapshot, &peer)?;
			if !found.key_changed {
				return Err(CliError::Usage(format!(
					"{} has no changed key to replace; `relay peers` marks a changed key",
					found.display_name
				)));
			}
			let op = client
				.replace_pinned_key(&found.peer_id, &hex::encode(digits))
				.await?;
			return report(&op, out);
		}
	};
	let op = client.decide_trust(decision).await?;
	report(&op, out)
}

/// Accepts 64 hex characters, ignoring `:`/space separators.
pub fn parse_fingerprint(text: &str) -> Result<Vec<u8>, CliError> {
	let cleaned: String = text
		.chars()
		.filter(|c| !matches!(c, ':' | ' ' | '-'))
		.collect();
	match hex::decode(&cleaned) {
		Ok(bytes) if bytes.len() == 32 => Ok(bytes),
		_ => Err(CliError::Usage(
			"fingerprint must be 64 hex characters (SHA-256 of the peer's public key)".into(),
		)),
	}
}

/// `relay connect --trust <string>`: the string carries the device's fingerprint, so scanning or
/// pasting it is an out-of-band exchange. The engine dials with that key pinned and trusts the
/// device as verified only when it proves the key.
pub async fn connect<C: Control>(
	client: &mut C,
	args: &ConnectArgs,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let op = client.connect_and_trust(args.string.trim()).await?;
	report(&op, out)
}
