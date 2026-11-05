//! Inline accumulation: bounded emphasis nesting and URL detection with `linkify`.

use linkify::{LinkFinder, LinkKind};

use super::ast::Inline;
use super::url::parse_http;
use crate::limits::{MD_MAX_DEPTH, MD_MAX_NODES};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
	Bold,
	Italic,
	/// Nesting beyond the depth bound: children join the parent.
	Flat,
}

/// Builds one inline run. Counts every node it creates in `nodes`.
pub(super) struct Inlines {
	kinds: Vec<Kind>,
	/// `runs[0]` is the output; each open emphasis adds one run.
	runs: Vec<Vec<Inline>>,
	pending: String,
	pub nodes: usize,
}

fn finder() -> LinkFinder {
	let mut f = LinkFinder::new();
	f.kinds(&[LinkKind::Url]);
	f
}

impl Inlines {
	pub fn new() -> Self {
		Self {
			kinds: Vec::new(),
			runs: vec![Vec::new()],
			pending: String::new(),
			nodes: 0,
		}
	}

	pub fn is_empty(&self) -> bool {
		self.runs[0].is_empty() && self.pending.is_empty()
	}

	/// Literal text; adjacent calls merge before URL detection.
	pub fn text(&mut self, s: &str) {
		self.pending.push_str(s);
	}

	pub fn line_break(&mut self) {
		self.push(Inline::LineBreak);
	}

	pub fn code(&mut self, s: &str) {
		self.push(Inline::Code(s.to_owned()));
	}

	pub fn open(&mut self, bold: bool) {
		self.flush();
		if self.runs.len() > MD_MAX_DEPTH {
			self.kinds.push(Kind::Flat);
			return;
		}
		self.nodes += 1;
		self.kinds
			.push(if bold { Kind::Bold } else { Kind::Italic });
		self.runs.push(Vec::new());
	}

	pub fn close(&mut self) {
		self.flush();
		let Some(kind) = self.kinds.pop() else {
			return;
		};
		if kind == Kind::Flat {
			return;
		}
		let kids = self.runs.pop().unwrap_or_default();
		let node = if kind == Kind::Bold {
			Inline::Bold(kids)
		} else {
			Inline::Italic(kids)
		};
		self.top().push(node);
	}

	/// Finishes the run and resets for the next block. Trailing breaks are dropped.
	pub fn take(&mut self) -> Vec<Inline> {
		while !self.kinds.is_empty() {
			self.close();
		}
		self.flush();
		let mut out = std::mem::take(&mut self.runs[0]);
		while out.last() == Some(&Inline::LineBreak) {
			out.pop();
		}
		out
	}

	/// Drops everything in progress (the caller falls back to plain text).
	pub fn discard(&mut self) {
		self.kinds.clear();
		self.runs.truncate(1);
		self.runs[0].clear();
		self.pending.clear();
	}

	fn top(&mut self) -> &mut Vec<Inline> {
		let last = self.runs.len() - 1;
		&mut self.runs[last]
	}

	fn push(&mut self, node: Inline) {
		self.flush();
		self.nodes += 1;
		self.top().push(node);
	}

	fn push_plain(&mut self, s: &str) {
		if s.is_empty() {
			return;
		}
		if let Some(Inline::Text(t)) = self.top().last_mut() {
			t.push_str(s);
		} else {
			self.nodes += 1;
			self.top().push(Inline::Text(s.to_owned()));
		}
	}

	fn flush(&mut self) {
		if self.pending.is_empty() {
			return;
		}
		let text = std::mem::take(&mut self.pending);
		for (n, line) in text.split('\n').enumerate() {
			if n > 0 {
				self.nodes += 1;
				self.top().push(Inline::LineBreak);
			}
			self.linkified(line);
		}
	}

	fn linkified(&mut self, line: &str) {
		if self.nodes >= MD_MAX_NODES {
			return self.push_plain(line);
		}
		let finder = finder();
		let mut at = 0;
		for found in finder.links(line) {
			if self.nodes >= MD_MAX_NODES {
				break;
			}
			// Only http(s) with a validated host becomes a link; the rest stays text.
			// linkify trims an empty port (`host:/`); that is malformed, not a shorter link.
			let empty_port = line[found.end()..]
				.strip_prefix(':')
				.is_some_and(|r| r.starts_with(['/', '?', '#']));
			if !empty_port && let Some(link) = parse_http(found.as_str()) {
				self.push_plain(&line[at..found.start()]);
				self.nodes += 1;
				self.top().push(Inline::Link(link));
				at = found.end();
			}
		}
		self.push_plain(&line[at..]);
	}
}
