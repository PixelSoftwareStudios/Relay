//! Permissions, clearing and export.

use std::path::Path;

use serde::Serialize;

use super::ack::Ack;
use super::target::{absolutize, accepted, check, path_text, resolve};
use super::view::{SessionView, size};
use crate::cli::{CliError, Switch};
use crate::client::Messaging;
use crate::commands::Out;

fn onoff(on: bool) -> &'static str {
	if on { "on" } else { "off" }
}

fn session_text(v: &SessionView) -> String {
	let mut text = format!(
		"{} ({}): messages {}, files {}, {} message{}, {} of {} used",
		v.peer,
		v.state,
		onoff(v.allow_messages),
		onoff(v.allow_files),
		v.messages,
		if v.messages == 1 { "" } else { "s" },
		size(v.used_bytes),
		size(v.limit_bytes),
	);
	if v.host {
		text.push_str(" (you are the host and set these for the call)");
	}
	if v.budget_full() {
		text.push_str("\nthe transcript budget is full: export it, then clear it");
	}
	text
}

/// `relay messages perms [--messages on|off] [--files on|off]`
pub async fn perms<M: Messaging>(
	client: &mut M,
	call: Option<&str>,
	messages: Option<Switch>,
	files: Option<Switch>,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let mut session = resolve(client, call).await?;
	if messages.is_some() || files.is_some() {
		accepted(
			client
				.set_permissions(
					&session.session_id,
					messages.map(Switch::enabled),
					files.map(Switch::enabled),
				)
				.await?,
		)?;
		session = resolve(client, Some(&hex::encode(&session.session_id))).await?;
	}
	let view = SessionView::new(&session);
	out.emit_with(&view, || session_text(&view))
}

/// `relay messages clear [--yes]`
pub async fn clear<M: Messaging>(
	client: &mut M,
	call: Option<&str>,
	yes: bool,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let session = resolve(client, call).await?;
	let view = SessionView::new(&session);
	if !yes {
		let what = if view.state == "active" {
			"Empty the transcript"
		} else {
			"Remove the retained transcript"
		};
		let question = format!(
			"{what} with {} ({} message{})? Only this device is affected and it cannot be undone.",
			view.peer,
			view.messages,
			if view.messages == 1 { "" } else { "s" }
		);
		if !out.confirm(&question)? {
			return Err(CliError::Usage(
				"not cleared (pass --yes to skip the question)".into(),
			));
		}
	}
	let result = accepted(client.clear(&session.session_id).await?)?;
	out.emit(&Ack::new("cleared", &result))
}

#[derive(Serialize)]
struct Written {
	action: &'static str,
	path: String,
}

/// `relay messages export [--out FILE] [--embed-images]`
pub async fn export<M: Messaging>(
	client: &mut M,
	call: Option<&str>,
	destination: Option<&Path>,
	embed_images: bool,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let target = destination.map(absolutize).transpose()?;
	if let Some(path) = &target
		&& path.symlink_metadata().is_ok()
	{
		return Err(CliError::Usage(format!(
			"{} exists; an export never replaces a file",
			path.display()
		)));
	}
	let session = resolve(client, call).await?;
	let response = client
		.export(
			&session.session_id,
			embed_images,
			target.as_deref().map(path_text).transpose()?,
		)
		.await?;
	check(response.operation.as_ref())?;
	match target {
		Some(_) => {
			let path = response.written_path;
			out.emit_with(
				&Written {
					action: "exported",
					path: path.clone(),
				},
				|| format!("exported to {path} (owner-only)"),
			)
		}
		None => {
			let json = String::from_utf8(response.json)
				.map_err(|_| CliError::Usage("the export is not valid UTF-8".into()))?;
			out.line(json.trim_end())
		}
	}
}
