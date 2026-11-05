//! Schema migrations, tracked with `PRAGMA user_version`.
//!
//! Each entry of [`MIGRATIONS`] upgrades the schema by one version inside its own
//! transaction. Append new entries; never edit shipped ones. A database whose
//! version is higher than this build knows is refused by the caller.
//!
//! Reserved: the channel feature appends its tables as the next entry once its
//! design is final; nothing here depends on it.

use rusqlite::Connection;

use crate::error::Result;

const V1: &str = "
CREATE TABLE prefs (
	key TEXT PRIMARY KEY NOT NULL,
	value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE trust (
	public_key TEXT PRIMARY KEY NOT NULL CHECK (length(public_key) = 64),
	display_name TEXT NOT NULL,
	level TEXT NOT NULL CHECK (level IN ('tofu', 'verified')),
	first_seen INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE avatars (
	hash BLOB PRIMARY KEY NOT NULL CHECK (length(hash) = 32),
	data BLOB NOT NULL,
	size INTEGER NOT NULL,
	last_used INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX avatars_by_use ON avatars (last_used);

CREATE TABLE state (
	key TEXT PRIMARY KEY NOT NULL,
	value BLOB NOT NULL
) WITHOUT ROWID;
";

const MIGRATIONS: &[&str] = &[V1];

/// Highest schema version this build writes and understands.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// Applies every migration above the database's current version.
pub fn run(conn: &mut Connection, from: u32) -> Result<()> {
	for (index, sql) in MIGRATIONS.iter().enumerate().skip(from as usize) {
		let tx = conn.transaction()?;
		tx.execute_batch(sql)?;
		tx.pragma_update(None, "user_version", index as u32 + 1)?;
		tx.commit()?;
	}
	Ok(())
}
