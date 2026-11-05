//! Header, tab bar and footer.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};

use super::meter;
use crate::client::ConnectionState;
use crate::tui::model::{HotkeyBackend, Model, Prompt, Tab};

pub fn header(frame: &mut Frame, area: Rect, model: &Model) {
	let (link, color) = match &model.connection {
		ConnectionState::Live => ("live".to_owned(), Color::Green),
		ConnectionState::Resyncing(why) => (format!("resyncing ({why})"), Color::Yellow),
		ConnectionState::Lost(why) => (format!("offline ({why})"), Color::Red),
	};
	let hotkeys = match model.hotkey_backend() {
		HotkeyBackend::Global => "hotkeys: global",
		HotkeyBackend::TerminalOnly => "hotkeys: terminal only",
	};
	let settings = model.snapshot.settings.clone().unwrap_or_default();
	let mic = model.telemetry.as_ref().and_then(|t| t.microphone_dbfs);
	let first = Line::from(vec![
		Span::styled("relay ", Style::new().add_modifier(Modifier::BOLD)),
		Span::styled(link, Style::new().fg(color)),
		Span::raw(format!("  rev {}  {hotkeys}", model.snapshot.revision)),
	]);
	let mic_label = if settings.global_muted {
		"muted"
	} else {
		"mic"
	};
	let second = Line::from(format!(
		"{mic_label} [{}]  {}  ns: {}",
		meter::bar(mic, 20),
		if settings.global_deafened {
			"deafened"
		} else {
			"hearing"
		},
		if settings.ns_backend_id.is_empty() {
			"off"
		} else {
			&settings.ns_backend_id
		},
	));
	let block = Block::default().borders(Borders::BOTTOM);
	frame.render_widget(Paragraph::new(vec![first, second]).block(block), area);
}

pub fn tabs(frame: &mut Frame, area: Rect, model: &Model) {
	let titles: Vec<&str> = Tab::ALL.iter().map(|t| t.title()).collect();
	let widget = Tabs::new(titles)
		.select(model.tab.index())
		.highlight_style(Style::new().add_modifier(Modifier::REVERSED))
		.divider("|");
	frame.render_widget(widget, area);
}

pub fn footer(frame: &mut Frame, area: Rect, model: &Model) {
	let text = if let Some(Prompt::Consent { mode, .. }) = &model.prompt {
		let mode = crate::commands::names::transmit_mode(*mode as i32);
		format!("Allow the microphone on this call ({mode})? y = allow, any other key = no")
	} else if !model.status.is_empty() {
		model.status.clone()
	} else {
		"tab/1-4 switch  j/k move  enter act  c call  a/x/e accept/decline/end  t route  space ptt  m mute  d deafen  q quit".to_owned()
	};
	frame.render_widget(
		Paragraph::new(text).style(Style::new().fg(Color::DarkGray)),
		area,
	);
}
