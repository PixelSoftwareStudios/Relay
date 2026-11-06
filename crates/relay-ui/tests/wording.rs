//! People have "devices": no catalog text may name a kind of machine (computer, PC, Mac,
//! phone, laptop). Technical terms belong in the docs only.

use relay_ui::i18n::{Language, embedded_sections};

/// Whole words (case does not matter) that are banned in English text.
const ENGLISH: [&str; 10] = [
	"computer",
	"computers",
	"pc",
	"pcs",
	"mac",
	"macs",
	"laptop",
	"laptops",
	"phone",
	"phones",
];

/// Substrings that name a computer in each other language.
fn others(language: Language) -> &'static [&'static str] {
	match language {
		Language::De => &["Computer", "Rechner", " PC"],
		Language::Es => &["ordenador", "computadora", " equipo"],
		Language::PtBr => &["computador", " PC"],
		Language::Ru => &["компьютер"],
		Language::Ja => &["コンピュータ", "パソコン", " PC"],
		Language::Ko => &["컴퓨터"],
		Language::ZhCn => &["电脑", "计算机"],
		Language::Hi => &["कंप्यूटर", "कम्प्यूटर"],
		Language::En => &[],
	}
}

#[test]
fn no_english_text_names_a_kind_of_machine() {
	let mut found = Vec::new();
	for (section, text) in embedded_sections(Language::En) {
		for line in text.lines().filter(|l| !l.starts_with('#')) {
			let lower = line.to_lowercase();
			for word in lower.split(|c: char| !c.is_alphanumeric()) {
				if ENGLISH.contains(&word) {
					found.push(format!("{section}: {line}"));
				}
			}
		}
	}
	assert!(found.is_empty(), "say \"device\":\n{}", found.join("\n"));
}

#[test]
fn no_translation_names_a_kind_of_machine_either() {
	let mut found = Vec::new();
	for language in Language::ALL {
		for (section, text) in embedded_sections(language) {
			for line in text.lines().filter(|l| !l.starts_with('#')) {
				if others(language).iter().any(|w| line.contains(w)) {
					found.push(format!("{language:?} {section}: {line}"));
				}
			}
		}
	}
	assert!(found.is_empty(), "say \"device\":\n{}", found.join("\n"));
}

#[test]
fn the_sound_help_says_device() {
	assert_eq!(
		relay_ui::i18n::t("ring-help"),
		"Sounds play on this device only."
	);
}
