//! The schema, created idempotently on every open.
//!
//! Every statement is `IF NOT EXISTS`, so a database that lacks a table (made by a build
//! that had fewer) gains it in place and keeps its rows. A database whose `user_version`
//! is higher than this build knows is refused by the caller.

use rusqlite::Connection;

use crate::error::{ConfigError, Result};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS prefs (
	key TEXT PRIMARY KEY NOT NULL,
	value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS trust (
	public_key TEXT PRIMARY KEY NOT NULL CHECK (length(public_key) = 64),
	display_name TEXT NOT NULL,
	level TEXT NOT NULL CHECK (level IN ('tofu', 'verified')),
	first_seen INTEGER NOT NULL
) WITHOUT ROWID;

-- Channel metadata only. Secrets (signing keys, password and invite credentials)
-- live in the SecretVault; channel_secrets lists which vault entries exist.
CREATE TABLE IF NOT EXISTS channels (
	id BLOB PRIMARY KEY NOT NULL CHECK (length(id) = 16),
	name TEXT NOT NULL,
	flags INTEGER NOT NULL,
	role INTEGER NOT NULL,
	owner_public BLOB NOT NULL CHECK (length(owner_public) = 32),
	delegation BLOB,
	cert BLOB
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS channel_issued (
	channel BLOB NOT NULL REFERENCES channels (id) ON DELETE CASCADE,
	subject BLOB NOT NULL CHECK (length(subject) = 32),
	expires_at INTEGER NOT NULL,
	PRIMARY KEY (channel, subject)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS channel_revocations (
	channel BLOB NOT NULL REFERENCES channels (id) ON DELETE CASCADE,
	seq INTEGER NOT NULL,
	data BLOB NOT NULL,
	PRIMARY KEY (channel, seq)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS channel_invites (
	channel BLOB NOT NULL REFERENCES channels (id) ON DELETE CASCADE,
	invite_id BLOB NOT NULL CHECK (length(invite_id) = 8),
	expires_at INTEGER NOT NULL,
	uses_left INTEGER NOT NULL,
	PRIMARY KEY (channel, invite_id)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS channel_secrets (
	channel BLOB NOT NULL,
	slot TEXT NOT NULL,
	gen INTEGER NOT NULL,
	state TEXT NOT NULL CHECK (state IN ('pending', 'committed', 'dead')),
	PRIMARY KEY (channel, slot, gen)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS state (
	key TEXT PRIMARY KEY NOT NULL,
	value BLOB NOT NULL
) WITHOUT ROWID;

-- Past calls: who, which way, how it ended, when and for how long. Nothing else.
CREATE TABLE IF NOT EXISTS call_history (
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

-- Quick-call favorites: an ordered subset of the trusted peers. Forgetting a peer
-- removes its favorite; the count limit lives in crate::favorites.
CREATE TABLE IF NOT EXISTS favorites (
	public_key TEXT PRIMARY KEY NOT NULL REFERENCES trust (public_key) ON DELETE CASCADE,
	position INTEGER NOT NULL UNIQUE CHECK (position >= 0)
) WITHOUT ROWID;

-- Contacts whose incoming calls are answered automatically. Only verified contacts can be
-- listed (checked in crate::autoanswer); forgetting, blocking or replacing the key of a
-- peer removes its trust row and with it this flag.
CREATE TABLE IF NOT EXISTS auto_answer (
	public_key TEXT PRIMARY KEY NOT NULL REFERENCES trust (public_key) ON DELETE CASCADE
) WITHOUT ROWID;

-- Devices the user blocked, by the digest of their key (the peer id). The name is
-- what the device called itself when it was blocked, for the list in Settings. The count
-- limit lives in crate::blocklist.
CREATE TABLE IF NOT EXISTS blocked (
	fingerprint TEXT PRIMARY KEY NOT NULL CHECK (length(fingerprint) = 64),
	display_name TEXT NOT NULL,
	blocked_at INTEGER NOT NULL
) WITHOUT ROWID;
";

/// Every table the schema defines; checked after setup.
const TABLES: &[&str] = &[
	"prefs",
	"trust",
	"channels",
	"channel_issued",
	"channel_revocations",
	"channel_invites",
	"channel_secrets",
	"state",
	"call_history",
	"favorites",
	"auto_answer",
	"blocked",
];

/// Highest schema version this build writes and understands.
pub const SCHEMA_VERSION: u32 = 1;

/// Creates what is missing, stamps the version and verifies every table exists.
pub fn ensure(conn: &mut Connection) -> Result<()> {
	let tx = conn.transaction()?;
	tx.execute_batch(SCHEMA)?;
	tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
	tx.commit()?;
	for table in TABLES {
		let found: bool = conn.query_row(
			"SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
			[table],
			|r| r.get(0),
		)?;
		if !found {
			return Err(ConfigError::MissingTable((*table).to_owned()));
		}
	}
	Ok(())
}
