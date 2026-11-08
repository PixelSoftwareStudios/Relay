//! `relay config get|set|export|import`.
//!
//! With a daemon running every key goes through `SetSettings` and each key's outcome is
//! printed (applied live, saved for the next start, or rejected with the reason); only the
//! security-sensitive keys (`identity.*`, `network.allow_*`, `diagnostics.record_raw_audio`)
//! need the daemon stopped, and the daemon says which. With none running the settings
//! database is written directly under the writer lock. `get` and `export` read the live
//! values from a running daemon (`GetSettings`) and otherwise the database without a lock.

use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::path::Path;

use relay_api::ApiError;
use relay_api::pb::SettingOutcome;
use relay_config::prefs::{PreferenceStore, Preferences, export_toml, flat, import_toml};
use relay_config::{ConfigError, Paths, Storage};
use serde::Serialize;

use super::Out;
use crate::cli::{CliError, ConfigCommand, GlobalArgs};
use crate::client::{self, Manage};

/// Largest settings file accepted by `import`.
const IMPORT_MAX_BYTES: u64 = 1024 * 1024;

/// One setting.
#[derive(Debug, Serialize)]
pub struct Setting {
	pub key: String,
	pub value: String,
}

impl Display for Setting {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.write_str(&self.value)
	}
}

/// Every setting, sorted by key.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct Settings(pub BTreeMap<String, String>);

impl Display for Settings {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let lines: Vec<String> = self.0.iter().map(|(k, v)| format!("{k} = {v}")).collect();
		f.write_str(&lines.join("\n"))
	}
}

/// A file that was written.
#[derive(Debug, Serialize)]
pub struct Written {
	pub path: String,
}

impl Display for Written {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(f, "wrote {}", self.path)
	}
}

/// A plain confirmation.
#[derive(Debug, Serialize)]
pub struct Message {
	pub message: String,
}

impl Display for Message {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.write_str(&self.message)
	}
}

/// Where `config` reads and writes; tests point it at a temporary directory.
pub async fn run(
	global: &GlobalArgs,
	cmd: ConfigCommand,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let paths = Paths::platform().map_err(config_error)?;
	run_in(global, &paths, cmd, out).await
}

pub async fn run_in(
	global: &GlobalArgs,
	paths: &Paths,
	cmd: ConfigCommand,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	match cmd {
		ConfigCommand::Get { key } => get(global, paths, key, out).await,
		ConfigCommand::Export { file } => export(global, paths, file.as_deref(), out).await,
		ConfigCommand::Set { key, value } => set(global, paths, key, value, out).await,
		ConfigCommand::Import { file } => import(global, paths, &file, out).await,
	}
}

fn config_error(err: ConfigError) -> CliError {
	match err {
		ConfigError::Locked(path) => CliError::Other(anyhow::anyhow!(
			"the settings are locked by another process ({}); stop it, or start the daemon so changes go through it",
			path.display()
		)),
		ConfigError::InvalidPreference(message) => CliError::Usage(message),
		other => CliError::Other(other.into()),
	}
}

fn load(paths: &Paths) -> Result<Preferences, CliError> {
	Ok(PreferenceStore::read_only(paths)
		.map_err(config_error)?
		.unwrap_or_default())
}

pub(super) fn load_prefs(paths: &Paths) -> Result<Preferences, CliError> {
	load(paths)
}

pub(super) async fn engine_running(
	global: &GlobalArgs,
) -> Result<Option<client::RealClient>, CliError> {
	match client::connect_existing(global).await {
		Ok(client) => Ok(Some(client)),
		Err(CliError::Unreachable { .. }) => Ok(None),
		Err(other) => Err(other),
	}
}

/// All settings as flat text: from the running daemon, else the database.
async fn current(global: &GlobalArgs, paths: &Paths) -> Result<BTreeMap<String, String>, CliError> {
	if let Some(mut engine) = engine_running(global).await? {
		return Ok(engine.get_settings().await?);
	}
	flat::all(&load(paths)?).map_err(config_error)
}

async fn get(
	global: &GlobalArgs,
	paths: &Paths,
	key: Option<String>,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let all = current(global, paths).await?;
	match key {
		Some(key) => match all.get(&key) {
			Some(value) => out.emit(&Setting {
				value: value.clone(),
				key,
			}),
			None => Err(CliError::Usage(format!("unknown setting `{key}`"))),
		},
		None => out.emit(&Settings(all)),
	}
}

async fn export(
	global: &GlobalArgs,
	paths: &Paths,
	file: Option<&Path>,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let entries: Vec<(String, String)> = current(global, paths).await?.into_iter().collect();
	let (prefs, answers) = flat::set_many(&Preferences::default(), &entries);
	if let Some(reason) = answers.into_iter().flatten().next() {
		return Err(CliError::Usage(reason));
	}
	let text = export_toml(&prefs).map_err(config_error)?;
	match file {
		Some(file) => {
			relay_io::fs::write_private(file, text.as_bytes())
				.map_err(|e| std::io::Error::other(e.to_string()))?;
			out.emit(&Written {
				path: file.display().to_string(),
			})
		}
		None => out.line(text.trim_end()),
	}
}

