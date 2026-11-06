mod msgkit;

use std::sync::Arc;
use std::time::Duration;

use msgkit::*;
use relay_ui::actions::{Actions, ShellSink};
use relay_ui::emitter::Sink;
use relay_ui::messaging::mock::{ADA_CALL, ALAN_CALL, seed};
use relay_ui::messaging::state::Feed;
use relay_ui::ui::RelayApp;

struct Rig {
	ctx: egui::Context,
	app: RelayApp,
	actions: Actions,
	shell: Arc<FakeShell>,
}

fn rig() -> Rig {
	let shell = Arc::new(FakeShell::default());
	let actions = actions(shell.clone());
	let ctx = egui::Context::default();
	let app = RelayApp::new(actions.clone(), &ctx);
	Rig {
		ctx,
		app,
		actions,
		shell,
	}
}

impl Rig {
	fn frame_with(&mut self, width: f32, events: Vec<egui::Event>) -> (Duration, String) {
		let input = egui::RawInput {
			screen_rect: Some(egui::Rect::from_min_size(
				egui::Pos2::ZERO,
				// A tall window shows the whole panel below a card.
				egui::vec2(width, if width < 600.0 { 800.0 } else { 3000.0 }),
			)),
			events,
			..Default::default()
		};
		let app = &mut self.app;
		let mut out = self.ctx.run_ui(input, |ui| {
			app.begin_frame(ui.ctx());
			app.ui(ui);
		});
		out.textures_delta.clear();
		let mut text = String::new();
		for clipped in &out.shapes {
			collect(&clipped.shape, &mut text);
		}
		let delay = out
			.viewport_output
			.get(&egui::ViewportId::ROOT)
			.map_or(Duration::MAX, |v| v.repaint_delay);
		(delay, text)
	}

	/// Draw a few frames so measured heights settle; returns the last text.
	fn settle(&mut self, width: f32) -> String {
		let mut last = String::new();
		for _ in 0..6 {
			last = self.frame_with(width, vec![]).1;
		}
		last
	}

	/// Run the call of the seeded Ada session and open its chat.
	fn open(&mut self, width: f32) {
		let mut state = relay_ui::mock::seed();
		let mut call = state.calls[0].clone();
		call.call_id = hex::encode([ADA_CALL; 16]);
		call.phase = relay_ui::model::Phase::Connected;
		state.calls = vec![call];
		self.open_with(width, state, ADA_CALL);
	}

	/// Open the chat of the seeded call that has ended, from the history.
	fn open_ended(&mut self, width: f32) {
		let mut state = relay_ui::mock::seed();
		state.calls.clear();
		state.history = vec![relay_ui::model::HistoryEntry {
			id: 1,
			call_id: hex::encode([ALAN_CALL; 16]),
			peer_id: state.peers[0].peer_id.clone(),
			peer_name: "Alan Turing".into(),
			fingerprint_short: String::new(),
			direction: relay_ui::model::HistoryDirection::Incoming,
			outcome: relay_ui::model::HistoryOutcome::Completed,
			started_at_ms: 1,
			duration_ms: 60_000,
		}];
		self.open_with(width, state, ALAN_CALL);
	}

	fn open_with(&mut self, width: f32, state: relay_ui::model::AppState, call: u8) {
		ShellSink {
			shared: self.actions.shared.clone(),
			shell: self.shell.clone(),
		}
		.state(&state);
		self.frame_with(width, vec![]);
		self.app.open_chat(&hex::encode([call; 16]));
	}
}

fn collect(shape: &egui::Shape, out: &mut String) {
	match shape {
		egui::Shape::Text(t) => {
			out.push_str(t.galley.text());
			out.push('\n');
		}
		egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
		_ => {}
	}
}

const WIDTHS: [f32; 2] = [390.0, 900.0];

#[tokio::test]
async fn off_state_explains_and_offers_to_turn_on_in_both_layouts() {
	for width in WIDTHS {
		let mut r = rig();
		r.actions.shared.messages.set_enabled(false); // a deliberate opt-out
		r.open(width);
		let text = r.settle(width);
		assert!(
			text.contains("Messages are off in this app"),
			"{width}: {text}"
		);
		assert!(text.contains("Turn on messages"));
	}
}

#[tokio::test]
async fn every_feed_state_draws_in_both_layouts() {
	let cases: Vec<(Feed, &str)> = vec![
		(Feed::Connecting, "Connecting to the daemon"),
		(
			Feed::Denied,
			"did not grant this app the messages capability",
		),
		(Feed::Offline("daemon down".into()), "daemon down"),
	];
	for width in WIDTHS {
		for (feed, expect) in &cases {
			let mut r = rig();
			r.open(width);
			r.actions.shared.messages.set_enabled(true);
			r.actions.shared.messages.update(|s| s.feed = feed.clone());
			let text = r.settle(width);
			assert!(text.contains(expect), "{width} {feed:?}: {text}");
		}
	}
}

#[tokio::test]
async fn daemon_disabled_and_no_session_states() {
	for width in WIDTHS {
		let mut r = rig();
		r.open(width);
		let hub = r.actions.shared.messages.clone();
		hub.set_enabled(true);
		hub.update(|s| {
			s.feed = Feed::Live;
			s.load_list(&[], Some(&list(vec![], false).settings.unwrap()));
		});
		let text = r.settle(width);
		assert!(text.contains("messages.enabled = false"), "{text}");
		hub.update(|s| s.load_list(&[], Some(&list(vec![], true).settings.unwrap())));
		let text = r.settle(width);
		assert!(
			text.contains("That call has no messaging session"),
			"{text}"
		);
	}
}

