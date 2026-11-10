//! The app-wide time formats, against fixed instants, zones and languages.

use relay_io::localtime::LocalZone;
use relay_ui::i18n::{self, Language};
use relay_ui::timefmt::{self, Civil, Day, NO_TIME};

/// UTC unix milliseconds of a civil UTC date and time.
fn ms(y: i32, mo: u8, d: u8, h: u8, mi: u8) -> u64 {
	let days = Civil {
		year: y,
		month: mo,
		day: d,
		hour: h,
		minute: mi,
	}
	.day_number();
	((days * 86_400 + i64::from(h) * 3_600 + i64::from(mi) * 60) * 1_000) as u64
}

fn en(zone: &LocalZone, now: u64, then: u64) -> String {
	timefmt::format_in(Language::En, zone, now, then, &|t| {
		format!("Yesterday at {t}")
	})
}

fn utc(now: u64, then: u64) -> String {
	en(&LocalZone::utc(), now, then)
}

#[test]
fn same_day_yesterday_and_older_in_english() {
	let now = ms(2026, 11, 6, 9, 0);
	assert_eq!(utc(now, ms(2026, 11, 6, 1, 54)), "1:54 AM");
	assert_eq!(utc(now, ms(2026, 11, 5, 18, 51)), "Yesterday at 6:51 PM");
	assert_eq!(utc(now, ms(2026, 11, 4, 15, 33)), "11/04/26, 3:33 PM");
	assert_eq!(utc(now, ms(2025, 1, 2, 7, 5)), "01/02/25, 7:05 AM");
}

#[test]
fn yesterday_is_a_calendar_day_not_24_hours() {
	// Five minutes apart across midnight.
	let now = ms(2026, 11, 6, 0, 5);
	assert_eq!(utc(now, ms(2026, 11, 5, 23, 58)), "Yesterday at 11:58 PM");
	// Almost 24 hours, still the same day.
	assert_eq!(
		utc(ms(2026, 11, 6, 23, 59), ms(2026, 11, 6, 0, 1)),
		"12:01 AM"
	);
	// More than 24 hours but only one calendar day back.
	assert_eq!(
		utc(ms(2026, 11, 6, 23, 59), ms(2026, 11, 5, 0, 1)),
		"Yesterday at 12:01 AM"
	);
	// Two calendar days back, though under 48 hours.
	assert_eq!(
		utc(ms(2026, 11, 6, 0, 10), ms(2026, 11, 4, 23, 50)),
		"11/04/26, 11:50 PM"
	);
}

#[test]
fn midnight_and_noon_are_twelve() {
	let now = ms(2026, 11, 6, 23, 0);
	assert_eq!(utc(now, ms(2026, 11, 6, 0, 0)), "12:00 AM");
	assert_eq!(utc(now, ms(2026, 11, 6, 0, 30)), "12:30 AM");
	assert_eq!(utc(now, ms(2026, 11, 6, 12, 0)), "12:00 PM");
	assert_eq!(utc(now, ms(2026, 11, 6, 12, 59)), "12:59 PM");
	assert_eq!(utc(now, ms(2026, 11, 6, 13, 0)), "1:00 PM");
	assert_eq!(utc(now, ms(2026, 11, 6, 23, 0)), "11:00 PM");
}

#[test]
fn a_time_after_now_is_clamped_to_now() {
	let now = ms(2026, 11, 6, 9, 0);
	assert_eq!(utc(now, ms(2026, 11, 6, 11, 0)), "9:00 AM");
	assert_eq!(utc(now, ms(2026, 11, 9, 11, 0)), "9:00 AM");
	assert_eq!(
		timefmt::day_of(&LocalZone::utc(), now, now + 5_000_000_000),
		Some(Day::Today)
	);
}

#[test]
fn the_zone_decides_the_day_and_the_clock() {
	// 20:00 UTC on the 6th is 05:00 on the 7th in Tokyo; 10:00 UTC is 19:00 on the 6th.
	let tokyo = LocalZone::fixed(9 * 3600).unwrap();
	let now = ms(2026, 11, 6, 20, 0);
	assert_eq!(
		en(&tokyo, now, ms(2026, 11, 6, 10, 0)),
		"Yesterday at 7:00 PM"
	);
	assert_eq!(utc(now, ms(2026, 11, 6, 10, 0)), "10:00 AM");
	let west = LocalZone::fixed(-5 * 3600).unwrap();
	assert_eq!(
		en(&west, ms(2026, 11, 6, 2, 0), ms(2026, 11, 5, 23, 0)),
		"6:00 PM"
	);
}

#[test]
fn daylight_saving_changes_keep_calendar_days() {
	let Ok(ny) = LocalZone::named("America/New_York") else {
		return; // no time zone database on this machine
	};
	// Spring forward 2026-03-08 02:00 EST: the day has 23 hours.
	// 23:50 EDT on the 8th (03:50 UTC on the 9th), seen at 00:30 EDT on the 9th.
	let now = ms(2026, 3, 9, 4, 30);
	assert_eq!(en(&ny, now, ms(2026, 3, 9, 3, 50)), "Yesterday at 11:50 PM");
	// Fall back 2026-11-01 02:00 EDT: the day has 25 hours. 01:30 EDT is 05:30 UTC.
	let now = ms(2026, 11, 2, 13, 0); // 08:00 EST on the 2nd
	assert_eq!(en(&ny, now, ms(2026, 11, 1, 5, 30)), "Yesterday at 1:30 AM");
	// The repeated 01:30 EST (06:30 UTC) reads the same.
	assert_eq!(en(&ny, now, ms(2026, 11, 1, 6, 30)), "Yesterday at 1:30 AM");
	// A winter message keeps the winter offset when read in summer.
	let now = ms(2026, 7, 1, 16, 0);
	assert_eq!(en(&ny, now, ms(2026, 1, 15, 17, 0)), "01/15/26, 12:00 PM");
}

