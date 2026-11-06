//! The `call_history` table.

use rusqlite::{Connection, params};

use crate::db::Database;
use crate::error::{ConfigError, Result};

use super::limits::{Limits, MAX_LIST, MAX_NAME_CHARS, MAX_SHORT_CHARS};

/// Which side placed the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
	Incoming,
	Outgoing,
}

/// How a call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOutcome {
	/// The call connected and then ended.
	Completed,
	/// An incoming call that was never answered.
	Missed,
	/// Declined, by either side.
	Declined,
	/// An outgoing call that never reached the peer or never connected.
	Failed,
	/// An outgoing call the user cancelled before it connected.
	Cancelled,
}

impl Direction {
	fn as_sql(self) -> &'static str {
		match self {
			Self::Incoming => "incoming",
			Self::Outgoing => "outgoing",
		}
	}

	fn from_sql(text: &str) -> Option<Self> {
		match text {
			"incoming" => Some(Self::Incoming),
			"outgoing" => Some(Self::Outgoing),
			_ => None,
		}
	}
}

impl CallOutcome {
	fn as_sql(self) -> &'static str {
		match self {
			Self::Completed => "completed",
			Self::Missed => "missed",
			Self::Declined => "declined",
			Self::Failed => "failed",
			Self::Cancelled => "cancelled",
		}
	}

	fn from_sql(text: &str) -> Option<Self> {
		match text {
			"completed" => Some(Self::Completed),
			"missed" => Some(Self::Missed),
			"declined" => Some(Self::Declined),
			"failed" => Some(Self::Failed),
			"cancelled" => Some(Self::Cancelled),
			_ => None,
		}
	}
}

/// What to record about a finished call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCallRecord {
	/// Display name of the peer as seen at the time.
	pub peer_name: String,
	/// Full fingerprint: lowercase hex of the 32-byte peer id.
	pub peer_fingerprint: String,
	/// Short fingerprint hint, as shown next to the peer elsewhere.
	pub peer_fingerprint_short: String,
	pub direction: Direction,
	pub outcome: CallOutcome,
	/// Unix milliseconds when the call started.
	pub started_at_ms: u64,
	/// Connected time in milliseconds; `0` when it never connected.
	pub duration_ms: u64,
	/// The call's id (16 bytes), which names its chat session.
	pub call_id: [u8; 16],
}

/// A stored entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRecord {
	/// Row id; grows with every entry and is the paging cursor.
	pub id: u64,
	pub peer_name: String,
	pub peer_fingerprint: String,
	pub peer_fingerprint_short: String,
	pub direction: Direction,
	pub outcome: CallOutcome,
	pub started_at_ms: u64,
	pub duration_ms: u64,
	pub call_id: [u8; 16],
}

fn clean(text: &str, max: usize) -> String {
	text.chars().filter(|c| !c.is_control()).take(max).collect()
}

