//! Keeps the phone's call audio state ahead of the engine's streams.
//!
//! Android disconnects every open stream when the audio mode or route changes
//! (`MODE_IN_COMMUNICATION`, `setCommunicationDevice`). Entering that state
//! from the foreground service, after the engine already opened the ring's
//! output, made the first stream die underneath the ring and wait for the
//! reconnect policy's reopen. [`SessionBackend`] makes the order fixed: the
//! host is told the streams are wanted *before* the first stream opens (the
//! call is synchronous, so the mode and route are settled when the open
//! starts) and told again when the last one is gone. Kotlin holds the state
//! a little longer, so a reopen on a route change or the step from ring to
//! call never flaps the mode.
//!
//! [`SessionBackend::warm`] is the other half of preparing: the first AAudio
//! open in a process pays for loading the library and the audio service
//! connection, so one output is opened (never started, so it makes no sound
//! and takes no audio focus) and dropped while the engine starts.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use relay_audio::device::{
	AudioBackend, CaptureStream, DeviceHandle, DeviceSelection, Direction, Enumeration,
	PlaybackStream, Resolution, StreamHandle, StreamRequest,
};
use relay_audio::{FRAME_SAMPLES, Result};

/// Told when the engine's audio streams are first wanted and last released.
pub trait AudioSession: Send + Sync {
	fn streams_wanted(&self, wanted: bool);
}

struct Lease {
	open: AtomicUsize,
	session: Arc<dyn AudioSession>,
}

/// One open (or opening) stream's claim on the session.
struct Held(Arc<Lease>);

impl Lease {
	fn acquire(self: &Arc<Self>) -> Held {
		if self.open.fetch_add(1, Ordering::AcqRel) == 0 {
			self.session.streams_wanted(true);
		}
		Held(Arc::clone(self))
	}
}

impl Drop for Held {
	fn drop(&mut self) {
		if self.0.open.fetch_sub(1, Ordering::AcqRel) == 1 {
			self.0.session.streams_wanted(false);
		}
	}
}

/// A stream handle that lets go of the session after the stream is closed.
struct HeldHandle {
	inner: Box<dyn StreamHandle>,
	_held: Held,
}

impl StreamHandle for HeldHandle {
	fn play(&self) -> Result<()> {
		self.inner.play()
	}

	fn pause(&self) -> Result<()> {
		self.inner.pause()
	}
}

/// Wraps a backend with the session rules above.
pub struct SessionBackend<B> {
	inner: B,
	lease: Arc<Lease>,
}

impl<B: AudioBackend> SessionBackend<B> {
	pub fn new(inner: B, session: Arc<dyn AudioSession>) -> Self {
		Self {
			inner,
			lease: Arc::new(Lease {
				open: AtomicUsize::new(0),
				session,
			}),
		}
	}

	/// Opens the default output once without starting it, then drops it.
	/// Returns whether a stream was opened. Does not touch the session.
	pub fn warm(&mut self) -> bool {
		let started = Instant::now();
		let Ok(enumeration) = self.inner.enumerate() else {
			return false;
		};
		let Resolution::Device(device) =
			enumeration.resolve(Direction::Output, &DeviceSelection::FollowDefault)
		else {
			return false;
		};
		let request = StreamRequest {
			buffer_frames: Some(FRAME_SAMPLES as u32 * 4),
			..StreamRequest::default()
		};
		let opened = self.inner.open_playback(device, &request).is_ok();
		tracing::info!(opened, ms = started.elapsed().as_millis() as u64, "output warmed up");
		opened
	}
}

impl<B: AudioBackend> AudioBackend for SessionBackend<B> {
	fn enumerate(&mut self) -> Result<Enumeration> {
		self.inner.enumerate()
	}

	fn open_capture(
		&mut self,
		device: DeviceHandle,
		request: &StreamRequest,
	) -> Result<CaptureStream> {
		let held = self.lease.acquire();
		let mut stream = self.inner.open_capture(device, request)?;
		stream.handle = Box::new(HeldHandle {
			inner: stream.handle,
			_held: held,
		});
		Ok(stream)
	}

