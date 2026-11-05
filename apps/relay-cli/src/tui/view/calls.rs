//! Call cards with route, effective-PTT state, meters and speaking indicators.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::meter;
use crate::commands::names;
use crate::tui::model::{Model, Tab};

/// Height of one card in rows.
const CARD_HEIGHT: u16 = 6;

pub fn calls(frame: &mut Frame, area: Rect, model: &Model) {
	let live = model.live_calls();
	if live.is_empty() {
		let block = Block::default().borders(Borders::ALL).title("Calls");
		frame.render_widget(
			Paragraph::new("no active calls; call a peer from the Peers tab").block(block),
			area,
		);
		return;
	}
	let rows = Layout::vertical(live.iter().map(|_| Constraint::Length(CARD_HEIGHT))).split(area);
	let selected = model.selection(Tab::Calls);
	for (i, (call, slot)) in live.iter().zip(rows.iter()).enumerate() {
		card(frame, *slot, model, call, i == selected);
	}
}

fn card(frame: &mut Frame, area: Rect, model: &Model, call: &relay_api::pb::Call, selected: bool) {
	let transmit = call.transmit.clone().unwrap_or_default();
	let sample = model
		.telemetry
		.as_ref()
		.and_then(|t| t.calls.iter().find(|c| c.call_id == call.call_id));
	let speaking = sample.is_some_and(|c| c.speaking);
	let state = names::call_state(call.state);
	let state_color = match state {
		"connected" => Color::Green,
		"incoming-ringing" => Color::Yellow,
		"interrupted" => Color::Red,
		_ => Color::Reset,
	};
	let (speak_mark, speak_color) = if speaking {
		("(speaking)", Color::Green)
	} else {
		("", Color::Reset)
	};
	let title = Line::from(vec![
		Span::raw(format!(" {} ", model.peer_name(&call.peer_id))),
		Span::styled(state, Style::new().fg(state_color)),
		Span::raw(" "),
		Span::styled(speak_mark, Style::new().fg(speak_color)),
	]);
	let border = if selected {
		Style::new().add_modifier(Modifier::BOLD)
	} else {
		Style::new()
	};
	let block = Block::default()
		.borders(Borders::ALL)
		.border_style(border)
		.title(title);

	let tx = if transmit.effective_transmitting {
		Span::styled(
			"TRANSMITTING",
			Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
		)
	} else {
		Span::styled("mic closed", Style::new().fg(Color::DarkGray))
	};
	let mut route = vec![
		Span::raw(format!(
			"route: {}  consent: {}  ",
			names::transmit_mode(transmit.mode),
			yes_no(transmit.consent_granted)
		)),
		tx,
	];
	if model.ptt_latched.as_deref() == Some(call.call_id.as_slice()) {
		route.push(Span::raw("  [ptt held]"));
	}
	let mut lines = vec![Line::from(route)];
	if !transmit.blocked_reason.is_empty() {
		lines.push(Line::styled(
			format!("blocked: {}", transmit.blocked_reason),
			Style::new().fg(Color::Yellow),
		));
	}
	lines.push(Line::from(format!(
		"in  [{}]  muted: {}  deafened: {}  gain x{:.2}",
		meter::bar(sample.and_then(|c| c.receive_dbfs), 20),
		yes_no(call.user_muted),
		yes_no(call.user_deafened),
		call.receive_gain,
	)));
	if let Some(c) = sample {
		lines.push(Line::from(format!(
			"rtt {}  jitter {}  late {}  missing {}",
			c.rtt_ms.map_or("-".into(), |v| format!("{v:.1}ms")),
			c.rtp_interarrival_jitter_ms
				.map_or("-".into(), |v| format!("{v:.1}ms")),
			c.packets_late,
			c.packets_missing_at_deadline,
		)));
	}
	frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn yes_no(v: bool) -> &'static str {
	if v { "yes" } else { "no" }
}
