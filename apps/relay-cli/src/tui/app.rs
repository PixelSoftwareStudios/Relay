//! The event loop and terminal lifecycle.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossterm::event::{self, Event};
use ratatui::Terminal;
use ratatui::backend::Backend;
use tokio::sync::mpsc;

use super::keys::{self, TuiCommand};
use super::model::Model;
use super::{exec, view};
use crate::cli::{CliError, GlobalArgs};
use crate::client::{self, Control, Update};

/// Interval of the push-to-talk lease renewal and periodic redraw.
const TICK: Duration = Duration::from_millis(100);
const RETRY: Duration = Duration::from_secs(1);
const METERS_HZ: u32 = 10;

fn terminal_error(e: impl std::fmt::Debug) -> CliError {
	CliError::Other(anyhow::anyhow!("terminal error: {e:?}"))
}

/// Run the UI until quit, the event source ends or the update feed closes.
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
					None
				}
				None => return Ok(()),
			},
			_ = ticker.tick() => keys::on_tick(model),
		};
		if let Some(command) = command {
			run_command(client, model, command).await;
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

/// `relay tui`: connect, subscribe and run the interactive UI.
pub async fn run(global: &GlobalArgs) -> Result<(), CliError> {
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
		update_tx,
		METERS_HZ,
		RETRY * 2,
	));

	let (event_tx, mut events) = mpsc::channel(64);
	let stop = Arc::new(AtomicBool::new(false));
	let reader = spawn_key_reader(event_tx, Arc::clone(&stop));

	let mut model = Model::new(client.info().capabilities.clone());
	let mut terminal = ratatui::init();
	let result = drive(
		&mut terminal,
		&mut client,
		&mut model,
		&mut updates,
		&mut events,
	)
	.await;
	ratatui::restore();
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
