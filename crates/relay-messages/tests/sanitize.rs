use proptest::prelude::*;
use relay_messages::sanitize::*;

#[test]
fn text_accepts_plain_unicode_and_whitespace() {
	assert_eq!(
		sanitize_text("héllo\twörld\nline 2 日本語").unwrap(),
		"héllo\twörld\nline 2 日本語"
	);
	assert_eq!(sanitize_text("a\r\nb").unwrap(), "a\nb");
	assert_eq!(
		sanitize_text("family 👨\u{200D}👩\u{200D}👧").unwrap(),
		"family 👨\u{200D}👩\u{200D}👧"
	);
}

#[test]
fn text_refuses_controls() {
	for c in [
		'\0', '\u{1}', '\u{7}', '\u{1b}', '\u{7f}', '\r', '\u{85}', '\u{9f}',
	] {
		let r = sanitize_text(&format!("a{c}b"));
		assert!(matches!(r, Err(TextError::Control(_))), "{c:?} -> {r:?}");
	}
}

#[test]
fn text_refuses_bidi_zero_width_and_format() {
	for c in [
		'\u{202A}',
		'\u{202B}',
		'\u{202C}',
		'\u{202D}',
		'\u{202E}',
		'\u{2066}',
		'\u{2067}',
		'\u{2068}',
		'\u{2069}',
		'\u{200B}',
		'\u{200C}',
		'\u{200E}',
		'\u{200F}',
		'\u{2060}',
		'\u{FEFF}',
		'\u{00AD}',
		'\u{061C}',
		'\u{E0041}',
	] {
		let r = sanitize_text(&format!("a{c}b"));
		assert!(matches!(r, Err(TextError::Format(_))), "{c:?} -> {r:?}");
	}
	// ZWJ between ASCII is a smuggling vector, only emoji-style joins pass.
	assert!(sanitize_text("a\u{200D}b").is_err());
	assert!(sanitize_text("👨\u{200D}b").is_err());
}

#[test]
fn text_limits_and_empty() {
	assert_eq!(sanitize_text("   \n\t"), Err(TextError::Empty));
	assert_eq!(sanitize_text(""), Err(TextError::Empty));
	assert!(sanitize_text(&"a".repeat(4096)).is_ok());
	assert!(matches!(
		sanitize_text(&"a".repeat(4097)),
		Err(TextError::TooLong { .. })
	));
	// 4096 bytes of multibyte is fine, 4097 is not.
	assert!(sanitize_text(&"é".repeat(2048)).is_ok());
	assert!(sanitize_text(&format!("{}a", "é".repeat(2048))).is_err());
}

#[test]
fn display_names_are_stripped_not_refused() {
	assert_eq!(
		sanitize_display_name("Al\u{202E}ice\u{200B}  Smith\n"),
		"Alice Smith"
	);
	assert_eq!(sanitize_display_name(&"é".repeat(100)).len(), 64);
	assert_eq!(sanitize_display_name("\u{202E}\u{0}"), "");
}

#[test]
fn file_name_table() {
	let cases: &[(&str, &str)] = &[
		("report.pdf", "report.pdf"),
		("../../etc/passwd", "passwd"),
		("C:\\Users\\bob\\evil.exe", "evil.exe"),
		("a/b\\c.txt", "c.txt"),
		(".bashrc", "bashrc"),
		("...hidden", "hidden"),
		("..", "file"),
		(".", "file"),
		("", "file"),
		("   ", "file"),
		("name. ", "name"),
		("tab\there.txt", "tabhere.txt"),
		("nul\0byte.txt", "nulbyte.txt"),
		("bidi\u{202E}gpj.exe", "bidigpj.exe"),
		("zero\u{200B}width.txt", "zerowidth.txt"),
		("a<b>c:d\"e|f?g*h.txt", "a_b_c_d_e_f_g_h.txt"),
		("CON", "_CON"),
		("con.txt", "_con.txt"),
		("NUL.tar.gz", "_NUL.tar.gz"),
		("COM1", "_COM1"),
		("lpt9.log", "_lpt9.log"),
		("COM10.txt", "COM10.txt"),
		("console.txt", "console.txt"),
		("dir/", "file"),
		("ünïcödé.txt", "ünïcödé.txt"),
	];
	for (input, want) in cases {
		assert_eq!(sanitize_file_name(input).as_str(), *want, "input {input:?}");
	}
}

