//! Keys of the Messages tab. Typing modes (compose box, prompts) take every key, so
//! tab switching and quit only apply while browsing.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use relay_api::pb::transcript_entry::Body;
use relay_api::pb::{OfferState, SessionPermissions};
use relay_messages::limits::MAX_TEXT_BYTES;

use super::prompt::{Prompt, PromptKind};
use super::state::MessagesState;

/// Something the pane asks the daemon (or the clipboard) to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MsgCommand {
	Send {
		session: Vec<u8>,
		text: String,
	},
	Offer {
		session: Vec<u8>,
		path: String,
	},
	Respond {
		session: Vec<u8>,
		offer: Vec<u8>,
		accept: bool,
		dest: String,
	},
	RequestPreview {
		session: Vec<u8>,
		offer: Vec<u8>,
	},
	ApprovePreview {
		session: Vec<u8>,
		offer: Vec<u8>,
	},
	Cancel {
		session: Vec<u8>,
		offer: Vec<u8>,
	},
	Pin {
		session: Vec<u8>,
		outgoing: bool,
		seq: u64,
		pinned: bool,
	},
	Search {
		session: Vec<u8>,
		query: String,
	},
	Export {
		session: Vec<u8>,
		path: String,
	},
	Clear {
		session: Vec<u8>,
	},
	Copy(String),
}

/// What a key did: nothing for the pane (let the global keys look at it) or handled.
pub enum Handled {
	/// Not a Messages key.
	Pass,
	/// Consumed; maybe a command to run, maybe a status line.
	Done(Option<MsgCommand>, Option<String>),
}

fn done(command: Option<MsgCommand>) -> Handled {
	Handled::Done(command, None)
}

fn say(text: impl Into<String>) -> Handled {
	Handled::Done(None, Some(text.into()))
}

fn permissions(state: &MessagesState) -> SessionPermissions {
	state
		.pane()
		.and_then(|p| p.summary.permissions)
		.unwrap_or_default()
}

fn cwd_text() -> String {
	std::env::current_dir()
		.ok()
		.and_then(|p| p.to_str().map(str::to_owned))
		.unwrap_or_default()
}

/// Handle one key on the Messages tab.
pub fn handle(state: &mut MessagesState, key: KeyEvent) -> Handled {
	let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
	if ctrl && key.code == KeyCode::Char('y') {
		return copy_block(state);
	}
	if state.prompt.is_some() {
		return prompting(state, key);
	}
	if state.composing {
		return composing(state, key);
	}
	if ctrl {
		return Handled::Pass;
	}
	browse(state, key.code)
}

fn copy_block(state: &MessagesState) -> Handled {
	let Some(row) = state.pane().and_then(|p| p.focused().map(|r| (r, p.block))) else {
		return say("select a message with a code block first");
	};
	match row.0.block_text(row.1) {
		Some(text) => done(Some(MsgCommand::Copy(text))),
		None => say("the selected message has no code block"),
	}
}

fn prompting(state: &mut MessagesState, key: KeyEvent) -> Handled {
	let Some(prompt) = state.prompt.as_mut() else {
		return Handled::Pass;
	};
	if prompt.kind == PromptKind::ConfirmClear {
		let yes = matches!(key.code, KeyCode::Char('y' | 'Y'));
		let session = state.pane().map(|p| p.summary.session_id.clone());
		state.prompt = None;
		return match (yes, session) {
			(true, Some(session)) => done(Some(MsgCommand::Clear { session })),
			_ => say("not cleared"),
		};
	}
	match key.code {
		KeyCode::Esc => {
			state.prompt = None;
			Handled::Done(None, None)
		}
		KeyCode::Backspace => {
			prompt.input.pop();
			Handled::Done(None, None)
		}
		KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
			prompt.input.push(c);
			Handled::Done(None, None)
		}
		KeyCode::Enter => submit(state),
		_ => Handled::Done(None, None),
	}
}

