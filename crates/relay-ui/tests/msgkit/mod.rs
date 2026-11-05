//! Builders and a scripted engine for the messaging tests.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use relay_api::SessionItem;
use relay_api::pb::{self, session_event::Event, transcript_entry};
use relay_ui::engine::EngineError;
use relay_ui::messaging::engine::{
	MsgCommand, MsgConnection, MsgEngine, MsgFeed, MsgOps, MsgWatch,
};
use tokio::sync::mpsc;

pub const SECRET: &str = "ZEBRA-SECRET-7731";

pub fn sid(n: u8) -> Vec<u8> {
	vec![n; 16]
}

pub fn key(n: u8) -> String {
	hex::encode(sid(n))
}

pub fn summary(n: u8, name: &str, live: bool) -> pb::SessionSummary {
	pb::SessionSummary {
		session_id: sid(n),
		call_id: vec![n; 16],
		peer_id: vec![n; 32],
		peer_name: name.into(),
		state: if live {
			pb::SessionState::Active
		} else {
			pb::SessionState::Ended
		} as i32,
		permissions: Some(pb::SessionPermissions {
			allow_messages: true,
			allow_files: true,
		}),
		local_is_host: true,
		started_at_unix_ms: 1_700_000_000_000,
		ended_at_unix_ms: if live { 0 } else { 1_700_000_600_000 },
		message_count: 0,
		next_ordinal: 100,
		used_bytes: 0,
		limit_bytes: 64 * 1024 * 1024,
	}
}

pub fn entry(ordinal: u64, outgoing: bool, body: transcript_entry::Body) -> pb::TranscriptEntry {
	pb::TranscriptEntry {
		ordinal,
		outgoing,
		seq: ordinal,
		sent_at_unix_ms: 1_700_000_000_000 + ordinal * 1000,
		received_at_unix_ms: 1_700_000_000_000 + ordinal * 1000,
		delivery: pb::DeliveryState::Delivered as i32,
		body: Some(body),
		..Default::default()
	}
}

pub fn text(ordinal: u64, outgoing: bool, t: &str) -> pb::TranscriptEntry {
	entry(ordinal, outgoing, transcript_entry::Body::Text(t.into()))
}

pub fn offer(n: u8, state: pb::OfferState, outgoing: bool) -> pb::FileOfferBody {
	pb::FileOfferBody {
		offer_id: vec![n; 16],
		name: "report.pdf".into(),
		size: 1_000_000,
		media_type: "application/pdf".into(),
		blake3: vec![0xab; 32],
		state: state as i32,
		outgoing,
		..Default::default()
	}
}

pub fn file(ordinal: u64, n: u8, state: pb::OfferState, outgoing: bool) -> pb::TranscriptEntry {
	entry(
		ordinal,
		outgoing,
		transcript_entry::Body::File(offer(n, state, outgoing)),
	)
}

pub fn new_entry(session: u8, e: pb::TranscriptEntry) -> pb::SessionEvent {
	pb::SessionEvent {
		event: Some(Event::Entry(pb::NewEntry {
			session_id: sid(session),
			entry: Some(e),
		})),
	}
}

pub fn event(e: Event) -> pb::SessionEvent {
	pb::SessionEvent { event: Some(e) }
}

pub fn list(sessions: Vec<pb::SessionSummary>, enabled: bool) -> pb::ListSessionsResponse {
	pb::ListSessionsResponse {
		sessions,
		revision: 10,
		settings: Some(pb::MessagingSettings {
			enabled,
			max_offer_size_mib: 1024,
			transfer_rate_limit_mbps: 16,
			transfer_port: 0,
		}),
	}
}

/// What the scripted daemon holds; tests change it between reads.
#[derive(Default)]
pub struct World {
	pub list: pb::ListSessionsResponse,
	pub transcripts: Vec<(u8, Vec<pb::TranscriptEntry>)>,
	pub list_calls: usize,
	pub watch_after: Vec<u64>,
	pub commands: Vec<MsgCommand>,
	pub fail: Option<String>,
}

type Rx = mpsc::UnboundedReceiver<Result<Option<SessionItem>, EngineError>>;
pub type WatchTx = mpsc::UnboundedSender<Result<Option<SessionItem>, EngineError>>;

pub struct FakeEngine {
	pub world: Arc<Mutex<World>>,
	watches: Arc<Mutex<VecDeque<Rx>>>,
	pub connects: Mutex<usize>,
	pub connect_error: Mutex<Option<EngineError>>,
}

impl FakeEngine {
	/// An engine whose watches are fed from the returned senders, one per watch.
	pub fn new(world: World, watches: usize) -> (Arc<Self>, Vec<WatchTx>) {
		let mut txs = Vec::new();
		let mut rxs = VecDeque::new();
		for _ in 0..watches {
			let (tx, rx) = mpsc::unbounded_channel();
			txs.push(tx);
			rxs.push_back(rx);
		}
		let engine = Arc::new(Self {
			world: Arc::new(Mutex::new(world)),
			watches: Arc::new(Mutex::new(rxs)),
			connects: Mutex::new(0),
			connect_error: Mutex::new(None),
		});
		(engine, txs)
	}
}

struct Feed(Arc<Mutex<World>>, Arc<Mutex<VecDeque<Rx>>>);
struct Watch(mpsc::UnboundedReceiver<Result<Option<SessionItem>, EngineError>>);
struct Ops(Arc<Mutex<World>>);

