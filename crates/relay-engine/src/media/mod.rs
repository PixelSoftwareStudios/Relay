//! Media: capture, send and receive pipelines, playback and resource admission.

mod gate;
mod inbox;
mod packet;
mod recv;
mod send;

use thiserror::Error;

pub use gate::{FrameStamp, GateBoard, GateCell};
pub use inbox::Inbox;
pub use packet::{Ingress, PAYLOAD_MAX, PacketBuf, RTP_MAX};
pub use recv::{CallReceiver, RecvStats, RemoteStream, Rendered, SourceId};
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
