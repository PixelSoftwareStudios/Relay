use relay_messages::markdown::*;

fn text(src: &str) -> String {
	parse(src).plain_text()
}

#[test]
fn unsupported_blocks_stay_literal() {
	assert_eq!(text("# title"), "# title");
	assert_eq!(text("> quoted"), "> quoted");
	assert_eq!(text("---"), "---");
	assert_eq!(text("| a | b |\n|---|---|"), "| a | b |\n|---|---|");
	assert!(matches!(parse("# title").blocks[0], Block::Paragraph(_)));
}

#[test]
fn labelled_links_and_images_keep_their_source() {
	assert_eq!(
		text("[click](https://e.example/x)"),
		"[click](https://e.example/x)"
	);
	assert_eq!(
		text("![alt](http://h.example/a.png)"),
		"![alt](http://h.example/a.png)"
	);
}

#[test]
fn nested_lists_and_loose_items_flatten_to_one_run() {
	let d = parse("- a\n  - b\n\n- c\n\n  more");
	let Block::List { items, .. } = &d.blocks[0] else {
		panic!("{:?}", d.blocks);
	};
	assert_eq!(items.len(), 2);
	assert!(items[0].contains(&Inline::LineBreak));
}

#[test]
fn entities_and_escapes_decode() {
	assert_eq!(text("a &amp; b \\*c\\*"), "a & b *c*");
}

#[test]
fn hard_and_soft_breaks_are_line_breaks() {
	assert_eq!(
		parse("a  \nb\nc").blocks[0],
		Block::Paragraph(vec![
			Inline::Text("a".into()),
			Inline::LineBreak,
			Inline::Text("b".into()),
			Inline::LineBreak,
			Inline::Text("c".into()),
		])
	);
}