	fn open_playback(
		&mut self,
		device: DeviceHandle,
		request: &StreamRequest,
	) -> Result<PlaybackStream> {
		let started = Instant::now();
		let held = self.lease.acquire();
		let ready = started.elapsed();
		let mut stream = self.inner.open_playback(device, request)?;
		tracing::info!(
			session_ms = ready.as_millis() as u64,
			open_ms = (started.elapsed() - ready).as_millis() as u64,
			"output opened"
		);
		stream.handle = Box::new(HeldHandle {
			inner: stream.handle,
			_held: held,
		});
		Ok(stream)
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Mutex;

	use relay_audio::device::{FakeBackend, FakeDeviceSpec};

	use super::*;

	/// Records every call and what the backend had open when it was made.
	#[derive(Default)]
	struct Log(Mutex<Vec<String>>);

	impl AudioSession for Log {
		fn streams_wanted(&self, wanted: bool) {
			self.0.lock().unwrap().push(format!("wanted={wanted}"));
		}
	}

	fn backend() -> (SessionBackend<FakeBackend>, Arc<Log>) {
		let mut output = FakeDeviceSpec::new("speaker", Direction::Output);
		output.is_default = true;
		let mut input = FakeDeviceSpec::new("mic", Direction::Input);
		input.is_default = true;
		let log = Arc::new(Log::default());
		let session: Arc<dyn AudioSession> = log.clone();
		(SessionBackend::new(FakeBackend::new(vec![output, input]), session), log)
	}

	fn device(backend: &mut SessionBackend<FakeBackend>, direction: Direction) -> DeviceHandle {
		let found = backend.enumerate().unwrap();
		match found.resolve(direction, &DeviceSelection::FollowDefault) {
			Resolution::Device(handle) => handle,
			_ => panic!("no default device"),
		}
	}

	fn calls(log: &Log) -> Vec<String> {
		log.0.lock().unwrap().clone()
	}

	#[test]
	fn the_session_is_wanted_before_the_first_stream_opens_and_released_after_the_last() {
		let (mut backend, log) = backend();
		let out = device(&mut backend, Direction::Output);
		let playback = backend.open_playback(out, &StreamRequest::default()).unwrap();
		assert_eq!(calls(&log), ["wanted=true"]);
		let input = device(&mut backend, Direction::Input);
		let capture = backend.open_capture(input, &StreamRequest::default()).unwrap();
		assert_eq!(calls(&log), ["wanted=true"], "a second stream does not repeat it");
		drop(playback);
		assert_eq!(calls(&log), ["wanted=true"], "one stream is still open");
		drop(capture);
		assert_eq!(calls(&log), ["wanted=true", "wanted=false"]);
	}

	#[test]
	fn a_failed_open_releases_what_it_took() {
		let (mut backend, log) = backend();
		let bogus = device(&mut backend, Direction::Output);
		backend.enumerate().unwrap();
		assert!(backend.open_playback(bogus, &StreamRequest::default()).is_err());
		assert_eq!(calls(&log), ["wanted=true", "wanted=false"]);
	}

	#[test]
	fn a_reopen_after_a_route_change_keeps_the_session_until_the_new_stream_is_up() {
		let (mut backend, log) = backend();
		let out = device(&mut backend, Direction::Output);
		let first = backend.open_playback(out, &StreamRequest::default()).unwrap();
		let again = device(&mut backend, Direction::Output);
		let second = backend.open_playback(again, &StreamRequest::default()).unwrap();
		drop(first);
		assert_eq!(calls(&log), ["wanted=true"]);
		drop(second);
		assert_eq!(calls(&log), ["wanted=true", "wanted=false"]);
	}

	#[test]
	fn warming_opens_an_output_without_touching_the_session() {
		let (mut backend, log) = backend();
		assert!(backend.warm());
		assert!(calls(&log).is_empty());
	}
}
