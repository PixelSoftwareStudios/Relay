//! Block structure from `pulldown-cmark` events, mapped into the safe AST.
//!
//! Only paragraphs, emphasis, inline code, fenced code and lists are kept. Every other
//! construct (HTML, images, links with labels, headings, quotes, tables, rules) is shown as
//! its own source text, so nothing hides what was sent.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use super::ast::{Block, CodeBlock, Document, Inline};
use super::inline::Inlines;
use crate::limits::{MD_MAX_BLOCKS, MD_MAX_LANG_BYTES, MD_MAX_NODES};

/// Longest input considered; the rest is ignored. Callers sanitize to 4 KiB first.
const MAX_INPUT: usize = 16 * 1024;

fn valid_lang(s: &str) -> bool {
	!s.is_empty()
		&& s.len() <= MD_MAX_LANG_BYTES
		&& s.chars()
			.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '#' | '.' | '-'))
}

/// A fence is closed when its last line is a fence at least as long as the opening one.
fn is_closed(source: &str) -> bool {
	let source = source.trim_end();
	let first = source.lines().next().unwrap_or("").trim_start();
	let Some(ch) = first.chars().next().filter(|c| matches!(c, '`' | '~')) else {
		return false;
	};
	let opening = first.chars().take_while(|&c| c == ch).count();
	let Some((_, last)) = source.rsplit_once('\n') else {
		return false;
	};
	let last = last.trim();
	last.len() >= opening && last.chars().all(|c| c == ch)
}

struct Code {
	language: Option<String>,
	text: String,
	closed: bool,
}

struct List {
	ordered: bool,
	start: u32,
	items: Vec<Vec<Inline>>,
}

struct Builder<'a> {
	src: &'a str,
	blocks: Vec<Block>,
	run: Inlines,
	list: Option<List>,
	/// Nesting of lists; only the outermost becomes a [`Block::List`].
	list_depth: usize,
	code: Option<Code>,
	/// Source offset of the current top-level block or list item, for the overflow fallback.
	unit_start: usize,
}

impl<'a> Builder<'a> {
	fn raw(&mut self, range: Range<usize>) {
		let text = self.src[range].trim_end_matches(['\n', '\r']);
		self.run.text(text);
	}

	fn finish_paragraph(&mut self) {
		let inlines = self.run.take();
		if !inlines.is_empty() {
			self.blocks.push(Block::Paragraph(inlines));
		}
	}

	fn push_block(&mut self, block: Block) {
		if let (
			Block::List {
				ordered,
				items: more,
				..
			},
			Some(Block::List {
				ordered: prev,
				items,
				..
			}),
		) = (&block, self.blocks.last_mut())
			&& ordered == prev
		{
			items.extend(more.iter().cloned());
			return;
		}
		self.blocks.push(block);
	}

	fn flush_list(&mut self) {
		if let Some(l) = self.list.take()
			&& !l.items.is_empty()
		{
			self.push_block(Block::List {
				ordered: l.ordered,
				start: l.start,
				items: l.items,
			});
		}
	}

	/// Budget exhausted: keep what is done and show the rest as one plain paragraph.
	fn overflow(&mut self) {
		self.run.discard();
		self.flush_list();
		let rest = &self.src[self.unit_start..];
		if !rest.trim().is_empty() {
			self.blocks
				.push(Block::Paragraph(vec![Inline::Text(rest.to_owned())]));
		}
	}

	fn over_budget(&self) -> bool {
		self.run.nodes >= MD_MAX_NODES
	}

