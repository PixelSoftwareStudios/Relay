//! `relay messages`: text, images and file offers inside a call session.
//!
//! These commands run on a connection registered with the `messages` capability
//! and nothing else. Message text is never logged at any level; it only passes
//! through the request and output paths.

mod ack;
mod files;
mod manage;
mod read;
mod send;
mod target;
mod view;
mod watch;

use crate::cli::{CliError, ImageCommand, MessagesArgs, MessagesCommand};
use crate::client::Messaging;
use crate::commands::Out;

pub use send::MAX_INLINE_IMAGE_BYTES;
pub use target::{absolutize, accepted, check, pick_session};
pub use view::{
	CodeBlockView, EntryView, LinkView, OfferView, SessionView, clock, delivery_name, offer_state_name,
	plain, size,
};

/// Run one `relay messages` command.
pub async fn run<M: Messaging>(
	client: &mut M,
	args: MessagesArgs,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let call = args.call.as_deref();
	match &args.command {
		MessagesCommand::Send { text } => send::text(client, call, text, out).await,
		MessagesCommand::Watch => watch::run(client, call, out).await,
		MessagesCommand::Log { since, limit } => read::log(client, call, *since, *limit, out).await,
		MessagesCommand::Search { query } => read::search(client, call, query, out).await,
		MessagesCommand::Pin { message } => read::pin(client, call, *message, true, out).await,
		MessagesCommand::Unpin { message } => read::pin(client, call, *message, false, out).await,
		MessagesCommand::Clear { yes } => manage::clear(client, call, *yes, out).await,
		MessagesCommand::Export {
			out: path,
			embed_images,
		} => manage::export(client, call, path.as_deref(), *embed_images, out).await,
		MessagesCommand::Copy {
			target,
			number,
			print,
		} => read::copy(client, call, target, *number, *print, out).await,
		MessagesCommand::Perms { messages, files } => {
			manage::perms(client, call, *messages, *files, out).await
		}
		MessagesCommand::Image(ImageCommand::Send { file }) => {
			send::image(client, call, file, out).await
		}
		MessagesCommand::Offer { path } => send::offer(client, call, path, out).await,
		MessagesCommand::Offers => files::list(client, call, out).await,
		MessagesCommand::Accept { offer, dest, name } => {
			files::accept(client, call, offer, dest.as_deref(), name.as_deref(), out).await
		}
		MessagesCommand::Reject { offer } => files::reject(client, call, offer, out).await,
		MessagesCommand::Preview(command) => files::preview(client, call, command, out).await,
		MessagesCommand::Cancel { offer } => files::cancel(client, call, offer, out).await,
	}
}
