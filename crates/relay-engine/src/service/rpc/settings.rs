//! `GetSettings` and `SetSettings`: any preference by its flat key.

use std::collections::BTreeMap;
use std::time::Duration;

use prost::Message;
use relay_api::pb::{
	ClientContext, Operation, OperationState, SetSettingsRequest, SetSettingsResponse,
	SettingOutcome, SettingResult, SettingsDump,
};
use relay_core::{Command, OperationId};
use tonic::{Request, Response, Status};

use super::EngineService;
use super::commands::gain;
use crate::messaging::{Caller, MessagingLimits};
use crate::service::convert::Mutation;
use crate::service::error::ServiceError;
use crate::service::settings_keys::{
	KeyOutcome, KeyResult, is_messaging, is_messaging_limit, is_sensitive,
};
use relay_config::prefs::Limits;

const MAX_ENTRIES: usize = 64;
const MAX_KEY_BYTES: usize = 128;
const MAX_VALUE_BYTES: usize = 1024;

fn invalid(what: &str) -> ServiceError {
	ServiceError::InvalidArgument(what.to_owned())
}

async fn all(service: &EngineService) -> Result<BTreeMap<String, String>, Status> {
	let settings = service.shared.settings.clone();
	let found = tokio::task::spawn_blocking(move || settings.all())
		.await
		.map_err(|_| ServiceError::Stopped)?
		.map_err(ServiceError::from)?;
	Ok(found)
}

pub(super) async fn get(
	service: &EngineService,
	request: Request<ClientContext>,
) -> Result<Response<SettingsDump>, Status> {
	service.observer(&request, |body| Some(body))?;
	Ok(Response::new(SettingsDump {
		values: all(service).await?.into_iter().collect(),
		usage: usage(service).await,
	}))
}

/// Bytes in use next to the limit that bounds them, keyed by that limit's preference key.
/// Only what is cheap to know is reported: the avatar cache size, the ended transcripts and
/// the largest live transcript. Anything unknown is left out.
async fn usage(service: &EngineService) -> std::collections::HashMap<String, u64> {
	let mut out = std::collections::HashMap::new();
	let settings = service.shared.settings.clone();
	if let Ok(Some(bytes)) =
		tokio::task::spawn_blocking(move || settings.avatar_cache_bytes()).await
	{
		out.insert("limits.avatar_cache_mib".to_owned(), bytes);
	}
	if let Some(hub) = service.shared.messaging.clone()
		&& let Ok(used) = hub.ask(|h| h.usage()).await
	{
		out.insert(
			"limits.messages_retained_mib".to_owned(),
			used.retained_bytes,
		);
		out.insert(
			"limits.messages_session_mib".to_owned(),
			used.largest_live_bytes,
		);
	}
	out
}

/// Refuses a malformed request, and any security-sensitive key whose value would change.
/// A sensitive key given its current value is a no-op, so an export can be imported again.
async fn screen(service: &EngineService, body: &SetSettingsRequest) -> Result<(), Status> {
	if body.values.is_empty() {
		return Err(invalid("nothing to change").into());
	}
	if body.values.len() > MAX_ENTRIES {
		return Err(invalid("too many settings in one request").into());
	}
	for (key, value) in &body.values {
		if key.is_empty() || key.len() > MAX_KEY_BYTES || key.chars().any(char::is_control) {
			return Err(invalid("malformed setting key").into());
		}
		if value.len() > MAX_VALUE_BYTES {
			return Err(invalid("setting value too long").into());
		}
	}
	let restricted: Vec<&String> = body.values.keys().filter(|k| is_sensitive(k)).collect();
	if restricted.is_empty() {
		return Ok(());
	}
	let current = all(service).await?;
	for key in restricted {
		if current.get(key.as_str()) != body.values.get(key) {
			return Err(Status::permission_denied(format!(
				"SETTING_RESTRICTED: `{key}` cannot be changed over the API; stop the daemon and use `relay config set`"
			)));
		}
	}
	Ok(())
}

/// An operation id for the follow-up work, distinct per request so a retry replays and a
/// different request never collides with the caller's own id.
fn derived(mutation: &Mutation, print: &[u8; 32]) -> OperationId {
	let mut hasher = blake3::Hasher::new();
	hasher.update(b"relay/set-settings");
	hasher.update(&mutation.operation.to_bytes());
	hasher.update(print);
	let digest = hasher.finalize();
	let mut bytes = [0u8; 16];
	bytes.copy_from_slice(&digest.as_bytes()[..16]);
	OperationId::from_u128(u128::from_be_bytes(bytes))
}

fn downgrade(results: &mut [KeyResult], keys: impl Fn(&str) -> bool, why: &str) {
	for result in results.iter_mut() {
		if result.outcome == KeyOutcome::AppliedLive && keys(&result.key) {
			result.outcome = KeyOutcome::SavedNeedsRestart(why.to_owned());
		}
	}
}

fn applied(results: &[KeyResult], key: &str) -> bool {
	results
		.iter()
		.any(|r| r.key == key && r.outcome == KeyOutcome::AppliedLive)
}

