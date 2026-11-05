//! Shared terminal table style for the human-readable listings.
//!
//! One look everywhere: no borders, two spaces between columns, bold headers.
//! Styling and width lookup only happen when stdout is a terminal and
//! `NO_COLOR` is unset; piped output is plain and never wrapped.

use std::io::IsTerminal;

use comfy_table::presets::NOTHING;
use comfy_table::{Attribute, Cell, ContentArrangement, Table};

/// How a table is rendered: the available width and whether to style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Layout {
	/// Total width to wrap to; `None` leaves content unwrapped.
	pub width: Option<u16>,
	/// Whether headers and labels use terminal attributes.
	pub styled: bool,
}

impl Layout {
	/// Plain, unwrapped output (what a pipe or file receives).
	pub const PLAIN: Self = Self {
		width: None,
		styled: false,
	};

	/// Plain output wrapped to a fixed width.
	pub fn plain_width(width: u16) -> Self {
		Self {
			width: Some(width),
			styled: false,
		}
	}

	/// Inspect stdout and the environment.
	pub fn detect() -> Self {
		let tty = std::io::stdout().is_terminal();
		let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
		Self::from_env(tty, no_color, || {
			crossterm::terminal::size().ok().map(|(w, _)| w)
		})
	}

	fn from_env(tty: bool, no_color: bool, size: impl FnOnce() -> Option<u16>) -> Self {
		if !tty {
			return Self::PLAIN;
		}
		Self {
			width: size().filter(|w| *w > 0),
			styled: !no_color,
		}
	}
}

/// A bold cell when styled, plain otherwise (comfy-table drops attributes off a tty).
fn label(text: &str) -> Cell {
	Cell::new(text).add_attribute(Attribute::Bold)
}

fn base(layout: Layout) -> Table {
	let mut table = Table::new();
	table.load_style(NOTHING);
	if layout.styled {
		table.enforce_styling();
	} else {
		table.force_no_tty();
	}
	if let Some(width) = layout.width {
		table
			.set_content_arrangement(ContentArrangement::Dynamic)
			.set_width(width);
	}
	table
}

/// A table with a header row.
pub fn with_header(layout: Layout, headers: &[&str]) -> Table {
	let mut table = base(layout);
	table.set_header(headers.iter().map(|h| label(h)));
	table
}

/// A two-column table of `label  value` rows with a bold first column.
pub fn key_values(layout: Layout, rows: &[(&str, String)]) -> Table {
	let mut table = base(layout);
	for (key, value) in rows {
		table.add_row([label(key), Cell::new(value)]);
	}
	table
}

/// Render to text: columns separated by two spaces, no trailing padding.
pub fn render(mut table: Table) -> String {
	let last = table.column_count().saturating_sub(1);
	for (i, column) in table.column_iter_mut().enumerate() {
		column.set_padding((0, if i == last { 0 } else { 2 }));
	}
	let text = table.to_string();
	let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
	lines.join("\n")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn non_tty_is_plain_and_unwrapped() {
		assert_eq!(Layout::from_env(false, false, || Some(80)), Layout::PLAIN);
	}

	#[test]
	fn tty_gets_width_and_style_unless_no_color() {
		let styled = Layout::from_env(true, false, || Some(100));
		assert_eq!((styled.width, styled.styled), (Some(100), true));
		let plain = Layout::from_env(true, true, || None);
		assert_eq!((plain.width, plain.styled), (None, false));
	}
}
