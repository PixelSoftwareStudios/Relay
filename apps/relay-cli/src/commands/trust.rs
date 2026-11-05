//! `trust list|verify|forget`.

use relay_api::pb::TrustState;

use super::resolve::resolve_peer;
use super::views::PeerList;
use super::{Out, report};
use crate::cli::{CliError, TrustCommand};
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
