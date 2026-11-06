//! `docs/limits.md` lists every tweakable limit with the defaults and ranges of the code.

use relay_config::prefs::limits::SPECS;

fn cells(line: &str) -> Vec<String> {
	line.trim()
		.trim_matches('|')
		.split('|')
		.map(|c| c.trim().trim_matches('`').to_owned())
		.collect()
}

#[test]
fn the_table_matches_the_code() {
	let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/limits.md");
	let text = std::fs::read_to_string(&path).expect("docs/limits.md");
	for spec in &SPECS {
		let row = text
			.lines()
			.map(cells)
			.find(|c| c.first().is_some_and(|k| k == spec.key))
			.unwrap_or_else(|| panic!("{} is not in docs/limits.md", spec.key));
		let want = [
			spec.default.to_string(),
			spec.min.to_string(),
			spec.max.to_string(),
			spec.unit.to_owned(),
		];
		assert_eq!(row[1..5], want, "{}", spec.key);
	}
}
