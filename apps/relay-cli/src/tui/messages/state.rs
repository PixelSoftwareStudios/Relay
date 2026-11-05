//! The Messages pane's cached copy of the daemon's sessions and transcripts.
//!
//! Everything here is bounded: a few retained sessions and at most [`MAX_ROWS`]
//! entries each; older rows fall off the front. Message text lives only in this
//! struct and on screen, never in a log line.

use relay_api::pb::session_event::Event;
use relay_api::pb::transcript_entry::Body;
use relay_api::pb::{
	MessagingSettings, SessionEvent, SessionState, SessionSummary, TranscriptEntry,
};
use relay_messages::markdown::{self, Document};

use super::prompt::Prompt;

/// Most entries kept per session in the pane.
pub const MAX_ROWS: usize = 1000;
/// Most sessions shown.
pub const MAX_SESSIONS: usize = 16;

/// The state of the messages connection, shown honestly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
	/// Not yet connected.
	Connecting,
	/// A consistent stream.
	Live,
	/// A gap or lag was detected; reading the transcripts again.
	Resyncing(String),
	/// The stream is gone; retrying.
	Lost(String),
	/// Messaging cannot be used on this connection at all.
	Unavailable(String),
}

/// Something the feed delivers.
#[derive(Debug, Clone)]
pub enum MsgUpdate {
	/// Link state.
	Link(Link),
	/// Fresh session list (and settings): replaces what was known.
	Sessions(Vec<SessionSummary>, Option<MessagingSettings>),
	/// Transcript of one session read from the start: merged by ordinal.
	Transcript(Vec<u8>, Vec<TranscriptEntry>),
	/// A watch event.
	Event(Box<SessionEvent>),
}

/// One transcript entry with what the view needs precomputed.
#[derive(Debug, Clone)]
pub struct Row {
	pub entry: TranscriptEntry,
	/// Parsed once; `None` for non-text entries.
	pub doc: Option<Document>,
	/// Code blocks in earlier rows, so block numbers are stable across the session.
	pub first_block: usize,
}

impl Row {
	/// Number of code blocks in this row.
	pub fn blocks(&self) -> usize {
		self.doc.as_ref().map_or(0, |d| d.code_blocks().count())
	}

	/// The text of the `n`th (0-based) code block of this row.
	pub fn block_text(&self, n: usize) -> Option<String> {
		self.doc
			.as_ref()?
			.code_blocks()
			.nth(n)
			.map(|b| b.copy_text().to_owned())
	}
}

/// One session with its rows.
#[derive(Debug, Clone)]
pub struct SessionPane {
	pub summary: SessionSummary,
	pub rows: Vec<Row>,
	/// Selected row; `None` follows the newest.
	pub focus: Option<usize>,
	/// Code block selected in the focused row.
	pub block: usize,
	/// Ordinals matched by the last search.
	pub hits: Vec<u64>,
}

impl SessionPane {
	fn new(summary: SessionSummary) -> Self {
		Self {
			summary,
			rows: Vec::new(),
			focus: None,
			block: 0,
			hits: Vec::new(),
		}
	}

	/// Whether the call is still live (an ended session is a read-only transcript).
	pub fn active(&self) -> bool {
		self.summary.state == SessionState::Active as i32
	}

	fn renumber(&mut self) {
		let mut total = 0;
		for row in &mut self.rows {
			row.first_block = total;
			total += row.blocks();
		}
	}

	fn row_for(entry: TranscriptEntry) -> Row {
		let doc = match &entry.body {
			Some(Body::Text(text)) => Some(markdown::parse(text)),
			_ => None,
		};
		Row {
			entry,
			doc,
			first_block: 0,
		}
	}

	/// Insert or replace by ordinal, keeping order and the bound.
	pub fn upsert(&mut self, entry: TranscriptEntry) {
		let at = self
			.rows
			.partition_point(|r| r.entry.ordinal < entry.ordinal);
		match self.rows.get_mut(at) {
			Some(row) if row.entry.ordinal == entry.ordinal => {
				*row = Self::row_for(entry);
			}
			_ => self.rows.insert(at, Self::row_for(entry)),
		}
		if self.rows.len() > MAX_ROWS {
			let extra = self.rows.len() - MAX_ROWS;
			self.rows.drain(..extra);
			self.focus = self.focus.and_then(|f| f.checked_sub(extra));
		}
		self.renumber();
	}

	/// The focused row.
	pub fn focused(&self) -> Option<&Row> {
		self.rows.get(self.focus?)
	}

	fn row_mut(&mut self, f: impl Fn(&TranscriptEntry) -> bool) -> Option<&mut TranscriptEntry> {
		self.rows.iter_mut().map(|r| &mut r.entry).find(|e| f(e))
	}
}

/// Everything the Messages tab shows and edits.
#[derive(Debug)]
pub struct MessagesState {
	pub link: Link,
	pub settings: Option<MessagingSettings>,
	pub sessions: Vec<SessionPane>,
	/// Session id of the pane in view.
	pub current: Option<Vec<u8>>,
	/// Compose box content.
	pub compose: String,
	/// Whether keys type into the compose box.
	pub composing: bool,
	/// An open question (search text, a path, a confirmation).
	pub prompt: Option<Prompt>,
}

