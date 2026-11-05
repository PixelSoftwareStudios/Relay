//! `InputControl`: push-to-talk presses with short, renewable leases.
//!
//! One stream is one input source. Closing the stream, or sending
//! `RELEASE_ALL`, releases everything the source held.

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;
use std::time::Duration;

use relay_api::pb::{InputAction, InputControlAck, InputControlMessage};
use relay_core::{
	CallId, ClientId, Command, ControllerLeaseId, InputSourceId, MonoTime, PttUpdate, Reply,
	Request as CoreRequest,
};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Response, Status, Streaming};

use super::EngineService;
use crate::service::convert::{call_id, check_context, problem_of};
use crate::service::error::ServiceError;

/// Sequence used for releases the engine issues on the source's behalf.
const FINAL_SEQUENCE: u64 = u64::MAX;

pub(super) fn open(
	service: &EngineService,
	caller: ClientId,
	mut inbound: Streaming<InputControlMessage>,
) -> Result<Response<ReceiverStream<Result<InputControlAck, Status>>>, Status> {
	let shared = &service.shared;
	let slot = shared.limiter.acquire()?;
	let ticket = shared.book.open_stream(caller);
	let source = InputSourceId::new(shared.next_source.fetch_add(1, Ordering::Relaxed));
	let (tx, rx) = mpsc::channel(shared.api.subscriber_queue);
	let service = service.clone();
	tokio::spawn(async move {
		let mut source_state = Source {
			service: &service,
			caller,
			source,
			wire_id: None,
			held: BTreeSet::new(),
		};
		while let Some(Ok(message)) = inbound.next().await {
			let ack = source_state.handle(message).await;
			if tx.send(Ok(ack)).await.is_err() {
				break;
			}
		}
		source_state.release_all().await;
		drop((slot, ticket));
	});
	Ok(Response::new(ReceiverStream::new(rx)))
}

struct Source<'a> {
	service: &'a EngineService,
	caller: ClientId,
	source: InputSourceId,
	wire_id: Option<Vec<u8>>,
	held: BTreeSet<(CallId, ControllerLeaseId)>,
}

fn reject_ack(message: &InputControlMessage, code: &str, text: &str) -> InputControlAck {
	InputControlAck {
		source_id: message.source_id.clone(),
		input_sequence: message.input_sequence,
		accepted: false,
		effective_lease_ms: 0,
		effective_transmitting: false,
		problem: Some(relay_api::pb::Problem {
			code: code.to_owned(),
			message: text.to_owned(),
			retryable: false,
			recovery_action: String::new(),
		}),
	}
}

impl Source<'_> {
	async fn handle(&mut self, message: InputControlMessage) -> InputControlAck {
		match self.try_handle(&message).await {
			Ok(ack) => ack,
			Err(error) => reject_ack(&message, "INVALID_INPUT", &error.to_string()),
		}
	}

	async fn try_handle(
		&mut self,
		message: &InputControlMessage,
	) -> Result<InputControlAck, ServiceError> {
		check_context(
			self.caller,
			message.client.as_ref(),
			&self.service.incarnation(),
		)?;
		let wire_id = self
			.wire_id
			.get_or_insert_with(|| message.source_id.clone());
		if *wire_id != message.source_id {
			return Err(ServiceError::InvalidArgument(
				"source id changed within a stream".to_owned(),
			));
		}
		let action = InputAction::try_from(message.action).unwrap_or(InputAction::Unspecified);
		if action == InputAction::ReleaseAll {
			self.release_all().await;
			return Ok(accepted(message, 0, false));
		}
		let pressed = match action {
			InputAction::Press | InputAction::Renew => true,
			InputAction::Release => false,
			_ => return Err(ServiceError::InvalidArgument("input action".to_owned())),
		};
		let call = call_id(&message.call_id)?;
		let lease = ControllerLeaseId::new(message.controller_generation);
		let ttl = (message.requested_lease_ms > 0)
			.then(|| Duration::from_millis(u64::from(message.requested_lease_ms)));
		let update = PttUpdate {
			call,
			lease,
			source: self.source,
			sequence: message.input_sequence,
			pressed,
			ttl,
			immediate: false,
		};
		let applied = self.send(update).await?;
		match applied.result {
			Ok(reply) => {
				if pressed {
					self.held.insert((call, lease));
				} else {
					self.held.remove(&(call, lease));
				}
				let lease_ms = match reply {
					Reply::Input { expires_at, .. } => remaining_ms(expires_at, &applied),
					_ => 0,
				};
				let open = self.service.shared.handle.gates().load().is_open(call);
				Ok(accepted(message, lease_ms, open))
			}
			Err(reject) => {
				let mut ack = reject_ack(message, "", "");
				ack.problem = Some(problem_of(&reject));
				Ok(ack)
			}
		}
	}

	async fn send(
		&self,
		update: PttUpdate,
	) -> Result<crate::service::handle::Applied, ServiceError> {
		let request = CoreRequest {
			client: self.caller,
			operation: self.service.shared.ids.operation(),
			command: Command::Ptt(update),
		};
		self.service.shared.handle.submit(request).await
	}

	/// Releases every held press, then retires the source.
	async fn release_all(&mut self) {
		for (call, lease) in std::mem::take(&mut self.held) {
			let update = PttUpdate {
				call,
				lease,
				source: self.source,
				sequence: FINAL_SEQUENCE,
				pressed: false,
				ttl: None,
				// Retiring a source closes at once; only a key-up is held.
				immediate: true,
			};
			let _ = self.send(update).await;
		}
	}
}

fn accepted(message: &InputControlMessage, lease_ms: u32, transmitting: bool) -> InputControlAck {
	InputControlAck {
		source_id: message.source_id.clone(),
		input_sequence: message.input_sequence,
		accepted: true,
		effective_lease_ms: lease_ms,
		effective_transmitting: transmitting,
		problem: None,
	}
}

/// Remaining lease time, using the actor clock captured with the answer.
fn remaining_ms(expires_at: MonoTime, applied: &crate::service::handle::Applied) -> u32 {
	let remaining = expires_at.saturating_since(applied.at);
	u32::try_from(remaining.as_millis()).unwrap_or(u32::MAX)
}
