//! The desktop "choose a ringtone" dialog: a native file dialog, then the file's bytes. The file
//! is read here (after a size check, so a huge file is never loaded) and handed to the engine,
//! which decodes it; only the file name, never the path, goes on.

use std::path::Path;

use relay_ui::ringtone_pick::PICK_MAX_BYTES;
use relay_ui::shell::PickedFile;
use relay_ui::tr;

/// Extensions the decoder understands; the format itself is judged from the bytes.
const EXTENSIONS: [&str; 8] = ["mp3", "ogg", "oga", "flac", "wav", "aac", "m4a", "mp4"];

/// Shows the dialog. `Ok(None)` when the user cancelled.
pub async fn pick_audio() -> Result<Option<PickedFile>, String> {
	let chosen = rfd::AsyncFileDialog::new()
		.set_title(tr!("dialog-ringtone"))
		.add_filter(tr!("dialog-audio-filter"), &EXTENSIONS)
		.pick_file()
		.await;
	match chosen {
		Some(file) => read_audio(file.path()).await.map(Some),
		None => Ok(None),
	}
}

/// Reads `path` unless it is larger than the UI hands to the engine.
pub async fn read_audio(path: &Path) -> Result<PickedFile, String> {
	let read_error = |e: std::io::Error| tr!("err-read-file", error = e.to_string());
	let size = tokio::fs::metadata(path).await.map_err(read_error)?.len();
	if size > PICK_MAX_BYTES as u64 {
		return Err(tr!("ringtone-err-too-large").to_owned());
	}
	let bytes = tokio::fs::read(path).await.map_err(read_error)?;
	let name = path
		.file_name()
		.map_or_else(String::new, |n| n.to_string_lossy().into_owned());
	Ok(PickedFile { name, bytes })
}