/// The domain core owns playback gain and the push-to-talk release delay.
async fn domain(
	service: &EngineService,
	mutation: &Mutation,
	print: &[u8; 32],
	current: &BTreeMap<String, String>,
	results: &mut [KeyResult],
) {
	let playback = applied(results, "audio.playback_gain");
	let delay = applied(results, "ptt.release_delay_ms");
	if !playback && !delay {
		return;
	}
	let output_gain = playback
		.then(|| current.get("audio.playback_gain")?.parse::<f32>().ok())
		.flatten()
		.and_then(|g| gain(g, 2.0).ok());
	let ptt_release_delay = delay
		.then(|| current.get("ptt.release_delay_ms")?.parse::<u64>().ok())
		.flatten()
		.map(Duration::from_millis);
	let command = Command::SetGlobalControls {
		muted: None,
		deafened: None,
		output_gain,
		ptt_release_delay,
		expected: None,
	};
	let follow = Mutation {
		operation: derived(mutation, print),
		expected_revision: None,
		..*mutation
	};
	let ok = matches!(
		service.dispatch(&follow, command).await,
		Ok(done) if done.result.is_ok()
	);
	if !ok {
		let why = "the call engine did not accept it; restart to apply";
		downgrade(
			results,
			|k| k == "audio.playback_gain" || k == "ptt.release_delay_ms",
			why,
		);
	}
}

/// The messaging actor owns `messages.*`.
async fn messaging(
	service: &EngineService,
	mutation: &Mutation,
	print: &[u8; 32],
	current: &BTreeMap<String, String>,
	results: &mut [KeyResult],
) {
	if !results
		.iter()
		.any(|r| is_messaging(&r.key) && r.outcome == KeyOutcome::AppliedLive)
	{
		return;
	}
	let Some(hub) = service.shared.messaging.clone() else {
		downgrade(
			results,
			is_messaging,
			"session messaging is not running; restart to apply",
		);
		return;
	};
	let read = |key: &str| current.get(key).map(String::as_str);
	let who = Caller {
		client: mutation.client.to_bytes(),
		operation: derived(mutation, print).to_bytes(),
		print: *print,
	};
	let (enabled, max, rate) = (
		read("messages.enabled").and_then(|v| v.parse::<bool>().ok()),
		read("messages.max_offer_size_mib").and_then(|v| v.parse::<u32>().ok()),
		read("messages.transfer_rate_limit_mbps").and_then(|v| v.parse::<u32>().ok()),
	);
	let done = hub
		.ask(move |h| h.set_settings(who, enabled, max, rate))
		.await
		.ok()
		.and_then(|r| r.operation)
		.is_some_and(|o| o.state == OperationState::Applied as i32);
	if !done {
		downgrade(
			results,
			is_messaging,
			"the messaging actor did not accept it; restart to apply",
		);
	}
}

/// The messaging actor enforces the transcript budgets and the transfer concurrency.
async fn storage_limits(
	service: &EngineService,
	current: &BTreeMap<String, String>,
	results: &mut [KeyResult],
) {
	if !results
		.iter()
		.any(|r| is_messaging_limit(&r.key) && r.outcome == KeyOutcome::AppliedLive)
	{
		return;
	}
	let Some(hub) = service.shared.messaging.clone() else {
		downgrade(
			results,
			is_messaging_limit,
			"session messaging is not running; restart to apply",
		);
		return;
	};
	let limits = MessagingLimits::from_prefs(&Limits::from_map(current));
	if hub.ask(move |h| h.set_limits(limits)).await.is_err() {
		downgrade(
			results,
			is_messaging_limit,
			"the messaging actor did not accept it; restart to apply",
		);
	}
}

fn answer(operation: Operation, results: Vec<KeyResult>) -> SetSettingsResponse {
	let results = results
		.into_iter()
		.map(|r| {
			let (outcome, reason) = match r.outcome {
				KeyOutcome::AppliedLive => (SettingOutcome::AppliedLive, String::new()),
				KeyOutcome::SavedNeedsRestart(why) => (SettingOutcome::SavedNeedsRestart, why),
				KeyOutcome::Rejected(why) => (SettingOutcome::Rejected, why),
			};
			SettingResult {
				key: r.key,
				outcome: outcome as i32,
				reason,
			}
		})
		.collect();
	SetSettingsResponse {
		operation: Some(operation),
		results,
	}
}

pub(super) async fn set(
	service: &EngineService,
	request: Request<SetSettingsRequest>,
) -> Result<Response<SetSettingsResponse>, Status> {
	let (mutation, body) = service.begin(request, |b| b.context.as_ref())?;
	screen(service, &body).await?;
	let print = *blake3::hash(&body.encode_to_vec()).as_bytes();
	let mut entries: Vec<(String, String)> = body.values.into_iter().collect();
	entries.sort();
	let mut results = service.shared.handle.settings(entries).await?;
	let current = if results.iter().any(|r| r.outcome == KeyOutcome::AppliedLive) {
		all(service).await?
	} else {
		BTreeMap::new()
	};
	domain(service, &mutation, &print, &current, &mut results).await;
	messaging(service, &mutation, &print, &current, &mut results).await;
	storage_limits(service, &current, &mut results).await;
	let operation = Operation {
		operation_id: mutation.operation.to_bytes().to_vec(),
		state: OperationState::Applied as i32,
		revision: 0,
		call_id: Vec::new(),
		problem: None,
	};
	Ok(Response::new(answer(operation, results)))
}
