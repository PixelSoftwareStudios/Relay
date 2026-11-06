//! `relay identity`: the one deliberate way to replace this device's identity.
//!
//! The identity key is what contacts pin. A new key makes every contact see a changed key and
//! verify this device again, so the reset is only done on an explicit `--yes`. Like every
//! secret, it is touched by the daemon only; this command asks it to.

use relay_api::{Endpoint, RelayClient};
use relay_client::daemon;

use super::Out;
use crate::cli::{CliError, GlobalArgs, IdentityAction, IdentityArgs, endpoint};

pub async fn run(
	global: &GlobalArgs,
	args: &IdentityArgs,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	match args.action {
		IdentityAction::Reset { yes } => reset(&endpoint::resolve(global)?, yes, out).await,
	}
}

async fn reset(endpoint: &Endpoint, yes: bool, out: &mut Out<'_>) -> Result<(), CliError> {
	if !yes {
		return Err(CliError::Usage(
			"this deletes this device's identity key. The next start creates a new one, every \
			 contact will see a changed key and has to verify this device again. Pass --yes to \
			 do it"
				.into(),
		));
	}
	if !daemon::reachable(endpoint).await {
		return Err(CliError::NotRunning {
			endpoint: endpoint::describe(endpoint),
		});
	}
	let mut client = RelayClient::connect(endpoint).await?;
	client.reset_identity().await?;
	out.note("the identity was deleted; a new one exists once the daemon has started again")
}
