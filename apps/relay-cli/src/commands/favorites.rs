//! `favorites [list|add|remove|move]`: the quick-call list of up to 6 trusted peers.
//!
//! Positions shown and typed are 1-based, in the order the Calls page shows them.

use std::fmt::{self, Display, Formatter};

use relay_api::pb::{Peer, Snapshot};
use serde::Serialize;

use super::resolve::resolve_peer;
use super::{Out, names, report, table};
use crate::cli::{CliError, FavoritesCommand};
use crate::client::{Control, Manage};

/// Most favorites the engine keeps (`relay_config::favorites::MAX_FAVORITES`).
const MAX: usize = 6;

/// One favorite.
#[derive(Debug, Serialize)]
pub struct FavoriteView {
	/// 1-based place in the list.
	pub position: usize,
	pub id: String,
	pub name: String,
	pub fingerprint_short: String,
	pub reachable: bool,
}

/// The favorites in order.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct FavoriteList(pub Vec<FavoriteView>);

impl FavoriteList {
	pub fn from_snapshot(snapshot: &Snapshot) -> Self {
		Self(
			ordered(snapshot)
				.into_iter()
				.enumerate()
				.map(|(index, p)| FavoriteView {
					position: index + 1,
					id: names::hex_id(&p.peer_id),
					name: p.display_name.clone(),
					fingerprint_short: p.fingerprint_short.clone(),
					reachable: p.reachable,
				})
				.collect(),
		)
	}
}

impl Display for FavoriteList {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		if self.0.is_empty() {
			return f.write_str("no favorites yet; add one with `relay favorites add <peer>`");
		}
		let mut t = table::with_header(
			table::Layout::detect(),
			&["#", "NAME", "FINGERPRINT", "ONLINE"],
		);
		for v in &self.0 {
			t.add_row([
				v.position.to_string(),
				v.name.clone(),
				v.fingerprint_short.clone(),
				if v.reachable { "yes" } else { "no" }.to_owned(),
			]);
		}
		f.write_str(&table::render(t))
	}
}

/// The favorite peers, first place first.
fn ordered(snapshot: &Snapshot) -> Vec<&Peer> {
	let mut peers: Vec<&Peer> = snapshot.peers.iter().filter(|p| p.favorite).collect();
	peers.sort_by_key(|p| p.favorite_position);
	peers
}

pub async fn run<C: Control + Manage>(
	client: &mut C,
	cmd: Option<FavoritesCommand>,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let snapshot = client.snapshot().await?;
	let mut ids: Vec<Vec<u8>> = ordered(&snapshot)
		.iter()
		.map(|p| p.peer_id.clone())
		.collect();
	match cmd.unwrap_or(FavoritesCommand::List) {
		FavoritesCommand::List => return out.emit(&FavoriteList::from_snapshot(&snapshot)),
		FavoritesCommand::Add { peer } => {
			let found = resolve_peer(&snapshot, &peer)?;
			// Adding a favorite twice is harmless: the same list is set again.
			if !ids.contains(&found.peer_id) {
				if ids.len() >= MAX {
					return Err(CliError::Usage(format!(
						"You can have up to {MAX} favorites; remove one first"
					)));
				}
				ids.push(found.peer_id.clone());
			}
		}
		FavoritesCommand::Remove { peer } => {
			let found = resolve_peer(&snapshot, &peer)?;
			let Some(index) = ids.iter().position(|id| *id == found.peer_id) else {
				return Err(CliError::Usage(format!(
					"{} is not a favorite",
					found.display_name
				)));
			};
			ids.remove(index);
		}
		FavoritesCommand::Move { peer, position } => {
			let found = resolve_peer(&snapshot, &peer)?;
			let Some(from) = ids.iter().position(|id| *id == found.peer_id) else {
				return Err(CliError::Usage(format!(
					"{} is not a favorite",
					found.display_name
				)));
			};
			if position == 0 || position > ids.len() {
				return Err(CliError::Usage(format!(
					"the position must be between 1 and {}",
					ids.len()
				)));
			}
			let id = ids.remove(from);
			ids.insert(position - 1, id);
		}
	}
	let op = client.set_favorites(&ids).await?;
	report(&op, out)
}
