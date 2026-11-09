//! Channel storage over SQLite metadata plus a secret vault.

use std::collections::BTreeMap;
use std::sync::Arc;

use rusqlite::Connection;
use zeroize::Zeroizing;

use super::{ChannelData, rows};
use crate::db::Database;
use crate::error::{ConfigError, Result};
use crate::vault::SecretVault;

/// Vault key of one generation of a secret slot.
fn key(slot: &str, version: i64) -> String {
	format!("{slot}.{version}")
}

/// Channels with metadata in the database and secrets in the vault.
///
/// Every mutation is one database transaction for the metadata; the matching vault
/// writes happen before it commits and are undone if it fails.
#[derive(Clone)]
pub struct ChannelStorage {
	db: Database,
	vault: Arc<dyn SecretVault>,
}

impl std::fmt::Debug for ChannelStorage {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ChannelStorage")
			.field("vault", &self.vault.describe())
			.finish_non_exhaustive()
	}
}

impl ChannelStorage {
	/// Opens the storage and wipes vault entries left behind by an interrupted write
	/// or removal.
	pub fn open(db: &Database, vault: Arc<dyn SecretVault>) -> Result<Self> {
		let storage = Self {
			db: db.clone(),
			vault,
		};
		storage.db.with(|conn| storage.purge(conn, None))?;
		Ok(storage)
	}

	/// Where the secrets of this storage live, for display.
	pub fn vault_description(&self) -> &'static str {
		self.vault.describe()
	}

	pub fn list(&self) -> Result<Vec<ChannelData>> {
		self.db.with(|conn| {
			let mut out = Vec::new();
			for id in rows::ids(conn)? {
				out.extend(self.load(conn, &id)?);
			}
			Ok(out)
		})
	}

	pub fn get(&self, id: &[u8; 16]) -> Result<Option<ChannelData>> {
		self.db.with(|conn| self.load(conn, id))
	}

	/// Inserts or replaces. A new channel beyond `max_channels` is
	/// [`ConfigError::ChannelLimit`].
	pub fn put(&self, data: &ChannelData, max_channels: usize) -> Result<()> {
		self.db
			.with(|conn| self.apply(conn, &data.row.id, Some(data), max_channels))
			.map(drop)
	}

	/// Atomic read-modify-write. `apply` runs once while nothing else can write
	/// (the connection is held for the whole operation). Returns false if the channel
	/// does not exist.
	/// The callback must not re-enter this database, including through a cloned handle.
	///
	/// # Errors
	/// Returns an error if reading, validating, storing, or cleaning up channel data fails.
	pub fn update(&self, id: &[u8; 16], apply: &mut dyn FnMut(&mut ChannelData)) -> Result<bool> {
		self.db.with(|conn| {
			let Some(mut data) = self.load(conn, id)? else {
				return Ok(false);
			};
			apply(&mut data);
			data.row.id = *id;
			self.apply(conn, id, Some(&data), usize::MAX)?;
			Ok(true)
		})
	}

	/// Deletes the channel and wipes all its secrets. Returns whether it existed.
	pub fn remove(&self, id: &[u8; 16]) -> Result<bool> {
		self.db.with(|conn| self.apply(conn, id, None, usize::MAX))
	}

	fn load(&self, conn: &Connection, id: &[u8; 16]) -> Result<Option<ChannelData>> {
		let Some(row) = rows::read(conn, id)? else {
			return Ok(None);
		};
		let mut secrets = BTreeMap::new();
		for (slot, version) in rows::committed(conn, id)? {
			let value = self.vault.get(id, &key(&slot, version))?.ok_or_else(|| {
				ConfigError::CredentialStore(format!("secret `{slot}` is missing from the vault"))
			})?;
			secrets.insert(slot, value);
		}
		Ok(Some(ChannelData { row, secrets }))
	}

	/// Writes `next` (or removes the channel when `None`); returns whether a row
	/// existed before a removal, and `true` after a successful set.
	fn apply(
		&self,
		conn: &mut Connection,
		id: &[u8; 16],
		next: Option<&ChannelData>,
		max_channels: usize,
	) -> Result<bool> {
		if let Some(data) = next {
			data.validate()?;
			if data.row.id != *id {
				return Err(ConfigError::InvalidPreference(
					"channel id mismatch".to_owned(),
				));
			}
			if !rows::exists(conn, id)? && rows::count(conn)? >= max_channels {
				return Err(ConfigError::ChannelLimit);
			}
		}
		let current = rows::committed(conn, id)?;
		let mut keep = BTreeMap::new();
		let mut staged: Vec<(&String, i64, &Zeroizing<Vec<u8>>)> = Vec::new();
		for (slot, value) in next.iter().flat_map(|d| d.secrets.iter()) {
			let same = match current.get(slot) {
				Some(version) => self
					.vault
					.get(id, &key(slot, *version))?
					.is_some_and(|old| old.as_slice() == value.as_slice()),
				None => false,
			};
			if same {
				keep.insert(slot.clone(), current[slot]);
			} else {
				staged.push((slot, rows::next_gen(conn, id, slot)?, value));
			}
		}
		// Record the intent durably before any secret reaches the vault.
		if !staged.is_empty() {
			let tx = conn.transaction()?;
			for (slot, version, _) in &staged {
				rows::add_pending(&tx, id, slot, *version)?;
			}
			tx.commit()?;
		}
		let committed = self.write_and_commit(conn, id, next, &staged, &current, &keep);
		let existed = match committed {
			Ok(existed) => existed,
			Err(e) => {
				self.abort(conn, id, &staged);
				return Err(e);
			}
		};
		self.purge(conn, Some(id))?;
		Ok(existed)
	}

	fn write_and_commit(
		&self,
		conn: &mut Connection,
		id: &[u8; 16],
		next: Option<&ChannelData>,
		staged: &[(&String, i64, &Zeroizing<Vec<u8>>)],
		current: &BTreeMap<String, i64>,
		keep: &BTreeMap<String, i64>,
	) -> Result<bool> {
		for (slot, version, value) in staged {
			self.vault.put(id, &key(slot, *version), value)?;
		}
		let tx = conn.transaction()?;
		let existed = match next {
			Some(data) => {
				rows::write(&tx, &data.row)?;
				true
			}
			None => rows::delete(&tx, id)?,
		};
		for (slot, version, _) in staged {
			rows::set_state(&tx, id, slot, *version, "committed")?;
		}
		for (slot, version) in current {
			if keep.get(slot) != Some(version) {
				rows::set_state(&tx, id, slot, *version, "dead")?;
			}
		}
		tx.commit()?;
		Ok(existed)
	}

	/// Undoes a failed write: staged vault entries and their pending rows.
	fn abort(
		&self,
		conn: &Connection,
		id: &[u8; 16],
		staged: &[(&String, i64, &Zeroizing<Vec<u8>>)],
	) {
		for (slot, version, _) in staged {
			if self.vault.delete(id, &key(slot, *version)).is_ok() {
				let _ = rows::forget(conn, id, slot, *version);
			}
		}
	}

	/// Wipes vault entries of pending (crashed) and dead (superseded or removed) rows.
	fn purge(&self, conn: &Connection, only: Option<&[u8; 16]>) -> Result<()> {
		for (channel, slot, version) in rows::garbage(conn, only)? {
			self.vault.delete(&channel, &key(&slot, version))?;
			rows::forget(conn, &channel, &slot, version)?;
		}
		Ok(())
	}
}
