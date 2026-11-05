//! `trust list|verify|forget`.

use relay_api::pb::TrustState;

use super::resolve::resolve_peer;
use super::views::PeerList;
use super::{Out, report};
use crate::cli::{CliError, TrustCommand};
use crate::client::{Control, TrustDecision};

pub async fn run<C: Control>(
	client: &mut C,
	cmd: TrustCommand,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let snapshot = client.snapshot().await?;
	let decision = match cmd {
		TrustCommand::List => return out.emit(&PeerList::from_snapshot(&snapshot)),
		TrustCommand::Verify { peer, fingerprint } => {
			let hash = parse_fingerprint(&fingerprint)?;
			TrustDecision {
				peer_id: resolve_peer(&snapshot, &peer)?.peer_id.clone(),
				public_key_sha256: hash,
				requested: TrustState::Verified,
				out_of_band_verified: true,
			}
		}
		TrustCommand::Forget { peer } => TrustDecision {
			peer_id: resolve_peer(&snapshot, &peer)?.peer_id.clone(),
			public_key_sha256: Vec::new(),
			requested: TrustState::Unknown,
			out_of_band_verified: false,
		},
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
