//! `auto-answer [list|on|off]`: contacts whose calls the engine answers by itself.

use std::fmt::{self, Display, Formatter};

use relay_api::pb::{Snapshot, TrustState};
use serde::Serialize;

use super::resolve::resolve_peer;
use super::{Out, names, report, table};
use crate::cli::{AutoAnswerCommand, CliError};
use crate::client::{Control, Manage};

/// One contact that answers automatically.
#[derive(Debug, Serialize)]
pub struct AutoAnswerView {
	pub id: String,
	pub name: String,
	pub fingerprint_short: String,
	pub reachable: bool,
}

/// The contacts that answer automatically, and whether the global switch is on.
#[derive(Debug, Serialize)]
pub struct AutoAnswerList {
	/// `calls.auto_answer_enabled`: off ignores every flag below.
	pub enabled: bool,
	pub contacts: Vec<AutoAnswerView>,
}

impl AutoAnswerList {
	pub fn from_snapshot(snapshot: &Snapshot) -> Self {
		Self {
			enabled: snapshot
				.settings
				.as_ref()
				.is_none_or(|s| s.auto_answer_enabled),
			contacts: snapshot
				.peers
				.iter()
				.filter(|p| p.auto_answer)
				.map(|p| AutoAnswerView {
					id: names::hex_id(&p.peer_id),
					name: p.display_name.clone(),
					fingerprint_short: p.fingerprint_short.clone(),
					reachable: p.reachable,
				})
				.collect(),
		}
	}
}

impl Display for AutoAnswerList {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		if !self.enabled {
			writeln!(
				f,
				"auto-answer is switched off (calls.auto_answer_enabled): every call rings"
			)?;
		}
		if self.contacts.is_empty() {
			return f
				.write_str("no contact answers automatically; use `relay auto-answer on <peer>`");
		}
		let mut t = table::with_header(table::Layout::detect(), &["NAME", "FINGERPRINT", "ONLINE"]);
		for v in &self.contacts {
			t.add_row([
				v.name.clone(),
				v.fingerprint_short.clone(),
				if v.reachable { "yes" } else { "no" }.to_owned(),
			]);
		}
		f.write_str(&table::render(t))
	}
}

pub async fn run<C: Control + Manage>(
	client: &mut C,
	cmd: Option<AutoAnswerCommand>,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let snapshot = client.snapshot().await?;
	let (peer, enabled) = match cmd.unwrap_or(AutoAnswerCommand::List) {
		AutoAnswerCommand::List => return out.emit(&AutoAnswerList::from_snapshot(&snapshot)),
		AutoAnswerCommand::On { peer } => (peer, true),
		AutoAnswerCommand::Off { peer } => (peer, false),
	};
	let found = resolve_peer(&snapshot, &peer)?;
	if enabled && found.trust != TrustState::Verified as i32 {
		return Err(CliError::Usage(format!(
			"{} is not verified; compare the safety code first (`relay trust verify`)",
			found.display_name
		)));
	}
	let op = client.set_peer_auto_answer(&found.peer_id, enabled).await?;
	report(&op, out)
}
