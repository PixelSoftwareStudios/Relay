//! Media: capture, send and receive pipelines, playback and resource admission.

mod capture_worker;
mod devices;
mod events;
mod gate;
mod inbox;
mod load;
mod net;
mod packet;
mod recv;
mod render_worker;
mod send;
mod sink;
mod state;
mod system;

use thiserror::Error;

pub use capture_worker::{CaptureCmd, CaptureInput, spawn as spawn_capture_worker};
pub use devices::{BackendFactory, DeviceCmd, DeviceSetup, DeviceThread, Selection};
pub use events::{EventSink, MediaEvent};
pub use gate::{FrameStamp, GateBoard, GateCell};
pub use inbox::Inbox;
pub use load::{AdmissionLimits, LoadMeter, LoadSnapshot, Refusal};
pub use net::{EndpointSink, NetStats, PeerSocket};
pub use packet::{Ingress, PAYLOAD_MAX, PacketBuf, RTP_MAX};
pub use recv::{CallReceiver, RecvStats, RemoteStream, Rendered, SourceId};
pub use render_worker::{RenderCmd, RenderOutput, spawn as spawn_render_worker};
pub use send::{CallSender, Emit, PacketSink, SendStats, StreamIdentity};

/// Media failures.
#[derive(Debug, Error)]
pub enum MediaError {
	#[error("audio: {0}")]
	Audio(#[from] relay_audio::AudioError),
	#[error("network: {0}")]
	Net(#[from] relay_net::media::MediaError),
	#[error("rtp: {0}")]
	Rtp(#[from] relay_protocol::RtpError),
	#[error("packet sink closed")]
	SinkClosed,
}
pub use sink::RenderSink;
pub use state::{CallReport, Phase};
pub use system::{MediaConfig, MediaSystem, PrepareError};
