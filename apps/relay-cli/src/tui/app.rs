//! The event loop and terminal lifecycle.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossterm::event::{self, Event};
use ratatui::Terminal;
use ratatui::backend::Backend;
use tokio::sync::{mpsc, oneshot};

use super::keys::{self, TuiCommand};
use super::messages::{self, Link, MsgCommand};
use super::model::Model;
use super::{exec, view};
use crate::cli::{CliError, GlobalArgs};
use crate::client::{self, Control, Messaging, RealClient, Update};
use crate::commands::clipboard::{Clipboard, SystemClipboard};
use crate::hotkey::{HotkeyFeed, HotkeyReport, HotkeyUpdate, run_against_engine};

/// Interval of the push-to-talk lease renewal and periodic redraw.
const TICK: Duration = Duration::from_millis(100);
const RETRY: Duration = Duration::from_secs(1);
const METERS_HZ: u32 = 10;

fn terminal_error(e: impl std::fmt::Debug) -> CliError {
	CliError::Other(anyhow::anyhow!("terminal error: {e:?}"))
}

/// The Messages tab's way to the daemon: its own `messages` connection and the clipboard.
pub struct MessagesPort<'a, M: Messaging> {
	pub client: &'a mut M,
	pub clipboard: &'a mut dyn Clipboard,
}

/// Most queued updates applied between two redraws, so a burst costs one frame.
const BATCH: usize = 256;

/// Run the UI without a messages connection (the Messages tab says so).
pub async fn drive<C: Control, B: Backend>(
	terminal: &mut Terminal<B>,
	client: &mut C,
	model: &mut Model,
	updates: &mut mpsc::Receiver<Update>,
	events: &mut mpsc::Receiver<Event>,
) -> Result<(), CliError>
where
	B::Error: std::fmt::Debug,
{
	drive_with::<C, B, RealClient>(terminal, client, None, model, updates, events).await
}

/// Run the UI until quit, the event source ends or the update feed closes.
pub async fn drive_with<C: Control, B: Backend, M: Messaging>(
	terminal: &mut Terminal<B>,
	client: &mut C,
	mut port: Option<MessagesPort<'_, M>>,
	model: &mut Model,
	updates: &mut mpsc::Receiver<Update>,
	events: &mut mpsc::Receiver<Event>,
) -> Result<(), CliError>
where
	B::Error: std::fmt::Debug,
{
	let mut ticker = tokio::time::interval(TICK);
	loop {
		terminal
			.draw(|f| view::draw(f, model))
			.map_err(terminal_error)?;
		if model.quit {
			return Ok(());
		}
		let command = tokio::select! {
			event = events.recv() => match event {
				Some(Event::Key(key)) => {
					model.status.clear();
					keys::handle_key(model, key)
				}
				Some(_) => None,
				None => return Ok(()),
			},
			update = updates.recv() => match update {
				Some(update) => {
					model.apply(update);
					// Coalesce a burst into this one redraw.
					for _ in 0..BATCH {
						match updates.try_recv() {
							Ok(more) => model.apply(more),
							Err(_) => break,
						}
					}
					None
				}
				None => return Ok(()),
			},
			_ = ticker.tick() => keys::on_tick(model),
		};
		match command {
			Some(TuiCommand::Messages(command)) => {
				run_messages(port.as_mut(), model, command).await;
			}
			Some(command) => run_command(client, model, command).await,
			None => {}
		}
	}
}

async fn run_command<C: Control>(client: &mut C, model: &mut Model, command: TuiCommand) {
	match exec::execute(client, command).await {
		Ok(status) if !status.is_empty() => model.status = status,
		Ok(_) => {}
		Err(err) => model.status = err.to_string(),
	}
}

async fn run_messages<M: Messaging>(
	port: Option<&mut MessagesPort<'_, M>>,
	model: &mut Model,
	command: MsgCommand,
) {
	let Some(port) = port else {
		model.status = "messages are unavailable on this connection".into();
		return;
	};
	match messages::exec::execute(&mut *port.client, &mut *port.clipboard, command).await {
		Ok(outcome) => {
			model.status = outcome.status;
			if outcome.sent {
				model.messages.compose.clear();
			}
			if let Some((session, hits)) = outcome.hits
				&& let Some(pane) = model
					.messages
					.sessions
					.iter_mut()
					.find(|p| p.summary.session_id == session)
			{
				pane.hits = hits;
			}
		}
		Err(err) => model.status = err.to_string(),
	}
}

