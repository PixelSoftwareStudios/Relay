//! Block parsing: paragraphs, fenced code and flat lists.

use super::ast::{Block, CodeBlock, Document, Inline};
use super::inline::{Ctx, parse_inline};
use crate::limits::{MD_MAX_BLOCKS, MD_MAX_LANG_BYTES, MD_MAX_NODES};

/// Longest input considered; the rest is ignored. Callers sanitize to 4 KiB first.
const MAX_INPUT: usize = 16 * 1024;

fn valid_lang(s: &str) -> bool {
	!s.is_empty()
		&& s.len() <= MD_MAX_LANG_BYTES
		&& s.chars()
			.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '#' | '.' | '-'))
}

fn fence_open(line: &str) -> Option<Option<String>> {
	let info = line.strip_prefix("```")?;
	if info.contains('`') {
		return None;
	}
	let info = info.trim();
	Some(valid_lang(info).then(|| info.to_owned()))
}

fn list_marker(line: &str) -> Option<(bool, u32, &str)> {
	if let Some(r) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
		return Some((false, 1, r));
	}
	let d = line.bytes().take_while(u8::is_ascii_digit).count();
	if (1..=9).contains(&d)
		&& matches!(line.as_bytes().get(d), Some(b'.' | b')'))
		&& let Some(r) = line[d + 1..].strip_prefix(' ')
	{
		return Some((true, line[..d].parse().ok()?, r));
	}
	None
}

/// Parses `input` into a bounded document. Never fails, never panics.
pub fn parse(input: &str) -> Document {
	let mut end = input.len().min(MAX_INPUT);
	while !input.is_char_boundary(end) {
		end -= 1;
	}
	let lines: Vec<&str> = input[..end].split('\n').collect();
	let mut ctx = Ctx { nodes: 0 };
	let mut blocks = Vec::new();
	let mut i = 0;
	while i < lines.len() {
		if blocks.len() >= MD_MAX_BLOCKS || ctx.nodes >= MD_MAX_NODES {
			let rest = lines[i..].join("\n");
			if !rest.trim().is_empty() {
				blocks.push(Block::Paragraph(vec![Inline::Text(rest)]));
			}
			break;
		}
		let line = lines[i];
		if let Some(lang) = fence_open(line) {
			let mut body = Vec::new();
			let mut closed = false;
			i += 1;
			while i < lines.len() {
				if lines[i].trim_end() == "```" {
					closed = true;
					i += 1;
					break;
				}
				body.push(lines[i]);
				i += 1;
			}
			ctx.nodes += 1;
			blocks.push(Block::Code(CodeBlock {
				language: lang,
				text: body.join("\n"),
				closed,
			}));
		} else if let Some((ordered, start, first)) = list_marker(line) {
			let mut items = vec![parse_inline(first, 0, &mut ctx)];
			i += 1;
			while i < lines.len() && items.len() < MD_MAX_BLOCKS {
				match list_marker(lines[i]) {
					Some((o, _, r)) if o == ordered => {
						items.push(parse_inline(r, 0, &mut ctx));
						i += 1;
					}
					_ => break,
				}
			}
			blocks.push(Block::List {
				ordered,
				start,
				items,
			});
		} else if line.trim().is_empty() {
			i += 1;
		} else {
			let mut para = vec![line];
			i += 1;
			while i < lines.len()
				&& !lines[i].trim().is_empty()
				&& fence_open(lines[i]).is_none()
				&& list_marker(lines[i]).is_none()
			{
				para.push(lines[i]);
				i += 1;
			}
			blocks.push(Block::Paragraph(parse_inline(
				&para.join("\n"),
				0,
				&mut ctx,
			)));
		}
	}
	Document { blocks }
}
