//! Schema migrations, tracked with `PRAGMA user_version`.
//!
//! Each entry of [`MIGRATIONS`] upgrades the schema by one version inside its own
//! transaction. Append new entries; never edit shipped ones. A database whose
//! version is higher than this build knows is refused by the caller.
//!

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

-- Channel metadata only. Secrets (signing keys, password and invite credentials)
-- live in the SecretVault; channel_secrets lists which vault entries exist.
CREATE TABLE channels (
	id BLOB PRIMARY KEY NOT NULL CHECK (length(id) = 16),
	name TEXT NOT NULL,
	flags INTEGER NOT NULL,
	role INTEGER NOT NULL,
	owner_public BLOB NOT NULL CHECK (length(owner_public) = 32),
	delegation BLOB,
	cert BLOB
) WITHOUT ROWID;

CREATE TABLE channel_issued (
	channel BLOB NOT NULL REFERENCES channels (id) ON DELETE CASCADE,
	subject BLOB NOT NULL CHECK (length(subject) = 32),
	expires_at INTEGER NOT NULL,
	PRIMARY KEY (channel, subject)
) WITHOUT ROWID;

CREATE TABLE channel_revocations (
	channel BLOB NOT NULL REFERENCES channels (id) ON DELETE CASCADE,
	seq INTEGER NOT NULL,
	data BLOB NOT NULL,
	PRIMARY KEY (channel, seq)
) WITHOUT ROWID;

CREATE TABLE channel_invites (
	channel BLOB NOT NULL REFERENCES channels (id) ON DELETE CASCADE,
	invite_id BLOB NOT NULL CHECK (length(invite_id) = 8),
	expires_at INTEGER NOT NULL,
	uses_left INTEGER NOT NULL,
	PRIMARY KEY (channel, invite_id)
) WITHOUT ROWID;

CREATE TABLE channel_secrets (
	channel BLOB NOT NULL,
	slot TEXT NOT NULL,
	gen INTEGER NOT NULL,
	state TEXT NOT NULL CHECK (state IN ('pending', 'committed', 'dead')),
	PRIMARY KEY (channel, slot, gen)
) WITHOUT ROWID;

CREATE TABLE state (
	key TEXT PRIMARY KEY NOT NULL,
	value BLOB NOT NULL
) WITHOUT ROWID;

-- Past calls: who, which way, how it ended, when and for how long. Nothing else.
CREATE TABLE call_history (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	peer_name TEXT NOT NULL,
	peer_fingerprint TEXT NOT NULL CHECK (length(peer_fingerprint) = 64),
	peer_fingerprint_short TEXT NOT NULL,
	direction TEXT NOT NULL CHECK (direction IN ('incoming', 'outgoing')),
	outcome TEXT NOT NULL CHECK (outcome IN ('completed', 'missed', 'declined', 'failed', 'cancelled')),
	started_at INTEGER NOT NULL,
	duration_ms INTEGER NOT NULL,
	call_id BLOB NOT NULL CHECK (length(call_id) = 16)
);
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
