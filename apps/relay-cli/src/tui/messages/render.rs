//! One transcript entry as styled terminal lines.
//!
//! Links are drawn, never opened. A link whose host could be a lookalike says so in red, with
//! the real (ASCII) host. Code blocks show a stable session-wide number and language so
//! Ctrl-Y can pick one.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use relay_api::pb::transcript_entry::Body;
use relay_api::pb::{DeliveryState, FileOfferBody, OfferState};
use relay_messages::markdown::{Block, CodeBlock, HostWarning, Inline, Link};

use super::state::Row;
use crate::commands::messages::{OfferView, clock, offer_state_name, plain, size};

/// Longest code block shown before it is cut (copy still takes all of it).
const CODE_LINES: usize = 12;
/// Longest preview shown.
const PREVIEW_LINES: usize = 8;

/// How a row is drawn.
pub struct Look<'a> {
	pub peer: &'a str,
	pub width: u16,
	pub selected: bool,
	pub block: usize,
	pub hit: bool,
}

fn dim() -> Style {
	Style::new().fg(Color::DarkGray)
}

fn wrap(text: &str, width: usize) -> Vec<String> {
	let width = width.max(8);
	let mut lines = Vec::new();
	for raw in text.split('\n') {
		let mut line = String::new();
		let mut len = 0;
		for word in raw.split_inclusive(' ') {
			let w = word.chars().count();
			if len + w > width && len > 0 {
				lines.push(std::mem::take(&mut line));
				len = 0;
			}
			if w > width {
				for c in word.chars() {
					if len == width {
						lines.push(std::mem::take(&mut line));
						len = 0;
					}
					line.push(c);
					len += 1;
				}
			} else {
				line.push_str(word);
				len += w;
			}
		}
		lines.push(line);
	}
	lines
}

fn warning_text(w: HostWarning) -> &'static str {
	match w {
		HostWarning::Idn => "international domain name",
		HostWarning::MixedScript => "mixed scripts, a likely lookalike",
		HostWarning::InvalidPunycode => "invalid encoding",
	}
}

fn link_spans(link: &Link, out: &mut Vec<Span<'static>>) {
	out.push(Span::styled(
		plain(&link.url, false),
		Style::new()
			.fg(Color::Blue)
			.add_modifier(Modifier::UNDERLINED),
	));
	if let Some(w) = link.host.warning {
		out.push(Span::styled(
			format!(
				" [WARNING {}: real host {}]",
				warning_text(w),
				link.host.ascii
			),
			Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
		));
	}
}

fn inline_spans(inlines: &[Inline], style: Style, out: &mut Vec<Span<'static>>) {
	for inline in inlines {
		match inline {
			Inline::Text(t) => out.push(Span::styled(plain(t, false), style)),
			Inline::Bold(c) => inline_spans(c, style.add_modifier(Modifier::BOLD), out),
			Inline::Italic(c) => inline_spans(c, style.add_modifier(Modifier::ITALIC), out),
			Inline::Code(t) => out.push(Span::styled(plain(t, false), style.fg(Color::Yellow))),
			Inline::Link(l) => link_spans(l, out),
			Inline::LineBreak => out.push(Span::raw("\n")),
		}
	}
}

/// Split spans at `\n` and wrap each styled run to `width`.
fn flow(spans: Vec<Span<'static>>, width: usize, indent: &str) -> Vec<Line<'static>> {
	let mut lines: Vec<Line<'static>> = Vec::new();
	let mut current: Vec<Span<'static>> = vec![Span::raw(indent.to_owned())];
	let mut used = indent.chars().count();
	for span in spans {
		for (i, part) in span.content.split('\n').enumerate() {
			if i > 0 {
				lines.push(Line::from(std::mem::replace(
					&mut current,
					vec![Span::raw(indent.to_owned())],
				)));
				used = indent.chars().count();
			}
			for chunk in wrap(part, width.saturating_sub(used).max(8)) {
				let n = chunk.chars().count();
				if used + n > width && used > indent.chars().count() {
					lines.push(Line::from(std::mem::replace(
						&mut current,
						vec![Span::raw(indent.to_owned())],
					)));
					used = indent.chars().count();
				}
				used += n;
				current.push(Span::styled(chunk, span.style));
			}
		}
	}
	lines.push(Line::from(current));
	lines
}

fn code_block(number: usize, b: &CodeBlock, selected: bool, width: usize) -> Vec<Line<'static>> {
	let mark = if selected { " <- Ctrl-Y copies" } else { "" };
	let mut out = vec![Line::styled(
		format!(
			"  +- code {number}: {}{}{mark}",
			plain(b.language.as_deref().unwrap_or("no language"), false),
			if b.closed { "" } else { " (not closed)" }
		),
		Style::new().fg(Color::Magenta),
	)];
	let total = b.text.lines().count();
	for line in b.text.lines().take(CODE_LINES) {
		for part in wrap(&plain(line, false), width.saturating_sub(5)) {
			out.push(Line::styled(
				format!("  | {part}"),
				Style::new().fg(Color::Yellow),
			));
		}
	}
	if total > CODE_LINES {
		out.push(Line::styled(
			format!("  | ... {} more lines", total - CODE_LINES),
			dim(),
		));
	}
	out
}

