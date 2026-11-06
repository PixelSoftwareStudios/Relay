//! The notices model: dismissal by identity, withdrawn notices, the fade of
//! informational ones and the folding of identical messages.

use relay_ui::model::{AppState, Problem};
use relay_ui::notice_words::Severity;
use relay_ui::notices::{Board, INFO_LIFETIME_MS, Notice};

fn problem(code: &str, message: &str) -> Problem {
	Problem {
		code: code.into(),
		message: message.into(),
		retryable: false,
		recovery_action: String::new(),
	}
}

fn state(problems: &[(&str, &str)]) -> AppState {
	AppState {
		warnings: problems.iter().map(|(c, m)| problem(c, m)).collect(),
		..AppState::default()
	}
}

fn titles(board: &Board, s: &AppState) -> Vec<String> {
	board.visible(s).into_iter().map(|n| n.title).collect()
}

fn dismiss_all(board: &mut Board, s: &AppState) {
	let ids: Vec<u64> = board
		.visible(s)
		.iter()
		.flat_map(|n| n.ids.clone())
		.collect();
	board.dismiss(&ids);
}

#[test]
fn a_dismissed_notice_stays_hidden_across_refreshes() {
	let s = state(&[("SOMETHING", "Careful")]);
	let mut board = Board::default();
	board.sync(&s, 0, true);
	assert_eq!(titles(&board, &s), ["Careful"]);
	dismiss_all(&mut board, &s);
	for now in [1, 500, 60_000] {
		board.sync(&s.clone(), now, true);
		assert!(titles(&board, &s).is_empty(), "hidden at {now}");
	}
}

#[test]
fn a_changed_message_is_a_new_occurrence_and_shows_again() {
	let mut board = Board::default();
	let first = state(&[("SOMETHING", "Careful")]);
	board.sync(&first, 0, true);
	dismiss_all(&mut board, &first);
	let second = state(&[("SOMETHING", "Careful, again")]);
	board.sync(&second, 1, true);
	assert_eq!(titles(&board, &second), ["Careful, again"]);
}

#[test]
fn a_warning_raised_again_after_it_was_withdrawn_shows_again() {
	let mut board = Board::default();
	let raised = state(&[("SOMETHING", "Careful")]);
	board.sync(&raised, 0, true);
	dismiss_all(&mut board, &raised);
	let withdrawn = state(&[]);
	board.sync(&withdrawn, 1, true);
	assert!(titles(&board, &withdrawn).is_empty(), "withdrawn: gone");
	board.sync(&raised, 2, true);
	assert_eq!(titles(&board, &raised), ["Careful"]);
}

#[test]
fn identical_messages_fold_into_one_line_with_a_count() {
	let s = state(&[
		(
			"MEDIA_WORKER_FAILED",
			"The daemon's audio playback stopped responding. It was reset.",
		),
		(
			"MEDIA_WORKER_FAILED",
			"The daemon's audio playback stopped responding. It was reset!",
		),
		("OUTPUT_DEVICE_LOST", "gone"),
	]);
	let board = Board::default();
	let shown = board.visible(&s);
	assert_eq!(shown.len(), 2);
	let audio: &Notice = shown.iter().find(|n| n.count == 2).expect("folded");
	assert_eq!(audio.ids.len(), 2);
	let mut board = Board::default();
	board.dismiss(&audio.ids);
	assert_eq!(
		board.visible(&s).len(),
		1,
		"dismissing the line dismisses both"
	);
}

#[test]
fn warnings_come_before_info() {
	let mut s = state(&[("AVATAR_REJECTED", "x"), ("SOMETHING", "y")]);
	s.warnings.push(problem("MEDIA_WORKER_FAILED", "z"));
	let order: Vec<Severity> = Board::default()
		.visible(&s)
		.iter()
		.map(|n| n.severity)
		.collect();
	assert_eq!(
		order,
		[Severity::Warning, Severity::Warning, Severity::Info]
	);
}

#[test]
fn trust_required_is_never_a_notice() {
	let s = state(&[("TRUST_REQUIRED", "`Laptop` is waiting")]);
	assert!(Board::default().visible(&s).is_empty());
}

#[test]
fn an_info_notice_fades_after_ten_seconds_and_a_warning_never_does() {
	let mut s = state(&[
		("AVATAR_REJECTED", "avatar refused"),
		("SOMETHING", "Careful"),
	]);
	s.notice = Some("Background service stopped.".into());
	let mut board = Board::default();
	board.sync(&s, 1_000, true);
	assert_eq!(board.visible(&s).len(), 3);
	board.sync(&s, 1_000 + INFO_LIFETIME_MS - 1, true);
	assert_eq!(board.visible(&s).len(), 3, "not yet");
	board.sync(&s, 1_000 + INFO_LIFETIME_MS, true);
	assert_eq!(titles(&board, &s), ["Careful"]);
	board.sync(&s, 10_000_000, true);
	assert_eq!(titles(&board, &s), ["Careful"], "the warning stays");
}

#[test]
fn hovering_restarts_the_countdown_and_it_waits_while_the_strip_is_not_drawn() {
	let s = state(&[("AVATAR_REJECTED", "avatar refused")]);
	let mut board = Board::default();
	board.sync(&s, 0, true);
	let ids = board.visible(&s)[0].ids.clone();
	board.hold(&ids, 9_000);
	board.sync(&s, INFO_LIFETIME_MS + 1, true);
	assert_eq!(board.visible(&s).len(), 1, "held at 9 s");
	// Away from the strip (Settings) the time does not run.
	board.sync(&s, 500_000, false);
	board.sync(&s, 500_001, true);
	assert_eq!(board.visible(&s).len(), 1);
}

#[test]
fn only_a_counting_down_notice_asks_for_a_repaint() {
	let mut s = state(&[("SOMETHING", "Careful")]);
	let mut board = Board::default();
	board.sync(&s, 0, true);
	assert_eq!(board.next_fade_ms(&board.visible(&s), 0), None);
	s.warnings.push(problem("AVATAR_REJECTED", "x"));
	board.sync(&s, 100, true);
	assert_eq!(
		board.next_fade_ms(&board.visible(&s), 4_100),
		Some(INFO_LIFETIME_MS - 4_000)
	);
}