	/// Returns true when the whole subtree must be skipped (it was taken as literal text).
	fn start(&mut self, tag: Tag<'_>, range: Range<usize>) -> bool {
		match tag {
			Tag::Paragraph => {
				if self.list_depth > 0 && !self.run.is_empty() {
					self.run.line_break();
				}
			}
			Tag::Emphasis => self.run.open(false),
			Tag::Strong => self.run.open(true),
			Tag::List(first) => {
				self.list_depth += 1;
				if self.list_depth == 1 {
					self.list = Some(List {
						ordered: first.is_some(),
						start: first.map_or(1, |n| u32::try_from(n).unwrap_or(1)),
						items: Vec::new(),
					});
				}
			}
			Tag::Item => {
				if self.list_depth > 1 {
					if !self.run.is_empty() {
						self.run.line_break();
					}
				} else {
					self.unit_start = range.start;
				}
			}
			Tag::CodeBlock(kind) => {
				let (language, closed) = match kind {
					CodeBlockKind::Fenced(info) => {
						let info = info.trim();
						let lang = valid_lang(info).then(|| info.to_owned());
						(lang, is_closed(&self.src[range]))
					}
					CodeBlockKind::Indented => (None, true),
				};
				self.code = Some(Code {
					language,
					text: String::new(),
					closed,
				});
			}
			// Links, images, HTML, headings, quotes, tables, footnotes: literal source.
			_ => {
				self.raw(range);
				return true;
			}
		}
		false
	}

	fn end(&mut self, tag: TagEnd) {
		match tag {
			TagEnd::Paragraph if self.list_depth == 0 => self.finish_paragraph(),
			TagEnd::Emphasis | TagEnd::Strong => self.run.close(),
			TagEnd::Item if self.list_depth == 1 => {
				let item = self.run.take();
				if let Some(l) = self.list.as_mut() {
					l.items.push(item);
				}
			}
			TagEnd::List(_) => {
				self.list_depth = self.list_depth.saturating_sub(1);
				if self.list_depth == 0 {
					self.flush_list();
				}
			}
			TagEnd::CodeBlock => {
				let Some(mut c) = self.code.take() else {
					return;
				};
				if c.text.ends_with('\n') {
					c.text.pop();
					if c.text.ends_with('\r') {
						c.text.pop();
					}
				}
				if self.list_depth == 0 {
					self.blocks.push(Block::Code(CodeBlock {
						language: c.language,
						text: c.text,
						closed: c.closed,
					}));
					self.run.nodes += 1;
				} else {
					if !self.run.is_empty() {
						self.run.line_break();
					}
					self.run.code(&c.text);
				}
			}
			_ => {}
		}
	}
}

/// Parses `input` into a bounded document. Never fails, never panics.
pub fn parse(input: &str) -> Document {
	let mut end = input.len().min(MAX_INPUT);
	while !input.is_char_boundary(end) {
		end -= 1;
	}
	let src = &input[..end];
	let mut b = Builder {
		src,
		blocks: Vec::new(),
		run: Inlines::new(),
		list: None,
		list_depth: 0,
		code: None,
		unit_start: 0,
	};
	let mut events = Parser::new_ext(src, Options::empty()).into_offset_iter();
	while let Some((event, range)) = events.next() {
		let top = b.list_depth == 0 && b.code.is_none() && b.run.is_empty();
		if top && !matches!(event, Event::End(_)) {
			b.unit_start = range.start;
		}
		if (top && b.blocks.len() >= MD_MAX_BLOCKS) || b.over_budget() {
			b.overflow();
			return Document { blocks: b.blocks };
		}
		match event {
			Event::Start(tag) => {
				let tag_is_item = matches!(tag, Tag::Item);
				let literal = b.start(tag, range);
				if literal {
					let mut depth = 1;
					for (e, _) in events.by_ref() {
						match e {
							Event::Start(_) => depth += 1,
							Event::End(_) => depth -= 1,
							_ => {}
						}
						if depth == 0 {
							break;
						}
					}
					if b.list_depth == 0 {
						b.finish_paragraph();
					}
				} else if tag_is_item
					&& b.list_depth == 1
					&& b.list
						.as_ref()
						.is_some_and(|l| l.items.len() >= MD_MAX_BLOCKS)
				{
					b.overflow();
					return Document { blocks: b.blocks };
				}
			}
			Event::End(tag) => b.end(tag),
			Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => match b.code.as_mut() {
				Some(c) => c.text.push_str(&t),
				None => b.run.text(&t),
			},
			Event::Code(t) => b.run.code(&t),
			Event::SoftBreak | Event::HardBreak => b.run.line_break(),
			Event::Rule => {
				b.raw(range);
				if b.list_depth == 0 {
					b.finish_paragraph();
				}
			}
			_ => {}
		}
	}
	b.finish_paragraph();
	Document { blocks: b.blocks }
}