async fn set(
	global: &GlobalArgs,
	paths: &Paths,
	key: String,
	value: String,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	if !flat::keys().contains(&key) {
		return Err(CliError::Usage(format!("unknown setting `{key}`")));
	}
	if let Some(mut engine) = engine_running(global).await? {
		return set_live(&mut engine, vec![(key, value)], out).await;
	}
	let storage = Storage::open(paths).map_err(config_error)?;
	let store = PreferenceStore::open(&storage.database).map_err(config_error)?;
	store.set(&key, &value).map_err(config_error)?;
	out.emit(&Setting {
		value: store.get(&key).map_err(config_error)?,
		key,
	})
}

/// Send `values` through the running daemon and print each key's outcome. Any rejected key
/// makes the command fail (exit 4) after the full report is printed.
pub async fn set_live<C: Manage>(
	client: &mut C,
	values: Vec<(String, String)>,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let outcomes = apply(client, values).await?;
	let rejected = outcomes.iter().filter(|o| o.outcome == "rejected").count();
	out.emit(&Outcomes(outcomes))?;
	if rejected > 0 {
		return Err(CliError::Rejected {
			code: "SETTING_REJECTED".into(),
			message: format!("{rejected} setting(s) were not saved"),
			retryable: false,
		});
	}
	Ok(())
}

/// The key a `SETTING_RESTRICTED` refusal names.
fn restricted_key(err: &ApiError) -> Option<String> {
	let ApiError::Status(status) = err else {
		return None;
	};
	if status.code() != tonic::Code::PermissionDenied {
		return None;
	}
	let rest = status.message().strip_prefix("SETTING_RESTRICTED:")?;
	let mut parts = rest.split('`');
	parts.next()?;
	parts.next().map(str::to_owned).filter(|k| !k.is_empty())
}

/// `SetSettings`, setting aside the keys the daemon refuses to change over the API (it
/// refuses the whole request naming one; the others are sent again without it).
async fn apply<C: Manage>(
	client: &mut C,
	mut pending: Vec<(String, String)>,
) -> Result<Vec<KeyOutcome>, CliError> {
	let mut outcomes = Vec::new();
	while !pending.is_empty() {
		match client.set_settings(pending.clone()).await {
			Ok(results) => {
				outcomes.extend(results.iter().map(KeyOutcome::from));
				break;
			}
			Err(err) => {
				let Some(key) = restricted_key(&err) else {
					return Err(err.into());
				};
				let before = pending.len();
				pending.retain(|(k, _)| *k != key);
				if pending.len() == before {
					return Err(err.into());
				}
				outcomes.push(KeyOutcome::restricted(key));
			}
		}
	}
	outcomes.sort_by(|a, b| a.key.cmp(&b.key));
	Ok(outcomes)
}

async fn import(
	global: &GlobalArgs,
	paths: &Paths,
	file: &Path,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	if std::fs::metadata(file)?.len() > IMPORT_MAX_BYTES {
		return Err(CliError::Usage(
			"the settings file is larger than 1 MiB".into(),
		));
	}
	let text = std::fs::read_to_string(file)?;
	if let Some(mut engine) = engine_running(global).await? {
		let wanted = flat::all(&import_toml(&text).map_err(config_error)?).map_err(config_error)?;
		let live = engine.get_settings().await?;
		let changes: Vec<(String, String)> = wanted
			.into_iter()
			.filter(|(key, value)| live.get(key) != Some(value))
			.collect();
		if changes.is_empty() {
			return out.emit(&Message {
				message: "nothing to change: the daemon already has these settings".into(),
			});
		}
		return set_live(&mut engine, changes, out).await;
	}
	let storage = Storage::open(paths).map_err(config_error)?;
	let store = PreferenceStore::open(&storage.database).map_err(config_error)?;
	store.import_toml(&text).map_err(config_error)?;
	out.emit(&Message {
		message: "settings imported".into(),
	})
}

/// What happened to one key.
#[derive(Debug, Serialize)]
pub struct KeyOutcome {
	pub key: String,
	/// `applied_live`, `saved_needs_restart` or `rejected`.
	pub outcome: &'static str,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub reason: Option<String>,
}

impl KeyOutcome {
	fn restricted(key: String) -> Self {
		Self {
			key,
			outcome: "rejected",
			reason: Some(
				"security-sensitive: change it with the daemon stopped (`relay daemon stop`, then `relay config set`)"
					.into(),
			),
		}
	}
}

impl From<&relay_api::pb::SettingResult> for KeyOutcome {
	fn from(r: &relay_api::pb::SettingResult) -> Self {
		let outcome = match SettingOutcome::try_from(r.outcome) {
			Ok(SettingOutcome::AppliedLive) => "applied_live",
			Ok(SettingOutcome::SavedNeedsRestart) => "saved_needs_restart",
			_ => "rejected",
		};
		Self {
			key: r.key.clone(),
			outcome,
			reason: Some(r.reason.clone()).filter(|t| !t.is_empty()),
		}
	}
}

impl Display for KeyOutcome {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let reason = self.reason.as_deref().unwrap_or_default();
		match self.outcome {
			"applied_live" => write!(f, "{}: applied live", self.key),
			"saved_needs_restart" if reason.is_empty() => {
				write!(f, "{}: saved, needs restart", self.key)
			}
			"saved_needs_restart" => write!(f, "{}: saved, needs restart ({reason})", self.key),
			_ => write!(f, "{}: rejected: {reason}", self.key),
		}
	}
}

/// The per-key report of a live `set` or `import`.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct Outcomes(pub Vec<KeyOutcome>);

impl Display for Outcomes {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		let lines: Vec<String> = self.0.iter().map(ToString::to_string).collect();
		f.write_str(&lines.join("\n"))
	}
}
