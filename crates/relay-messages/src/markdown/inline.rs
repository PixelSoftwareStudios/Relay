//! Inline parsing: bold, italic, code and auto-detected URLs.

use super::ast::Inline;
use super::url::parse_http;
use crate::limits::{MD_MAX_DEPTH, MD_MAX_NODES};

/// Node counter shared by a whole document parse.
pub(super) struct Ctx {
	pub nodes: usize,
}

fn flush(text: &mut String, out: &mut Vec<Inline>, ctx: &mut Ctx) {
	if !text.is_empty() {
		ctx.nodes += 1;
		out.push(Inline::Text(std::mem::take(text)));
	}
}

fn url_end(s: &str) -> usize {
	s.find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '`'))
		.unwrap_or(s.len())
}

fn trim_url(tok: &str) -> &str {
	let mut t = tok;
	loop {
		let Some(last) = t.chars().next_back() else {
			return t;
		};
		let strip = match last {
			'.' | ',' | ';' | ':' | '!' | '?' | '\'' => true,
			')' => t.matches(')').count() > t.matches('(').count(),
			']' => t.matches(']').count() > t.matches('[').count(),
			'}' => t.matches('}').count() > t.matches('{').count(),
			_ => false,
		};
		if !strip {
			return t;
		}
		t = &t[..t.len() - last.len_utf8()];
	}
}

fn starts_with_ci(s: &str, p: &str) -> bool {
	s.len() >= p.len() && s.is_char_boundary(p.len()) && s[..p.len()].eq_ignore_ascii_case(p)
}

fn find_italic_close(s: &str, from: usize) -> Option<usize> {
	let b = s.as_bytes();
	(from..b.len()).find(|&j| {
		b[j] == b'*'
			&& b[j - 1] != b'*'
			&& b.get(j + 1) != Some(&b'*')
			&& !b[j - 1].is_ascii_whitespace()
	})
}

pub(super) fn parse_inline(s: &str, depth: usize, ctx: &mut Ctx) -> Vec<Inline> {
	let mut out = Vec::new();
	let mut text = String::new();
	let mut i = 0;
	while i < s.len() {
		if ctx.nodes >= MD_MAX_NODES {
			text.push_str(&s[i..]);
			break;
		}
		let rest = &s[i..];
		let c = rest.chars().next().unwrap_or('\0');
		let prev_ws =
			i == 0 || s.as_bytes()[i - 1].is_ascii_whitespace() || s.as_bytes()[i - 1] == b'(';
		if c == '`' {
			if let Some(j) = rest[1..].find('`').filter(|&j| j > 0) {
				flush(&mut text, &mut out, ctx);
				ctx.nodes += 1;
				out.push(Inline::Code(rest[1..1 + j].to_owned()));
				i += j + 2;
				continue;
			}
		} else if rest.starts_with("**") && depth < MD_MAX_DEPTH {
			let inner = &rest[2..];
			if let Some(j) = inner.find("**").filter(|&j| {
				j > 0
					&& !inner.as_bytes()[0].is_ascii_whitespace()
					&& !inner.as_bytes()[j - 1].is_ascii_whitespace()
			}) {
				flush(&mut text, &mut out, ctx);
				ctx.nodes += 1;
				let kids = parse_inline(&inner[..j], depth + 1, ctx);
				out.push(Inline::Bold(kids));
				i += j + 4;
				continue;
			}
		} else if c == '*' && !rest.starts_with("**") && depth < MD_MAX_DEPTH {
			let ok_open = rest
				.as_bytes()
				.get(1)
				.is_some_and(|b| !b.is_ascii_whitespace());
			if let Some(j) = ok_open.then(|| find_italic_close(rest, 2)).flatten() {
				flush(&mut text, &mut out, ctx);
				ctx.nodes += 1;
				let kids = parse_inline(&rest[1..j], depth + 1, ctx);
				out.push(Inline::Italic(kids));
				i += j + 1;
				continue;
			}
		} else if c == '\n' {
			flush(&mut text, &mut out, ctx);
			ctx.nodes += 1;
			out.push(Inline::LineBreak);
			i += 1;
			continue;
		} else if (c == 'h' || c == 'H')
			&& prev_ws
			&& (starts_with_ci(rest, "http://") || starts_with_ci(rest, "https://"))
		{
			let tok = trim_url(&rest[..url_end(rest)]);
			if let Some(link) = parse_http(tok) {
				flush(&mut text, &mut out, ctx);
				ctx.nodes += 1;
				i += tok.len();
				out.push(Inline::Link(link));
				continue;
			}
		}
		text.push(c);
		i += c.len_utf8();
	}
	flush(&mut text, &mut out, ctx);
	out
}
