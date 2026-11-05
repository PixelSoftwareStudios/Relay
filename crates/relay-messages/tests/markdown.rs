use proptest::prelude::*;
use relay_messages::limits::*;
use relay_messages::markdown::*;

fn para(src: &str) -> Vec<Inline> {
	match parse(src).blocks.into_iter().next() {
		Some(Block::Paragraph(v)) => v,
		other => panic!("not a paragraph: {other:?}"),
	}
}

fn link(src: &str) -> Option<Link> {
	let d = parse(src);
	d.links().into_iter().next().cloned()
}

#[test]
fn inline_styles() {
	let v = para("a **bold** and *it* and `code` z");
	assert_eq!(
		v,
		vec![
			Inline::Text("a ".into()),
			Inline::Bold(vec![Inline::Text("bold".into())]),
			Inline::Text(" and ".into()),
			Inline::Italic(vec![Inline::Text("it".into())]),
			Inline::Text(" and ".into()),
			Inline::Code("code".into()),
			Inline::Text(" z".into()),
		]
	);
	// Unmatched or spaced delimiters stay literal; snake_case is untouched.
	assert_eq!(para("2 * 3 * 4"), vec![Inline::Text("2 * 3 * 4".into())]);
	assert_eq!(
		para("my_var_name **x"),
		vec![Inline::Text("my_var_name **x".into())]
	);
	assert_eq!(para("`open"), vec![Inline::Text("`open".into())]);
}

#[test]
fn code_blocks_keep_exact_bytes() {
	let src = "before\n```rust\nfn main() {\n\tprintln!(\"**not bold** https://x.com\");\n}\n\n  indented\n```\nafter";
	let d = parse(src);
	let c: Vec<_> = d.code_blocks().collect();
	assert_eq!(c.len(), 1);
	assert_eq!(c[0].language.as_deref(), Some("rust"));
	assert_eq!(
		c[0].copy_text(),
		"fn main() {\n\tprintln!(\"**not bold** https://x.com\");\n}\n\n  indented"
	);
	assert!(c[0].closed);
	assert!(d.links().is_empty(), "no links inside code");
	assert_eq!(d.blocks.len(), 3);
}

#[test]
fn code_block_edge_cases() {
	let d = parse("```\nopen forever\nstill");
	let c = d.code_blocks().next().unwrap();
	assert!(!c.closed);
	assert_eq!(c.copy_text(), "open forever\nstill");
	assert_eq!(
		parse("```evil lang\nx\n```")
			.code_blocks()
			.next()
			.unwrap()
			.language,
		None
	);
	assert_eq!(
		parse(&format!("```{}\nx\n```", "a".repeat(40)))
			.code_blocks()
			.next()
			.unwrap()
			.language,
		None
	);
	assert_eq!(
		parse("```c++\nx\n```")
			.code_blocks()
			.next()
			.unwrap()
			.language
			.as_deref(),
		Some("c++")
	);
	assert_eq!(
		parse("```\n```").code_blocks().next().unwrap().copy_text(),
		""
	);
	// Inline triple backticks are not a fence.
	assert_eq!(parse("a ```b``` c").code_blocks().count(), 0);
}

#[test]
fn lists() {
	let d = parse("- one\n- **two**\n* three\n\n3. a\n4) b");
	match &d.blocks[0] {
		Block::List {
			ordered: false,
			items,
			..
		} => assert_eq!(items.len(), 3),
		b => panic!("{b:?}"),
	}
	match &d.blocks[1] {
		Block::List {
			ordered: true,
			start: 3,
			items,
		} => assert_eq!(items.len(), 2),
		b => panic!("{b:?}"),
	}
}

#[test]
fn html_images_and_labelled_links_are_inert() {
	let d = parse(
		"<script>alert(1)</script> <img src=x onerror=y> ![i](http://h.com/a.png) [click](https://evil.example/x)",
	);
	assert!(
		d.plain_text().contains("<script>"),
		"raw html stays literal text"
	);
	// The URL inside the markdown syntax is shown as a plain link, never an image or hidden label.
	for l in d.links() {
		assert!(l.url.starts_with("http"));
		assert!(l.host.ascii == "h.com" || l.host.ascii == "evil.example");
	}
	assert!(d.plain_text().contains("[click]"));
}

#[test]
fn urls_accepted() {
	let l = link("see https://example.com.").unwrap();
	assert_eq!(l.url, "https://example.com");
	assert!(l.secure);
	assert_eq!(l.host.warning, None);
	assert_eq!(
		link("(http://192.168.1.1:8080/a?b=c#d)").unwrap().url,
		"http://192.168.1.1:8080/a?b=c#d"
	);
	assert_eq!(
		link("HTTPS://EXAMPLE.COM/Path").unwrap().host.ascii,
		"example.com"
	);
	assert_eq!(
		link("https://en.wikipedia.org/wiki/Rust_(language)")
			.unwrap()
			.url,
		"https://en.wikipedia.org/wiki/Rust_(language)"
	);
	assert_eq!(link("**https://a.com**").unwrap().url, "https://a.com");
}

