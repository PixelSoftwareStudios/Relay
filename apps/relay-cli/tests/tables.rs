//! Text table rendering: wrapping, wide characters and plain output.

use relay_cli::commands::table::Layout;
use relay_cli::commands::views::{DeviceList, DeviceView, PeerList, PeerView};

fn peer(name: &str, addresses: &[&str]) -> PeerView {
	PeerView {
		id: "ab".repeat(32),
		name: name.to_owned(),
		trust: "verified",
		reachable: true,
		addresses: addresses.iter().map(|a| (*a).to_owned()).collect(),
	}
}

/// Terminal columns: wide CJK and emoji count as two.
fn unicode_width(s: &str) -> usize {
	s.chars()
		.map(|c| if (c as u32) >= 0x1100 { 2 } else { 1 })
		.sum()
}

#[test]
fn plain_output_has_no_escape_codes_or_wrapping() {
	let long = "very-long-name-".repeat(8);
	let list = PeerList(vec![peer(&long, &["10.0.0.5:7000", "10.0.0.6:7000"])]);
	let text = list.render(Layout::PLAIN);
	assert!(!text.contains('\u{1b}'), "{text:?}");
	let lines: Vec<&str> = text.lines().collect();
	assert_eq!(lines.len(), 2, "header and one row: {text}");
	assert!(lines[1].contains(&long));
	assert!(lines[1].contains("10.0.0.5:7000, 10.0.0.6:7000"));
	assert!(
		lines.iter().all(|l| l == &l.trim_end()),
		"no trailing space"
	);
}

#[test]
fn narrow_width_wraps_without_losing_content() {
	let list = PeerList(vec![peer(
		"Alice Wonderland",
		&["192.168.100.200:7000", "10.0.0.5:7000"],
	)]);
	let text = list.render(Layout::plain_width(60));
	for line in text.lines() {
		assert!(unicode_width(line) <= 60, "line too wide: {line:?}");
	}
	assert!(text.lines().count() > 2, "row should wrap: {text}");
	for part in [
		"Alice",
		"Wonderland",
		"192.168.100.200:7000",
		"10.0.0.5:7000",
	] {
		assert!(text.contains(part), "{part} missing in {text}");
	}
}

#[test]
fn wide_character_names_stay_intact_and_aligned() {
	let list = PeerList(vec![peer("山田太郎", &["10.0.0.5:7000"]), peer("Zoë", &[])]);
	let text = list.render(Layout::plain_width(80));
	assert!(text.contains("山田太郎") && text.contains("Zoë"));
	let col = |line: &str, needle: &str| unicode_width(&line[..line.find(needle).unwrap()]);
	let rows: Vec<&str> = text.lines().collect();
	assert_eq!(
		col(rows[1], "verified"),
		col(rows[2], "verified"),
		"columns misaligned:\n{text}"
	);
}

#[test]
fn empty_results_keep_their_messages() {
	assert_eq!(PeerList(vec![]).render(Layout::PLAIN), "no peers found");
	assert_eq!(
		DeviceList(vec![]).render(Layout::PLAIN),
		"no audio devices reported"
	);
}

#[test]
fn device_rows_mark_selection_default_and_availability() {
	let dev = |id: &str, selected, system_default, available| DeviceView {
		id: id.to_owned(),
		name: format!("Device {id}"),
		direction: "input",
		available,
		system_default,
		selected,
	};
	let text = DeviceList(vec![
		dev("a", true, true, true),
		dev("b", false, false, false),
	])
	.render(Layout::PLAIN);
	let a: Vec<&str> = text.lines().nth(1).unwrap().split_whitespace().collect();
	let b: Vec<&str> = text.lines().nth(2).unwrap().split_whitespace().collect();
	assert_eq!(a.first(), Some(&"*"));
	assert!(a.contains(&"default"));
	assert_ne!(b.first(), Some(&"*"));
	assert!(b.contains(&"unavailable"));
}
