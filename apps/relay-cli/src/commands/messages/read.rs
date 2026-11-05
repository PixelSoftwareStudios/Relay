//! Reading a transcript: log, search, pin, copy.

use relay_api::pb::TranscriptEntry;
use relay_api::pb::transcript_entry::Body;
use relay_io::localtime::LocalZone;
use relay_messages::markdown;
use serde::Serialize;

use super::ack::Ack;
use super::target::{accepted, entry, read_all, resolve};
use super::view::{EntryView, SessionView};
use crate::cli::{CliError, TargetError};
use crate::client::Messaging;
use crate::commands::Out;

/// Entries as views, numbering code blocks across the whole slice.
pub fn views(entries: &[TranscriptEntry], zone: &LocalZone) -> Vec<EntryView> {
	let mut blocks = 0;
	entries
		.iter()
		.map(|e| {
			let view = EntryView::new(e, blocks, zone);
			blocks += view.block_count();
			view
		})
		.collect()
}

/// `relay messages log [--since N] [--limit N]`
pub async fn log<M: Messaging>(
	client: &mut M,
	call: Option<&str>,
	since: u64,
	limit: Option<usize>,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let session = resolve(client, call).await?;
	// Numbering code blocks needs the entries before `since` too.
	let entries = read_all(client, &session.session_id, 0).await?;
	let mut shown: Vec<EntryView> = views(&entries, out.zone())
		.into_iter()
		.filter(|v| v.message >= since)
		.collect();
	let more = limit.filter(|l| shown.len() > *l).map(|l| {
		let next = shown[l].message;
		shown.truncate(l);
		next
	});
	let view = SessionView::new(&session);
	out.emit_with(&shown, || {
		let mut text = format!(
			"{} with {} (local times), {} message{}\n",
			view.state,
			view.peer,
			view.messages,
			if view.messages == 1 { "" } else { "s" }
		);
		if shown.is_empty() {
			text.push_str("no messages");
		}
		for v in &shown {
			text.push_str(&format!("{v}\n"));
		}
		if let Some(next) = more {
			text.push_str(&format!("-- more: continue with --since {next}"));
		}
		text.trim_end().to_owned()
	})
}

/// `relay messages search <query>`
pub async fn search<M: Messaging>(
	client: &mut M,
	call: Option<&str>,
	query: &str,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	if query.trim().is_empty() {
		return Err(CliError::Usage("the search text is empty".into()));
	}
	let session = resolve(client, call).await?;
	let found = client.search(&session.session_id, query).await?;
	let entries = read_all(client, &session.session_id, 0).await?;
	let all = views(&entries, out.zone());
	let hits: Vec<&EntryView> = found
		.hits
		.iter()
		.filter_map(|h| all.iter().find(|v| v.message == h.ordinal))
		.collect();
	let truncated = found.truncated;
	out.emit_with(&hits, || {
		let mut text = String::new();
		if hits.is_empty() {
			text.push_str("no matches");
		}
		for v in &hits {
			text.push_str(&format!("{v}\n"));
		}
		if truncated {
			text.push_str("-- only the first 100 matches are shown");
		}
		text.trim_end().to_owned()
	})
}

/// `relay messages pin|unpin <message>`
pub async fn pin<M: Messaging>(
	client: &mut M,
	call: Option<&str>,
	message: u64,
	pinned: bool,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let session = resolve(client, call).await?;
	let target = entry(client, &session.session_id, message).await?;
	let result = accepted(
		client
			.pin(&session.session_id, target.outgoing, target.seq, pinned)
			.await?,
	)?;
	let mut ack = Ack::new(if pinned { "pinned" } else { "unpinned" }, &result);
	ack.message = Some(message);
	out.emit(&ack)
}

#[derive(Serialize)]
struct Copied {
	copied: bool,
	message: u64,
	code_block: Option<usize>,
	bytes: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	text: Option<String>,
}

/// `relay messages copy <message>` or `copy code-block <n>`.
pub async fn copy<M: Messaging>(
	client: &mut M,
	call: Option<&str>,
	target: &str,
	number: Option<usize>,
	print: bool,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let session = resolve(client, call).await?;
	let (message, code_block, text) = if target == "code-block" {
		let n = number.filter(|n| *n > 0).ok_or_else(|| {
			CliError::Usage("`copy code-block` needs the block number `log` printed".into())
		})?;
		let entries = read_all(client, &session.session_id, 0).await?;
		code_block_text(&entries, n)?
	} else {
		let ordinal: u64 = target.trim_start_matches('#').parse().map_err(|_| {
			CliError::Usage(format!(
				"`{target}` is not a message number (as printed by `log`, e.g. 12) or `code-block N`"
			))
		})?;
		let found = entry(client, &session.session_id, ordinal).await?;
		match found.body {
			Some(Body::Text(text)) => (ordinal, None, text),
			_ => {
				return Err(CliError::Usage(format!(
					"message #{ordinal} has no text to copy"
				)));
			}
		}
	};
	let result = out.copy(&text);
	let summary = Copied {
		copied: result.is_ok(),
		message,
		code_block,
		bytes: text.len(),
		text: print.then(|| text.clone()),
	};
	if print && !out.is_json() {
		out.line(&text)?;
	} else {
		out.emit_with(&summary, || {
			format!(
				"copied {} bytes of {}",
				text.len(),
				code_block.map_or_else(
					|| format!("message #{message}"),
					|n| format!("code block {n} (message #{message})")
				)
			)
		})?;
	}
	result.map_err(|reason| {
		CliError::Other(anyhow::anyhow!(
			"the text could not be copied: {reason}{}",
			if print {
				""
			} else {
				" (use --print to see it)"
			}
		))
	})
}

/// The `n`th code block of the session (1-based, in transcript order) and its message.
fn code_block_text(
	entries: &[TranscriptEntry],
	n: usize,
) -> Result<(u64, Option<usize>, String), CliError> {
	let mut seen = 0;
	for e in entries {
		let Some(Body::Text(text)) = &e.body else {
			continue;
		};
		for block in markdown::parse(text).code_blocks() {
			seen += 1;
			if seen == n {
				return Ok((e.ordinal, Some(n), block.copy_text().to_owned()));
			}
		}
	}
	Err(TargetError::NotFound {
		kind: "code block",
		query: n.to_string(),
	}
	.into())
}
