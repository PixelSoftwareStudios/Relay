//! clap definitions of `relay messages`.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use super::Switch;

/// `relay messages ...`: text, images and file offers inside a call session.
#[derive(Debug, Clone, Args)]
pub struct MessagesArgs {
	/// Session to act on, by call id prefix or peer name. Default: the only live call;
	/// several live calls are an error, never a guess.
	#[arg(long, global = true, value_name = "CALL")]
	pub call: Option<String>,
	/// What to do.
	#[command(subcommand)]
	pub command: MessagesCommand,
}

/// `relay messages <command>`
#[derive(Debug, Clone, Subcommand)]
pub enum MessagesCommand {
	/// Send text; `-` reads it from stdin.
	Send {
		/// The text, or `-` for stdin.
		text: String,
	},
	/// Stream new messages and offers as they happen (Ctrl-C stops).
	Watch,
	/// Print the transcript.
	Log {
		/// First ordinal wanted.
		#[arg(long, value_name = "N", default_value_t = 0)]
		since: u64,
		/// At most this many entries.
		#[arg(long, value_name = "N")]
		limit: Option<usize>,
	},
	/// Case-insensitive search over text, file names and previews.
	Search {
		/// Text to look for.
		query: String,
	},
	/// Pin a message (its number is the `#n` that `log` prints).
	Pin {
		/// Message number.
		message: u64,
	},
	/// Unpin a message.
	Unpin {
		/// Message number.
		message: u64,
	},
	/// Empty the transcript of an active session, or remove an ended one. Local only.
	Clear {
		/// Do not ask for confirmation.
		#[arg(long)]
		yes: bool,
	},
	/// Write the transcript as JSON (to stdout, or to a new file the daemon creates).
	Export {
		/// Destination file; relative paths are made absolute, an existing file is never replaced.
		#[arg(long, value_name = "FILE")]
		out: Option<PathBuf>,
		/// Embed image bytes in the export.
		#[arg(long)]
		embed_images: bool,
	},
	/// Copy a message (`copy 12`) or a code block (`copy code-block 3`) to the clipboard.
	Copy {
		/// A message number, or `code-block`.
		target: String,
		/// The code block number (as `log` prints it) after `code-block`.
		number: Option<usize>,
		/// Also print the copied text.
		#[arg(long)]
		print: bool,
	},
	/// Show or change what the session allows.
	Perms {
		/// Allow or refuse messages.
		#[arg(long, value_enum)]
		messages: Option<Switch>,
		/// Allow or refuse file transfers.
		#[arg(long, value_enum)]
		files: Option<Switch>,
	},
	/// Inline images (at most 15 KiB; use `offer` for anything larger).
	#[command(subcommand)]
	Image(ImageCommand),
	/// Offer a file to the peer; the daemon opens it, nothing moves until they accept.
	Offer {
		/// Path of the file.
		path: PathBuf,
	},
	/// List file offers with status and progress.
	Offers,
	/// Accept an incoming offer.
	Accept {
		/// Offer id prefix.
		offer: String,
		/// Existing directory to save into (default: the current directory).
		#[arg(long, value_name = "DIR")]
		dest: Option<PathBuf>,
		/// Suggested file name (the daemon sanitizes it and never overwrites).
		#[arg(long, value_name = "NAME")]
		name: Option<String>,
	},
	/// Reject an incoming offer.
	Reject {
		/// Offer id prefix.
		offer: String,
	},
	/// Text previews of offered files.
	#[command(subcommand)]
	Preview(PreviewCommand),
	/// Cancel a running transfer or withdraw an open offer.
	Cancel {
		/// Offer id prefix.
		offer: String,
	},
}

/// `relay messages image ...`
#[derive(Debug, Clone, Subcommand)]
pub enum ImageCommand {
	/// Send a PNG, JPEG, WebP or GIF of at most 15 KiB.
	Send {
		/// Image file.
		file: PathBuf,
	},
}

/// `relay messages preview ...`
#[derive(Debug, Clone, Subcommand)]
pub enum PreviewCommand {
	/// Ask the sender for a text preview; their user decides.
	Request {
		/// Offer id prefix.
		offer: String,
	},
	/// Sender: approve the peer's preview request (the daemon reads at most 100 lines).
	Approve {
		/// Offer id prefix.
		offer: String,
		/// Refuse instead.
		#[arg(long)]
		deny: bool,
	},
}
