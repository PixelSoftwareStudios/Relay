//! Media: capture, send and receive pipelines, playback and resource admission.

mod capture_worker;
mod control;
mod devices;
mod early;
mod events;
mod gate;
mod inbox;
mod live;
mod load;
mod meters;
mod net;
mod packet;
mod recv;
mod rekey;
mod render_worker;
mod send;
mod sink;
mod state;
mod supervise;
mod system;
mod system_calls;

use thiserror::Error;

pub use capture_worker::{CaptureCmd, CaptureInput, spawn as spawn_capture_worker};
pub use control::device_id;
pub use devices::{BackendFactory, DeviceCmd, DeviceSetup, DeviceThread, Selection};
pub use early::{EARLY_PER_PEER, EARLY_TOTAL, EARLY_TTL, EarlyReady};
pub use events::{EventSink, MediaEvent};
pub use gate::{FrameStamp, GateBoard, GateCell};
pub use inbox::Inbox;
pub use live::MIC_TEST_MAX;
pub use load::{AdmissionLimits, LoadMeter, LoadSnapshot, Refusal};
pub use meters::{Level, Meters, MonitorFrame, MonitorRx, MonitorTx, level_dbfs, monitor_pair};
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
	#[error("socket buffer full; packet dropped")]
	WouldBlock,
}
pub use sink::RenderSink;
pub use state::{CallReport, Phase, RemoteReady};
pub use supervise::{
	FailureKind, HANG_AFTER, Heartbeat, Supervised, Supervision, Watched, WorkerFailure, WorkerId,
	run as supervise, watchdog,
};
pub use system::{MediaConfig, MediaSystem, PrepareError};
