//! Settings, Calls: the Ringtone group (volume, current file, choose, reset, preview) and the
//! friendly words for a refused file.

mod phone;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use phone::{Drawn, Phone};
use relay_ui::capabilities::SettingsCategory;
use relay_ui::engine::{Command, EngineError, Operations, Outcome};
use relay_ui::i18n::Language;
use relay_ui::ringtone_words;

/// Records the commands it is given and refuses ringtones with `refusal` when set.
struct Ops {
	sent: Mutex<Vec<Command>>,
	refusal: Option<&'static str>,
}

#[async_trait]
impl Operations for Ops {
	async fn run(&self, command: Command) -> Result<Outcome, EngineError> {
		let refuse = matches!(command, Command::SetRingtone { .. })
			.then_some(self.refusal)
			.flatten();
		self.sent.lock().unwrap().push(command);
		match refuse {
			Some(code) => Err(EngineError::Rejected {
				code: code.to_owned(),
				message: "engine text".to_owned(),
			}),
			None => Ok(Outcome::default()),
		}
	}
}

fn rig(name: &str) -> (Phone, Arc<Ops>) {
	let mut phone = Phone::new(360.0, 2400.0, Language::En);
	let mut state = phone::busy_state();
	state.settings.as_mut().unwrap().ringtone_name = name.to_owned();
	phone.show_state(&state);
	let ops = Arc::new(Ops {
		sent: Mutex::new(Vec::new()),
		refusal: None,
	});
	phone.slot.set(ops.clone());
	phone.app.open_settings(Some(SettingsCategory::Calls));
	phone.settle();
	(phone, ops)
}

fn has(drawn: &Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l == text)
}

async fn settle_until(phone: &mut Phone, text: &str) -> Drawn {
	for _ in 0..200 {
		let drawn = phone.settle();
		if has(&drawn, text) {
			return drawn;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	panic!("{text:?} never appeared: {:?}", phone.settle().labels);
}

#[tokio::test]
async fn the_group_shows_the_volume_the_current_sound_and_the_buttons() {
	let (mut phone, _ops) = rig("");
	let drawn = phone.settle();
	for text in [
		"Ringtone",
		"Ringtone volume",
		"Current ringtone: Built-in chime",
		"Choose file…",
		"Use built-in chime",
		"Test ring",
	] {
		assert!(has(&drawn, text), "{text}: {:?}", drawn.labels);
	}
}

#[tokio::test]
async fn a_chosen_file_is_named_and_reset_goes_back_to_the_chime() {
	let (mut phone, ops) = rig("morning.mp3");
	let drawn = phone.settle();
	assert!(
		has(&drawn, "Current ringtone: morning.mp3"),
		"{:?}",
		drawn.labels
	);
	phone.click_label("Use built-in chime");
	settle_until(&mut phone, "Saved.").await;
	assert!(ops.sent.lock().unwrap().contains(&Command::ResetRingtone));
}

#[tokio::test]
async fn reset_is_unavailable_while_the_chime_is_in_use() {
	let (mut phone, ops) = rig("");
	phone.click_label("Use built-in chime");
	phone.settle();
	assert!(!ops.sent.lock().unwrap().contains(&Command::ResetRingtone));
}

#[tokio::test]
async fn a_shell_without_a_picker_says_so_in_words() {
	let (mut phone, ops) = rig("");
	phone.click_label("Choose file…");
	settle_until(&mut phone, "This device cannot choose a ringtone file.").await;
	assert!(
		ops.sent.lock().unwrap().is_empty(),
		"nothing reached the engine"
	);
}

#[tokio::test]
async fn preview_asks_the_engine_to_ring() {
	let (mut phone, ops) = rig("x.ogg");
	phone.click_label("Test ring");
	phone.settle();
	for _ in 0..100 {
		if ops.sent.lock().unwrap().contains(&Command::PreviewTone) {
			return;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	panic!("preview never sent");
}

#[test]
fn every_refusal_code_has_friendly_words_and_unknown_codes_have_none() {
	for code in [
		"RINGTONE_EMPTY",
		"RINGTONE_TOO_LARGE",
		"RINGTONE_TOO_LONG",
		"RINGTONE_TOO_SHORT",
		"RINGTONE_UNSUPPORTED",
		"RINGTONE_CORRUPT",
		"RINGTONE_SILENT",
	] {
		let words = ringtone_words::words(code).unwrap_or_else(|| panic!("{code}"));
		assert!(
			!words.contains("RINGTONE") && words.ends_with('.'),
			"{words}"
		);
	}
	assert!(ringtone_words::words("INVALID_REQUEST").is_none());
	let error = ringtone_words::control_error(EngineError::Rejected {
		code: "RINGTONE_TOO_LARGE".into(),
		message: "the file is larger than 100 bytes".into(),
	});
	assert!(
		error.to_string().starts_with("That file is too big"),
		"{error}"
	);
	let other = ringtone_words::control_error(EngineError::Timeout);
	assert!(!other.to_string().contains("too big"));
}

#[tokio::test]
async fn a_refused_file_reaches_the_user_as_a_sentence_not_a_code() {
	use relay_ui::controller::{Controller, SessionSlot};
	use relay_ui::hub::Hub;
	let slot = Arc::new(SessionSlot::default());
	slot.set(Arc::new(Ops {
		sent: Mutex::new(Vec::new()),
		refusal: Some("RINGTONE_UNSUPPORTED"),
	}));
	let controller = Controller::new(Arc::new(Hub::default()), slot);
	let error = controller
		.set_ringtone("a.xyz", vec![1, 2, 3])
		.await
		.unwrap_err();
	assert_eq!(
		error.to_string(),
		"Relay cannot play that kind of file. Use MP3, Ogg Vorbis, FLAC, WAV or AAC."
	);
	let empty = controller
		.set_ringtone("a.mp3", Vec::new())
		.await
		.unwrap_err();
	assert_eq!(empty.to_string(), "That file is empty.");
}
