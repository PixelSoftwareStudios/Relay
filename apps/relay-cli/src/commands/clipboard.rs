//! Clipboard access behind a trait so tests never touch the real clipboard.

/// Something text can be copied to.
pub trait Clipboard {
	/// Copy `text`; the error is a short reason for the user.
	fn copy(&mut self, text: &str) -> Result<(), String>;
}

/// The operating system clipboard (text only).
#[derive(Debug, Default)]
pub struct SystemClipboard;

impl Clipboard for SystemClipboard {
	fn copy(&mut self, text: &str) -> Result<(), String> {
		arboard::Clipboard::new()
			.and_then(|mut board| board.set_text(text.to_owned()))
			.map_err(|e| e.to_string())
	}
}
