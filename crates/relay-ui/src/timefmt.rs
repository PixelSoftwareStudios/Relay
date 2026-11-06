//! The one place that turns an instant into the words the app shows next to
//! messages, calls and history rows.
//!
//! * the same local day: the clock only (`1:54 AM`);
//! * the day before: `Yesterday at 6:51 PM`;
//! * anything older: the short date and the clock (`11/04/26, 3:33 PM`).
//!
//! Pure functions of an explicit `now` and a [`LocalZone`], so a test picks
//! both (the shell supplies `Shell::now_unix_ms` and `Shell::time_zone`).
//! A time after `now` (a peer clock that runs ahead) is clamped to `now`.
//!
//! Locale rules are a small table instead of ICU4X: nine languages need nine
//! short patterns, while `icu_datetime` plus its data would add many crates
//! and hundreds of kilobytes to every shell. The patterns follow CLDR's
//! short date and time formats of each language's main region. Only the
//! "Yesterday at {time}" wording is a catalog message (`time-yesterday-at`).

use relay_io::localtime::LocalZone;

use crate::i18n::{self, Language};
use crate::tr;

/// Shown when an instant is outside the range the date library handles.
pub const NO_TIME: &str = "--:--";

/// A local calendar date and clock time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
	pub year: i32,
	pub month: u8,
	pub day: u8,
	pub hour: u8,
	pub minute: u8,
}

impl Civil {
	/// Days since 1970-01-01 of the date (proleptic Gregorian).
	pub fn day_number(&self) -> i64 {
		let (year, month, day) = (
			i64::from(self.year),
			i64::from(self.month),
			i64::from(self.day),
		);
		let y = if month <= 2 { year - 1 } else { year };
		let era = y.div_euclid(400);
		let year_of_era = y - era * 400;
		let shifted = (month + 9) % 12;
		let day_of_year = (153 * shifted + 2) / 5 + day - 1;
		let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
		era * 146_097 + day_of_era - 719_468
	}
}

/// The local date and clock of `unix_ms` in `zone`, if representable.
pub fn civil(zone: &LocalZone, unix_ms: u64) -> Option<Civil> {
	// `YYYY-MM-DD HH:MM:SS +HH:MM`
	let full = zone.full(unix_ms);
	let mut parts = full.split([' ', '-', ':']).filter(|p| !p.is_empty());
	let year = parts.next()?.parse().ok()?;
	let month = parts.next()?.parse().ok()?;
	let day = parts.next()?.parse().ok()?;
	let hour = parts.next()?.parse().ok()?;
	let minute = parts.next()?.parse().ok()?;
	Some(Civil {
		year,
		month,
		day,
		hour,
		minute,
	})
}

/// How a calendar day relates to today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Day {
	Today,
	Yesterday,
	Earlier(Civil),
}

/// Which day `then_ms` falls on, as seen at `now_ms` in `zone`. `None` when
/// either instant cannot be shown.
pub fn day_of(zone: &LocalZone, now_ms: u64, then_ms: u64) -> Option<Day> {
	let then_ms = then_ms.min(now_ms);
	let today = civil(zone, now_ms)?.day_number();
	let then = civil(zone, then_ms)?;
	Some(match today - then.day_number() {
		0 => Day::Today,
		1 => Day::Yesterday,
		_ => Day::Earlier(then),
	})
}

/// The clock as the language writes it: `3:33 PM`, `15:33`, `오후 3:33`.
pub fn clock(language: Language, c: &Civil) -> String {
	let (hour12, am) = match c.hour {
		0 => (12, true),
		h @ 1..=11 => (h, true),
		12 => (12, false),
		h => (h - 12, false),
	};
	let m = c.minute;
	match language {
		Language::En => format!("{hour12}:{m:02} {}", if am { "AM" } else { "PM" }),
		Language::Hi => format!("{hour12}:{m:02} {}", if am { "am" } else { "pm" }),
		Language::Ko => format!("{} {hour12}:{m:02}", if am { "오전" } else { "오후" }),
		Language::Es | Language::Ja => format!("{}:{m:02}", c.hour),
		Language::De | Language::PtBr | Language::Ru | Language::ZhCn => {
			format!("{:02}:{m:02}", c.hour)
		}
	}
}

/// The short date: `11/04/26`, `04.11.26`, `2026/11/04`.
pub fn date(language: Language, c: &Civil) -> String {
	let (y, m, d) = (c.year, c.month, c.day);
	let yy = y.rem_euclid(100);
	match language {
		Language::En => format!("{m:02}/{d:02}/{yy:02}"),
		Language::De => format!("{d:02}.{m:02}.{yy:02}"),
		Language::Es | Language::Hi => format!("{d}/{m}/{yy:02}"),
		Language::Ja => format!("{y}/{m:02}/{d:02}"),
		Language::Ko => format!("{yy:02}. {m}. {d}."),
		Language::PtBr => format!("{d:02}/{m:02}/{y}"),
		Language::Ru => format!("{d:02}.{m:02}.{y}"),
		Language::ZhCn => format!("{y}/{m}/{d}"),
	}
}

/// Date and clock together, as the older form shows them.
pub fn date_time(language: Language, c: &Civil) -> String {
	let (date, clock) = (date(language, c), clock(language, c));
	match language {
		Language::Ja | Language::Ko | Language::ZhCn | Language::PtBr => format!("{date} {clock}"),
		_ => format!("{date}, {clock}"),
	}
}

/// [`format`] for an explicit language and "Yesterday at" wording, so tests
/// need not switch the active language.
pub fn format_in(
	language: Language,
	zone: &LocalZone,
	now_ms: u64,
	then_ms: u64,
	yesterday_at: &dyn Fn(&str) -> String,
) -> String {
	let then_ms = then_ms.min(now_ms);
	let (Some(day), Some(c)) = (day_of(zone, now_ms, then_ms), civil(zone, then_ms)) else {
		return NO_TIME.to_owned();
	};
	match day {
		Day::Today => clock(language, &c),
		Day::Yesterday => yesterday_at(&clock(language, &c)),
		Day::Earlier(_) => date_time(language, &c),
	}
}

/// `then_ms` as the app words it, in the active language.
pub fn format(zone: &LocalZone, now_ms: u64, then_ms: u64) -> String {
	format_in(i18n::active(), zone, now_ms, then_ms, &|time| {
		tr!("time-yesterday-at", time = time)
	})
}

/// The heading of a day in a transcript: Today, Yesterday or the short date.
pub fn day_label(zone: &LocalZone, now_ms: u64, then_ms: u64) -> String {
	match day_of(zone, now_ms, then_ms) {
		Some(Day::Today) => tr!("time-today").to_owned(),
		Some(Day::Yesterday) => tr!("time-yesterday").to_owned(),
		Some(Day::Earlier(c)) => date(i18n::active(), &c),
		None => NO_TIME.to_owned(),
	}
}

/// Whether two instants fall on the same local day.
pub fn same_day(zone: &LocalZone, a_ms: u64, b_ms: u64) -> bool {
	match (civil(zone, a_ms), civil(zone, b_ms)) {
		(Some(a), Some(b)) => a.day_number() == b.day_number(),
		_ => a_ms == b_ms,
	}
}