#[test]
fn urls_refused_stay_text() {
	for bad in [
		"javascript:alert(1)",
		"data:text/html,<b>x</b>",
		"file:///etc/passwd",
		"ftp://example.com/x",
		"mailto:a@b.c",
		"//example.com",
		"www.example.com",
		"https://good.com@evil.com/",
		"http://user:pw@host/",
		"http://[::1]/",
		"http://host:99999/",
		"http://host:/",
		"http:///path",
		"http://-bad-.com",
		"http://a..b",
		"http://例え.jp/",
		"https://exаmple.com/",
		"httpx://example.com",
	] {
		assert!(link(bad).is_none(), "{bad} must not be linkified");
	}
	// Not at a word boundary.
	assert!(link("xhttps://example.com").is_none());
}

#[test]
fn idn_lookalikes_are_flagged() {
	// "аpple" with a Cyrillic first letter: mixed script label.
	let l = link("https://xn--pple-43d.com/").unwrap();
	assert_eq!(l.host.warning, Some(HostWarning::MixedScript));
	assert_eq!(l.host.ascii, "xn--pple-43d.com");
	assert_ne!(l.host.display, l.host.ascii);
	assert!(!l.host.display.is_ascii());
	// A whole-script label is still an IDN and flagged.
	let l = link("https://xn--80ak6aa92e.com").unwrap();
	assert_eq!(l.host.warning, Some(HostWarning::Idn));
	assert_eq!(l.host.display, "аррӏе.com");
	// Garbage punycode.
	assert_eq!(
		link("https://xn--a.com").unwrap().host.warning,
		Some(HostWarning::InvalidPunycode)
	);
	assert_eq!(
		link("https://xn--99999999999999999.com")
			.unwrap()
			.host
			.warning,
		Some(HostWarning::InvalidPunycode)
	);
	assert!(link("https://xn--.com").is_none());
	// Plain hosts are not flagged.
	assert_eq!(link("https://example.com").unwrap().host.warning, None);
}

#[test]
fn oversized_urls_are_text() {
	let long = format!("https://example.com/{}", "a".repeat(MD_MAX_URL_BYTES));
	assert!(link(&long).is_none());
}

fn depth(v: &[Inline]) -> usize {
	v.iter()
		.map(|i| match i {
			Inline::Bold(c) | Inline::Italic(c) => 1 + depth(c),
			_ => 0,
		})
		.max()
		.unwrap_or(0)
}

fn nodes(v: &[Inline]) -> usize {
	v.iter()
		.map(|i| match i {
			Inline::Bold(c) | Inline::Italic(c) => 1 + nodes(c),
			_ => 1,
		})
		.sum()
}

fn check_bounds(src: &str) -> Result<(), TestCaseError> {
	let d = parse(src);
	prop_assert!(d.blocks.len() <= MD_MAX_BLOCKS + 1);
	let mut total = 0;
	for b in &d.blocks {
		match b {
			Block::Paragraph(v) => {
				prop_assert!(depth(v) <= MD_MAX_DEPTH);
				total += nodes(v);
			}
			Block::List { items, .. } => {
				for it in items {
					prop_assert!(depth(it) <= MD_MAX_DEPTH);
					total += nodes(it);
				}
			}
			Block::Code(_) => total += 1,
		}
	}
	prop_assert!(total <= MD_MAX_NODES + MD_MAX_BLOCKS * 2, "nodes {}", total);
	prop_assert!(d.plain_text().len() <= src.len() + d.blocks.len() + 1);
	prop_assert!(parse(src) == d);
	for l in d.links() {
		prop_assert!(
			l.url.starts_with("http://")
				|| l.url.starts_with("https://")
				|| l.url.to_ascii_lowercase().starts_with("http")
		);
		prop_assert!(
			l.url.is_ascii() && !l.url.contains('@')
				|| l.url.split('/').nth(2).is_some_and(|a| !a.contains('@'))
		);
	}
	Ok(())
}

proptest! {
	#![proptest_config(ProptestConfig::with_cases(256))]

	#[test]
	fn never_panics_on_arbitrary_text(s in "\\PC{0,2000}") {
		check_bounds(&s)?;
	}

	#[test]
	fn bounded_on_markup_heavy_text(parts in prop::collection::vec(
		prop::sample::select(vec!["*", "**", "`", "```", "\n", "- ", "1. ", " ", "a", "https://x.com", "http://", "xn--", "(", ")", "é", "\n```rs\n"]),
		0..600,
	)) {
		check_bounds(&parts.concat())?;
	}

	#[test]
	fn deep_nesting_is_cut(n in 1usize..400) {
		let s = format!("{}x{}", "*a **b ".repeat(n), " b** a*".repeat(n));
		check_bounds(&s)?;
		check_bounds(&"*".repeat(n * 10))?;
		check_bounds(&"- ".repeat(n * 10))?;
	}
}

#[test]
fn many_blocks_degrade_to_text() {
	let src = "- a\n\nb\n\n".repeat(2000);
	let d = parse(&src);
	assert!(d.blocks.len() <= MD_MAX_BLOCKS + 1);
}