/// `relay tui`: connect, subscribe and run the interactive UI.
pub async fn run(global: &GlobalArgs, hotkeys: HotkeyFeed) -> Result<(), CliError> {
	let mut client = client::connect(global).await?;
	let state_client = client.duplicate().await?;
	let telemetry_client = client.duplicate().await?;
	let (update_tx, mut updates) = mpsc::channel(256);
	tokio::spawn(client::run_state_feed(
		state_client,
		update_tx.clone(),
		RETRY,
	));
	tokio::spawn(client::run_telemetry_feed(
		telemetry_client,
		update_tx.clone(),
		METERS_HZ,
		RETRY * 2,
	));

	let (stop_hotkey, hotkey_task) = start_hotkey(&client, hotkeys, update_tx.clone()).await;

	let (event_tx, mut events) = mpsc::channel(64);
	let stop = Arc::new(AtomicBool::new(false));
	let reader = spawn_key_reader(event_tx, Arc::clone(&stop));

	let mut model = Model::new();
	// The messages connection is separate and asks for `messages` only; without it the
	// rest of the UI works and the Messages tab says why it cannot.
	let mut messaging = match client::connect_existing_messages(global).await {
		Ok(commands) => match commands.duplicate().await {
			Ok(feed) => {
				tokio::spawn(client::run_session_feed(feed, update_tx.clone(), RETRY));
				Some(commands)
			}
			Err(err) => {
				model.messages.link = Link::Unavailable(err.to_string());
				None
			}
		},
		Err(err) => {
			model.messages.link = Link::Unavailable(err.to_string());
			None
		}
	};
	let mut clipboard = SystemClipboard;
	let mut terminal = ratatui::init();
	let result = drive_with(
		&mut terminal,
		&mut client,
		messaging.as_mut().map(|client| MessagesPort {
			client,
			clipboard: &mut clipboard,
		}),
		&mut model,
		&mut updates,
		&mut events,
	)
	.await;
	ratatui::restore();
	let _ = stop_hotkey.send(());
	if let Some(task) = hotkey_task {
		// The driver releases push-to-talk before it returns.
		let _ = tokio::time::timeout(Duration::from_secs(1), task).await;
	}
	stop.store(true, Ordering::Relaxed);
	let _ = reader.join();
	result
}

/// Terminal events are read on a plain thread; crossterm polling blocks.
fn spawn_key_reader(tx: mpsc::Sender<Event>, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
	std::thread::spawn(move || {
		while !stop.load(Ordering::Relaxed) {
			match event::poll(Duration::from_millis(100)) {
				Ok(true) => match event::read() {
					Ok(ev) => {
						if tx.blocking_send(ev).is_err() {
							return;
						}
					}
					Err(_) => return,
				},
				Ok(false) => {}
				Err(_) => return,
			}
		}
	})
}

/// Report the hotkey state to the UI and, when keys are flowing, drive push-to-talk
/// on its own engine connection until the returned sender fires.
async fn start_hotkey(
	client: &RealClient,
	feed: HotkeyFeed,
	updates: mpsc::Sender<Update>,
) -> (oneshot::Sender<()>, Option<tokio::task::JoinHandle<()>>) {
	let (stop_tx, stop_rx) = oneshot::channel();
	let report = |r| Update::Hotkey(HotkeyUpdate::Report(r));
	let _ = updates.send(report(feed.report.clone())).await;
	let Some(keys) = feed.stream else {
		return (stop_tx, None);
	};
	let mut engine = match client.duplicate().await {
		Ok(engine) => engine,
		Err(err) => {
			let failed = HotkeyReport::Failed {
				backend: "hotkey".into(),
				reason: err.to_string(),
			};
			let _ = updates.send(report(failed)).await;
			return (stop_tx, None);
		}
	};
	let task = tokio::spawn(async move {
		let sender = updates.clone();
		let notify = move |u| {
			let _ = sender.try_send(Update::Hotkey(u));
		};
		let stop = async {
			let _ = stop_rx.await;
		};
		if let Err(err) = run_against_engine(&mut engine, keys, notify, stop).await {
			let failed = HotkeyReport::Failed {
				backend: "hotkey".into(),
				reason: err.to_string(),
			};
			let _ = updates.send(report(failed)).await;
		}
	});
	(stop_tx, Some(task))
}
