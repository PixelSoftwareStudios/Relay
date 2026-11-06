//! The Storage and limits settings page: the values it edits, the keys it sends, the outcome
//! it shows for each (applied, saved until restart, rejected) and Reset to defaults, against
//! a fake daemon; and that it fits the screen in the longest languages.

mod phone;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use phone::{LANGUAGES, Phone};
use relay_config::prefs::limits::SPECS;
use relay_ui::capabilities::SettingsCategory;
use relay_ui::controller::{Controller, SessionSlot};
use relay_ui::engine::{Command, EngineError, Operations, Outcome, SettingOutcome, SettingResult};
use relay_ui::hub::Hub;
use relay_ui::i18n::Language;
use relay_ui::limits_settings::{DEFS, LimitValues, LimitsPanel, Loaded, outcome_text};

const MIB: u64 = 1024 * 1024;

/// A daemon that stores the limits and answers per key as scripted.
struct FakeDaemon {
	store: Mutex<BTreeMap<String, String>>,
	sent: Mutex<Vec<Vec<(String, String)>>>,
	/// Keys answered "saved, restart" and keys answered "rejected" with a reason.
	restart: Vec<&'static str>,
	reject: Vec<&'static str>,
}

impl FakeDaemon {
	fn new(restart: &[&'static str], reject: &[&'static str]) -> Arc<Self> {
		let store = DEFS
			.iter()
			.map(|d| (d.key.to_owned(), d.default.to_string()))
			.collect();
		Arc::new(Self {
			store: Mutex::new(store),
			sent: Mutex::default(),
			restart: restart.to_vec(),
			reject: reject.to_vec(),
		})
	}

	fn with(self: Arc<Self>, key: &str, value: u32) -> Arc<Self> {
		self.store
			.lock()
			.unwrap()
			.insert(key.to_owned(), value.to_string());
		self
	}
}

#[async_trait]
impl Operations for FakeDaemon {
	async fn run(&self, _command: Command) -> Result<Outcome, EngineError> {
		Ok(Outcome::default())
	}

	async fn get_settings(&self) -> Result<BTreeMap<String, String>, EngineError> {
		Ok(self.store.lock().unwrap().clone())
	}

	async fn limit_usage(&self) -> Result<BTreeMap<String, u64>, EngineError> {
		Ok(BTreeMap::from([
			("limits.avatar_cache_mib".to_owned(), 3 * MIB),
			("limits.messages_retained_mib".to_owned(), MIB),
		]))
	}

	async fn set_settings(
		&self,
		values: Vec<(String, String)>,
	) -> Result<Vec<SettingResult>, EngineError> {
		self.sent.lock().unwrap().push(values.clone());
		let mut out = Vec::new();
		for (key, value) in values {
			let (outcome, reason) = if self.reject.contains(&key.as_str()) {
				(SettingOutcome::Rejected, "too small to be useful")
			} else if self.restart.contains(&key.as_str()) {
				(
					SettingOutcome::SavedNeedsRestart,
					"read when the daemon starts",
				)
			} else {
				(SettingOutcome::AppliedLive, "")
			};
			if outcome != SettingOutcome::Rejected {
				self.store.lock().unwrap().insert(key.clone(), value);
			}
			out.push(SettingResult {
				key,
				outcome,
				reason: reason.into(),
			});
		}
		Ok(out)
	}
}

fn controller(ops: Arc<FakeDaemon>) -> Controller {
	let slot = Arc::new(SessionSlot::default());
	slot.set(ops);
	Controller::new(Arc::new(Hub::default()), slot)
}

