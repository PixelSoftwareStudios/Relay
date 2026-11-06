//! Switching the language through a fake shell changes the drawn text at once,
//! and a message a language lacks falls back to English per key. This file is
//! its own test binary because the language is process-global.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use relay_ui::actions::{Actions, Shared};
use relay_ui::controller::{Controller, SessionSlot};
use relay_ui::host_options::{HostOptions, LoginStatus};
use relay_ui::hub::Hub;
use relay_ui::i18n::{self, Catalogs, Language};
use relay_ui::mock::seed;
use relay_ui::shell::{BoxFuture, FilePicker, PickedFile, Shell};
use relay_ui::ui::RelayApp;

struct FakeShell {
	language: Mutex<Language>,
	repaints: AtomicUsize,
	visible: AtomicBool,
}

impl FilePicker for FakeShell {
	fn pick_image(&self) -> BoxFuture<'_, Result<Option<PickedFile>, String>> {
		Box::pin(async { Ok(None) })
	}
}

impl HostOptions for FakeShell {
	fn notify_incoming(&self) -> bool {
		true
	}
	fn set_notify_incoming(&self, _: bool) {}
	fn notify_failure(&self) -> Option<String> {
		None
	}
	fn language(&self) -> Language {
		*self.language.lock().unwrap()
	}
	fn language_choice(&self) -> Option<Language> {
		Some(*self.language.lock().unwrap())
	}
	fn system_language(&self) -> Language {
		Language::En
	}
	fn set_language_choice(&self, choice: Option<Language>) {
		*self.language.lock().unwrap() = choice.unwrap_or(Language::En);
	}
	fn login_status(&self) -> LoginStatus {
		LoginStatus::Off
	}
	fn set_launch_at_login(&self, _: bool) -> Result<(), String> {
		Ok(())
	}
}

impl Shell for FakeShell {
	fn copy_text(&self, _: &str) {}
	fn request_repaint(&self) {
		self.repaints.fetch_add(1, Ordering::SeqCst);
	}
	fn window_visible(&self) -> bool {
		self.visible.load(Ordering::SeqCst)
	}
	fn show_window(&self) {}
	fn hide_window(&self) {}
	fn quit(&self) {}
	fn hides_to_tray(&self) -> bool {
		false
	}
	fn host_options(&self) -> Option<&dyn HostOptions> {
		Some(self)
	}
}

fn texts(shapes: &[egui::epaint::ClippedShape]) -> String {
	fn walk(shape: &egui::Shape, out: &mut String) {
		match shape {
			egui::Shape::Text(t) => {
				out.push_str(t.galley.text());
				out.push('\n');
			}
			egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
			_ => {}
		}
	}
	let mut out = String::new();
	shapes.iter().for_each(|s| walk(&s.shape, &mut out));
	out
}

/// A fake German with two messages; every other message is missing on purpose.
const FAKE_DE: &str = "calls-heading = Anrufe\nlink-connected = Verbunden\n";

#[tokio::test]
async fn changing_the_language_redraws_in_it_with_per_key_fallback() {
	let en = i18n::embedded_source(Language::En);
	i18n::install(Catalogs::from_sources([
		(Language::En, en.as_str()),
		(Language::De, FAKE_DE),
	]));
	let hub = Arc::new(Hub::default());
	hub.update(|s| *s = seed());
	let shell = Arc::new(FakeShell {
		language: Mutex::new(Language::En),
		repaints: AtomicUsize::new(0),
		visible: AtomicBool::new(true),
	});
	let actions = Actions {
		controller: Arc::new(Controller::new(
			hub.clone(),
			Arc::new(SessionSlot::default()),
		)),
		hub,
		shared: Arc::new(Shared::default()),
		runtime: tokio::runtime::Handle::current(),
		shell: shell.clone(),
	};
	let ctx = egui::Context::default();
	let mut app = RelayApp::new(actions, &ctx);
	let frame = |app: &mut RelayApp| {
		let input = egui::RawInput {
			screen_rect: Some(egui::Rect::from_min_size(
				egui::Pos2::ZERO,
				egui::vec2(900.0, 800.0),
			)),
			..Default::default()
		};
		let mut out = ctx.run_ui(input, |ui| {
			app.begin_frame(ui.ctx());
			app.ui(ui);
		});
		out.textures_delta.clear();
		texts(&out.shapes)
	};

	let english = frame(&mut app);
	assert!(english.contains("Calls"), "{english}");
	assert!(english.contains("Nearby peers"));
	assert_eq!(i18n::active(), Language::En);

	// What the picker does: the shell stores the choice; the next frame applies it.
	shell.set_language_choice(Some(Language::De));
	let german = frame(&mut app);
	assert_eq!(i18n::active(), Language::De);
	assert!(german.contains("Anrufe"), "{german}");
	assert!(!german.contains("Calls"), "{german}");
	// A message German lacks is English, key by key.
	assert!(german.contains("Nearby peers"), "{german}");

	shell.set_language_choice(Some(Language::En));
	let back = frame(&mut app);
	assert!(back.contains("Calls") && !back.contains("Anrufe"));
}

#[test]
fn lookup_falls_back_to_english_per_key_and_to_the_id_last() {
	let catalogs = Catalogs::from_sources([
		(Language::En, "a = English a\nb = English b { $n }\n"),
		(Language::De, "a = Deutsch a\nb = Kaputt { $other }\n"),
	]);
	assert_eq!(catalogs.text(Language::De, "a"), "Deutsch a");
	assert_eq!(catalogs.text(Language::Ja, "a"), "English a");
	assert_eq!(catalogs.text(Language::De, "nope"), "nope");
	let n = [("n", i18n::Value::from(3))];
	// A translation that formats with an error falls back to English too.
	assert_eq!(catalogs.text_with(Language::De, "b", &n), "English b 3");
}
