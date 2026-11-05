//! Where lease messages go: the real `InputControl` stream or a test fake.

use relay_api::pb::{InputControlAck, InputControlMessage};
use relay_api::{ApiError, RelayClient};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

/// Channel failure.
#[derive(Debug, thiserror::Error)]
pub enum SinkError {
	/// The stream is closed.
	#[error("input stream closed")]
	Closed,
	/// The engine ended the stream with a status.
	#[error("engine status: {0}")]
	Status(String),
	/// The client is not registered, so no context can be attached.
	#[error("client is not registered")]
	NotRegistered,
}

/// Sends one control message and returns the engine's answer for it.
///
/// Implementations must tolerate cancellation (the driver wraps calls in a timeout).
pub trait LeaseSink: Send {
	/// Exchange one message for its acknowledgement.
	fn exchange(
		&mut self,
		msg: InputControlMessage,
	) -> impl Future<Output = Result<InputControlAck, SinkError>> + Send;
}

/// `InputControl` bidirectional stream of a [`RelayClient`].
pub struct GrpcLeaseSink {
	tx: mpsc::Sender<InputControlMessage>,
	acks: tonic::Streaming<InputControlAck>,
}

impl GrpcLeaseSink {
	/// Open the stream. The client must be registered; the interceptor attaches
	/// its context headers. `RelayClient` has no helper for this RPC, so the raw
	/// generated client is used here.
	pub async fn open(client: &mut RelayClient) -> Result<Self, ApiError> {
		if client.context().is_none() {
			return Err(ApiError::NotRegistered);
		}
		let (tx, rx) = mpsc::channel(8);
		let acks = client
			.raw()
			.input_control(ReceiverStream::new(rx))
			.await
			.map_err(|s| ApiError::Status(Box::new(s)))?
			.into_inner();
		Ok(Self { tx, acks })
	}
}

impl LeaseSink for GrpcLeaseSink {
	async fn exchange(&mut self, msg: InputControlMessage) -> Result<InputControlAck, SinkError> {
		let sequence = msg.input_sequence;
		self.tx.send(msg).await.map_err(|_| SinkError::Closed)?;
		loop {
			match self.acks.message().await {
				Ok(Some(ack)) if ack.input_sequence < sequence => continue, // stale after a timeout
				Ok(Some(ack)) => return Ok(ack),
				Ok(None) => return Err(SinkError::Closed),
				Err(status) => return Err(SinkError::Status(status.to_string())),
			}
		}
	}
}
