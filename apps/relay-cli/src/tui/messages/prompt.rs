//! Questions the pane asks in place of the compose box.

/// What a prompt is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptKind {
	/// Search text.
	Search,
	/// Path of a file to offer (absolute, or relative to the current directory).
	OfferPath,
	/// Directory to save an incoming offer into.
	AcceptInto {
		/// The offer.
		offer: Vec<u8>,
	},
	/// Where the daemon should write the export (a new file).
	ExportPath,
	/// Remove the transcript? Any key but `y` declines.
	ConfirmClear,
}

/// An open prompt and what has been typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
	pub kind: PromptKind,
	pub input: String,
}

impl Prompt {
	/// A prompt with an initial answer.
	pub fn new(kind: PromptKind, input: impl Into<String>) -> Self {
		Self {
			kind,
			input: input.into(),
		}
	}

	/// The question shown before the typed text.
	pub fn label(&self) -> &'static str {
		match self.kind {
			PromptKind::Search => "search",
			PromptKind::OfferPath => "offer file",
			PromptKind::AcceptInto { .. } => "save into directory",
			PromptKind::ExportPath => "export to new file",
			PromptKind::ConfirmClear => "empty this transcript on this device? y = yes",
		}
	}
}
