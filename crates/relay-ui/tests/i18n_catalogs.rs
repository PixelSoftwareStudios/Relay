//! Catalog hygiene. Translators run exactly this before handing a language
//! in: `cargo test -p relay-ui --test i18n_catalogs`.

use std::path::Path;

use relay_ui::i18n::check::{layout_problems, problems, translation_problems};

fn root() -> std::path::PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../../localizations")
}

#[test]
fn every_catalog_is_consistent_with_english() {
	let found = problems();
	assert!(found.is_empty(), "catalog problems:\n{}", found.join("\n"));
}

#[test]
fn every_language_folder_has_the_section_files_of_english() {
	let found = layout_problems(&root());
	assert!(found.is_empty(), "layout problems:\n{}", found.join("\n"));
}

#[test]
fn english_defines_something_in_every_section() {
	for section in relay_ui::i18n::SECTIONS {
		let text =
			std::fs::read_to_string(root().join("en").join(format!("{section}.ftl"))).unwrap();
		assert!(
			text.lines().any(|l| l.contains(" =")),
			"en/{section}.ftl is empty"
		);
		assert!(text.lines().count() < 500, "en/{section}.ftl is too long");
	}
}

const EN: [(&str, &str); 2] = [
	(
		"calls",
		"greeting = Hello { $name }\nitems =\n    { $count ->\n        [one] One\n       *[other] { $count }\n    }\n",
	),
	("peers", "bye = Bye\n"),
];

fn check(sections: &[(&str, &str)]) -> Vec<String> {
	translation_problems(&EN, "xx", sections)
}

#[test]
fn a_matching_translation_passes() {
	let ok = [
		(
			"calls",
			"greeting = Hallo { $name }\nitems =\n    { $count ->\n        [one] Eins\n       *[other] { $count } mal\n    }\n",
		),
		("peers", ""),
	];
	assert_eq!(check(&ok), Vec::<String>::new());
}

#[test]
fn a_changed_placeholder_is_reported() {
	let found = check(&[("calls", "greeting = Hallo { $nom }\n")]);
	assert_eq!(found.len(), 1, "{found:?}");
	assert!(found[0].contains("greeting") && found[0].contains("variables"));
}

#[test]
fn a_missing_placeholder_is_reported() {
	let found = check(&[("calls", "greeting = Hallo\n")]);
	assert!(found.iter().any(|p| p.contains("greeting")), "{found:?}");
}

#[test]
fn a_key_english_lacks_is_reported() {
	let found = check(&[("calls", "extra = Zusatz\n")]);
	assert!(
		found
			.iter()
			.any(|p| p.contains("extra does not exist in en"))
	);
}

#[test]
fn a_key_in_the_wrong_section_is_reported() {
	let found = check(&[("peers", "greeting = Hallo { $name }\n")]);
	assert!(
		found.iter().any(|p| p.contains("belongs in calls.ftl")),
		"{found:?}"
	);
}

#[test]
fn a_key_defined_in_two_sections_is_reported() {
	let found = check(&[("calls", "bye = Tschuess\n"), ("peers", "bye = Tschuess\n")]);
	assert!(
		found.iter().any(|p| p.contains("also defined in")),
		"{found:?}"
	);
}

#[test]
fn a_syntax_error_is_reported() {
	let found = check(&[("calls", "greeting = Hallo { $name\n")]);
	assert!(
		found.iter().any(|p| p.contains("syntax error")),
		"{found:?}"
	);
}
