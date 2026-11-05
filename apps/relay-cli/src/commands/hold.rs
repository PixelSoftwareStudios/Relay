//! `call` and `accept` stay in the foreground.
//!
//! A call belongs to the client that controls it, and a client counts as
//! present only while it holds an open state watch. So these commands attach
//! first, start or answer the call, print each state change, and hang up when
//! the user presses Ctrl-C or types `q` and Enter. Closing the terminal or
//! killing the process also ends the call (after the daemon's short grace).

use std::future::pending;
use std::time::Duration;

use relay_api::WatchItem;
use relay_api::pb::{Call, CallState, state_event};
use serde::Serialize;
use tokio::sync::mpsc::Receiver;

use super::names::{call_is_live, call_state};
use super::{Out, call};
use crate::cli::{CallTarget as CallArg, CliError};
use crate::client::{CallAction, Control, Feed, Manage, Subscribe};

/// How long a hangup waits for the call to be over before giving up.
const HANGUP_WAIT: Duration = Duration::from_secs(5);

/// What ends a held call from the user's side.
pub enum Hold<'a> {
	/// Do not hold: print the outcome and return (library and test use).
	Off,
	/// Ctrl-C, or `q` on standard input.
	Signals(Option<Receiver<()>>),
	/// An explicit channel; a message hangs up (tests without a terminal).
	Channel(&'a mut Receiver<()>),
}

impl Hold<'_> {
	pub(crate) fn holds(&self) -> bool {
		!matches!(self, Self::Off)
	}

	/// Resolves when the user asks to hang up.
	async fn hangup(&mut self) {
		match self {
			Self::Off => pending::<()>().await,
			Self::Signals(typed) => {
				let typed = typed.get_or_insert_with(watch_stdin);
				tokio::select! {
					_ = tokio::signal::ctrl_c() => {}
					got = typed.recv() => if got.is_none() { pending::<()>().await },
				}
			}
			Self::Channel(rx) => {
				if rx.recv().await.is_none() {
					pending::<()>().await;
				}
			}
		}
	}
}

/// Reads standard input on its own thread and reports each `q` line. When
/// input ends the channel closes, which is not a request to hang up.
fn watch_stdin() -> Receiver<()> {
	let (tx, rx) = tokio::sync::mpsc::channel(1);
	std::thread::spawn(move || {
		for line in std::io::stdin().lines() {
			match line {
				Ok(line) if line.trim().eq_ignore_ascii_case("q") => {
					if tx.blocking_send(()).is_err() {
						return;
					}
				}
				Ok(_) => {}
				Err(_) => return,
			}
		}
	});
	rx
}

/// One state change of the held call.
#[derive(Debug, Serialize)]
struct Line {
	event: &'static str,
	call_id: String,
	state: &'static str,
}

impl std::fmt::Display for Line {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(
			f,
			"call {} {}",
			&self.call_id[..self.call_id.len().min(8)],
			self.state
		)
	}
}

/// `relay call <peer>`, staying attached while the call lives.
pub async fn start<C: Control + Subscribe + Manage>(
	client: &mut C,
	peer: &str,
	out: &mut Out<'_>,
	hold: &mut Hold<'_>,
) -> Result<(), CliError> {
	if !hold.holds() {
		return call::start(client, peer, out).await;
	}
	// Attach before the call exists, so it is never ownerless.
	let mut watch = client.open_watch().await?;
	let op = call::start_op(client, peer).await?;
	let id = op.call_id.clone();
	super::report(&op, out)?;
	follow(client, &mut watch, &id, out, hold).await
}

/// `relay accept [call]`, then stay attached for the rest of the call.
pub async fn accept<C: Control + Subscribe>(
	client: &mut C,
	target: CallArg,
	out: &mut Out<'_>,
	hold: &mut Hold<'_>,
) -> Result<(), CliError> {
	if !hold.holds() {
		return call::lifecycle(client, CallAction::Accept, target, out).await;
	}
	let mut watch = client.open_watch().await?;
	let snapshot = client.snapshot().await?;
	let found = super::resolve::resolve_call(&snapshot, target.call.as_deref())?;
	let id = found.call_id.clone();
	let op = client.call_action(CallAction::Accept, &id).await?;
	super::report(&op, out)?;
	follow(client, &mut watch, &id, out, hold).await
}

async fn follow<C: Control, F: Feed<Item = WatchItem>>(
	client: &mut C,
	watch: &mut F,
	id: &[u8],
	out: &mut Out<'_>,
	hold: &mut Hold<'_>,
) -> Result<(), CliError> {
	let mut last = None;
	let mut hanging_up = false;
	let deadline = tokio::time::sleep(Duration::from_secs(60 * 60 * 24 * 365));
	tokio::pin!(deadline);
	loop {
		let item = tokio::select! {
			item = watch.next() => item?,
			() = hold.hangup(), if !hanging_up => {
				hanging_up = true;
				out.note("hanging up...")?;
				let _ = client.call_action(CallAction::End, id).await;
				deadline.as_mut().reset(tokio::time::Instant::now() + HANGUP_WAIT);
				continue;
			}
			() = &mut deadline => return Ok(()),
		};
		let Some(item) = item else {
			return Err(CliError::Other(anyhow::anyhow!(
				"lost the connection to the daemon; the call ends with it"
			)));
		};
		let Some(call) = changed_call(&item, id) else {
			continue;
		};
		if last != Some(call.state) {
			last = Some(call.state);
			out.emit(&Line {
				event: "call",
				call_id: hex::encode(id),
				state: call_state(call.state),
			})?;
		}
		if !call_is_live(call.state) {
			return match (CallState::try_from(call.state), &call.problem) {
				(Ok(CallState::Failed), Some(problem)) => Err(CliError::from_problem(problem)),
				_ => Ok(()),
			};
		}
	}
}

/// The held call as the item shows it, if the item concerns it.
fn changed_call(item: &WatchItem, id: &[u8]) -> Option<Call> {
	match item {
		WatchItem::Snapshot(snapshot) => snapshot.calls.iter().find(|c| c.call_id == id).cloned(),
		WatchItem::Event { event, .. } => match &event.event {
			Some(state_event::Event::Call(call)) if call.call_id == id => Some(call.clone()),
			_ => None,
		},
		WatchItem::Resync(_) => None,
	}
}