#[tokio::test]
async fn live_transcript_shows_markdown_offers_and_compose() {
	for width in WIDTHS {
		let mut r = rig();
		r.open(width);
		seed(&r.actions.shared.messages);
		r.actions.shared.messages.set_enabled(true);
		r.actions.shared.messages.update(|s| s.feed = Feed::Live);
		let text = r.settle(width);
		assert!(text.contains("Ada Lovelace"), "{text}");
		assert!(text.contains("\n\u{e396}\n"), "the Send button: {text}");
		assert!(text.contains("Message"), "the composer hint: {text}");
		assert!(!text.contains("Permissions for this call"), "in the menu");
		// Newest rows are at the bottom: the sending message and the offers are drawn.
		assert!(
			text.contains("Retrying in a moment")
				|| text.contains("install.sh")
				|| text.contains("Download…"),
			"{width}: {text}"
		);
	}
}

#[tokio::test]
async fn ended_read_only_session_offers_export_and_clear() {
	for width in WIDTHS {
		let mut r = rig();
		r.open_ended(width);
		let hub = r.actions.shared.messages.clone();
		seed(&hub);
		hub.set_enabled(true);
		hub.update(|s| {
			s.feed = Feed::Live;
			s.sessions.retain(|x| !x.is_live());
		});
		let text = r.settle(width);
		assert!(text.contains("This call has ended"), "{text}");
		assert!(text.contains("Export JSON"));
		assert!(text.contains("Clear"));
		assert!(!text.contains("Offer file"));
	}
}

#[tokio::test]
async fn permissions_off_disables_compose_with_an_explanation() {
	for width in WIDTHS {
		let mut r = rig();
		r.open(width);
		let hub = r.actions.shared.messages.clone();
		seed(&hub);
		hub.set_enabled(true);
		hub.update(|s| {
			s.feed = Feed::Live;
			for v in &mut s.sessions {
				v.perms.allow_messages = false;
			}
		});
		let text = r.settle(width);
		assert!(
			text.contains("Messages are turned off for this call"),
			"{text}"
		);
		assert!(!text.contains("\n\u{e396}\n"), "no composer: {text}");
	}
}

#[tokio::test]
async fn a_long_history_lays_out_only_the_visible_rows() {
	for width in WIDTHS {
		let mut r = rig();
		r.open_ended(width);
		let hub = r.actions.shared.messages.clone();
		seed(&hub);
		hub.set_enabled(true);
		hub.update(|s| {
			s.feed = Feed::Live;
			s.sessions.retain(|x| !x.is_live());
		});
		let text = r.settle(width);
		let drawn = text.matches("the quick brown fox").count();
		assert!(drawn > 0 && drawn < 60, "{drawn} rows drawn of 3000");
		assert!(text.contains("Message 3000"), "newest is at the bottom");
		assert!(!text.contains("Message 1:"));
	}
}

#[tokio::test]
async fn settled_panel_is_idle() {
	for width in WIDTHS {
		let mut r = rig();
		r.open(width);
		seed(&r.actions.shared.messages);
		r.actions.shared.messages.set_enabled(true);
		r.actions.shared.messages.update(|s| s.feed = Feed::Live);
		for _ in 0..12 {
			r.frame_with(width, vec![]);
		}
		let (delay, _) = r.frame_with(width, vec![]);
		assert_eq!(delay, Duration::MAX, "width {width}");
	}
}

#[tokio::test]
async fn clipboard_flow_needs_a_choice_with_several_calls() {
	for width in WIDTHS {
		let mut r = rig();
		r.open(width);
		let hub = r.actions.shared.messages.clone();
		seed(&hub); // two live calls
		hub.set_enabled(true);
		hub.update(|s| s.feed = Feed::Live);
		r.frame_with(width, vec![]);
		r.app.begin_clipboard_send();
		r.frame_with(width, vec![]);
		let (_, t) = r.frame_with(width, vec![]);
		assert!(t.contains("Reading the clipboard"), "{t}");
		let (_, t) = r.frame_with(width, vec![egui::Event::Paste("deploy now".into())]);
		assert!(t.contains("More than one call is live"), "{t}");
		assert!(
			t.contains("deploy now") && t.contains("Ada Lovelace") && t.contains("Grace Hopper")
		);
		assert!(
			!t.contains("Send to "),
			"no send button before a choice: {t}"
		);
	}
}

#[tokio::test]
async fn one_call_names_the_destination_on_the_confirm_button() {
	let mut r = rig();
	r.open(900.0);
	let hub = r.actions.shared.messages.clone();
	seed(&hub);
	hub.set_enabled(true);
	hub.update(|s| {
		s.feed = Feed::Live;
		s.sessions.retain(|x| x.peer_name != "Grace Hopper");
	});
	r.frame_with(900.0, vec![]);
	r.app.begin_clipboard_send();
	r.frame_with(900.0, vec![]);
	r.frame_with(900.0, vec![]);
	let (_, t) = r.frame_with(900.0, vec![egui::Event::Paste("deploy now".into())]);
	assert!(t.contains("Send to Ada Lovelace"), "{t}");
	assert!(r.shell.copied.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_oversized_clipboard_is_refused_with_no_send_button() {
	let mut r = rig();
	r.open(900.0);
	let hub = r.actions.shared.messages.clone();
	seed(&hub);
	hub.set_enabled(true);
	hub.update(|s| s.feed = Feed::Live);
	r.frame_with(900.0, vec![]);
	r.app.begin_clipboard_send();
	r.frame_with(900.0, vec![]);
	r.frame_with(900.0, vec![]);
	let (_, t) = r.frame_with(900.0, vec![egui::Event::Paste("x".repeat(5000))]);
	assert!(
		t.contains("Nothing was sent") && !t.contains("Send to "),
		"{t}"
	);
}