#[test]
fn every_language_has_idiomatic_patterns() {
	let zone = LocalZone::utc();
	let now = ms(2026, 11, 6, 20, 0);
	let cases = [
		(Language::En, "1:54 PM", "11/04/26, 3:33 PM"),
		(Language::De, "13:54", "04.11.26, 15:33"),
		(Language::Es, "13:54", "4/11/26, 15:33"),
		(Language::Hi, "1:54 pm", "4/11/26, 3:33 pm"),
		(Language::Ja, "13:54", "2026/11/04 15:33"),
		(Language::Ko, "오후 1:54", "26. 11. 4. 오후 3:33"),
		(Language::PtBr, "13:54", "04/11/2026 15:33"),
		(Language::Ru, "13:54", "04.11.2026, 15:33"),
		(Language::ZhCn, "13:54", "2026/11/4 15:33"),
	];
	for (language, today, older) in cases {
		let f = |then| timefmt::format_in(language, &zone, now, then, &|t| format!("Y {t}"));
		assert_eq!(f(ms(2026, 11, 6, 13, 54)), today, "{language:?}");
		assert_eq!(
			f(ms(2026, 11, 5, 13, 54)),
			format!("Y {today}"),
			"{language:?}"
		);
		assert_eq!(f(ms(2026, 11, 4, 15, 33)), older, "{language:?}");
	}
	// Small hours and single-digit days keep their padding rules.
	let f =
		|language| timefmt::format_in(language, &zone, now, ms(2026, 11, 6, 9, 5), &|t| t.into());
	assert_eq!(f(Language::De), "09:05");
	assert_eq!(f(Language::Es), "9:05");
	assert_eq!(f(Language::Ko), "오전 9:05");
	assert_eq!(f(Language::Hi), "9:05 am");
	assert_eq!(timefmt::clock(Language::Ko, &civil(0, 0)), "오전 12:00");
	assert_eq!(timefmt::clock(Language::Ko, &civil(12, 0)), "오후 12:00");
}

fn civil(hour: u8, minute: u8) -> Civil {
	Civil {
		year: 2026,
		month: 11,
		day: 4,
		hour,
		minute,
	}
}

#[test]
fn yesterday_wording_comes_from_the_catalog() {
	let zone = LocalZone::utc();
	let now = ms(2026, 11, 6, 20, 0);
	let then = ms(2026, 11, 5, 18, 51);
	for (language, expect, today) in [
		(Language::En, "Yesterday at 6:51 PM", "Today"),
		(Language::De, "Gestern um 18:51", "Heute"),
		(Language::Es, "Ayer, 18:51", "Hoy"),
		(Language::Hi, "कल 6:51 pm", "आज"),
		(Language::Ja, "昨日 18:51", "今日"),
		(Language::Ko, "어제 오후 6:51", "오늘"),
		(Language::PtBr, "Ontem, 18:51", "Hoje"),
		(Language::Ru, "Вчера в 18:51", "Сегодня"),
		(Language::ZhCn, "昨天 18:51", "今天"),
	] {
		i18n::isolate_thread();
		i18n::set_active(language);
		assert_eq!(timefmt::format(&zone, now, then), expect, "{language:?}");
		assert_eq!(timefmt::day_label(&zone, now, now), today, "{language:?}");
	}
	// The comma form has no article to agree with the hour.
	let one_am = ms(2026, 11, 5, 1, 0);
	i18n::isolate_thread();
	i18n::set_active(Language::Es);
	assert_eq!(timefmt::format(&zone, now, one_am), "Ayer, 1:00");
	i18n::isolate_thread();
	i18n::set_active(Language::PtBr);
	assert_eq!(timefmt::format(&zone, now, one_am), "Ontem, 01:00");
	i18n::isolate_thread();
	i18n::set_active(Language::En);
	assert_eq!(timefmt::day_label(&zone, now, then), "Yesterday");
	assert_eq!(
		timefmt::day_label(&zone, now, ms(2026, 11, 4, 3, 0)),
		"11/04/26"
	);
}

#[test]
fn same_day_compares_local_days() {
	let tokyo = LocalZone::fixed(9 * 3600).unwrap();
	let (a, b) = (ms(2026, 11, 6, 14, 0), ms(2026, 11, 6, 16, 0));
	assert!(timefmt::same_day(&LocalZone::utc(), a, b));
	assert!(!timefmt::same_day(&tokyo, a, b));
}

#[test]
fn instants_out_of_range_do_not_panic() {
	assert_eq!(utc(u64::MAX, 0), NO_TIME);
	assert_eq!(utc(0, u64::MAX), "12:00 AM");
	assert!(timefmt::civil(&LocalZone::utc(), u64::MAX).is_none());
}
