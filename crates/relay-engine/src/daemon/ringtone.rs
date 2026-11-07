//! The custom ringtone on disk: the processed ring and the name of the file it came from.
//!
//! Local to this device and kept in the data directory beside the database, never inside the
//! preferences (so it is not exported or synced, and no path outside the app's own folder is
//! ever stored). The uploaded bytes are decoded once, off the real-time thread, by
//! `relay_audio::ringtone`; what lands here is the finished 48 kHz mono ring as Ogg Opus (about 8 KB per second), decoded again at start.

use std::path::PathBuf;
use std::sync::Arc;

use relay_audio::ringtone::{self, RingtoneError, RingtoneLimits};

use crate::service::ports::PortError;

const RING_FILE: &str = "ring.opus";
const NAME_FILE: &str = "name.txt";

/// Where the custom ringtone lives.
#[derive(Debug, Clone)]
pub struct RingtoneStore {
	dir: PathBuf,
}

impl RingtoneStore {
	/// A store in `dir` (created on the first save).
	pub fn new(dir: PathBuf) -> Self {
		Self { dir }
	}

	/// Decodes `data` within `limits`, then replaces the stored ring. The old ring stays in
	/// place if anything fails.
	pub fn save(
		&self,
		name: &str,
		data: &[u8],
		limits: RingtoneLimits,
	) -> Result<Arc<[f32]>, PortError> {
		let processed = ringtone::process(data, limits).map_err(rejected)?;
		let encoded = ringtone::to_stored(&processed).ok_or_else(encode_failed)?;
		// What plays now is what plays after a restart: the decoded Opus, not the pre-encode copy.
		let samples = ringtone::from_stored(&encoded).ok_or_else(encode_failed)?;
		std::fs::create_dir_all(&self.dir).map_err(io)?;
		write_replacing(&self.dir.join(RING_FILE), &encoded)?;
		write_replacing(&self.dir.join(NAME_FILE), name.as_bytes())?;
		Ok(samples.into())
	}

	/// The stored name and ring, if a complete pair is on disk.
	pub fn load(&self) -> Option<(String, Arc<[f32]>)> {
		let name = std::fs::read_to_string(self.dir.join(NAME_FILE)).ok()?;
		let samples = ringtone::from_stored(&std::fs::read(self.dir.join(RING_FILE)).ok()?)?;
		Some((name, samples.into()))
	}

	/// The stored name alone (no audio is read).
	pub fn name(&self) -> String {
		std::fs::read_to_string(self.dir.join(NAME_FILE)).unwrap_or_default()
	}

	/// Deletes the stored ring; nothing stored is fine.
	pub fn clear(&self) -> Result<(), PortError> {
		for file in [RING_FILE, NAME_FILE] {
			match std::fs::remove_file(self.dir.join(file)) {
				Ok(()) => {}
				Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
				Err(e) => return Err(io(e)),
			}
		}
		Ok(())
	}
}

/// Writes through a temporary file and renames it over `path`.
fn write_replacing(path: &std::path::Path, bytes: &[u8]) -> Result<(), PortError> {
	let temp = path.with_extension("tmp");
	std::fs::write(&temp, bytes).map_err(io)?;
	std::fs::rename(&temp, path).map_err(io)
}

fn encode_failed() -> PortError {
	PortError::Unavailable("the ringtone could not be encoded".to_owned())
}

fn io(error: std::io::Error) -> PortError {
	PortError::Unavailable(format!("the ringtone could not be stored: {error}"))
}

fn rejected(error: RingtoneError) -> PortError {
	PortError::Rejected {
		code: error.code(),
		message: error.to_string(),
	}
}
