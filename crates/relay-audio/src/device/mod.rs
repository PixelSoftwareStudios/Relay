//! Device enumeration, selection, stream opening and health signalling.

mod backend;
mod cpal_backend;
mod descriptor;
mod fake;
mod health;

pub use backend::{
	ActualConfig, AudioBackend, CaptureStream, PlaybackStream, StreamHandle, StreamRequest,
	ring_samples_for,
};
pub use cpal_backend::CpalBackend;
pub use descriptor::{
	ConfigRange, DeviceDescriptor, DeviceHandle, DeviceSelection, Direction, Enumeration,
	Resolution, SampleFormatKind, StableDeviceKey,
};
pub use fake::{FakeBackend, FakeCaptureDriver, FakeDeviceSpec, FakePlaybackDriver, Signal};
pub use health::{Recovery, StreamHealth, StreamState, recovery_for};
