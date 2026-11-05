//! Local wall-clock rendering of Unix timestamps for message logs.
//!
//! Timestamps are stored and transported as UTC Unix milliseconds. A
//! [`LocalZone`] only decides how they are *shown*. The zone is a value, not a
//! global, so a caller (and every test) picks it: [`LocalZone::system`] for the
//! real app, [`LocalZone::utc`], [`LocalZone::fixed`] or [`LocalZone::named`]
//! for a deterministic one. Daylight saving is applied per instant, so an old
//! message keeps the offset that was in force when it was sent.

use jiff::Timestamp;
use jiff::tz::{Offset, TimeZone};

/// Shown when a timestamp is outside the range a date library can represent.
pub const NO_TIME: &str = "--:--:--";

/// Why a zone could not be built.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ZoneError(String);

/// A time zone used only to render timestamps.
#[derive(Debug, Clone)]
pub struct LocalZone {
	tz: TimeZone,
}

impl LocalZone {
	/// The zone the operating system is configured with (`TZ`, `/etc/localtime`,
	/// the Windows registry). Read once, here. If the system gives none, jiff
	/// falls back to UTC, and the rendered offset then says `+00:00`.
	pub fn system() -> Self {
		Self {
			tz: TimeZone::system(),
		}
	}

	pub fn utc() -> Self {
		Self { tz: TimeZone::UTC }
	}

	/// A constant offset east of UTC, in seconds (for tests and `--utc`-style use).
	pub fn fixed(offset_seconds: i32) -> Result<Self, ZoneError> {
		let offset = Offset::from_seconds(offset_seconds).map_err(|e| ZoneError(e.to_string()))?;
		Ok(Self {
			tz: TimeZone::fixed(offset),
		})
	}

	/// An IANA zone such as `Europe/Berlin`, from the system database or the
	/// copy bundled on platforms that have none.
	pub fn named(name: &str) -> Result<Self, ZoneError> {
		TimeZone::get(name)
			.map(|tz| Self { tz })
			.map_err(|e| ZoneError(e.to_string()))
	}

	/// `HH:MM:SS` in this zone.
	pub fn clock(&self, unix_ms: u64) -> String {
		self.civil(unix_ms)
			.map_or_else(|| NO_TIME.to_owned(), |(_, time, _)| time)
	}

	/// `YYYY-MM-DD HH:MM:SS +HH:MM` in this zone, for hover text.
	pub fn full(&self, unix_ms: u64) -> String {
		self.civil(unix_ms).map_or_else(
			|| NO_TIME.to_owned(),
			|(date, time, offset)| format!("{date} {time} {offset}"),
		)
	}

	/// The hover text of a timestamp: local date, time and offset, then the
	/// UTC clock it was derived from.
	pub fn hover(&self, unix_ms: u64) -> String {
		format!("{} ({} UTC)", self.full(unix_ms), utc_clock(unix_ms))
	}

	fn civil(&self, unix_ms: u64) -> Option<(String, String, String)> {
		let ts = Timestamp::from_millisecond(i64::try_from(unix_ms).ok()?).ok()?;
		let dt = self.tz.to_datetime(ts);
		let seconds = self.tz.to_offset(ts).seconds();
		let sign = if seconds < 0 { '-' } else { '+' };
		let abs = seconds.unsigned_abs();
		Some((
			format!("{:04}-{:02}-{:02}", dt.year(), dt.month(), dt.day()),
			format!("{:02}:{:02}:{:02}", dt.hour(), dt.minute(), dt.second()),
			format!("{sign}{:02}:{:02}", abs / 3600, abs / 60 % 60),
		))
	}
}

/// `HH:MM:SS` in UTC.
pub fn utc_clock(unix_ms: u64) -> String {
	LocalZone::utc().clock(unix_ms)
}