fn valid_fingerprint(text: &str) -> bool {
	text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn clamp(value: u64) -> i64 {
	i64::try_from(value).unwrap_or(i64::MAX)
}

/// Reads and writes the history. Cheap to clone; clones share the database.
#[derive(Debug, Clone)]
pub struct CallHistory {
	db: Database,
	limits: Limits,
}

impl CallHistory {
	pub fn open(db: &Database) -> Self {
		Self::with_limits(db, Limits::default())
	}

	pub fn with_limits(db: &Database, limits: Limits) -> Self {
		Self {
			db: db.clone(),
			limits,
		}
	}

	/// Appends an entry, then prunes: entries older than 90 days (against `now_ms`) and
	/// everything beyond the 200 newest go. Returns the new id.
	pub fn record(&self, entry: &NewCallRecord, now_ms: u64) -> Result<u64> {
		let fingerprint = entry.peer_fingerprint.to_ascii_lowercase();
		if !valid_fingerprint(&fingerprint) {
			return Err(ConfigError::InvalidPreference(
				"call history fingerprint must be 64 hex digits".to_owned(),
			));
		}
		let name = clean(&entry.peer_name, MAX_NAME_CHARS);
		let short = clean(&entry.peer_fingerprint_short, MAX_SHORT_CHARS);
		self.db.with(|conn| {
			let tx = conn.transaction()?;
			tx.execute(
				"INSERT INTO call_history
				 (peer_name, peer_fingerprint, peer_fingerprint_short, direction, outcome,
				  started_at, duration_ms, call_id)
				 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
				params![
					name,
					fingerprint,
					short,
					entry.direction.as_sql(),
					entry.outcome.as_sql(),
					clamp(entry.started_at_ms),
					clamp(entry.duration_ms),
					entry.call_id.as_slice(),
				],
			)?;
			let id = tx.last_insert_rowid();
			prune(&tx, self.limits, now_ms)?;
			tx.commit()?;
			Ok(id.max(0) as u64)
		})
	}

	/// Newest first. `before` continues after the last id of the previous page.
	pub fn list(&self, limit: usize, before: Option<u64>) -> Result<Vec<CallRecord>> {
		let limit = limit.clamp(1, MAX_LIST);
		let before = before.map_or(i64::MAX, clamp);
		self.db.with(|conn| {
			let mut stmt = conn.prepare(
				"SELECT id, peer_name, peer_fingerprint, peer_fingerprint_short, direction,
				        outcome, started_at, duration_ms, call_id
				 FROM call_history WHERE id < ?1 ORDER BY id DESC LIMIT ?2",
			)?;
			let rows = stmt.query_map(params![before, limit as i64], |r| {
				Ok((
					r.get::<_, i64>(0)?,
					r.get::<_, String>(1)?,
					r.get::<_, String>(2)?,
					r.get::<_, String>(3)?,
					r.get::<_, String>(4)?,
					r.get::<_, String>(5)?,
					r.get::<_, i64>(6)?,
					r.get::<_, i64>(7)?,
					r.get::<_, Vec<u8>>(8)?,
				))
			})?;
			let mut out = Vec::new();
			for row in rows {
				let (
					id,
					peer_name,
					peer_fingerprint,
					peer_fingerprint_short,
					dir,
					outcome,
					at,
					ms,
					call,
				) = row?;
				let bad =
					|| ConfigError::InvalidPreference("unreadable call history row".to_owned());
				out.push(CallRecord {
					id: id.max(0) as u64,
					peer_name,
					peer_fingerprint,
					peer_fingerprint_short,
					direction: Direction::from_sql(&dir).ok_or_else(bad)?,
					outcome: CallOutcome::from_sql(&outcome).ok_or_else(bad)?,
					started_at_ms: at.max(0) as u64,
					duration_ms: ms.max(0) as u64,
					call_id: call.try_into().map_err(|_| bad())?,
				});
			}
			Ok(out)
		})
	}

	/// Number of stored entries.
	pub fn count(&self) -> Result<usize> {
		self.db.with(|conn| {
			let n: i64 = conn.query_row("SELECT COUNT(*) FROM call_history", [], |r| r.get(0))?;
			Ok(n.max(0) as usize)
		})
	}

	/// Deletes every entry; returns how many there were.
	pub fn clear(&self) -> Result<usize> {
		self.db
			.with(|conn| Ok(conn.execute("DELETE FROM call_history", [])?))
	}
}

fn prune(conn: &Connection, limits: Limits, now_ms: u64) -> Result<()> {
	let oldest = clamp(now_ms.saturating_sub(limits.max_age_ms));
	conn.execute("DELETE FROM call_history WHERE started_at < ?1", [oldest])?;
	conn.execute(
		"DELETE FROM call_history WHERE id NOT IN
		 (SELECT id FROM call_history ORDER BY id DESC LIMIT ?1)",
		[limits.max_entries as i64],
	)?;
	Ok(())
}
