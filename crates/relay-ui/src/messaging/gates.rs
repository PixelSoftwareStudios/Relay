//! Two confirmation rules that must never be bypassed: sending the clipboard
//! to a call, and opening a link.

use crate::tr;
use relay_messages::markdown::{HostWarning, Link, parse_http};

/// A live call that could receive the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
	pub session: String,
	pub peer_name: String,
}

/// What the clipboard flow may do right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
	/// No live call to send to.
	NoCall,
	/// Several calls are live and none is chosen: the user must pick one.
	MustPick,
	/// Show the destination and wait for the confirm button.
	Confirm(Destination),
}

/// Resolve the destination. With exactly one live call it is the target (and
/// is still shown and confirmed); with more than one there is no default.
pub fn clipboard_target(live: &[Destination], picked: Option<usize>) -> Target {
	match (live.len(), picked) {
		(0, _) => Target::NoCall,
		(1, _) => Target::Confirm(live[0].clone()),
		(_, Some(i)) if i < live.len() => Target::Confirm(live[i].clone()),
		_ => Target::MustPick,
	}
}

/// Why clipboard text cannot be sent at all (never truncated silently).
pub fn clipboard_problem(text: &str) -> Option<String> {
	if text.trim().is_empty() {
		return Some(tr!("clipboard-empty").to_owned());
	}
	let n = text.len();
	let limit = relay_messages::limits::MAX_TEXT_BYTES;
	(n > limit).then(|| tr!("clipboard-too-long", size = n, limit = limit))
}

/// A link waiting for the user's decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkPrompt {
	pub url: String,
	/// The host the browser will really connect to (lowercase ASCII).
	pub host: String,
	/// The decoded display form, only different for internationalised hosts.
	pub display: String,
	pub warning: Option<HostWarning>,
}

impl LinkPrompt {
	pub fn new(link: &Link) -> Self {
		Self {
			url: link.url.clone(),
			host: link.host.ascii.clone(),
			display: link.host.display.clone(),
			warning: link.host.warning,
		}
	}

	pub fn warning_text(&self) -> Option<&'static str> {
		self.warning.map(|w| match w {
			HostWarning::Idn => tr!("link-warn-idn"),
			HostWarning::MixedScript => {
				tr!("link-warn-mixed")
			}
			HostWarning::InvalidPunycode => tr!("link-warn-punycode"),
		})
	}
}

/// Holds at most one pending link. A URL leaves this type only through
/// [`LinkGate::confirm`], after [`LinkGate::ask`] showed its host.
#[derive(Debug, Default)]
pub struct LinkGate {
	pending: Option<LinkPrompt>,
}

impl LinkGate {
	pub fn ask(&mut self, link: &Link) {
		self.pending = Some(LinkPrompt::new(link));
	}

	pub fn pending(&self) -> Option<&LinkPrompt> {
		self.pending.as_ref()
	}

	pub fn cancel(&mut self) {
		self.pending = None;
	}

	/// The URL to open, once, and only if one was asked about and it still
	/// parses as a plain http(s) link with the host that was shown.
	pub fn confirm(&mut self) -> Option<String> {
		let prompt = self.pending.take()?;
		let link = parse_http(&prompt.url)?;
		(link.host.ascii == prompt.host).then_some(prompt.url)
	}
}
