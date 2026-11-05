//! Command implementations. Each takes a [`Control`] so tests can fake the engine.

mod call;
mod controls;
mod daemon;
mod doctor;
mod mic_test;
pub mod names;
mod output;
mod query;
pub mod resolve;
pub mod trust;
pub mod views;

use std::io::Write;

use relay_api::pb::{Operation, OperationState};

pub use daemon::{DaemonOptions, run_engine_host};
pub use output::Out;

use crate::cli::{Cli, CliError, Command, GlobalArgs};
use crate::client::{self, Control, Subscribe};
use views::Outcome;

/// Run a parsed command line, writing normal output to `w`.
pub async fn run(cli: Cli, w: &mut dyn Write) -> Result<(), CliError> {
	let mut out = Out::new(cli.global.json, w);
	match cli.command {
		Command::Daemon(args) => daemon::run(&cli.global, &args).await,
		Command::Doctor(args) => doctor::run(&cli.global, &args, &mut out).await,
		Command::Tui => crate::tui::run(&cli.global).await,
		command => {
			let mut client = client::connect(&cli.global).await?;
			execute(&mut client, command, &mut out).await
		}
	}
}

/// Run a command that needs a connected client.
pub async fn execute<C: Control + Subscribe>(
	client: &mut C,
	command: Command,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	match command {
		Command::Status => query::status(client, out).await,
		Command::Peers => query::peers(client, out).await,
		Command::Call { peer } => call::start(client, &peer, out).await,
		Command::Accept(t) => {
			call::lifecycle(client, crate::client::CallAction::Accept, t, out).await
		}
		Command::Decline(t) => {
			call::lifecycle(client, crate::client::CallAction::Decline, t, out).await
		}
		Command::End(t) => call::lifecycle(client, crate::client::CallAction::End, t, out).await,
		Command::Mute(args) => controls::mute(client, &args, out).await,
		Command::Deafen(args) => controls::deafen(client, &args, out).await,
		Command::Gain(args) => controls::gain(client, &args, out).await,
		Command::Talk(args) => controls::talk(client, &args, out).await,
		Command::Devices(cmd) => controls::devices(client, cmd, out).await,
		Command::Ns(cmd) => controls::noise_suppression(client, cmd, out).await,
		Command::Profile(cmd) => controls::profile(client, cmd, out).await,
		Command::Trust(cmd) => trust::run(client, cmd, out).await,
		Command::MicTest(args) => mic_test::run(client, &args, out).await,
		Command::Daemon(_) | Command::Doctor(_) | Command::Tui => Err(CliError::Usage(
			"this command manages its own connection".into(),
		)),
	}
}

/// Turn a settled operation into an [`Outcome`] or the engine's rejection.
pub fn outcome(op: &Operation) -> Result<Outcome, CliError> {
	if op.state == OperationState::Failed as i32 {
		return Err(match &op.problem {
			Some(problem) => CliError::from_problem(problem),
			None => CliError::Rejected {
				code: "operation_failed".into(),
				message: "the engine reported the operation as failed".into(),
				retryable: false,
			},
		});
	}
	Ok(Outcome {
		operation_id: names::hex_id(&op.operation_id),
		state: if op.state == OperationState::Applied as i32 {
			"applied"
		} else {
			"accepted"
		},
		revision: op.revision,
		call_id: Some(op.call_id.clone())
			.filter(|id| !id.is_empty())
			.map(|id| names::hex_id(&id)),
	})
}

/// Convenience used by commands: settle, check and print an operation.
pub(crate) fn report(op: &Operation, out: &mut Out<'_>) -> Result<(), CliError> {
	out.emit(&outcome(op)?)
}

/// Used by the TUI and doctor, which build their own client.
pub fn global_timeout(global: &GlobalArgs) -> std::time::Duration {
	std::time::Duration::from_millis(global.timeout_ms)
}
