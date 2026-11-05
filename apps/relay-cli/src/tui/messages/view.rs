//! The Messages tab: a session bar, the transcript, then the compose box or a prompt.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::keys::send_blocker;
use super::render::{Look, row_lines};
use super::state::{Link, MessagesState, SessionPane};
use crate::commands::messages::{plain, size};

fn centered(frame: &mut Frame, area: Rect, title: &str, text: &str) {
	let block = Block::default()
		.borders(Borders::ALL)
		.title(title.to_owned());
	frame.render_widget(
		Paragraph::new(text.to_owned())
			.block(block)
			.wrap(ratatui::widgets::Wrap { trim: true }),
		area,
	);
}

fn link_span(link: &Link) -> Span<'static> {
	match link {
		Link::Connecting => Span::styled("connecting", Style::new().fg(Color::Yellow)),
		Link::Live => Span::styled("live", Style::new().fg(Color::Green)),
		Link::Resyncing(why) => {
			Span::styled(format!("resyncing ({why})"), Style::new().fg(Color::Yellow))
		}
		Link::Lost(why) => Span::styled(format!("offline ({why})"), Style::new().fg(Color::Red)),
		Link::Unavailable(why) => {
			Span::styled(format!("unavailable ({why})"), Style::new().fg(Color::Red))
		}
	}
}

fn session_bar(state: &MessagesState) -> Line<'static> {
	let mut spans = vec![Span::raw(" ")];
	for pane in &state.sessions {
		let current = state.current.as_ref() == Some(&pane.summary.session_id);
		let label = format!(
			" {} ({}) ",
			plain(&pane.summary.peer_name, false),
			if pane.active() { "call" } else { "ended" }
		);
		spans.push(if current {
			Span::styled(label, Style::new().add_modifier(Modifier::REVERSED))
		} else {
			Span::raw(label)
		});
	}
	spans.push(Span::raw("  link: "));
	spans.push(link_span(&state.link));
	Line::from(spans)
}

fn info_lines(pane: &SessionPane) -> Vec<Line<'static>> {
	let s = &pane.summary;
	let p = s.permissions.unwrap_or_default();
	let on = |v| if v { "on" } else { "off" };
	let mut lines = vec![Line::styled(
		format!(
			" messages {}  files {}  {} message{}  {} of {}",
			on(p.allow_messages),
			on(p.allow_files),
			s.message_count,
			if s.message_count == 1 { "" } else { "s" },
			size(s.used_bytes),
			size(s.limit_bytes)
		),
		Style::new().fg(Color::DarkGray),
	)];
	if !pane.active() {
		lines.push(Line::styled(
			" the call has ended: read-only transcript, kept in memory until cleared (X)",
			Style::new().fg(Color::Yellow),
		));
	} else if !p.allow_messages {
		lines.push(Line::styled(
			" messages are turned off for this call (the host controls it; see `relay messages perms`)",
			Style::new().fg(Color::Yellow),
		));
	}
	if s.limit_bytes > 0 && s.used_bytes >= s.limit_bytes {
		lines.push(Line::styled(
			" transcript budget full: e exports it to a file, X empties it",
			Style::new().fg(Color::Red),
		));
	}
	lines
}

fn transcript(frame: &mut Frame, area: Rect, pane: &SessionPane) {
	let height = usize::from(area.height);
	if pane.rows.is_empty() {
		frame.render_widget(
			Paragraph::new(if pane.active() {
				"  no messages yet; press i to write one"
			} else {
				"  this transcript is empty"
			})
			.style(Style::new().fg(Color::DarkGray)),
			area,
		);
		return;
	}
	let len = pane.rows.len();
	let end = pane.focus.map_or(len - 1, |f| (f + 2).min(len - 1));
	let mut blocks: Vec<Vec<Line>> = Vec::new();
	let mut total = 0;
	let mut index = end + 1;
	while index > 0 && total < height {
		index -= 1;
		let row = &pane.rows[index];
		let look = Look {
			peer: &pane.summary.peer_name,
			width: area.width,
			selected: pane.focus == Some(index),
			block: pane.block,
			hit: pane.hits.contains(&row.entry.ordinal),
		};
		let lines = row_lines(row, &look);
		total += lines.len();
		blocks.push(lines);
	}
	let mut lines: Vec<Line> = blocks.into_iter().rev().flatten().collect();
	// Keep the newest lines when the window overflows.
	if lines.len() > height {
		lines.drain(..lines.len() - height);
	}
	frame.render_widget(Paragraph::new(lines), area);
}

fn bottom(frame: &mut Frame, area: Rect, state: &MessagesState) {
	if let Some(prompt) = &state.prompt {
		let block = Block::default().borders(Borders::ALL).title(prompt.label());
		let text = if prompt.kind == super::prompt::PromptKind::ConfirmClear {
			String::from("press y to confirm, any other key to keep it")
		} else {
			format!("{}_", prompt.input)
		};
		frame.render_widget(Paragraph::new(text).block(block), area);
		return;
	}
	let (title, text, style) = match send_blocker(state) {
		Some(why) => ("Message", why.to_owned(), Style::new().fg(Color::DarkGray)),
		None if state.composing => (
			"Message (Enter sends, Ctrl-J new line, Esc leaves)",
			format!("{}_", state.compose),
			Style::new(),
		),
		None => (
			"Message (i to write)",
			state.compose.clone(),
			Style::new().fg(Color::DarkGray),
		),
	};
	// Only the tail of a long draft fits the box.
	let inner = usize::from(area.width.saturating_sub(2));
	let last = text.lines().last().unwrap_or("");
	let shown: String = last
		.chars()
		.rev()
		.take(inner.max(1))
		.collect::<Vec<_>>()
		.into_iter()
		.rev()
		.collect();
	let block = Block::default().borders(Borders::ALL).title(title);
	frame.render_widget(Paragraph::new(shown).style(style).block(block), area);
}

/// Draw the tab into `area`.
pub fn draw(frame: &mut Frame, area: Rect, state: &MessagesState) {
	if let Link::Unavailable(why) = &state.link {
		centered(
			frame,
			area,
			"Messages",
			&format!("Messages are unavailable: {why}"),
		);
		return;
	}
	let Some(pane) = state.pane() else {
		let text = match &state.link {
			Link::Connecting => "connecting to the messages stream...".to_owned(),
			_ => "No call session. Messages exist only inside a call and are kept in memory until the call's transcript is cleared; connect a call to start.".to_owned(),
		};
		centered(frame, area, "Messages", &text);
		return;
	};
	let info = info_lines(pane);
	let [bar, infos, body, input] = Layout::vertical([
		Constraint::Length(1),
		Constraint::Length(u16::try_from(info.len()).unwrap_or(1)),
		Constraint::Min(2),
		Constraint::Length(3),
	])
	.areas(area);
	frame.render_widget(Paragraph::new(session_bar(state)), bar);
	frame.render_widget(Paragraph::new(info), infos);
	transcript(frame, body, pane);
	bottom(frame, input, state);
}

/// Footer hints for the tab.
pub fn hints(state: &MessagesState) -> &'static str {
	if state.prompt.is_some() {
		"enter confirm  esc cancel"
	} else if state.composing {
		"enter send  ctrl-j new line  ctrl-y copy code block  esc leave the box"
	} else {
		"i write  j/k move  / search n/N  p pin  b/ctrl-y code  o offer  a/r accept/reject  c cancel  v preview  e export  X clear  [ ] call  tab/1-5 screens  q quit"
	}
}