fn submit(state: &mut MessagesState) -> Handled {
	let (Some(prompt), Some(session)) = (
		state.prompt.take(),
		state.pane().map(|p| p.summary.session_id.clone()),
	) else {
		return Handled::Done(None, None);
	};
	let input = prompt.input.trim().to_owned();
	if input.is_empty() {
		return say("nothing entered");
	}
	done(Some(match prompt.kind {
		PromptKind::Search => MsgCommand::Search {
			session,
			query: input,
		},
		PromptKind::OfferPath => MsgCommand::Offer {
			session,
			path: input,
		},
		PromptKind::AcceptInto { offer } => MsgCommand::Respond {
			session,
			offer,
			accept: true,
			dest: input,
		},
		PromptKind::ExportPath => MsgCommand::Export {
			session,
			path: input,
		},
		PromptKind::ConfirmClear => return Handled::Done(None, None),
	}))
}

fn composing(state: &mut MessagesState, key: KeyEvent) -> Handled {
	match key.code {
		KeyCode::Esc => {
			state.composing = false;
			Handled::Done(None, None)
		}
		KeyCode::Enter => send(state),
		KeyCode::Backspace => {
			state.compose.pop();
			Handled::Done(None, None)
		}
		KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
			state.compose.push('\n');
			Handled::Done(None, None)
		}
		KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
			if state.compose.len() + c.len_utf8() <= MAX_TEXT_BYTES {
				state.compose.push(c);
				Handled::Done(None, None)
			} else {
				say(format!("a message holds at most {MAX_TEXT_BYTES} bytes"))
			}
		}
		_ => Handled::Done(None, None),
	}
}

/// Why the compose box cannot send right now, if it cannot.
pub fn send_blocker(state: &MessagesState) -> Option<&'static str> {
	let pane = state.pane()?;
	if !pane.active() {
		Some("the call has ended: the transcript is read-only")
	} else if !permissions(state).allow_messages {
		Some("messages are turned off for this call")
	} else {
		None
	}
}

fn send(state: &mut MessagesState) -> Handled {
	if let Some(why) = send_blocker(state) {
		return say(why);
	}
	let Some(session) = state.pane().map(|p| p.summary.session_id.clone()) else {
		return say("there is no call session");
	};
	if state.compose.trim().is_empty() {
		return say("type a message first");
	}
	done(Some(MsgCommand::Send {
		session,
		text: state.compose.clone(),
	}))
}

fn browse(state: &mut MessagesState, code: KeyCode) -> Handled {
	let Some(pane) = state.pane_mut() else {
		return match code {
			KeyCode::Char('[' | ']') => Handled::Done(None, None),
			_ => Handled::Pass,
		};
	};
	let len = pane.rows.len();
	match code {
		KeyCode::Char('j') | KeyCode::Down => {
			pane.focus = match pane.focus {
				Some(f) if f + 1 < len => Some(f + 1),
				Some(_) => None,
				None => None,
			};
			pane.block = 0;
			Handled::Done(None, None)
		}
		KeyCode::Char('k') | KeyCode::Up => {
			pane.focus = match pane.focus {
				Some(f) => Some(f.saturating_sub(1)),
				None => len.checked_sub(1),
			};
			pane.block = 0;
			Handled::Done(None, None)
		}
		KeyCode::Char('g') => {
			pane.focus = (len > 0).then_some(0);
			Handled::Done(None, None)
		}
		KeyCode::Char('G') => {
			pane.focus = None;
			Handled::Done(None, None)
		}
		KeyCode::Char('b') => {
			let blocks = pane.focused().map_or(0, |r| r.blocks());
			pane.block = if blocks == 0 {
				0
			} else {
				(pane.block + 1) % blocks
			};
			Handled::Done(None, None)
		}
		KeyCode::Char('n') => jump(state, true),
		KeyCode::Char('N') => jump(state, false),
		KeyCode::Char('[') => {
			state.cycle(-1);
			Handled::Done(None, None)
		}
		KeyCode::Char(']') => {
			state.cycle(1);
			Handled::Done(None, None)
		}
		KeyCode::Char('i') | KeyCode::Enter => {
			if let Some(why) = send_blocker(state) {
				return say(why);
			}
			state.composing = true;
			Handled::Done(None, None)
		}
		KeyCode::Char('/') => open(state, PromptKind::Search, ""),
		KeyCode::Char('p') => pin(state),
		KeyCode::Char('o') => offer_prompt(state),
		KeyCode::Char('e') => open(state, PromptKind::ExportPath, ""),
		KeyCode::Char('X') => open(state, PromptKind::ConfirmClear, ""),
		KeyCode::Char('a') => accept(state),
		KeyCode::Char(c @ ('r' | 'c' | 'v')) => offer_action(state, c),
		KeyCode::Char('y') => copy_block(state),
		_ => Handled::Pass,
	}
}

