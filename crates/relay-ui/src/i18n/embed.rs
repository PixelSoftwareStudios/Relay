//! The catalogs compiled into the binary: every `localizations/<lang>/*.ftl`
//! the build finds, with no list to maintain. Release builds embed the files;
//! debug builds read them from disk on each call (rust-embed's default), so a
//! translator edits an `.ftl` and restarts the app without a rebuild.

use rust_embed::RustEmbed;

use super::Language;

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../localizations"]
#[include = "*.ftl"]
struct Localizations;

/// The section files of `language` as `(section, source)`, sorted by section
/// name. A section is a file stem (`calls` for `calls.ftl`).
pub fn sections(language: Language) -> Vec<(String, String)> {
	let prefix = format!("{}/", language.code());
	let mut out: Vec<(String, String)> = Localizations::iter()
		.filter_map(|path| {
			let name = path.strip_prefix(&prefix)?.strip_suffix(".ftl")?;
			let file = Localizations::get(&path)?;
			let source = String::from_utf8(file.data.into_owned()).ok()?;
			Some((name.to_owned(), source))
		})
		.collect();
	out.sort();
	out
}

/// All sections of `language` merged into one source (one bundle per language).
pub fn source(language: Language) -> String {
	sections(language)
		.into_iter()
		.map(|(_, source)| source)
		.collect::<Vec<_>>()
		.join("\n")
}
