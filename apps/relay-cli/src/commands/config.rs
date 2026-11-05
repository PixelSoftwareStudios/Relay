//! `relay config get|set|export|import`.
//!
//! Reading never needs the settings lock (`PreferenceStore::read_only`). Writing
//! does, and a running engine holds it: `set` is then routed through the engine
//! for the settings it can change live, and everything else says so plainly.
//! `import` replaces every setting, so it needs the engine stopped.

use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::path::Path;

use relay_api::pb::{AdvertMode, InboundPolicy};
use relay_config::prefs::{PreferenceStore, Preferences, export_toml, flat};
use relay_config::{ConfigError, Paths, Storage};
use serde::Serialize;

use super::{Out, report};
use crate::cli::{CliError, ConfigCommand, GlobalArgs};
use crate::client::{self, Control, GlobalControls, Manage};

/// Largest settings file accepted by `import`.
const IMPORT_MAX_BYTES: u64 = 1024 * 1024;

/// Settings the engine can change while it runs.
pub const LIVE_KEYS: [&str; 5] = [
	"profile.display_name",
	"audio.microphone_gain",
	"audio.playback_gain",
	"privacy.inbound_policy",
	"privacy.advert_mode",
];

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
		ConfigCommand::Get { key } => get(paths, key, out),
		ConfigCommand::Export { file } => export(paths, file.as_deref(), out),
		ConfigCommand::Set { key, value } => set(global, paths, &key, &value, out).await,
		ConfigCommand::Import { file } => import(global, paths, &file, out).await,
	}
}

fn config_error(err: ConfigError) -> CliError {
	match err {
		ConfigError::Locked(path) => CliError::Other(anyhow::anyhow!(
			"the settings are locked by another process ({}); stop the engine or use the live settings with it running",
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

fn get(paths: &Paths, key: Option<String>, out: &mut Out<'_>) -> Result<(), CliError> {
	let prefs = load(paths)?;
	match key {
		Some(key) => {
			let value = flat::get(&prefs, &key).map_err(config_error)?;
			out.emit(&Setting { key, value })
		}
		None => {
			let mut all = BTreeMap::new();
			for key in flat::keys() {
				let value = flat::get(&prefs, &key).map_err(config_error)?;
				all.insert(key, value);
			}
			out.emit(&Settings(all))
		}
	}
}

fn export(paths: &Paths, file: Option<&Path>, out: &mut Out<'_>) -> Result<(), CliError> {
	let text = export_toml(&load(paths)?).map_err(config_error)?;
	match file {
		Some(file) => {
			std::fs::write(file, text)?;
			out.emit(&Written {
				path: file.display().to_string(),
			})
		}
		None => out.line(text.trim_end()),
	}
}

async fn engine_running(global: &GlobalArgs) -> Result<Option<client::RealClient>, CliError> {
	match client::connect(global).await {
		Ok(client) => Ok(Some(client)),
		Err(CliError::Unreachable { .. }) => Ok(None),
		Err(other) => Err(other),
	}
}

async fn set(
	global: &GlobalArgs,
	paths: &Paths,
	key: &str,
	value: &str,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	if !flat::keys().iter().any(|k| k == key) {
		return Err(CliError::Usage(format!("unknown setting `{key}`")));
	}
	if let Some(mut engine) = engine_running(global).await? {
		return set_live(&mut engine, key, value, out).await;
	}
	let storage = Storage::open(paths).map_err(config_error)?;
	let store = PreferenceStore::open(&storage.database).map_err(config_error)?;
	store.set(key, value).map_err(config_error)?;
	out.emit(&Setting {
		key: key.to_owned(),
		value: store.get(key).map_err(config_error)?,
	})
}

/// Change a setting through the running engine.
pub async fn set_live<C: Control + Manage>(
	client: &mut C,
	key: &str,
	value: &str,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let gain = |text: &str| {
		text.parse::<f32>()
			.ok()
			.filter(|g| g.is_finite() && (0.0..=4.0).contains(g))
			.ok_or_else(|| CliError::Usage("gain must be a number from 0.0 to 4.0".into()))
	};
	let op = match key {
		"profile.display_name" => client.set_profile(value).await?,
		"audio.microphone_gain" => {
			let controls = GlobalControls {
				microphone_gain: Some(gain(value)?),
				..GlobalControls::default()
			};
			client.set_global_controls(controls).await?
		}
		"audio.playback_gain" => {
			let controls = GlobalControls {
				playback_gain: Some(gain(value)?),
				..GlobalControls::default()
			};
			client.set_global_controls(controls).await?
		}
		"privacy.inbound_policy" => {
			let policy = match value {
				"anyone" => InboundPolicy::Anyone,
				"shared_channel_only" => InboundPolicy::SharedChannelOnly,
				_ => {
					return Err(CliError::Usage(
						"use `anyone` or `shared_channel_only`".into(),
					));
				}
			};
			client.set_privacy(Some(policy), None).await?
		}
		"privacy.advert_mode" => {
			let mode = match value {
				"visible" => AdvertMode::Visible,
				"hidden" => AdvertMode::Hidden,
				_ => return Err(CliError::Usage("use `visible` or `hidden`".into())),
			};
			client.set_privacy(None, Some(mode)).await?
		}
		_ => {
			return Err(CliError::Usage(format!(
				"the engine is running and owns the settings; it can change {} live. Stop it to change `{key}`",
				LIVE_KEYS.join(", ")
			)));
		}
	};
	report(&op, out)
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
	if engine_running(global).await?.is_some() {
		return Err(CliError::Usage(
			"the engine is running and holds the settings; stop it before importing".into(),
		));
	}
	let storage = Storage::open(paths).map_err(config_error)?;
	let store = PreferenceStore::open(&storage.database).map_err(config_error)?;
	store.import_toml(&text).map_err(config_error)?;
	out.emit(&Message {
		message: "settings imported".into(),
	})
}
