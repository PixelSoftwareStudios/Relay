//! The Messages tab: state from feed updates, keys, rendering on a TestBackend, and the
//! drive loop with a messages connection.

mod common;

use common::chat::{SESSION_A, SESSION_OLD};
use common::{FakeControl, MemoryClipboard};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use relay_api::pb::session_event::Event as Ev;
use relay_api::pb::*;
use relay_cli::client::Update;
use relay_cli::tui::keys::handle_key;
use relay_cli::tui::messages::{Link, MsgCommand, MsgUpdate};
use relay_cli::tui::view::draw;
use relay_cli::tui::{MessagesPort, Model, Tab, TuiCommand, drive_with};
use tokio::sync::mpsc;

fn key(c: char) -> KeyEvent {
	KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn code(code: KeyCode) -> KeyEvent {
	KeyEvent::new(code, KeyModifiers::NONE)
}

fn screen(model: &Model, width: u16, height: u16) -> String {
	let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
	terminal.draw(|f| draw(f, model)).unwrap();
	terminal
		.backend()
		.buffer()
		.content()
		.chunks(usize::from(width))
		.map(|row| {
			row.iter()
				.map(|c| c.symbol())
				.collect::<String>()
				.trim_end()
				.to_owned()
		})
		.collect::<Vec<_>>()
		.join("\n")
}

/// A model on the Messages tab showing the fake's sessions and transcripts.
fn loaded(c: &FakeControl) -> Model {
	let mut m = Model::new();
	m.tab = Tab::Messages;
	let list = c.chat.list();
	m.apply(Update::Messages(Box::new(MsgUpdate::Sessions(
		list.sessions,
		list.settings,
	))));
	for s in &c.chat.sessions {
		m.apply(Update::Messages(Box::new(MsgUpdate::Transcript(
			s.summary.session_id.clone(),
			s.entries.clone(),
		))));
	}
	m.apply(Update::Messages(Box::new(MsgUpdate::Link(Link::Live))));
	m
}

fn chat() -> FakeControl {
	let mut c = FakeControl::new();
	c.chat.incoming_text(&SESSION_A, "hello from Alice");
	c.chat.send_text(&SESSION_A, "hi back");
	c.chat.incoming_text(
		&SESSION_A,
		"run this\n```bash\nls -la\n```\nvia https://example.com/a",
	);
	c
}

#[test]
fn the_transcript_shows_times_delivery_pins_code_blocks_and_links() {
	let mut c = chat();
	c.chat.pin(&SESSION_A, false, 1, true);
	let m = loaded(&c);
	let text = screen(&m, 90, 22);
	assert!(
		text.contains("Alice (call)") && text.contains("link: live"),
		"{text}"
	);
	assert!(
		text.contains("22:13:21Z") && text.contains("Alice"),
		"{text}"
	);
	assert!(text.contains("you") && text.contains("sending"), "{text}");
	assert!(text.contains("hello from Alice"), "{text}");
	assert!(
		text.contains("+- code 1: bash") && text.contains("| ls -la"),
		"{text}"
	);
	assert!(text.contains("https://example.com/a"), "{text}");
	assert!(text.contains("messages on  files on"), "{text}");
}

#[test]
fn a_lookalike_link_is_flagged_with_its_real_host() {
	let mut c = FakeControl::new();
	c.chat
		.incoming_text(&SESSION_A, "open https://xn--80ak6aa92e.com/ now");
	let text = screen(&loaded(&c), 170, 14);
	assert!(
		text.contains("WARNING") && text.contains("real host xn--80ak6aa92e.com"),
		"{text}"
	);
}

#[test]
fn offers_show_progress_warnings_and_the_actions_for_the_focused_one() {
	let mut c = FakeControl::new();
	let id = c
		.chat
		.incoming_offer(&SESSION_A, "build.exe", 4 * 1024 * 1024);
	c.chat.update_offer(&SESSION_A, &id, |o| {
		o.state = OfferState::InProgress as i32;
		o.transferred_bytes = 2 * 1024 * 1024;
		o.executable_warning = true;
	});
	let mut m = loaded(&c);
	handle_key(&mut m, key('k'));
	let text = screen(&m, 100, 16);
	assert!(
		text.contains("file \"build.exe\" 4.0 MiB incoming in-progress"),
		"{text}"
	);
	assert!(text.contains("[############............] 50%"), "{text}");
	assert!(text.contains("looks executable"), "{text}");
	assert!(text.contains("c cancel"), "{text}");
}

#[test]
fn compose_enter_sends_and_the_text_is_kept_until_it_went_through() {
	let c = chat();
	let mut m = loaded(&c);
	assert_eq!(handle_key(&mut m, key('i')), None);
	for ch in "hey".chars() {
		handle_key(&mut m, key(ch));
	}
	// Typing mode owns every key: no tab switching, no quit.
	handle_key(&mut m, key('q'));
	handle_key(&mut m, key('3'));
	assert!(!m.quit && m.tab == Tab::Messages);
	assert_eq!(m.messages.compose, "heyq3");
	handle_key(&mut m, code(KeyCode::Backspace));
	handle_key(&mut m, code(KeyCode::Backspace));
	let cmd = handle_key(&mut m, code(KeyCode::Enter));
	assert_eq!(
		cmd,
		Some(TuiCommand::Messages(MsgCommand::Send {
			session: SESSION_A.to_vec(),
			text: "hey".into()
		}))
	);
	assert_eq!(
		m.messages.compose, "hey",
		"dropped only after the daemon accepted it"
	);
	handle_key(&mut m, code(KeyCode::Esc));
	assert!(!m.messages.composing);
	assert_eq!(handle_key(&mut m, key('q')), None);
	assert!(m.quit);
}

#[test]
fn honest_states_block_composing_and_say_why() {
	let mut c = chat();
	c.chat.permissions(&SESSION_A, Some(false), None);
	let mut m = loaded(&c);
	handle_key(&mut m, key('i'));
	assert!(!m.messages.composing);
	assert!(m.status.contains("turned off"), "{}", m.status);
	let text = screen(&m, 100, 16);
	assert!(
		text.contains("messages are turned off for this call"),
		"{text}"
	);

	let mut c = FakeControl::new();
	c.chat.sessions.clear();
	c.chat.add_session(SESSION_OLD, [0xc2; 16], "Bob", false);
	c.chat.incoming_text(&SESSION_OLD, "left over");
	let mut m = loaded(&c);
	handle_key(&mut m, key('i'));
	assert!(
		!m.messages.composing && m.status.contains("read-only"),
		"{}",
		m.status
	);
	let text = screen(&m, 100, 16);
	assert!(
		text.contains("Bob (ended)") && text.contains("read-only transcript"),
		"{text}"
	);
	assert!(text.contains("left over"), "history stays readable: {text}");
	assert!(handle_key(&mut m, key('o')).is_none() && m.status.contains("ended"));
}

#[test]
fn a_full_budget_offers_export_and_clear() {
	let mut c = FakeControl::new();
	c.chat.sessions[0].summary.used_bytes = c.chat.sessions[0].summary.limit_bytes;
	let text = screen(&loaded(&c), 100, 14);
	assert!(
		text.contains("budget full") && text.contains("e exports") && text.contains("X empties"),
		"{text}"
	);
}

#[test]
fn no_session_unavailable_and_offline_are_each_stated() {
	let mut m = Model::new();
	m.tab = Tab::Messages;
	assert!(screen(&m, 80, 12).contains("connecting to the messages stream"));
	m.apply(Update::Messages(Box::new(MsgUpdate::Sessions(
		vec![],
		None,
	))));
	m.apply(Update::Messages(Box::new(MsgUpdate::Link(Link::Live))));
	assert!(screen(&m, 80, 12).contains("No call session"));
	m.apply(Update::Messages(Box::new(MsgUpdate::Link(
		Link::Unavailable("permission denied".into()),
	))));
	assert!(screen(&m, 80, 12).contains("Messages are unavailable: permission denied"));
	let c = chat();
	let mut m = loaded(&c);
	m.apply(Update::Messages(Box::new(MsgUpdate::Link(
		Link::Resyncing("lagged".into()),
	))));
	assert!(screen(&m, 100, 12).contains("resyncing (lagged)"));
	m.apply(Update::Messages(Box::new(MsgUpdate::Link(Link::Lost(
		"gone".into(),
	)))));
	assert!(screen(&m, 100, 12).contains("offline (gone)"));
}

#[test]
fn events_update_the_pane_and_a_resync_reload_merges_without_duplicates() {
	let mut c = chat();
	let mut m = loaded(&c);
	let ev = |e| {
		Update::Messages(Box::new(MsgUpdate::Event(Box::new(SessionEvent {
			event: Some(e),
		}))))
	};
	let fresh = c.chat.incoming_text(&SESSION_A, "newest");
	m.apply(ev(Ev::Entry(NewEntry {
		session_id: SESSION_A.to_vec(),
		entry: Some(fresh),
	})));
	m.apply(ev(Ev::Delivery(DeliveryChanged {
		session_id: SESSION_A.to_vec(),
		seq: 1,
		delivery: DeliveryState::Failed as i32,
		failure: "timeout".into(),
	})));
	let text = screen(&m, 100, 24);
	assert!(
		text.contains("newest") && text.contains("FAILED (timeout)"),
		"{text}"
	);
	// Reloading the whole transcript does not duplicate rows.
	let all = c.chat.sessions[0].entries.clone();
	m.apply(Update::Messages(Box::new(MsgUpdate::Transcript(
		SESSION_A.to_vec(),
		all,
	))));
	assert_eq!(m.messages.pane().unwrap().rows.len(), 4);
	m.apply(ev(Ev::Cleared(SessionCleared {
		session_id: SESSION_A.to_vec(),
		removed: false,
		next_ordinal: 9,
	})));
	assert!(m.messages.pane().unwrap().rows.is_empty());
}

#[test]
fn rows_stay_bounded() {
	let mut c = FakeControl::new();
	for i in 0..1100 {
		c.chat.incoming_text(&SESSION_A, &format!("m{i}"));
	}
	let m = loaded(&c);
	assert_eq!(
		m.messages.pane().unwrap().rows.len(),
		relay_cli::tui::messages::MAX_ROWS
	);
}

#[test]
fn browse_keys_pin_search_offer_accept_and_copy() {
	let mut c = chat();
	let offer = c.chat.incoming_offer(&SESSION_A, "x.txt", 10);
	let mut m = loaded(&c);
	// Newest is the offer; focus it.
	handle_key(&mut m, key('k'));
	let accept = handle_key(&mut m, key('a'));
	assert_eq!(accept, None, "asks where to save first");
	let prompt = m.messages.prompt.clone().expect("destination prompt");
	assert_eq!(prompt.label(), "save into directory");
	assert!(
		!prompt.input.is_empty(),
		"defaults to the current directory"
	);
	let cmd = handle_key(&mut m, code(KeyCode::Enter));
	assert!(
		matches!(cmd, Some(TuiCommand::Messages(MsgCommand::Respond { accept: true, offer: ref o, .. })) if *o == offer)
	);
	assert_eq!(
		handle_key(&mut m, key('r')),
		Some(TuiCommand::Messages(MsgCommand::Respond {
			session: SESSION_A.to_vec(),
			offer: offer.clone(),
			accept: false,
			dest: String::new()
		}))
	);
	// Pin the code message and copy its block.
	handle_key(&mut m, key('k'));
	let pin = handle_key(&mut m, key('p'));
	assert!(matches!(
		pin,
		Some(TuiCommand::Messages(MsgCommand::Pin {
			outgoing: false,
			seq: 2,
			pinned: true,
			..
		}))
	));
	let copy = handle_key(
		&mut m,
		KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
	);
	assert_eq!(
		copy,
		Some(TuiCommand::Messages(MsgCommand::Copy("ls -la".into())))
	);
	// Offer prompt and search prompt.
	handle_key(&mut m, key('o'));
	for ch in "/tmp/a.bin".chars() {
		handle_key(&mut m, key(ch));
	}
	let cmd = handle_key(&mut m, code(KeyCode::Enter));
	assert_eq!(
		cmd,
		Some(TuiCommand::Messages(MsgCommand::Offer {
			session: SESSION_A.to_vec(),
			path: "/tmp/a.bin".into()
		}))
	);
	handle_key(&mut m, key('/'));
	for ch in "hello".chars() {
		handle_key(&mut m, key(ch));
	}
	assert!(matches!(
		handle_key(&mut m, code(KeyCode::Enter)),
		Some(TuiCommand::Messages(MsgCommand::Search { ref query, .. })) if query == "hello"
	));
	handle_key(&mut m, key('X'));
	assert_eq!(
		handle_key(&mut m, key('n')),
		None,
		"any other key keeps the transcript"
	);
	assert!(m.messages.prompt.is_none());
	handle_key(&mut m, key('X'));
	assert!(matches!(
		handle_key(&mut m, key('y')),
		Some(TuiCommand::Messages(MsgCommand::Clear { .. }))
	));
}

#[tokio::test]
async fn drive_runs_messages_commands_on_the_messages_connection() {
	let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
	let mut control = FakeControl::new();
	let mut messaging = FakeControl::new();
	let mut clipboard = MemoryClipboard::default();
	let mut model = Model::new();
	model.tab = Tab::Messages;
	let (utx, mut updates) = mpsc::channel(8);
	let (etx, mut events) = mpsc::channel(8);
	let list = messaging.chat.list();
	utx.send(Update::Messages(Box::new(MsgUpdate::Sessions(
		list.sessions,
		list.settings,
	))))
	.await
	.unwrap();
	let feeder = tokio::spawn(async move {
		tokio::time::sleep(std::time::Duration::from_millis(50)).await;
		for e in [
			key('i'),
			key('h'),
			key('i'),
			code(KeyCode::Enter),
			code(KeyCode::Esc),
			key('q'),
		] {
			etx.send(Event::Key(e)).await.unwrap();
		}
	});
	drive_with(
		&mut terminal,
		&mut control,
		Some(MessagesPort {
			client: &mut messaging,
			clipboard: &mut clipboard,
		}),
		&mut model,
		&mut updates,
		&mut events,
	)
	.await
	.unwrap();
	feeder.await.unwrap();
	assert_eq!(messaging.chat.log, ["send_text bytes=2"]);
	assert!(
		control.log.is_empty(),
		"the main connection never carries messages: {:?}",
		control.log
	);
	assert!(
		model.messages.compose.is_empty(),
		"sent text leaves the box"
	);
}

#[tokio::test]
async fn without_a_messages_connection_commands_say_so() {
	let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
	let mut control = FakeControl::new();
	let mut model = Model::new();
	model.tab = Tab::Messages;
	let c = chat();
	let list = c.chat.list();
	model.apply(Update::Messages(Box::new(MsgUpdate::Sessions(
		list.sessions,
		list.settings,
	))));
	let (_utx, mut updates) = mpsc::channel(8);
	let (etx, mut events) = mpsc::channel(8);
	for e in [key('i'), key('x'), code(KeyCode::Enter)] {
		etx.send(Event::Key(e)).await.unwrap();
	}
	drop(etx);
	relay_cli::tui::drive(
		&mut terminal,
		&mut control,
		&mut model,
		&mut updates,
		&mut events,
	)
	.await
	.unwrap();
	assert!(model.status.contains("unavailable"), "{}", model.status);
}
