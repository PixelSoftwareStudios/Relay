//! The time of a history row, against a fixed clock and zone.

use relay_io::localtime::LocalZone;
use relay_ui::history_time::{Relative, duration, relative};
use relay_ui::i18n::{self, Language};
use std::sync::{Mutex, MutexGuard};

/// 2026-11-05 12:00:00 UTC.
const NOON: u64 = 1_793_880_000_000;
const MIN: u64 = 60_000;
const HOUR: u64 = 60 * MIN;
const DAY: u64 = 24 * HOUR;

/// The active language is global: tests that format take turns, in English.
fn english() -> MutexGuard<'static, ()> {
	static LOCK: Mutex<()> = Mutex::new(());
	let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
	i18n::set_active(Language::En);
	guard
}

fn utc(now: u64, then: u64) -> Relative {
	relative(now, then, &LocalZone::utc())
}

fn at(text: &str) -> Relative {
	Relative::At(text.into())
}

#[test]
fn under_a_minute_is_just_now_after_that_the_clock() {
	let _english = english();
	assert_eq!(utc(NOON, NOON - 30_000), Relative::JustNow);
	assert_eq!(utc(NOON, NOON - 5 * MIN), at("11:55 AM"));
	assert_eq!(utc(NOON, NOON - 3 * HOUR - 20 * MIN), at("8:40 AM"));
}

#[test]
fn a_clock_that_moved_back_reads_just_now() {
	let _english = english();
	assert_eq!(utc(NOON, NOON + 10 * MIN), Relative::JustNow);
}

#[test]
fn yesterday_and_older_follow_the_calendar_not_24_hours() {
	let _english = english();
	// 25 hours before noon is 11:00 the day before.
	assert_eq!(utc(NOON, NOON - DAY - HOUR), at("Yesterday at 11:00 AM"));
	// 20 minutes before midnight, seen just after it, is yesterday.
	let after_midnight = NOON - 12 * HOUR + 10 * MIN;
	assert_eq!(
		utc(after_midnight, after_midnight - 20 * MIN),
		at("Yesterday at 11:50 PM")
	);
	// Two days back is the date and the clock.
	assert_eq!(utc(NOON, NOON - 2 * DAY), at("11/03/26, 12:00 PM"));
	assert_eq!(utc(NOON, NOON - 40 * DAY), at("09/26/26, 12:00 PM"));
}

#[test]
fn the_zone_decides_where_a_day_begins() {
	let _english = english();
	let plus3 = LocalZone::fixed(3 * 3600).unwrap();
	// 20:30 UTC is 23:30 at +03:00; 22:00 UTC is already 01:00 the next day.
	let now = NOON + 10 * HOUR;
	let then = NOON + 8 * HOUR + 30 * MIN;
	assert_eq!(relative(now, then, &plus3), at("Yesterday at 11:30 PM"));
	assert_eq!(relative(now, then, &LocalZone::utc()), at("8:30 PM"));
}

#[test]
fn the_words_follow_the_active_language() {
	let _english = english();
	assert_eq!(Relative::JustNow.text(), "Just now");
	assert_eq!(at("1:54 AM").text(), "1:54 AM");
	assert!(Relative::JustNow.is_recent() && !at("1:54 AM").is_recent());
	i18n::set_active(Language::De);
	assert_eq!(utc(NOON, NOON - DAY - HOUR).text(), "Gestern um 11:00");
	assert_eq!(utc(NOON, NOON - 2 * DAY).text(), "03.11.26, 12:00");
	assert_eq!(Relative::JustNow.text(), "Gerade eben");
	i18n::set_active(Language::En);
}

#[test]
fn durations_are_minutes_and_seconds_or_hours() {
	assert_eq!(duration(0), "0:00");
	assert_eq!(duration(65_000), "1:05");
	assert_eq!(duration(3_599_999), "59:59");
	assert_eq!(duration(3_725_000), "1:02:05");
}
