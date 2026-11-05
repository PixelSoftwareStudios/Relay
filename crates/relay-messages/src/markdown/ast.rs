//! The safe markdown AST. There is no node for raw HTML, images or arbitrary links.

use super::url::Link;

/// A parsed message.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Document {
	/// Top-level blocks.
	pub blocks: Vec<Block>,
}

impl std::fmt::Debug for Document {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Document(<redacted>, {} blocks)", self.blocks.len())
	}
}

/// Block-level node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
	/// A paragraph of inline nodes.
	Paragraph(Vec<Inline>),
	/// A fenced code block.
	Code(CodeBlock),
	/// A flat list.
	List {
		/// Numbered (`true`) or bulleted.
		ordered: bool,
		/// First number of an ordered list.
		start: u32,
		/// One inline run per item.
		items: Vec<Vec<Inline>>,
	},
}

/// Inline node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
	/// Plain text.
	Text(String),
	/// `**bold**`.
	Bold(Vec<Inline>),
	/// `*italic*`.
	Italic(Vec<Inline>),
	/// `` `code` ``.
	Code(String),
	/// An auto-detected http(s) URL.
	Link(Link),
	/// A newline inside a paragraph.
	LineBreak,
}

/// A fenced code block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeBlock {
	/// Optional language label.
	pub language: Option<String>,
	/// Content exactly as sent.
	pub text: String,
	/// False when the fence was never closed.
	pub closed: bool,
}

impl CodeBlock {
	/// The exact content bytes for a copy action.
	pub fn copy_text(&self) -> &str {
		&self.text
	}
}

impl Document {
	/// All code blocks in order.
	pub fn code_blocks(&self) -> impl Iterator<Item = &CodeBlock> {
		self.blocks.iter().filter_map(|b| match b {
			Block::Code(c) => Some(c),
			_ => None,
		})
	}

	/// All links in order.
	pub fn links(&self) -> Vec<&Link> {
		fn walk<'a>(v: &'a [Inline], out: &mut Vec<&'a Link>) {
			for i in v {
				match i {
					Inline::Link(l) => out.push(l),
					Inline::Bold(c) | Inline::Italic(c) => walk(c, out),
					_ => {}
				}
			}
		}
		let mut out = Vec::new();
		for b in &self.blocks {
			match b {
				Block::Paragraph(v) => walk(v, &mut out),
				Block::List { items, .. } => items.iter().for_each(|i| walk(i, &mut out)),
				Block::Code(_) => {}
			}
		}
		out
	}

	/// Visible text without markup, for search and accessibility.
	pub fn plain_text(&self) -> String {
		fn walk(v: &[Inline], out: &mut String) {
			for i in v {
				match i {
					Inline::Text(t) | Inline::Code(t) => out.push_str(t),
					Inline::Bold(c) | Inline::Italic(c) => walk(c, out),
					Inline::Link(l) => out.push_str(&l.url),
					Inline::LineBreak => out.push('\n'),
				}
			}
		}
		let mut out = String::new();
		for b in &self.blocks {
			if !out.is_empty() {
				out.push('\n');
			}
			match b {
				Block::Paragraph(v) => walk(v, &mut out),
				Block::Code(c) => out.push_str(&c.text),
				Block::List { items, .. } => {
					for (n, it) in items.iter().enumerate() {
						if n > 0 {
							out.push('\n');
						}
						walk(it, &mut out);
					}
				}
			}
		}
		out
	}
}