fn text_lines(row: &Row, look: &Look<'_>) -> Vec<Line<'static>> {
	let Some(doc) = &row.doc else {
		return Vec::new();
	};
	let width = usize::from(look.width).saturating_sub(2);
	let mut out = Vec::new();
	let mut number = row.first_block;
	for block in &doc.blocks {
		match block {
			Block::Paragraph(inlines) => {
				let mut spans = Vec::new();
				inline_spans(inlines, Style::new(), &mut spans);
				out.extend(flow(spans, width, "  "));
			}
			Block::List {
				ordered,
				start,
				items,
			} => {
				for (i, item) in items.iter().enumerate() {
					let bullet = if *ordered {
						format!("  {}. ", *start as usize + i)
					} else {
						"  - ".to_owned()
					};
					let mut spans = vec![Span::raw(bullet.clone())];
					inline_spans(item, Style::new(), &mut spans);
					out.extend(flow(spans, width, "    "));
				}
			}
			Block::Code(b) => {
				let selected = look.selected && number - row.first_block == look.block;
				number += 1;
				out.extend(code_block(number, b, selected, width));
			}
		}
	}
	out
}

fn progress_bar(percent: u8, width: usize) -> String {
	let filled = usize::from(percent) * width / 100;
	format!(
		"[{}{}] {percent}%",
		"#".repeat(filled),
		".".repeat(width - filled)
	)
}

fn offer_lines(o: &FileOfferBody, selected: bool) -> Vec<Line<'static>> {
	let view = OfferView::new(o);
	let state_color = match o.state {
		s if s == OfferState::Completed as i32 => Color::Green,
		s if s == OfferState::Failed as i32 || s == OfferState::Rejected as i32 => Color::Red,
		_ => Color::Reset,
	};
	let mut out = vec![Line::from(vec![
		Span::raw(format!(
			"  file \"{}\" {} {} ",
			view.name,
			size(view.size),
			if view.outgoing {
				"outgoing"
			} else {
				"incoming"
			}
		)),
		Span::styled(offer_state_name(o.state), Style::new().fg(state_color)),
	])];
	if o.state == OfferState::InProgress as i32 {
		out.push(Line::raw(format!("  {}", progress_bar(view.percent, 24))));
	}
	if view.executable_warning {
		out.push(Line::styled(
			"  WARNING: the name looks executable; do not run it unless you trust the sender",
			Style::new().fg(Color::Red),
		));
	}
	if view.preview_approval_pending {
		out.push(Line::styled(
			"  the peer asked to preview this file; v approves (at most 100 lines of text)",
			Style::new().fg(Color::Yellow),
		));
	}
	if view.preview_requested && view.preview.is_none() {
		out.push(Line::styled(
			"  preview requested, waiting for the sender",
			dim(),
		));
	}
	if let Some(preview) = &view.preview {
		out.push(Line::styled("  preview:", dim()));
		out.extend(
			preview
				.lines()
				.take(PREVIEW_LINES)
				.map(|l| Line::styled(format!("  | {l}"), Style::new().fg(Color::Yellow))),
		);
	}
	if let Some(saved) = &view.saved_as {
		out.push(Line::raw(format!("  saved as \"{saved}\"")));
	}
	if let Some(reason) = &view.failure {
		out.push(Line::styled(
			format!("  {reason}"),
			Style::new().fg(Color::Red),
		));
	}
	if selected {
		let hint = match (o.outgoing, o.state) {
			(false, s) if s == OfferState::Offered as i32 => "a accept  r reject  v preview",
			(_, s) if s == OfferState::Offered as i32 || s == OfferState::InProgress as i32 => {
				"c cancel"
			}
			_ => "",
		};
		if !hint.is_empty() {
			out.push(Line::styled(format!("  {hint}"), dim()));
		}
	}
	out
}

/// The lines of one row.
pub fn row_lines(row: &Row, look: &Look<'_>) -> Vec<Line<'static>> {
	let e = &row.entry;
	let who = if e.outgoing {
		"you".to_owned()
	} else {
		plain(look.peer, false)
	};
	let mut head = vec![
		Span::raw(if look.selected { "> " } else { "  " }),
		Span::styled(clock(e.received_at_unix_ms), dim()),
		Span::raw(" "),
		Span::styled(
			who,
			Style::new()
				.fg(if e.outgoing {
					Color::Cyan
				} else {
					Color::Green
				})
				.add_modifier(Modifier::BOLD),
		),
	];
	if e.outgoing {
		let (text, color) = match DeliveryState::try_from(e.delivery) {
			Ok(DeliveryState::Delivered) => ("delivered".to_owned(), Color::DarkGray),
			Ok(DeliveryState::Failed) => {
				(format!("FAILED ({})", plain(&e.failure, false)), Color::Red)
			}
			_ => ("sending".to_owned(), Color::Yellow),
		};
		head.push(Span::styled(format!("  {text}"), Style::new().fg(color)));
	}
	if e.pinned {
		head.push(Span::styled("  [pinned]", Style::new().fg(Color::Yellow)));
	}
	if look.hit {
		head.push(Span::styled("  [match]", Style::new().fg(Color::Cyan)));
	}
	let mut lines = vec![Line::from(head)];
	match &e.body {
		Some(Body::Text(_)) => lines.extend(text_lines(row, look)),
		Some(Body::Image(i)) => lines.push(Line::raw(format!(
			"  [image {} {}x{} {}]",
			plain(&i.media_type, false),
			i.width,
			i.height,
			size(i.data.len() as u64)
		))),
		Some(Body::File(o)) => lines.extend(offer_lines(o, look.selected)),
		None => {}
	}
	if look.selected {
		for line in &mut lines {
			line.style = line.style.add_modifier(Modifier::BOLD);
		}
	}
	lines
}
