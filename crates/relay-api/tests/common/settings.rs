//! The fake engine's avatar-removal, trust-forgetting and settings behaviour.

use std::collections::BTreeMap;
use std::sync::Mutex;

use relay_api::pb::*;
use tonic::Status;

#[derive(Default)]
pub struct Settings {
	pub values: Mutex<BTreeMap<String, String>>,
	pub avatar_clears: Mutex<u32>,
	pub forgotten: Mutex<Vec<String>>,
	pub history: Mutex<Vec<CallHistoryEntry>>,
	pub favorites: Mutex<Vec<Vec<u8>>>,
	pub auto_answer: Mutex<Vec<(Vec<u8>, bool)>>,
}

fn applied(context: Option<&MutationContext>) -> Operation {
	Operation {
		operation_id: context.map(|c| c.operation_id.clone()).unwrap_or_default(),
		state: OperationState::Applied as i32,
		..Operation::default()
	}
}

fn result(key: &str, outcome: SettingOutcome, reason: &str) -> SettingResult {
	SettingResult {
		key: key.to_owned(),
		outcome: outcome as i32,
		reason: reason.to_owned(),
	}
}

impl Settings {
	pub fn clear_avatar(&self, context: &MutationContext) -> Operation {
		*self.avatar_clears.lock().unwrap() += 1;
		applied(Some(context))
	}

	pub fn forget(&self, body: &ForgetPeerRequest) -> Result<Operation, Status> {
		if body.peer_id.is_empty() && body.query.trim().is_empty() {
			return Err(Status::invalid_argument("name a peer"));
		}
		let mut operation = applied(body.context.as_ref());
		if body.query == "twin" {
			operation.state = OperationState::Failed as i32;
			operation.problem = Some(Problem {
				code: "PEER_AMBIGUOUS".into(),
				..Problem::default()
			});
		} else {
			self.forgotten.lock().unwrap().push(body.query.clone());
		}
		Ok(operation)
	}

	/// `SetPeerAutoAnswer`: recorded as sent; a peer id of the wrong length is refused.
	pub fn set_auto_answer(&self, body: &SetPeerAutoAnswerRequest) -> Operation {
		let mut operation = applied(body.context.as_ref());
		if body.peer_id.len() == 32 {
			self.auto_answer
				.lock()
				.unwrap()
				.push((body.peer_id.clone(), body.enabled));
		} else {
			operation.state = OperationState::Failed as i32;
			operation.problem = Some(Problem {
				code: "INVALID_REQUEST".into(),
				message: "malformed peer id".into(),
				..Problem::default()
			});
		}
		operation
	}

	/// The contract of `SetFavorites`: at most 6, none twice; a refusal changes nothing.
	pub fn set_favorites(&self, body: &SetFavoritesRequest) -> Operation {
		let mut operation = applied(body.context.as_ref());
		let twice = body
			.peer_ids
			.iter()
			.enumerate()
			.any(|(i, id)| body.peer_ids[..i].contains(id));
		if body.peer_ids.len() > 6 || twice {
			operation.state = OperationState::Failed as i32;
			operation.problem = Some(Problem {
				code: "INVALID_REQUEST".into(),
				message: "You can have up to 6 favorites".into(),
				..Problem::default()
			});
		} else {
			*self.favorites.lock().unwrap() = body.peer_ids.clone();
		}
		operation
	}

	pub fn dump(&self) -> SettingsDump {
		SettingsDump {
			build_revision: "fake".into(),
			api_revision: 0,
			values: self.values.lock().unwrap().clone().into_iter().collect(),
			usage: [("limits.avatar_cache_mib".to_owned(), 7)].into(),
		}
	}

	/// `identity.*` is refused as a whole, `bad.*` is rejected, `restart.*` waits for a restart.
	pub fn apply(&self, body: &SetSettingsRequest) -> Result<SetSettingsResponse, Status> {
		if let Some(key) = body.values.keys().find(|k| k.starts_with("identity.")) {
			return Err(Status::permission_denied(format!(
				"SETTING_RESTRICTED: {key}"
			)));
		}
		let mut values = self.values.lock().unwrap();
		let mut results = Vec::new();
		let mut sorted: Vec<_> = body.values.iter().collect();
		sorted.sort();
		for (key, value) in sorted {
			results.push(if key.starts_with("bad.") {
				result(key, SettingOutcome::Rejected, "not a setting")
			} else {
				values.insert(key.clone(), value.clone());
				if key.starts_with("restart.") {
					result(key, SettingOutcome::SavedNeedsRestart, "read at start")
				} else {
					result(key, SettingOutcome::AppliedLive, "")
				}
			});
		}
		Ok(SetSettingsResponse {
			operation: Some(applied(body.context.as_ref())),
			results,
		})
	}

	/// Newest first by id; one extra row is looked up to set `more`.
	pub fn list_history(&self, body: &ListCallHistoryRequest) -> CallHistoryPage {
		let limit = if body.limit == 0 {
			50
		} else {
			body.limit as usize
		};
		let rows = self.history.lock().unwrap();
		let mut found: Vec<_> = rows
			.iter()
			.filter(|e| body.before_id.is_none_or(|b| e.id < b))
			.cloned()
			.collect();
		found.sort_by_key(|e| std::cmp::Reverse(e.id));
		let more = found.len() > limit;
		found.truncate(limit);
		CallHistoryPage {
			entries: found,
			more,
			revision: rows.len() as u64,
		}
	}

	pub fn clear_history(&self, context: &MutationContext) -> Operation {
		self.history.lock().unwrap().clear();
		applied(Some(context))
	}
}
