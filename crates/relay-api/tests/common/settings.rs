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

	pub fn dump(&self) -> SettingsDump {
		SettingsDump {
			values: self.values.lock().unwrap().clone().into_iter().collect(),
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
}