#[async_trait]
impl MsgEngine for FakeEngine {
	async fn connect(&self) -> Result<MsgConnection, EngineError> {
		*self.connects.lock().unwrap() += 1;
		if let Some(e) = self.connect_error.lock().unwrap().take() {
			return Err(e);
		}
		Ok(MsgConnection {
			feed: Box::new(Feed(self.world.clone(), self.watches.clone())),
			ops: Arc::new(Ops(self.world.clone())),
		})
	}
}

#[async_trait]
impl MsgFeed for Feed {
	async fn list(&mut self) -> Result<pb::ListSessionsResponse, EngineError> {
		let mut w = self.0.lock().unwrap();
		w.list_calls += 1;
		Ok(w.list.clone())
	}

	async fn transcript(
		&mut self,
		session: &[u8],
		from: u64,
	) -> Result<Vec<pb::TranscriptEntry>, EngineError> {
		let w = self.0.lock().unwrap();
		let all = w
			.transcripts
			.iter()
			.find(|(n, _)| sid(*n) == session)
			.map(|(_, e)| e.clone())
			.unwrap_or_default();
		Ok(all.into_iter().filter(|e| e.ordinal >= from).collect())
	}

	async fn watch(&mut self, revision: u64) -> Result<Box<dyn MsgWatch>, EngineError> {
		self.0.lock().unwrap().watch_after.push(revision);
		let rx = self
			.1
			.lock()
			.unwrap()
			.pop_front()
			.ok_or_else(|| EngineError::Broken("no more watches".into()))?;
		Ok(Box::new(Watch(rx)))
	}
}

#[async_trait]
impl MsgWatch for Watch {
	async fn next(&mut self) -> Result<Option<SessionItem>, EngineError> {
		self.0.recv().await.unwrap_or(Ok(None))
	}
}

#[async_trait]
impl MsgOps for Ops {
	async fn run(&self, command: MsgCommand) -> Result<(), EngineError> {
		let mut w = self.0.lock().unwrap();
		w.commands.push(command);
		match &w.fail {
			Some(code) => Err(EngineError::Rejected {
				code: code.clone(),
				message: String::new(),
			}),
			None => Ok(()),
		}
	}
}

pub fn item_event(revision: u64, e: pb::SessionEvent) -> SessionItem {
	SessionItem::Event {
		revision,
		event: Box::new(e),
	}
}

pub fn item_list(sessions: Vec<pb::SessionSummary>) -> SessionItem {
	SessionItem::List(pb::SessionList {
		sessions,
		settings: None,
	})
}

// ---- shell and rig ----

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use relay_ui::actions::{Actions, Shared};
use relay_ui::controller::{Controller, SessionSlot};
use relay_ui::hub::Hub;
use relay_ui::shell::{BoxFuture, FilePicker, PickedFile, Shell};

#[derive(Default)]
pub struct FakeShell {
	pub repaints: AtomicUsize,
	pub copied: Mutex<Vec<String>>,
	pub opened: Mutex<Vec<String>>,
	pub path: Mutex<Option<PathBuf>>,
}

impl FilePicker for FakeShell {
	fn pick_image(&self) -> BoxFuture<'_, Result<Option<PickedFile>, String>> {
		Box::pin(async { Ok(None) })
	}
	fn pick_directory(&self, _t: &str) -> BoxFuture<'_, Result<Option<PathBuf>, String>> {
		let p = self.path.lock().unwrap().clone();
		Box::pin(async move { Ok(p) })
	}
	fn save_as(&self, _n: &str) -> BoxFuture<'_, Result<Option<PathBuf>, String>> {
		let p = self.path.lock().unwrap().clone();
		Box::pin(async move { Ok(p) })
	}
	fn pick_file(&self, _t: &str) -> BoxFuture<'_, Result<Option<PathBuf>, String>> {
		let p = self.path.lock().unwrap().clone();
		Box::pin(async move { Ok(p) })
	}
}

impl Shell for FakeShell {
	fn copy_text(&self, text: &str) {
		self.copied.lock().unwrap().push(text.to_owned());
	}
	fn open_url(&self, url: &str) {
		self.opened.lock().unwrap().push(url.to_owned());
	}
	fn request_repaint(&self) {
		self.repaints.fetch_add(1, Ordering::SeqCst);
	}
	fn window_visible(&self) -> bool {
		true
	}
	fn show_window(&self) {}
	fn hide_window(&self) {}
	fn quit(&self) {}
	fn hides_to_tray(&self) -> bool {
		false
	}
}

pub fn actions(shell: Arc<FakeShell>) -> Actions {
	let hub = Arc::new(Hub::default());
	Actions {
		controller: Arc::new(Controller::new(
			hub.clone(),
			Arc::new(SessionSlot::default()),
		)),
		hub,
		shared: Arc::new(Shared::default()),
		runtime: tokio::runtime::Handle::current(),
		shell,
	}
}

/// Poll until `f` holds (tokio time is real here), at most about 5 s.
pub async fn until(mut f: impl FnMut() -> bool) {
	for _ in 0..500 {
		if f() {
			return;
		}
		tokio::time::sleep(std::time::Duration::from_millis(10)).await;
	}
	panic!("condition not reached");
}
