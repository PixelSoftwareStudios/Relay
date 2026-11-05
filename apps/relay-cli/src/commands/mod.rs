//! Command implementations. Each takes a [`Control`] so tests can fake the engine.

mod address;
mod avatar;
mod call;
mod channel;
pub mod clipboard;
pub mod config;
mod controls;
mod daemon;
mod doctor;
mod hold;
mod hotkey;
pub mod messages;
mod mic_test;
pub mod names;
mod output;
mod privacy;
mod query;
pub mod resolve;
pub mod table;
pub mod trust;
pub mod views;

use std::io::Write;

use relay_api::pb::{Operation, OperationState};

pub use hold::Hold;
pub use hotkey::{set as hotkey_set, status_view as hotkey_status_view};
pub use output::Out;

use crate::cli::{Cli, CliError, Command, GlobalArgs};
use crate::client::{self, Control, Manage, Subscribe};
use crate::hotkey::HotkeyFeed;
use views::Outcome;

/// Run a parsed command line without a global hotkey, writing output to `w`.
pub async fn run(cli: Cli, w: &mut dyn Write) -> Result<(), CliError> {
	dispatch(cli, HotkeyFeed::off("not started"), w, Hold::Off).await
}

/// Like [`run`], but `call` and `accept` stay in the foreground until the call
/// ends or a message arrives on `hangup` (what Ctrl-C or `q` do in a terminal).
pub async fn run_holding(
	cli: Cli,
	w: &mut dyn Write,
	hangup: &mut tokio::sync::mpsc::Receiver<()>,
) -> Result<(), CliError> {
	dispatch(
		cli,
		HotkeyFeed::off("not started"),
		w,
		Hold::Channel(hangup),
	)
	.await
}

/// Run a parsed command line. `hotkeys` is what `main` got from starting the
/// platform backend on the main thread; only `hotkey` and `tui` use it.
pub async fn run_with(cli: Cli, hotkeys: HotkeyFeed, w: &mut dyn Write) -> Result<(), CliError> {
	dispatch(cli, hotkeys, w, Hold::Signals(None)).await
}

async fn dispatch(
	cli: Cli,
	hotkeys: HotkeyFeed,
	w: &mut dyn Write,
	mut hold: Hold<'_>,
) -> Result<(), CliError> {
	let mut out = Out::new(cli.global.json, w);
	match cli.command {
		Command::Daemon(args) => daemon::run(&cli.global, &args, &mut out).await,
		Command::Doctor(args) => doctor::run(&cli.global, &args, &mut out).await,
		Command::Tui(_) => crate::tui::run(&cli.global, hotkeys).await,
		Command::Hotkey(args) => hotkey::run(&cli.global, &args, hotkeys, &mut out).await,
		Command::Config(cmd) => config::run(&cli.global, cmd, &mut out).await,
		Command::Messages(args) => {
			let mut client = client::connect_messages(&cli.global).await?;
			messages::run(&mut client, args, &mut out).await
		}
		Command::Call { peer } => {
			let mut client = client::connect(&cli.global).await?;
			hold::start(&mut client, &peer, &mut out, &mut hold).await
		}
		Command::Accept(target) => {
			let mut client = client::connect(&cli.global).await?;
			hold::accept(&mut client, target, &mut out, &mut hold).await
		}
		command => {
			let mut client = client::connect(&cli.global).await?;
			execute(&mut client, command, &mut out).await
		}
	}
}

/// Run a command that needs a connected client.
pub async fn execute<C: Control + Subscribe + Manage>(
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
		Command::Address(args) => address::run(client, &args, out).await,
		Command::Privacy(cmd) => privacy::run(client, cmd, out).await,
		Command::Channel(cmd) => channel::run(client, cmd, out).await,
		Command::Avatar(cmd) => avatar::run(client, cmd, out).await,
		Command::Daemon(_)
		| Command::Doctor(_)
		| Command::Tui(_)
		| Command::Hotkey(_)
		| Command::Config(_)
		| Command::Messages(_) => Err(CliError::Usage(
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
