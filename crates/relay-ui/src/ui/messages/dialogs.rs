//! Confirmation dialogs: opening a link, sending the clipboard to a call, and
//! clearing a transcript. None of them acts without a button press.

use egui::{Align2, RichText, Ui};

use crate::messaging::actions::Intent;
use crate::messaging::gates::{Destination, LinkGate, Target, clipboard_problem, clipboard_target};
use crate::messaging::state::MessagesState;
use crate::theme;

/// Where the "Send clipboard to call" flow stands.
#[derive(Debug, Default)]
pub enum ClipFlow {
	#[default]
	Idle,
	/// The paste was requested; the text arrives as an egui event.
	Awaiting,
	/// The text is here; the user chooses and confirms the destination.
	Ready { text: String, picked: Option<usize> },
}

fn window(title: &str) -> egui::Window<'static> {
	egui::Window::new(title.to_owned())
		.collapsible(false)
		.resizable(false)
		.anchor(Align2::CENTER_CENTER, [0.0, 0.0])
}

/// The paste text delivered this frame, if any.
pub fn take_paste(ui: &Ui) -> Option<String> {
	ui.input_mut(|i| {
		let at = i
			.events
			.iter()
			.position(|e| matches!(e, egui::Event::Paste(_)))?;
		match i.events.remove(at) {
			egui::Event::Paste(t) => Some(t),
			_ => None,
		}
	})
}

pub fn link(ui: &mut Ui, gate: &mut LinkGate, intents: &mut Vec<Intent>) {
	let Some(prompt) = gate.pending().cloned() else {
		return;
	};
	let (mut open, mut cancel) = (false, false);
	window("Open this link?").show(ui.ctx(), |ui| {
		ui.label("Your browser will connect to:");
		ui.label(RichText::new(&prompt.host).strong().monospace().size(18.0));
		if prompt.display != prompt.host {
			ui.label(
				RichText::new(format!("It displays as: {}", prompt.display)).color(theme::WARN),
			);
		}
		if let Some(w) = prompt.warning_text() {
			ui.label(RichText::new(w).color(theme::ERROR).strong());
		}
		ui.add(egui::Label::new(RichText::new(&prompt.url).monospace().small()).selectable(true));
		ui.label(
			RichText::new("Relay never opens links by itself and never fetches them.")
				.small()
				.weak(),
		);
		ui.horizontal(|ui| {
			open = ui.button("Open in browser").clicked();
			cancel = ui.button("Cancel").clicked();
		});
	});
	if open {
		if let Some(url) = gate.confirm() {
			intents.push(Intent::OpenUrl(url));
		}
	} else if cancel {
		gate.cancel();
	}
}

pub fn clipboard(
	ui: &mut Ui,
	flow: &mut ClipFlow,
	state: &MessagesState,
	intents: &mut Vec<Intent>,
) {
	match flow {
		ClipFlow::Idle => {}
		ClipFlow::Awaiting => {
			let mut cancel = false;
			window("Send clipboard to call").show(ui.ctx(), |ui| {
				ui.label("Reading the clipboard…");
				cancel = ui.button("Cancel").clicked();
			});
			if cancel {
				*flow = ClipFlow::Idle;
			}
		}
		ClipFlow::Ready { text, picked } => {
			let live: Vec<Destination> = state
				.live_sessions()
				.map(|s| Destination {
					session: s.key.clone(),
					peer_name: s.peer_name.clone(),
				})
				.collect();
			let problem = clipboard_problem(text);
			let target = clipboard_target(&live, *picked);
			let (mut send, mut cancel) = (None, false);
			window("Send clipboard to call").show(ui.ctx(), |ui| {
				match (&problem, &target) {
					(Some(p), _) => {
						ui.label(RichText::new(p).color(theme::ERROR));
					}
					(None, Target::NoCall) => {
						ui.label("There is no live call to send this to.");
					}
					(None, _) => preview(ui, text),
				}
				if problem.is_none() && live.len() > 1 {
					ui.label(
						RichText::new("More than one call is live. Choose where this goes:")
							.strong(),
					);
					for (i, d) in live.iter().enumerate() {
						let tag = &d.session[..d.session.len().min(8)];
						ui.radio_value(picked, Some(i), format!("{} (call {tag})", d.peer_name));
					}
				}
				ui.horizontal(|ui| {
					if let (None, Target::Confirm(d)) = (&problem, &target) {
						let label = format!("Send to {}", d.peer_name);
						if ui.button(label).clicked() {
							send = Some(d.session.clone());
						}
					}
					cancel = ui.button("Cancel").clicked();
				});
			});
			if let Some(session) = send {
				let ClipFlow::Ready { text, .. } = std::mem::take(flow) else {
					return;
				};
				intents.push(Intent::Send { session, text });
			} else if cancel {
				*flow = ClipFlow::Idle;
			}
		}
	}
}

fn preview(ui: &mut Ui, text: &str) {
	let head: String = text.chars().take(160).collect();
	ui.label(format!("{} bytes from the clipboard:", text.len()));
	ui.add(egui::Label::new(RichText::new(head).monospace().small()));
	if text.chars().count() > 160 {
		ui.label(
			RichText::new("(shortened here; the whole text is sent)")
				.small()
				.weak(),
		);
	}
}

pub fn clear(
	ui: &mut Ui,
	pending: &mut Option<String>,
	state: &MessagesState,
	intents: &mut Vec<Intent>,
) {
	let Some(key) = pending.clone() else { return };
	let Some(session) = state.session(&key) else {
		*pending = None;
		return;
	};
	let (mut yes, mut no) = (false, false);
	window("Clear this transcript?").show(ui.ctx(), |ui| {
		let what = if session.is_live() {
			"This empties the messages of the running call on this device."
		} else {
			"This removes the ended call's transcript from this device."
		};
		ui.label(what);
		ui.label("The other side keeps its own copy. Export first if you want to keep it.");
		ui.horizontal(|ui| {
			yes = ui.button("Clear").clicked();
			no = ui.button("Keep").clicked();
		});
	});
	if yes {
		intents.push(Intent::Clear { session: key });
		*pending = None;
	} else if no {
		*pending = None;
	}
}