fn open(state: &mut MessagesState, kind: PromptKind, initial: &str) -> Handled {
	if state.pane().is_none() {
		return say("there is no call session");
	}
	state.prompt = Some(Prompt::new(kind, initial));
	Handled::Done(None, None)
}

fn jump(state: &mut MessagesState, forward: bool) -> Handled {
	let Some(pane) = state.pane_mut() else {
		return Handled::Done(None, None);
	};
	if pane.hits.is_empty() {
		return say("no search results; press / to search");
	}
	let current = pane.focused().map_or(0, |r| r.entry.ordinal);
	let target = if forward {
		pane.hits
			.iter()
			.find(|h| **h > current)
			.or(pane.hits.first())
	} else {
		pane.hits
			.iter()
			.rev()
			.find(|h| **h < current)
			.or(pane.hits.last())
	};
	if let Some(ordinal) = target.copied() {
		pane.focus = pane.rows.iter().position(|r| r.entry.ordinal == ordinal);
		pane.block = 0;
	}
	Handled::Done(None, None)
}

fn pin(state: &mut MessagesState) -> Handled {
	let Some(pane) = state.pane() else {
		return say("there is no call session");
	};
	let Some(row) = pane.focused() else {
		return say("select a message first");
	};
	done(Some(MsgCommand::Pin {
		session: pane.summary.session_id.clone(),
		outgoing: row.entry.outgoing,
		seq: row.entry.seq,
		pinned: !row.entry.pinned,
	}))
}

fn offer_prompt(state: &mut MessagesState) -> Handled {
	let Some(pane) = state.pane() else {
		return say("there is no call session");
	};
	if !pane.active() {
		return say("the call has ended: nothing can be offered");
	}
	if !permissions(state).allow_files {
		return say("file transfers are turned off for this call");
	}
	open(state, PromptKind::OfferPath, "")
}

fn focused_offer(state: &MessagesState) -> Option<(Vec<u8>, relay_api::pb::FileOfferBody)> {
	let pane = state.pane()?;
	match &pane.focused()?.entry.body {
		Some(Body::File(o)) => Some((pane.summary.session_id.clone(), o.clone())),
		_ => None,
	}
}

fn accept(state: &mut MessagesState) -> Handled {
	let Some((_, offer)) = focused_offer(state) else {
		return say("select a file offer first");
	};
	if offer.outgoing || offer.state != OfferState::Offered as i32 {
		return say("only an open incoming offer can be accepted");
	}
	if !permissions(state).allow_files {
		return say("file transfers are turned off for this call");
	}
	open(
		state,
		PromptKind::AcceptInto {
			offer: offer.offer_id,
		},
		&cwd_text(),
	)
}

fn offer_action(state: &mut MessagesState, key: char) -> Handled {
	let Some((session, offer)) = focused_offer(state) else {
		return say("select a file offer first");
	};
	let open_offer = offer.state == OfferState::Offered as i32;
	let id = offer.offer_id.clone();
	match key {
		'r' if !offer.outgoing && open_offer => done(Some(MsgCommand::Respond {
			session,
			offer: id,
			accept: false,
			dest: String::new(),
		})),
		'c' if open_offer || offer.state == OfferState::InProgress as i32 => {
			done(Some(MsgCommand::Cancel { session, offer: id }))
		}
		'v' if offer.preview_approval_pending => {
			done(Some(MsgCommand::ApprovePreview { session, offer: id }))
		}
		'v' if !offer.outgoing && open_offer && !offer.preview_requested => {
			done(Some(MsgCommand::RequestPreview { session, offer: id }))
		}
		_ => say("that does not apply to this offer"),
	}
}
