//! Relative times of the call history, against a fixed clock and zone.

use relay_io::localtime::LocalZone;
use relay_ui::history_time::{Relative, duration, relative};
use relay_ui::i18n::{self, Language};

/// 2026-11-05 12:00:00 UTC.
const NOON: u64 = 1_793_880_000_000;
const MIN: u64 = 60_000;
const HOUR: u64 = 60 * MIN;
const DAY: u64 = 24 * HOUR;

fn utc(now: u64, then: u64) -> Relative {
	relative(now, then, &LocalZone::utc())
}

#[test]
fn recent_times_count_minutes_then_hours() {
	assert_eq!(utc(NOON, NOON - 30_000), Relative::JustNow);
	assert_eq!(utc(NOON, NOON - 5 * MIN), Relative::MinutesAgo(5));
	assert_eq!(utc(NOON, NOON - 59 * MIN), Relative::MinutesAgo(59));
	assert_eq!(utc(NOON, NOON - 3 * HOUR - 20 * MIN), Relative::HoursAgo(3));
}

#[test]
fn a_clock_that_moved_back_reads_just_now() {
	assert_eq!(utc(NOON, NOON + 10 * MIN), Relative::JustNow);
}

#[test]
fn yesterday_and_older_days_follow_the_calendar_not_24_hours() {
	// 11:00 the day before is more than 24 h ago at 12:00? No: 25 h, still "yesterday".
	assert_eq!(
		utc(NOON, NOON - DAY - HOUR),
		Relative::Yesterday("11:00".into())
	);
	// 20 minutes before midnight, seen just after it, is minutes ago, not a day.
	let after_midnight = NOON - 12 * HOUR + 10 * MIN;
	assert_eq!(
		utc(after_midnight, after_midnight - 20 * MIN),
		Relative::MinutesAgo(20)
	);
	// ...but two hours before midnight is yesterday.
	assert_eq!(
		utc(after_midnight, after_midnight - 2 * HOUR),
		Relative::Yesterday("22:10".into())
	);
	assert_eq!(utc(NOON, NOON - 3 * DAY), Relative::DaysAgo(3));
	assert_eq!(utc(NOON, NOON - 6 * DAY), Relative::DaysAgo(6));
	assert_eq!(
		utc(NOON, NOON - 7 * DAY),
		Relative::Date("2026-10-29".into())
	);
}

#[test]
fn the_zone_decides_where_a_day_begins() {
	let plus3 = LocalZone::fixed(3 * 3600).unwrap();
	// 22:00 UTC is 01:00 the next day at +03:00; 20:30 UTC is 23:30 the day before that.
	let now = NOON + 10 * HOUR;
	let then = NOON + 8 * HOUR + 30 * MIN;
	assert_eq!(
		relative(now, then, &plus3),
		Relative::Yesterday("23:30".into())
	);
	assert_eq!(
		relative(now, then, &LocalZone::utc()),
		Relative::HoursAgo(1)
	);
}

#[test]
fn the_words_follow_the_active_language() {
	i18n::set_active(Language::En);
	assert_eq!(Relative::JustNow.text(), "Just now");
	assert_eq!(Relative::MinutesAgo(5).text(), "5 min ago");
	assert_eq!(Relative::HoursAgo(3).text(), "3 h ago");
	assert_eq!(
		Relative::Yesterday("14:03".into()).text(),
		"Yesterday, 14:03"
	);
	assert_eq!(Relative::DaysAgo(1).text(), "1 day ago");
	assert_eq!(Relative::DaysAgo(4).text(), "4 days ago");
	assert_eq!(Relative::Date("2026-01-02".into()).text(), "2026-01-02");
	assert!(Relative::MinutesAgo(5).is_recent() && !Relative::DaysAgo(2).is_recent());
}

#[test]
fn durations_are_minutes_and_seconds_or_hours() {
	assert_eq!(duration(0), "0:00");
	assert_eq!(duration(65_000), "1:05");
	assert_eq!(duration(3_599_999), "59:59");
	assert_eq!(duration(3_725_000), "1:02:05");
}
