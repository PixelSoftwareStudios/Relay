//! SQL row mapping for preferences.

use rusqlite::Connection;

use super::flat::{flatten, unflatten};
use super::model::Preferences;
use crate::error::Result;

/// Reads the stored preferences; `None` when nothing was ever saved.
pub(crate) fn load(conn: &Connection) -> Result<Option<Preferences>> {
	let mut stmt = conn.prepare("SELECT key, value FROM prefs")?;
	let rows = stmt
		.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
		.collect::<rusqlite::Result<Vec<_>>>()?;
	if rows.is_empty() {
		return Ok(None);
	}
	unflatten(rows).map(Some)
}

/// Replaces all rows; the caller supplies the transaction.
pub(crate) fn save_in(conn: &Connection, prefs: &Preferences) -> Result<()> {
	let flat = flatten(prefs)?;
	conn.execute("DELETE FROM prefs", [])?;
	let mut insert = conn.prepare("INSERT INTO prefs (key, value) VALUES (?1, ?2)")?;
	for (key, value) in &flat {
		insert.execute([key, value])?;
	}
	Ok(())
}

/// Replaces all rows atomically.
pub(crate) fn save(conn: &mut Connection, prefs: &Preferences) -> Result<()> {
	let tx = conn.transaction()?;
	save_in(&tx, prefs)?;
	tx.commit()?;
	Ok(())
}

/// Drops the microphone choice and forces receive-only, keeping other settings.
pub(crate) fn clear_microphone(conn: &mut Connection) -> Result<()> {
	let Some(mut prefs) = load(conn)? else {
		return Ok(());
	};
	prefs.clear_microphone();
	save(conn, &prefs)
}
