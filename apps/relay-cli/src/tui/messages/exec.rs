//! Carrying out [`MsgCommand`]s through a [`Messaging`] client.

use std::path::Path;

use super::keys::MsgCommand;
use crate::cli::CliError;
use crate::client::Messaging;
use crate::commands::clipboard::Clipboard;
use crate::commands::messages::{absolutize, accepted, check};

/// What running a command changes in the pane.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MsgOutcome {
	/// Footer line.
	pub status: String,
	/// The compose text was sent and can be dropped.
	pub sent: bool,
	/// Search results for a session.
	pub hits: Option<(Vec<u8>, Vec<u64>)>,
}

fn status(text: &str) -> MsgOutcome {
	MsgOutcome {
		status: text.into(),
		..MsgOutcome::default()
	}
}

fn text_of(path: &Path) -> Result<String, CliError> {
	path.to_str()
		.map(str::to_owned)
		.ok_or_else(|| CliError::Usage("the path is not valid UTF-8".into()))
}

/// Run one command. Message text and paths only travel in the request.
pub async fn execute<M: Messaging>(
	client: &mut M,
	clipboard: &mut dyn Clipboard,
	command: MsgCommand,
) -> Result<MsgOutcome, CliError> {
	match command {
		MsgCommand::Send { session, text } => {
			accepted(client.send_text(&session, &text).await?)?;
			Ok(MsgOutcome {
				sent: true,
				..status("sent")
			})
		}
		MsgCommand::Offer { session, path } => {
			let absolute = text_of(&absolutize(Path::new(&path))?)?;
			accepted(client.offer_file(&session, &absolute).await?)?;
			Ok(status("offered; nothing moves until the peer accepts"))
		}
		MsgCommand::Respond {
			session,
			offer,
			accept,
			dest,
		} => {
			let dir = if accept {
				let dir = absolutize(Path::new(&dest))?;
				if !dir.is_dir() {
					return Err(CliError::Usage(format!(
						"{} is not an existing directory",
						dir.display()
					)));
				}
				text_of(&dir)?
			} else {
				String::new()
			};
			accepted(
				client
					.respond_offer(&session, &offer, accept, &dir, "")
					.await?,
			)?;
			Ok(status(if accept { "accepted" } else { "rejected" }))
		}
		MsgCommand::RequestPreview { session, offer } => {
			accepted(client.request_preview(&session, &offer).await?)?;
			Ok(status("preview requested; the sender decides"))
		}
		MsgCommand::ApprovePreview { session, offer } => {
			accepted(client.approve_preview(&session, &offer, true).await?)?;
			Ok(status("preview approved"))
		}
		MsgCommand::Cancel { session, offer } => {
			accepted(client.cancel_offer(&session, &offer).await?)?;
			Ok(status("cancelled"))
		}
		MsgCommand::Pin {
			session,
			outgoing,
			seq,
			pinned,
		} => {
			accepted(client.pin(&session, outgoing, seq, pinned).await?)?;
			Ok(status(if pinned { "pinned" } else { "unpinned" }))
		}
		MsgCommand::Search { session, query } => {
			let found = client.search(&session, &query).await?;
			let count = found.hits.len();
			let hits = found.hits.iter().map(|h| h.ordinal).collect();
			Ok(MsgOutcome {
				status: format!(
					"{count} match{}{} (n / N to jump)",
					if count == 1 { "" } else { "es" },
					if found.truncated {
						", first 100 only"
					} else {
						""
					}
				),
				hits: Some((session, hits)),
				..MsgOutcome::default()
			})
		}
		MsgCommand::Export { session, path } => {
			let target = absolutize(Path::new(&path))?;
			if target.symlink_metadata().is_ok() {
				return Err(CliError::Usage(format!(
					"{} exists; an export never replaces a file",
					target.display()
				)));
			}
			let response = client
				.export(&session, false, Some(&text_of(&target)?))
				.await?;
			check(response.operation.as_ref())?;
			Ok(status(&format!("exported to {}", response.written_path)))
		}
		MsgCommand::Clear { session } => {
			accepted(client.clear(&session).await?)?;
			Ok(status("cleared on this device"))
		}
		MsgCommand::Copy(text) => match clipboard.copy(&text) {
			Ok(()) => Ok(status(&format!("copied {} bytes", text.len()))),
			Err(reason) => Err(CliError::Other(anyhow::anyhow!("could not copy: {reason}"))),
		},
	}
}
