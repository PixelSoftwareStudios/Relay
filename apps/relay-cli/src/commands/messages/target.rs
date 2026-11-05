//! Choosing the session a command acts on, and shared request helpers.

use std::path::{Component, Path, PathBuf};

use relay_api::pb::{
	Operation, SessionOpResult, SessionState, SessionSummary, Transcript, TranscriptEntry,
};

use crate::cli::{CliError, TargetError};
use crate::client::Messaging;
use crate::commands::names::{hex_id, short_id};

/// Most pages a full transcript read follows (a guard against a misbehaving engine).
const MAX_PAGES: usize = 256;

fn label(s: &SessionSummary) -> String {
	format!("{} ({})", s.peer_name, short_id(&s.call_id))
}

/// Pick the session: `query` is a call id prefix, session id prefix or peer name
/// (exact before prefix, any case). Without it the only active session is used; none or
/// several is an error, because guessing which conversation receives text is how
/// messages reach the wrong person.
pub fn pick_session<'a>(
	sessions: &'a [SessionSummary],
	query: Option<&str>,
) -> Result<&'a SessionSummary, TargetError> {
	let Some(query) = query else {
		let active: Vec<&SessionSummary> = sessions
			.iter()
			.filter(|s| s.state == SessionState::Active as i32)
			.collect();
		return match active.as_slice() {
			[one] => Ok(one),
			[] => Err(TargetError::NoDefault(if sessions.is_empty() {
				"there is no call session; connect a call first".into()
			} else {
				"there is no active call session; name a retained transcript with --call".into()
			})),
			_ => Err(TargetError::NoDefault(format!(
				"several calls are active ({}); name one with --call <id|name>",
				active
					.iter()
					.map(|s| label(s))
					.collect::<Vec<_>>()
					.join(", ")
			))),
		};
	};
	let q = query.to_lowercase();
	let exact = |s: &SessionSummary| {
		hex_id(&s.call_id) == q || hex_id(&s.session_id) == q || s.peer_name.to_lowercase() == q
	};
	let prefix = |s: &SessionSummary| {
		hex_id(&s.call_id).starts_with(&q)
			|| hex_id(&s.session_id).starts_with(&q)
			|| s.peer_name.to_lowercase().starts_with(&q)
	};
	for matcher in [&exact as &dyn Fn(&SessionSummary) -> bool, &prefix] {
		let hits: Vec<&SessionSummary> = sessions.iter().filter(|s| matcher(s)).collect();
		match hits.as_slice() {
			[] => {}
			[one] => return Ok(one),
			many => {
				return Err(TargetError::Ambiguous {
					query: query.to_owned(),
					candidates: many.iter().map(|s| label(s)).collect::<Vec<_>>().join(", "),
				});
			}
		}
	}
	Err(TargetError::NotFound {
		kind: "session",
		query: query.to_owned(),
	})
}

/// List the sessions and choose one.
pub async fn resolve<M: Messaging>(
	client: &mut M,
	query: Option<&str>,
) -> Result<SessionSummary, CliError> {
	let list = client.list_sessions().await?;
	Ok(pick_session(&list.sessions, query)?.clone())
}

/// A hint that tells the user what to do about a stable problem code.
fn hint(code: &str) -> Option<&'static str> {
	Some(match code {
		"HISTORY_FULL" => {
			"the transcript budget is full: `relay messages export --out FILE`, then `relay messages clear`"
		}
		"SESSION_PERMISSION_OFF" => "see `relay messages perms`",
		"MESSAGES_DISABLED" => "messaging is switched off in the daemon's settings",
		"SESSION_ENDED" => "the call is over; its transcript stays readable until cleared",
		"IMAGE_TOO_LARGE" => "inline images are at most 15 KiB; use `relay messages offer`",
		_ => return None,
	})
}

/// The final result of a messaging mutation, or the engine's rejection (with a hint).
pub fn accepted(result: SessionOpResult) -> Result<SessionOpResult, CliError> {
	check(result.operation.as_ref())?;
	Ok(result)
}

/// Fail with the engine's rejection when `operation` is not an applied one.
pub fn check(operation: Option<&Operation>) -> Result<(), CliError> {
	match operation {
		Some(op) if op.state == relay_api::pb::OperationState::Failed as i32 => {
			let mut error = op.problem.as_ref().map_or_else(
				|| CliError::Rejected {
					code: "operation_failed".into(),
					message: "the engine reported the operation as failed".into(),
					retryable: false,
				},
				CliError::from_problem,
			);
			if let CliError::Rejected { code, message, .. } = &mut error
				&& let Some(hint) = hint(code)
			{
				message.push_str(&format!(" ({hint})"));
			}
			Err(error)
		}
		Some(_) => Ok(()),
		None => Err(CliError::Rejected {
			code: "operation_missing".into(),
			message: "the engine answered without an operation".into(),
			retryable: false,
		}),
	}
}

/// Every transcript entry of a session from `from_ordinal`, following pages.
pub async fn read_all<M: Messaging>(
	client: &mut M,
	session: &[u8],
	mut from_ordinal: u64,
) -> Result<Vec<TranscriptEntry>, CliError> {
	let mut all = Vec::new();
	for _ in 0..MAX_PAGES {
		let Transcript {
			entries,
			more,
			next_ordinal,
			..
		} = client.get_transcript(session, from_ordinal, 0).await?;
		all.extend(entries);
		if !more || next_ordinal <= from_ordinal {
			return Ok(all);
		}
		from_ordinal = next_ordinal;
	}
	Ok(all)
}

/// Find one entry by its message number (ordinal).
pub async fn entry<M: Messaging>(
	client: &mut M,
	session: &[u8],
	ordinal: u64,
) -> Result<TranscriptEntry, CliError> {
	let page = client.get_transcript(session, ordinal, 1).await?;
	page.entries
		.into_iter()
		.find(|e| e.ordinal == ordinal)
		.ok_or_else(|| {
			TargetError::NotFound {
				kind: "message",
				query: format!("#{ordinal}"),
			}
			.into()
		})
}

/// Make a path absolute against the current directory and fold `.` and `..`
/// lexically. The daemon insists on absolute paths without `..`; it still
/// validates what it opens.
pub fn absolutize(path: &Path) -> Result<PathBuf, CliError> {
	let joined = if path.is_absolute() {
		path.to_path_buf()
	} else {
		std::env::current_dir()?.join(path)
	};
	let mut out = PathBuf::new();
	for part in joined.components() {
		match part {
			Component::ParentDir => {
				out.pop();
			}
			Component::CurDir => {}
			other => out.push(other.as_os_str()),
		}
	}
	Ok(out)
}

/// A path as the UTF-8 string the API carries.
pub fn path_text(path: &Path) -> Result<&str, CliError> {
	path.to_str()
		.ok_or_else(|| CliError::Usage("the path is not valid UTF-8".into()))
}