#[test]
fn long_names_keep_extension_and_char_boundaries() {
	let n = sanitize_file_name(&format!("{}.tar.gz", "x".repeat(300)));
	assert!(n.as_str().len() <= 120);
	assert!(n.as_str().ends_with(".gz"));
	let n = sanitize_file_name(&format!("{}.txt", "é".repeat(200)));
	assert!(n.as_str().len() <= 120 && n.as_str().ends_with(".txt"));
	let n = sanitize_file_name(&"y".repeat(500));
	assert_eq!(n.as_str().len(), 120);
}

#[test]
fn media_type_is_advisory_and_executables_flagged() {
	assert_eq!(guess_media_type("a.PNG"), "image/png");
	assert_eq!(guess_media_type("noext"), "application/octet-stream");
	assert!(looks_executable("setup.EXE"));
	assert!(looks_executable("run.sh"));
	assert!(!looks_executable("notes.txt"));
}

const WS: &str =
	"[ \\t\\n\\u{a0}\\u{85}\\u{2000}-\\u{200a}\\u{2028}\\u{2029}\\u{3000}a-zé.]{0,150}";

proptest! {
	#![proptest_config(ProptestConfig::with_cases(4000))]

	#[test]
	fn text_sanitizer_is_idempotent(s in "\\PC{0,200}|[\\x00-\\x20\\u{200B}-\\u{202E}a-z]{0,50}") {
		if let Ok(once) = sanitize_text(&s) {
			prop_assert_eq!(sanitize_text(&once), Ok(once.clone()));
			prop_assert!(!once.chars().any(|c| c.is_control() && c != '\n' && c != '\t'));
			let zwj = '\u{200D}';
			prop_assert!(once.chars().filter(|c| *c != zwj).all(|c| !is_format_char(c)));
		}
	}

	#[test]
	fn file_names_are_always_safe_and_idempotent(s in "\\PC{0,300}|[./\\\\a-zA-Z0-9 :<>]{0,60}") {
		let n = sanitize_file_name(&s);
		let t = n.as_str();
		prop_assert!(!t.is_empty() && t.len() <= 120);
		prop_assert!(!t.contains(['/', '\\', '\0', ':', '<', '>', '"', '|', '?', '*']));
		prop_assert!(!t.starts_with('.') && !t.ends_with('.') && !t.ends_with(' '));
		prop_assert!(!t.chars().any(|c| c.is_control() || is_format_char(c)));
		prop_assert_eq!(sanitize_file_name(t), n);
	}

	#[test]
	fn display_names_are_bounded(s in "\\PC{0,300}") {
		let n = sanitize_display_name(&s);
		prop_assert!(n.len() <= 64);
		prop_assert!(!n.chars().any(|c| c.is_control() || is_format_char(c)));
		prop_assert_eq!(sanitize_display_name(&n), n);
	}

	#[test]
	fn unicode_whitespace_cases_are_idempotent(s in WS) {
		let d = sanitize_display_name(&s);
		prop_assert_eq!(sanitize_display_name(&d), d.clone());
		prop_assert!(d == d.trim() && d.len() <= 64);
		let f = sanitize_file_name(&s);
		prop_assert_eq!(sanitize_file_name(f.as_str()), f);
		if let Ok(t) = sanitize_text(&s) {
			prop_assert_eq!(sanitize_text(&t), Ok(t.clone()));
		}
		let long = format!("{s}{}", "é ".repeat(40));
		let d = sanitize_display_name(&long);
		prop_assert_eq!(sanitize_display_name(&d), d);
		let f = sanitize_file_name(&format!("{long}.{s}"));
		prop_assert_eq!(sanitize_file_name(f.as_str()), f);
	}
}
