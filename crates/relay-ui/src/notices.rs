//! The notices strip's model: which warnings and passing notes are showing,
//! which the user dismissed on this client, and which fade away on their own.
//!
//! A notice is known by its identity, a hash of the engine code and message.
//! The dismissed set lives in UI memory only. An entry leaves it when its
//! notice is no longer there (the engine withdrew it, or the connection was
//! lost), so a warning raised again later shows again, while the same
//! unchanged warning does not return on every state refresh.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use crate::model::AppState;
use crate::notice_words::{Severity, words};

/// Notices drawn before the "+N more" expander.
pub const MAX_VISIBLE: usize = 3;
/// How long an informational notice stays, unless the pointer rests on it.
pub const INFO_LIFETIME_MS: u64 = 10_000;

/// The engine's code for a device that waits for a trust decision. Trust is
/// decided in Connect, so it is never a notice.
const TRUST_REQUIRED: &str = "TRUST_REQUIRED";
/// The code the app's own passing note (`AppState::notice`) is known by.
const NOTE: &str = "NOTE";

/// One line of the strip: one or more identical engine messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
	/// The first identity of the group; keys the strip's widgets.
	pub id: u64,
	/// Every identity the line stands for; dismissing it dismisses them all.
	pub ids: Vec<u64>,
	pub severity: Severity,
	pub title: String,
	pub next_step: Option<String>,
	pub details: Option<String>,
	/// How many identical messages were folded into this line.
	pub count: usize,
}

/// The identity of an engine warning.
pub fn identity(code: &str, message: &str) -> u64 {
	let mut hasher = std::collections::hash_map::DefaultHasher::new();
	(code, message).hash(&mut hasher);
	hasher.finish()
}

/// The identity of the app's passing note `text`.
pub fn note_identity(text: &str) -> u64 {
	identity(NOTE, text)
}

/// Dismissals, fade timers and the expander of the strip.
#[derive(Debug, Default)]
pub struct Board {
	dismissed: HashSet<u64>,
	/// When an informational notice started to count down (unix ms).
	seen: HashMap<u64, u64>,
	/// "+N more" was opened.
	pub expanded: bool,
}

impl Board {
	/// Forget what no longer applies and let old informational notices go.
	/// Call once per frame with the current time, before [`Board::visible`].
	/// An informational notice counts down only while `drawn` (the strip is on
	/// screen), so a note is not lost while the user is on another page.
	pub fn sync(&mut self, state: &AppState, now_ms: u64, drawn: bool) {
		let entries = entries(state);
		let present: HashSet<u64> = entries.iter().map(|e| e.0).collect();
		self.dismissed.retain(|id| present.contains(id));
		self.seen.retain(|id, _| present.contains(id));
		for (id, words) in &entries {
			if words.severity != Severity::Info || self.dismissed.contains(id) {
				continue;
			}
			let since = *self.seen.entry(*id).or_insert(now_ms);
			if !drawn {
				self.seen.insert(*id, now_ms);
			} else if now_ms.saturating_sub(since) >= INFO_LIFETIME_MS {
				self.dismissed.insert(*id);
			}
		}
	}

	/// What the strip shows now: not dismissed, identical messages folded,
	/// the most severe first.
	pub fn visible(&self, state: &AppState) -> Vec<Notice> {
		let mut out: Vec<Notice> = Vec::new();
		for (id, w) in entries(state) {
			if self.dismissed.contains(&id) {
				continue;
			}
			let same = |n: &&mut Notice| {
				n.severity == w.severity && n.title == w.title && n.next_step == w.next_step
			};
			match out.iter_mut().find(same) {
				Some(n) => {
					n.ids.push(id);
					n.count += 1;
				}
				None => out.push(Notice {
					id,
					ids: vec![id],
					severity: w.severity,
					title: w.title,
					next_step: w.next_step,
					details: w.details,
					count: 1,
				}),
			}
		}
		out.sort_by_key(|n| std::cmp::Reverse(n.severity));
		out
	}

	/// The user dismissed these identities.
	pub fn dismiss(&mut self, ids: &[u64]) {
		self.dismissed.extend(ids);
	}

	/// The pointer rests on these notices: their countdown starts over.
	pub fn hold(&mut self, ids: &[u64], now_ms: u64) {
		for id in ids {
			if let Some(since) = self.seen.get_mut(id) {
				*since = now_ms;
			}
		}
	}

	/// Whether the notice with this identity is dismissed or has faded.
	pub fn is_dismissed(&self, id: u64) -> bool {
		self.dismissed.contains(&id)
	}

	/// Milliseconds until the next informational notice among `shown` fades;
	/// `None` when nothing is counting down (then nothing needs a repaint).
	pub fn next_fade_ms(&self, shown: &[Notice], now_ms: u64) -> Option<u64> {
		shown
			.iter()
			.flat_map(|n| &n.ids)
			.filter_map(|id| self.seen.get(id))
			.map(|since| (since + INFO_LIFETIME_MS).saturating_sub(now_ms))
			.min()
	}
}

/// Every notice of the state with its words, the engine's first.
fn entries(state: &AppState) -> Vec<(u64, crate::notice_words::Words)> {
	let engine = state
		.warnings
		.iter()
		.filter(|w| w.code != TRUST_REQUIRED)
		.map(|w| (identity(&w.code, &w.message), words(w)));
	let note = state.notice.iter().map(|text| {
		let words = crate::notice_words::Words {
			severity: Severity::Info,
			title: text.clone(),
			next_step: None,
			details: None,
		};
		(note_identity(text), words)
	});
	engine.chain(note).collect()
}
