//! An in-process engine speaking the real API over a Unix socket.
//!
//! It serves a fixed snapshot and records mutations as text, which is enough
//! to prove the CLI's wire behavior end to end.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use relay_api::pb::*;
use relay_api::{ApiLimits, ClientRegistry, Endpoint, UdsListener, serve_unix};
use tempfile::TempDir;
use tokio::sync::oneshot;
use tonic::{Request, Response, Status};

use super::chat::ChatState;
use super::fixtures;

mod service;

type Resp<T> = Result<Response<T>, Status>;

#[derive(Clone)]
pub struct FakeEngine {
	registry: Arc<ClientRegistry>,
	pub log: Arc<Mutex<Vec<String>>>,
	pub snapshot: Arc<Mutex<Snapshot>>,
	pub fail_with: Arc<Mutex<Option<Problem>>>,
	/// The capabilities each client asked for when it registered.
	pub registered: Arc<Mutex<Vec<Vec<String>>>>,
	/// Every secret the engine received, to prove they arrive and never leak.
	pub secrets: Arc<Mutex<Vec<String>>>,
	/// Sessions, transcripts and events; clients need the `messages` capability.
	pub chat: Arc<Mutex<ChatState>>,
	messaging_clients: Arc<Mutex<Vec<Vec<u8>>>>,
	/// What `GetSettings` answers and `SetSettings` changes (flat key to text).
	pub settings: Arc<Mutex<BTreeMap<String, String>>>,
}

/// Keys the real daemon refuses to change over the API.
fn is_restricted(key: &str) -> bool {
	key.starts_with("identity.")
		|| matches!(
			key,
			"diagnostics.record_raw_audio" | "network.allow_wan" | "network.allow_overlay_manual"
		)
}

/// What a fresh daemon reports: every real key at its default, plus the display name.
fn default_settings() -> BTreeMap<String, String> {
	let mut all = relay_config::prefs::flat::all(&relay_config::prefs::Preferences::default())
		.expect("defaults flatten");
	all.insert("profile.display_name".to_owned(), "Desk".to_owned());
	all
}

fn ctx_client(c: &Option<MutationContext>) -> Option<&ClientContext> {
	c.as_ref().and_then(|c| c.client.as_ref())
}

impl FakeEngine {
	fn chat_resp<R>(
		&self,
		client: Option<&ClientContext>,
		f: impl FnOnce(&mut ChatState) -> R,
	) -> Resp<R> {
		self.chat(client, f).map(Response::new)
	}

	/// Run `f` on the chat state for a client that registered with `messages`.
	fn chat<R>(
		&self,
		client: Option<&ClientContext>,
		f: impl FnOnce(&mut ChatState) -> R,
	) -> Result<R, Status> {
		let id = client.map(|c| c.client_id.clone()).unwrap_or_default();
		if !self.messaging_clients.lock().unwrap().contains(&id) {
			return Err(Status::permission_denied(
				"the messages capability is required",
			));
		}
		Ok(f(&mut self.chat.lock().unwrap()))
	}

	fn op(&self, ctx: Option<&MutationContext>, what: String) -> Resp<Operation> {
		self.log.lock().unwrap().push(what);
		let operation_id = ctx.map(|c| c.operation_id.clone()).unwrap_or_default();
		let problem = self.fail_with.lock().unwrap().clone();
		Ok(Response::new(Operation {
			operation_id,
			state: if problem.is_some() {
				OperationState::Failed as i32
			} else {
				OperationState::Applied as i32
			},
			revision: 11,
			call_id: fixtures::CALL.to_vec(),
			problem,
		}))
	}
}

fn unsupported<T>() -> Resp<T> {
	Err(Status::unimplemented("not in the test engine"))
}

/// A running engine plus the endpoint to reach it.
pub struct EngineHarness {
	pub dir: TempDir,
	pub socket: std::path::PathBuf,
	pub engine: FakeEngine,
	shutdown: Option<oneshot::Sender<()>>,
}

impl EngineHarness {
	pub async fn start() -> Self {
		let dir = tempfile::Builder::new()
			.prefix("rlycli")
			.tempdir()
			.expect("tempdir");
		let socket = dir.path().join("run").join("engine.sock");
		let limits = ApiLimits::DEFAULT;
		let registry = Arc::new(ClientRegistry::new(limits.max_clients));
		let engine = FakeEngine {
			registry: Arc::clone(&registry),
			log: Arc::default(),
			snapshot: Arc::new(Mutex::new(fixtures::snapshot())),
			fail_with: Arc::default(),
			registered: Arc::default(),
			secrets: Arc::default(),
			chat: Arc::default(),
			messaging_clients: Arc::default(),
			settings: Arc::new(Mutex::new(default_settings())),
		};
		let listener = UdsListener::bind(&socket).await.expect("bind");
		let (tx, rx) = oneshot::channel();
		let served = engine.clone();
		tokio::spawn(async move {
			let _ = serve_unix(served, listener, registry, limits, async {
				let _ = rx.await;
			})
			.await;
		});
		Self {
			dir,
			socket,
			engine,
			shutdown: Some(tx),
		}
	}

	pub fn endpoint(&self) -> Endpoint {
		Endpoint::Unix(self.socket.clone())
	}

	pub fn log(&self) -> Vec<String> {
		self.engine.log.lock().unwrap().clone()
	}

	/// Run `relay <args>` in-process against this engine and capture stdout.
	pub async fn run(&self, args: &[&str]) -> Result<String, relay_cli::cli::CliError> {
		use clap::Parser;
		let socket = self.socket.to_str().expect("utf8 path");
		// No autostart: the stale-daemon probe would register an extra client these tests count.
		let mut argv = vec!["relay", "--socket", socket, "--no-autostart"];
		argv.extend_from_slice(args);
		let cli = relay_cli::cli::Cli::try_parse_from(argv).expect("valid arguments");
		let mut buffer = Vec::new();
		relay_cli::commands::run(cli, &mut buffer).await?;
		Ok(String::from_utf8(buffer).expect("utf8 output"))
	}
}

impl Drop for EngineHarness {
	fn drop(&mut self) {
		if let Some(tx) = self.shutdown.take() {
			let _ = tx.send(());
		}
	}
}