async fn until(mut done: impl FnMut() -> bool) {
	for _ in 0..100 {
		if done() {
			return;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	panic!("condition not reached in time");
}

#[test]
fn the_ranges_and_defaults_are_the_engines() {
	assert_eq!(DEFS.len(), SPECS.len());
	for (def, spec) in DEFS.iter().zip(&SPECS) {
		assert_eq!(def.key, spec.key);
		assert_eq!(
			(def.min, def.max, def.default),
			(spec.min, spec.max, spec.default)
		);
	}
}

#[tokio::test]
async fn reads_the_values_and_the_usage() {
	let loaded = controller(FakeDaemon::new(&[], &[]).with("limits.history_max_entries", 50))
		.limits_settings()
		.await
		.unwrap();
	assert_eq!(loaded.values.0[3], 50);
	assert_eq!(loaded.usage["limits.avatar_cache_mib"], 3 * MIB);
	let mut dump = BTreeMap::new();
	let error = LimitValues::from_dump(&dump).unwrap_err();
	assert!(error.contains("limits.avatar_cache_mib"), "{error}");
	dump.extend(DEFS.iter().map(|d| (d.key.to_owned(), "x".to_owned())));
	assert!(LimitValues::from_dump(&dump).is_err(), "no silent default");
}

#[tokio::test]
async fn saving_sends_only_the_changed_keys_and_nothing_when_nothing_changed() {
	let daemon = FakeDaemon::new(&[], &[]);
	let ctl = controller(daemon.clone());
	let stored = LimitValues::default();
	assert!(ctl.save_limits(&stored, &stored).await.unwrap().is_empty());
	assert!(daemon.sent.lock().unwrap().is_empty());
	let mut wanted = stored;
	wanted.0[0] = 64;
	wanted.0[4] = 30;
	let results = ctl.save_limits(&stored, &wanted).await.unwrap();
	assert_eq!(
		*daemon.sent.lock().unwrap(),
		[vec![
			("limits.avatar_cache_mib".to_owned(), "64".to_owned()),
			("limits.history_max_age_days".to_owned(), "30".to_owned()),
		]]
	);
	assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn a_bad_draft_never_leaves_the_window() {
	let daemon = FakeDaemon::new(&[], &[]);
	let ctl = controller(daemon.clone());
	let stored = LimitValues::default();
	let mut wanted = stored;
	wanted.0[0] = 4;
	let error = ctl.save_limits(&stored, &wanted).await.unwrap_err();
	assert!(error.to_string().contains("16"), "{error}");
	let mut pair = stored;
	pair.0[1] = 512;
	pair.0[2] = 256;
	assert!(pair.check().is_err(), "ended chats below one chat");
	assert!(daemon.sent.lock().unwrap().is_empty());
}

#[test]
fn every_outcome_is_worded_with_its_reason() {
	let result = |outcome, reason: &str| SettingResult {
		key: "limits.avatar_cache_mib".into(),
		outcome,
		reason: reason.into(),
	};
	relay_ui::i18n::set_active(Language::En);
	assert_eq!(
		outcome_text(&result(SettingOutcome::AppliedLive, "")),
		"Picture cache: applied now"
	);
	assert_eq!(
		outcome_text(&result(SettingOutcome::SavedNeedsRestart, "read at start")),
		"Picture cache: saved, takes effect after a restart (read at start)"
	);
	assert_eq!(
		outcome_text(&result(SettingOutcome::Rejected, "too small")),
		"Picture cache: rejected (too small)"
	);
	assert_eq!(
		outcome_text(&result(SettingOutcome::Rejected, "")),
		"Picture cache: rejected"
	);
}

#[test]
fn the_panel_tracks_draft_revert_and_reset() {
	let mut panel = LimitsPanel::default();
	let mut stored = LimitValues::default();
	stored.0[3] = 10;
	panel.loaded(Ok(Loaded {
		values: stored,
		usage: BTreeMap::new(),
	}));
	assert!(!panel.dirty());
	assert!(panel.resettable(), "10 entries is not the default");
	panel.reset_to_defaults();
	assert!(panel.dirty(), "the defaults differ from what is stored");
	assert!(!panel.resettable());
	assert_eq!(
		panel.draft.changes_from(&stored),
		[("limits.history_max_entries".to_owned(), "200".to_owned())]
	);
	panel.revert();
	assert!(!panel.dirty());
}

fn labels_contain(labels: &[String], text: &str) -> bool {
	labels.iter().any(|l| l == text)
}

async fn open(phone: &mut Phone) -> phone::Drawn {
	phone.app.open_settings(Some(SettingsCategory::Limits));
	let mut drawn = phone.settle();
	for _ in 0..50 {
		if labels_contain(&drawn.labels, "Reset to defaults") {
			return drawn;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
		drawn = phone.settle();
	}
	panic!("the page did not load: {:?}", drawn.labels);
}

async fn wait_for(phone: &mut Phone, text: &str) -> phone::Drawn {
	let mut drawn = phone.settle();
	for _ in 0..50 {
		if labels_contain(&drawn.labels, text) {
			return drawn;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
		drawn = phone.settle();
	}
	panic!("no {text:?} in {:?}", drawn.labels);
}

#[tokio::test]
async fn the_page_shows_limits_usage_and_what_happens_when_full() {
	let mut phone = Phone::new(360.0, 5000.0, Language::En);
	phone.slot.set(FakeDaemon::new(&[], &[]));
	let drawn = open(&mut phone).await;
	for text in [
		"Picture cache (MiB)",
		"Memory for one running chat (MiB)",
		"Memory for ended chats (MiB)",
		"Calls kept in history (entries)",
		"Keep calls for (days)",
		"Files at the same time (transfers)",
		"In use: 3.0 MiB",
		"Ended chats use 1.0 MiB",
	] {
		assert!(
			labels_contain(&drawn.labels, text),
			"{text}: {:?}",
			drawn.labels
		);
	}
	assert!(
		drawn
			.labels
			.iter()
			.any(|l| l.contains("the oldest ended chats are removed first")),
		"{:?}",
		drawn.labels
	);
}

#[tokio::test]
async fn reset_to_defaults_then_save_sends_only_what_differs_and_shows_the_answer() {
	let daemon = FakeDaemon::new(
		&["limits.history_max_age_days"],
		&["limits.avatar_cache_mib"],
	)
	.with("limits.avatar_cache_mib", 32)
	.with("limits.history_max_age_days", 30)
	.with("limits.transfer_concurrency", 2);
	let mut phone = Phone::new(360.0, 5000.0, Language::En);
	phone.slot.set(daemon.clone());
	open(&mut phone).await;
	phone.click_label("Reset to defaults");
	phone.click_label("Save");
	until(|| !daemon.sent.lock().unwrap().is_empty()).await;
	assert_eq!(
		*daemon.sent.lock().unwrap(),
		[vec![
			("limits.avatar_cache_mib".to_owned(), "128".to_owned()),
			("limits.history_max_age_days".to_owned(), "90".to_owned()),
		]]
	);
	let drawn = wait_for(
		&mut phone,
		"Picture cache: rejected (too small to be useful)",
	)
	.await;
	assert!(labels_contain(
		&drawn.labels,
		"Keep calls for: saved, takes effect after a restart (read when the daemon starts)"
	));
}

#[tokio::test]
async fn an_applied_save_says_so() {
	let daemon = FakeDaemon::new(&[], &[]).with("limits.history_max_entries", 5);
	let mut phone = Phone::new(360.0, 5000.0, Language::En);
	phone.slot.set(daemon.clone());
	open(&mut phone).await;
	phone.click_label("Reset to defaults");
	phone.click_label("Save");
	wait_for(&mut phone, "Calls kept in history: applied now").await;
}

#[tokio::test]
async fn an_unreachable_daemon_shows_a_retry_not_a_blank_page() {
	let mut phone = Phone::new(360.0, 3000.0, Language::En);
	phone.app.open_settings(Some(SettingsCategory::Limits));
	let drawn = phone.settle();
	let mut found = false;
	for _ in 0..30 {
		if labels_contain(&phone.settle().labels, "Try again") {
			found = true;
			break;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	assert!(found, "{:?}", drawn.labels);
}

#[tokio::test]
async fn nothing_overflows_and_touch_targets_stay_large() {
	let mut bad = Vec::new();
	for language in LANGUAGES {
		for (desktop, w, h) in [
			(true, 720.0, 5000.0),
			(false, 360.0, 5000.0),
			(false, 360.0, 800.0),
		] {
			let mut phone = if desktop {
				Phone::desktop(w, h, language)
			} else {
				Phone::new(w, h, language)
			};
			phone
				.slot
				.set(FakeDaemon::new(&["limits.transfer_concurrency"], &[]));
			phone.app.open_settings(Some(SettingsCategory::Limits));
			let mut drawn = phone.settle();
			for _ in 0..50 {
				if drawn
					.labels
					.iter()
					.any(|l| l.contains("MiB") || l.contains("МиБ"))
				{
					break;
				}
				tokio::time::sleep(Duration::from_millis(10)).await;
				drawn = phone.settle();
			}
			let tag = format!("{language:?} {w}x{h}");
			assert!(
				drawn
					.labels
					.iter()
					.any(|l| l.contains("MiB") || l.contains("МиБ")),
				"{tag}: not loaded {:?}",
				drawn.labels
			);
			let screen = phone.screen().expand(1.0);
			for r in drawn.widgets.iter().chain(&drawn.painted) {
				if r.min.x < screen.min.x || r.max.x > screen.max.x {
					bad.push(format!("{tag}: overflow {r:?}"));
				}
			}
			if !desktop {
				for r in drawn.tappable.iter().filter(|r| r.width() > 12.0) {
					if r.height() < 43.5 || r.width() < 43.5 {
						bad.push(format!("{tag}: small target {:?}", r.size()));
					}
				}
			}
		}
	}
	bad.sort();
	bad.dedup();
	assert!(
		bad.is_empty(),
		"{} problems: {:#?}",
		bad.len(),
		&bad[..bad.len().min(8)]
	);
}
