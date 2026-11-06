//! Plain-language wording for the engine's warnings: a title, a one-line next
//! step and, when the engine message holds technical detail (key digests), a
//! text for a "Details" expander. Codes this app does not know fall back to a
//! cleaned version of the engine's message.

use crate::model::Problem;
use crate::tr;

/// How loudly a notice is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
	/// Passing news: fades away on its own.
	Info,
	/// Something to look at: stays until dismissed or withdrawn.
	Warning,
	/// Something is broken: stays until dismissed or withdrawn.
	Error,
}

/// What the user reads for one engine warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Words {
	pub severity: Severity,
	pub title: String,
	pub next_step: Option<String>,
	pub details: Option<String>,
}

/// The engine's recovery hint for a media part that will not restart.
/// Restarted parts say `restarted_capture`, `restarted_render` or `restarted`.
const RESTART_SERVICE: &str = "restart_daemon";

/// The words for `problem`.
pub fn words(problem: &Problem) -> Words {
	let plain = |severity, title: &str, step: &str| Words {
		severity,
		title: title.to_owned(),
		next_step: Some(step.to_owned()),
		details: None,
	};
	match problem.code.as_str() {
		"PEER_KEY_CHANGED" => key_changed(problem),
		"MEDIA_WORKER_FAILED" => match problem.recovery_action.as_str() {
			RESTART_SERVICE => plain(
				Severity::Warning,
				tr!("notice-audio-failed-title"),
				tr!("notice-audio-failed-step"),
			),
			"restarted_capture" => plain(
				Severity::Warning,
				tr!("notice-mic-restarted-title"),
				tr!("notice-worker-restarted-step"),
			),
			"restarted_render" => plain(
				Severity::Warning,
				tr!("notice-render-restarted-title"),
				tr!("notice-worker-restarted-step"),
			),
			_ => plain(
				Severity::Warning,
				tr!("notice-audio-restarted-title"),
				tr!("notice-audio-restarted-step"),
			),
		},
		code if code.contains("STALL") => plain(
			Severity::Warning,
			tr!("notice-audio-restarted-title"),
			tr!("notice-audio-restarted-step"),
		),
		"OUTPUT_DEVICE_LOST" => plain(
			Severity::Warning,
			tr!("notice-output-lost-title"),
			tr!("notice-output-lost-step"),
		),
		"MICROPHONE_UNAVAILABLE" => plain(
			Severity::Warning,
			tr!("notice-mic-title"),
			tr!("notice-mic-step"),
		),
		"AVATAR_REJECTED" => plain(
			Severity::Info,
			tr!("notice-avatar-title"),
			tr!("notice-avatar-step"),
		),
		"PREFERENCES_RECOVERED" => plain(
			Severity::Info,
			tr!("notice-prefs-recovered-title"),
			tr!("notice-prefs-recovered-step"),
		),
		"PREFERENCES_RESET" => plain(
			Severity::Warning,
			tr!("notice-prefs-reset-title"),
			tr!("notice-prefs-reset-step"),
		),
		"TRUST_RECOVERED" => plain(
			Severity::Info,
			tr!("notice-trust-recovered-title"),
			tr!("notice-trust-recovered-step"),
		),
		"TRUST_LOST" => plain(
			Severity::Warning,
			tr!("notice-trust-lost-title"),
			tr!("notice-trust-lost-step"),
		),
		_ => Words {
			severity: Severity::Warning,
			title: clean(&problem.message),
			next_step: None,
			details: None,
		},
	}
}

/// "`Name` presented key A instead of the pinned B; the link was refused": the
/// device name is the backticked part (the API has no structured field for it),
/// the digests stay behind the details.
fn key_changed(problem: &Problem) -> Words {
	let name = backticked(&problem.message);
	let title = match &name {
		Some(name) => tr!("notice-key-changed-title", name = name.as_str()),
		None => tr!("notice-key-changed-title-unnamed").to_owned(),
	};
	Words {
		severity: Severity::Warning,
		title,
		next_step: Some(tr!("notice-key-changed-step").to_owned()),
		details: Some(clean(&problem.message)),
	}
}

/// The first `` `quoted` `` part, if it holds a name.
fn backticked(message: &str) -> Option<String> {
	let rest = message.strip_prefix('`')?;
	let end = rest.find('`')?;
	let name = rest[..end].trim();
	(!name.is_empty()).then(|| name.chars().take(64).collect())
}

/// An engine message for people: no `invalid: ` prefix, no backticks, and the
/// "daemon" is the background service.
pub fn clean(message: &str) -> String {
	let text = message.trim();
	let text = text.strip_prefix("invalid: ").unwrap_or(text);
	let text = text.replace('`', "");
	let mut out = String::with_capacity(text.len());
	let mut rest = text.as_str();
	while let Some(at) = find_word(rest, "daemon") {
		out.push_str(&rest[..at]);
		let upper = rest[at..].starts_with('D');
		out.push_str(if upper {
			"Background service"
		} else {
			"background service"
		});
		rest = &rest[at + "daemon".len()..];
	}
	out.push_str(rest);
	let mut chars = out.chars();
	match chars.next() {
		Some(first) => first.to_uppercase().chain(chars).collect(),
		None => out,
	}
}

/// Where the whole word `word` (any case) starts in `text`.
fn find_word(text: &str, word: &str) -> Option<usize> {
	let lower = text.to_ascii_lowercase();
	let mut from = 0;
	while let Some(i) = lower[from..].find(word) {
		let at = from + i;
		let before = lower[..at].chars().next_back();
		let after = lower[at + word.len()..].chars().next();
		if !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric) {
			return Some(at);
		}
		from = at + word.len();
	}
	None
}