impl Default for MessagesState {
	fn default() -> Self {
		Self {
			link: Link::Connecting,
			settings: None,
			sessions: Vec::new(),
			current: None,
			compose: String::new(),
			composing: false,
			prompt: None,
		}
	}
}

impl MessagesState {
	/// The pane in view.
	pub fn pane(&self) -> Option<&SessionPane> {
		let id = self.current.as_ref()?;
		self.sessions.iter().find(|s| &s.summary.session_id == id)
	}

	/// The pane in view, mutable.
	pub fn pane_mut(&mut self) -> Option<&mut SessionPane> {
		let id = self.current.clone()?;
		self.sessions
			.iter_mut()
			.find(|s| s.summary.session_id == id)
	}

	/// Show the next (`1`) or previous (`-1`) session.
	pub fn cycle(&mut self, delta: isize) {
		if self.sessions.is_empty() {
			return;
		}
		let at = self
			.current
			.as_ref()
			.and_then(|id| {
				self.sessions
					.iter()
					.position(|s| &s.summary.session_id == id)
			})
			.unwrap_or(0);
		let next = (at as isize + delta).rem_euclid(self.sessions.len() as isize) as usize;
		self.current = Some(self.sessions[next].summary.session_id.clone());
		self.compose.clear();
		self.composing = false;
		self.prompt = None;
	}

	fn pick_current(&mut self) {
		let known = self
			.current
			.as_ref()
			.is_some_and(|id| self.sessions.iter().any(|s| &s.summary.session_id == id));
		if !known {
			self.current = self
				.sessions
				.iter()
				.find(|s| s.active())
				.or(self.sessions.first())
				.map(|s| s.summary.session_id.clone());
			self.compose.clear();
			self.composing = false;
			self.prompt = None;
		}
	}

	fn session_mut(&mut self, id: &[u8]) -> Option<&mut SessionPane> {
		self.sessions
			.iter_mut()
			.find(|s| s.summary.session_id == id)
	}

	fn upsert_session(&mut self, summary: SessionSummary) {
		let known = self
			.sessions
			.iter()
			.position(|s| s.summary.session_id == summary.session_id);
		match known {
			Some(i) => self.sessions[i].summary = summary,
			None if self.sessions.len() < MAX_SESSIONS => {
				self.sessions.push(SessionPane::new(summary));
			}
			None => {}
		}
		self.pick_current();
	}

	/// Apply something from the feed.
	pub fn apply(&mut self, update: MsgUpdate) {
		match update {
			MsgUpdate::Link(link) => self.link = link,
			MsgUpdate::Sessions(list, settings) => {
				self.settings = settings;
				self.sessions
					.retain(|s| list.iter().any(|n| n.session_id == s.summary.session_id));
				for summary in list {
					self.upsert_session(summary);
				}
				self.pick_current();
			}
			MsgUpdate::Transcript(id, entries) => {
				if let Some(pane) = self.session_mut(&id) {
					for entry in entries {
						pane.upsert(entry);
					}
				}
			}
			MsgUpdate::Event(event) => {
				if let Some(event) = event.event {
					self.apply_event(event);
				}
			}
		}
	}

	fn apply_event(&mut self, event: Event) {
		match event {
			Event::Entry(n) => {
				if let (Some(pane), Some(entry)) = (self.session_mut(&n.session_id), n.entry) {
					pane.upsert(entry);
				}
			}
			Event::Delivery(d) => {
				if let Some(pane) = self.session_mut(&d.session_id)
					&& let Some(e) = pane.row_mut(|e| e.outgoing && e.seq == d.seq)
				{
					e.delivery = d.delivery;
					e.failure = d.failure;
				}
			}
			Event::Offer(o) => {
				if let (Some(pane), Some(offer)) = (self.session_mut(&o.session_id), o.offer)
					&& let Some(e) = pane.row_mut(|e| e.ordinal == o.ordinal)
				{
					e.body = Some(Body::File(offer));
				}
			}
			Event::Permissions(p) => {
				if let Some(pane) = self.session_mut(&p.session_id) {
					pane.summary.permissions = p.permissions;
				}
			}
			Event::Pin(p) => {
				if let (Some(pane), Some(m)) = (self.session_mut(&p.session_id), p.message)
					&& let Some(e) = pane.row_mut(|e| e.outgoing == m.outgoing && e.seq == m.seq)
				{
					e.pinned = p.pinned;
				}
			}
			Event::Session(summary) => self.upsert_session(summary),
			Event::Cleared(c) => {
				if c.removed {
					self.sessions
						.retain(|s| s.summary.session_id != c.session_id);
					self.pick_current();
				} else if let Some(pane) = self.session_mut(&c.session_id) {
					pane.rows.clear();
					pane.focus = None;
					pane.hits.clear();
					pane.summary.next_ordinal = c.next_ordinal;
					pane.summary.message_count = 0;
				}
			}
			Event::Evicted(e) => {
				self.sessions
					.retain(|s| s.summary.session_id != e.session_id);
				self.pick_current();
			}
		}
	}
}
