//! File offers: list, accept, reject, preview, cancel.

use std::path::Path;

use relay_api::pb::SessionSummary;
use relay_api::pb::transcript_entry::Body;
use serde::Serialize;

use super::ack::Ack;
use super::target::{absolutize, accepted, path_text, read_all, resolve};
use super::view::{OfferView, size};
use crate::cli::{CliError, PreviewCommand, TargetError};
use crate::client::Messaging;
use crate::commands::table::{self, Layout};
use crate::commands::{Out, names::hex_id};

/// Every offer of a session with its message number.
async fn offers_of<M: Messaging>(
	client: &mut M,
	session: &SessionSummary,
) -> Result<Vec<(u64, OfferView)>, CliError> {
	Ok(read_all(client, &session.session_id, 0)
		.await?
		.into_iter()
		.filter_map(|e| match e.body {
			Some(Body::File(o)) => Some((e.ordinal, OfferView::new(&o))),
			_ => None,
		})
		.collect())
}

/// The id of the one offer matching the typed prefix.
async fn find<M: Messaging>(
	client: &mut M,
	session: &SessionSummary,
	query: &str,
) -> Result<Vec<u8>, CliError> {
	let q = query.trim().to_lowercase();
	let offers = offers_of(client, session).await?;
	let hits: Vec<&OfferView> = offers
		.iter()
		.map(|(_, o)| o)
		.filter(|o| !q.is_empty() && o.offer.starts_with(&q))
		.collect();
	match hits.as_slice() {
		[one] => hex::decode(&one.offer)
			.map_err(|_| CliError::Usage("the engine sent a malformed offer id".into())),
		[] => Err(TargetError::NotFound {
			kind: "offer",
			query: query.to_owned(),
		}
		.into()),
		many => Err(TargetError::Ambiguous {
			query: query.to_owned(),
			candidates: many
				.iter()
				.map(|o| format!("{} ({})", o.name, o.short()))
				.collect::<Vec<_>>()
				.join(", "),
		}
		.into()),
	}
}

#[derive(Serialize)]
struct OfferRow {
	message: u64,
	#[serde(flatten)]
	offer: OfferView,
}

/// `relay messages offers`
pub async fn list<M: Messaging>(
	client: &mut M,
	call: Option<&str>,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let session = resolve(client, call).await?;
	let rows: Vec<OfferRow> = offers_of(client, &session)
		.await?
		.into_iter()
		.map(|(message, offer)| OfferRow { message, offer })
		.collect();
	out.emit_with(&rows, || offers_table(&rows))
}

fn offers_table(rows: &[OfferRow]) -> String {
	if rows.is_empty() {
		return "no file offers".into();
	}
	let mut t = table::with_header(
		Layout::detect(),
		&[
			"ID", "MSG", "DIR", "NAME", "SIZE", "STATE", "PROGRESS", "NOTE",
		],
	);
	for r in rows {
		let o = &r.offer;
		let progress = match o.state {
			"in-progress" => format!("{}% of {}", o.percent, size(o.size)),
			"completed" => "100%".into(),
			_ => "-".into(),
		};
		let mut notes = Vec::new();
		if o.executable_warning {
			notes.push("looks executable".to_owned());
		}
		if o.preview_approval_pending {
			notes.push("preview requested by peer".into());
		}
		if o.preview_requested {
			notes.push("preview requested".into());
		}
		if let Some(saved) = &o.saved_as {
			notes.push(format!("saved as {saved}"));
		}
		if let Some(reason) = &o.failure {
			notes.push(reason.clone());
		}
		t.add_row([
			o.short().to_owned(),
			format!("#{}", r.message),
			(if o.outgoing { "out" } else { "in" }).into(),
			o.name.clone(),
			size(o.size),
			o.state.into(),
			progress,
			notes.join("; "),
		]);
	}
	table::render(t)
}

/// `relay messages accept <offer>`: saved into an existing directory the user names
/// (default: the current one); the daemon sanitizes the name and never overwrites.
pub async fn accept<M: Messaging>(
	client: &mut M,
	call: Option<&str>,
	offer: &str,
	dest: Option<&Path>,
	name: Option<&str>,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let directory = absolutize(dest.unwrap_or_else(|| Path::new(".")))?;
	if !directory.is_dir() {
		return Err(CliError::Usage(format!(
			"{} is not an existing directory",
			directory.display()
		)));
	}
	let session = resolve(client, call).await?;
	let id = find(client, &session, offer).await?;
	let result = accepted(
		client
			.respond_offer(
				&session.session_id,
				&id,
				true,
				path_text(&directory)?,
				name.unwrap_or_default(),
			)
			.await?,
	)?;
	out.emit(&Ack::new("accepted", &result).with_detail(format!("into {}", directory.display())))
}

/// `relay messages reject <offer>`
pub async fn reject<M: Messaging>(
	client: &mut M,
	call: Option<&str>,
	offer: &str,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let session = resolve(client, call).await?;
	let id = find(client, &session, offer).await?;
	let result = accepted(
		client
			.respond_offer(&session.session_id, &id, false, "", "")
			.await?,
	)?;
	out.emit(&Ack::new("rejected", &result).with_detail(short(&id)))
}

/// `relay messages cancel <offer>`
pub async fn cancel<M: Messaging>(
	client: &mut M,
	call: Option<&str>,
	offer: &str,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let session = resolve(client, call).await?;
	let id = find(client, &session, offer).await?;
	let result = accepted(client.cancel_offer(&session.session_id, &id).await?)?;
	out.emit(&Ack::new("cancelled", &result).with_detail(short(&id)))
}

/// `relay messages preview request|approve <offer>`
pub async fn preview<M: Messaging>(
	client: &mut M,
	call: Option<&str>,
	command: &PreviewCommand,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let session = resolve(client, call).await?;
	let ack = match command {
		PreviewCommand::Request { offer } => {
			let id = find(client, &session, offer).await?;
			let result = accepted(client.request_preview(&session.session_id, &id).await?)?;
			Ack::new("preview requested", &result).with_detail(short(&id))
		}
		PreviewCommand::Approve { offer, deny } => {
			let id = find(client, &session, offer).await?;
			let result = accepted(
				client
					.approve_preview(&session.session_id, &id, !deny)
					.await?,
			)?;
			Ack::new(
				if *deny {
					"preview refused"
				} else {
					"preview approved"
				},
				&result,
			)
			.with_detail(short(&id))
		}
	};
	out.emit(&ack)
}

fn short(id: &[u8]) -> String {
	hex_id(id).chars().take(8).collect()
}
