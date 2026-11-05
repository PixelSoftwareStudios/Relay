//! SQL for the channel tables (metadata and the secret manifest).

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, params};

use super::{ChannelRow, InviteMeta, IssuedRow};
use crate::error::Result;

fn fixed<const N: usize>(blob: Vec<u8>) -> rusqlite::Result<[u8; N]> {
	blob.try_into().map_err(|_| rusqlite::Error::InvalidQuery)
}

pub(super) fn ids(conn: &Connection) -> Result<Vec<[u8; 16]>> {
	let mut stmt = conn.prepare("SELECT id FROM channels ORDER BY name, id")?;
	let rows = stmt.query_map([], |r| fixed::<16>(r.get(0)?))?;
	Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(super) fn exists(conn: &Connection, id: &[u8; 16]) -> Result<bool> {
	Ok(conn
		.query_row(
			"SELECT 1 FROM channels WHERE id = ?1",
			[id.as_slice()],
			|_| Ok(()),
		)
		.optional()?
		.is_some())
}

pub(super) fn count(conn: &Connection) -> Result<usize> {
	let n: i64 = conn.query_row("SELECT COUNT(*) FROM channels", [], |r| r.get(0))?;
	Ok(n.max(0) as usize)
}

pub(super) fn read(conn: &Connection, id: &[u8; 16]) -> Result<Option<ChannelRow>> {
	let head = conn
		.query_row(
			"SELECT name, flags, role, owner_public, delegation, cert FROM channels WHERE id = ?1",
			[id.as_slice()],
			|r| {
				Ok((
					r.get::<_, String>(0)?,
					r.get::<_, u8>(1)?,
					r.get::<_, u8>(2)?,
					fixed::<32>(r.get(3)?)?,
					r.get::<_, Option<Vec<u8>>>(4)?,
					r.get::<_, Option<Vec<u8>>>(5)?,
				))
			},
		)
		.optional()?;
	let Some((name, flags, role, owner_public, delegation, cert)) = head else {
		return Ok(None);
	};
	let issued = conn
		.prepare(
			"SELECT subject, expires_at FROM channel_issued WHERE channel = ?1 ORDER BY subject",
		)?
		.query_map([id.as_slice()], |r| {
			Ok(IssuedRow {
				subject: fixed(r.get(0)?)?,
				expires_at: r.get::<_, i64>(1)?.max(0) as u64,
			})
		})?
		.collect::<rusqlite::Result<_>>()?;
	let revocations = conn
		.prepare("SELECT data FROM channel_revocations WHERE channel = ?1 ORDER BY seq")?
		.query_map([id.as_slice()], |r| r.get(0))?
		.collect::<rusqlite::Result<_>>()?;
	let invites = conn
		.prepare("SELECT invite_id, expires_at, uses_left FROM channel_invites WHERE channel = ?1 ORDER BY invite_id")?
		.query_map([id.as_slice()], |r| {
			Ok(InviteMeta {
				id: fixed(r.get(0)?)?,
				expires_at: r.get::<_, i64>(1)?.max(0) as u64,
				uses_left: r.get(2)?,
			})
		})?
		.collect::<rusqlite::Result<_>>()?;
	Ok(Some(ChannelRow {
		id: *id,
		name,
		flags,
		role,
		owner_public,
		delegation,
		cert,
		issued,
		revocations,
		invites,
	}))
}

/// Replaces the row and all its children.
pub(super) fn write(conn: &Connection, row: &ChannelRow) -> Result<()> {
	let id = row.id.as_slice();
	conn.execute("DELETE FROM channels WHERE id = ?1", [id])?;
	conn.execute(
		"INSERT INTO channels (id, name, flags, role, owner_public, delegation, cert)
		 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
		params![
			id,
			row.name,
			row.flags,
			row.role,
			row.owner_public.as_slice(),
			row.delegation,
			row.cert
		],
	)?;
	for item in &row.issued {
		conn.execute(
			"INSERT OR REPLACE INTO channel_issued (channel, subject, expires_at) VALUES (?1, ?2, ?3)",
			params![id, item.subject.as_slice(), item.expires_at as i64],
		)?;
	}
	for (seq, data) in row.revocations.iter().enumerate() {
		conn.execute(
			"INSERT INTO channel_revocations (channel, seq, data) VALUES (?1, ?2, ?3)",
			params![id, seq as i64, data],
		)?;
	}
	for invite in &row.invites {
		conn.execute(
			"INSERT OR REPLACE INTO channel_invites (channel, invite_id, expires_at, uses_left)
			 VALUES (?1, ?2, ?3, ?4)",
			params![
				id,
				invite.id.as_slice(),
				invite.expires_at as i64,
				invite.uses_left
			],
		)?;
	}
	Ok(())
}

pub(super) fn delete(conn: &Connection, id: &[u8; 16]) -> Result<bool> {
	Ok(conn.execute("DELETE FROM channels WHERE id = ?1", [id.as_slice()])? > 0)
}

/// Committed vault generations of a channel, by slot.
pub(super) fn committed(conn: &Connection, id: &[u8; 16]) -> Result<BTreeMap<String, i64>> {
	let mut stmt = conn.prepare(
		"SELECT slot, gen FROM channel_secrets WHERE channel = ?1 AND state = 'committed'",
	)?;
	let rows = stmt.query_map([id.as_slice()], |r| {
		Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
	})?;
	Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(super) fn next_gen(conn: &Connection, id: &[u8; 16], slot: &str) -> Result<i64> {
	let max: Option<i64> = conn.query_row(
		"SELECT MAX(gen) FROM channel_secrets WHERE channel = ?1 AND slot = ?2",
		params![id.as_slice(), slot],
		|r| r.get(0),
	)?;
	Ok(max.unwrap_or(0) + 1)
}

pub(super) fn add_pending(
	conn: &Connection,
	id: &[u8; 16],
	slot: &str,
	version: i64,
) -> Result<()> {
	conn.execute(
		"INSERT INTO channel_secrets (channel, slot, gen, state) VALUES (?1, ?2, ?3, 'pending')",
		params![id.as_slice(), slot, version],
	)?;
	Ok(())
}

pub(super) fn set_state(
	conn: &Connection,
	id: &[u8; 16],
	slot: &str,
	version: i64,
	state: &str,
) -> Result<()> {
	conn.execute(
		"UPDATE channel_secrets SET state = ?4 WHERE channel = ?1 AND slot = ?2 AND gen = ?3",
		params![id.as_slice(), slot, version, state],
	)?;
	Ok(())
}

/// Vault entries that must be wiped: `(channel, slot, version)` of pending and dead rows.
pub(super) fn garbage(
	conn: &Connection,
	only: Option<&[u8; 16]>,
) -> Result<Vec<([u8; 16], String, i64)>> {
	let mut stmt = conn.prepare(
		"SELECT channel, slot, gen FROM channel_secrets
		 WHERE state != 'committed' AND (?1 IS NULL OR channel = ?1)",
	)?;
	let rows = stmt.query_map([only.map(|i| i.as_slice())], |r| {
		Ok((
			fixed::<16>(r.get(0)?)?,
			r.get::<_, String>(1)?,
			r.get::<_, i64>(2)?,
		))
	})?;
	Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(super) fn forget(conn: &Connection, id: &[u8; 16], slot: &str, version: i64) -> Result<()> {
	conn.execute(
		"DELETE FROM channel_secrets WHERE channel = ?1 AND slot = ?2 AND gen = ?3",
		params![id.as_slice(), slot, version],
	)?;
	Ok(())
}
