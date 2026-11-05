//! The two transfer tasks. Each owns one state machine, its file or sink and its data link,
//! and reports to the actor through [`MessagingHandle`]. The actor never blocks on a
//! transfer; the tasks never touch the transcript.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;

use relay_messages::transfer::{
	CancelBy, Effect, FailureReason, FileSink, Receiver, Sender, TransferMsg, TransferState,
};
use relay_messages::{FileOffer, OfferId, SessionId};
use relay_net::identity::Fingerprint;
use relay_net::transfer::{
	DataLink, PendingLink, Ticket, TransferHub, TransferRole, TransferToken,
};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};

use super::handle::MessagingHandle;
use super::session::{SourceInfo, XferCmd};
use super::source::{Source, SourceError};
use super::wire::offer_bytes;

/// Bytes between progress reports.
const PROGRESS_STEP: u64 = 256 * 1024;

/// How a transfer ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
	/// Verified (receiver: and visible under `saved_as`).
	Completed {
		/// Final file name (receiver only).
		saved_as: Option<String>,
	},
	/// Failed.
	Failed(FailureReason),
	/// Cancelled.
	Cancelled(CancelBy),
}

/// What a task needs to talk to the actor.
#[derive(Clone)]
pub struct TaskCtx {
	/// The actor.
	pub handle: MessagingHandle,
	/// The session.
	pub session: SessionId,
	/// The offer.
	pub offer: OfferId,
}

impl TaskCtx {
	async fn progress(&self, bytes: u64) {
		let (session, offer) = (self.session, self.offer);
		let _ = self
			.handle
			.post(move |hub| hub.xfer_progress(session, offer, bytes))
			.await;
	}

	async fn finish(&self, outcome: Outcome) {
		let (session, offer) = (self.session, self.offer);
		let _ = self
			.handle
			.post(move |hub| hub.xfer_finished(session, offer, outcome))
			.await;
	}
}

/// Everything the sending task needs.
pub struct SendJob {
	/// Actor link.
	pub ctx: TaskCtx,
	/// The data-link hub.
	pub hub: Arc<TransferHub>,
	/// Receiver's key.
	pub peer: Fingerprint,
	/// Where the receiver listens.
	pub addr: SocketAddr,
	/// The common session identifier (the call id).
	pub session_id: [u8; 16],
	/// The grant token.
	pub token: [u8; 32],
	/// The offered file's metadata.
	pub offer: FileOffer,
	/// The file as hashed; opened by the task on a blocking worker.
	pub source: SourceInfo,
	/// Commands from the actor.
	pub cmds: mpsc::Receiver<XferCmd>,
	/// Live pacing cap.
	pub rate: watch::Receiver<Option<u64>>,
}

fn cancel_by(cmd: XferCmd) -> CancelBy {
	match cmd {
		XferCmd::PeerCancel => CancelBy::Peer,
		_ => CancelBy::Local,
	}
}

/// How long a task whose data link died waits for the control link to say why.
const LINK_LOST_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// The data link ended before the transfer did. A peer that cancels closes the link and
/// says so on the control link; whichever arrives first must not decide the meaning, so
/// wait briefly for the cancel before calling it a protocol failure.
async fn link_lost(cmds: &mut mpsc::Receiver<XferCmd>) -> Outcome {
	let waited = tokio::time::timeout(LINK_LOST_GRACE, async {
		while let Some(cmd) = cmds.recv().await {
			if matches!(cmd, XferCmd::PeerCancel) {
				return true;
			}
		}
		false
	})
	.await;
	if waited == Ok(true) {
		Outcome::Cancelled(CancelBy::Peer)
	} else {
		Outcome::Failed(FailureReason::Protocol)
	}
}

fn outcome(state: TransferState, saved_as: Option<String>) -> Outcome {
	match state {
		TransferState::Completed => Outcome::Completed { saved_as },
		TransferState::Failed(why) => Outcome::Failed(why),
		TransferState::Cancelled(by) => Outcome::Cancelled(by),
		TransferState::Rejected => Outcome::Cancelled(CancelBy::Peer),
		_ => Outcome::Failed(FailureReason::Protocol),
	}
}

/// Runs the sending side to its end and reports.
pub async fn run_send(mut job: SendJob) {
	let result = drive_send(&mut job).await;
	job.ctx.finish(result).await;
}

async fn drive_send(job: &mut SendJob) -> Outcome {
	let Some(token) = TransferToken::from_slice(&job.token) else {
		return Outcome::Failed(FailureReason::Protocol);
	};
	let info = job.source.clone();
	let opened = tokio::task::spawn_blocking(move || Source::open(&info.path, &info.facts)).await;
	let source = match opened {
		Ok(Ok(source)) => source,
		Ok(Err(SourceError::Changed)) => return Outcome::Failed(FailureReason::SourceChanged),
		_ => return Outcome::Failed(FailureReason::ReadError),
	};
	let ticket = Ticket {
		session_id: job.session_id,
		offer_id: offer_bytes(job.offer.id),
		token,
	};
	let dial = job.hub.dial(
		job.addr,
		&job.peer,
		&ticket,
		TransferRole::Sender,
		job.offer.size,
	);
	let mut link: DataLink<TcpStream> = tokio::select! {
		linked = dial => match linked {
			Ok(link) => link,
			Err(error) => {
				tracing::debug!(%error, "data link could not be opened");
				return Outcome::Failed(FailureReason::Protocol);
			}
		},
		cmd = job.cmds.recv() => return Outcome::Cancelled(cmd.map_or(CancelBy::Local, cancel_by)),
	};
	link.tx.pacer().set_rate(*job.rate.borrow());
	let close = link.close_handle();
	let mut sender = Sender::new(&job.offer);
	let mut queue: VecDeque<Effect> = sender.on_message(TransferMsg::Accept).into();
	let mut reported = 0u64;
	loop {
		while let Some(effect) = queue.pop_front() {
			match effect {
				Effect::Send(msg) => {
					let sent = tokio::select! {
						sent = link.tx.send(&msg) => sent.is_ok(),
						cmd = job.cmds.recv() => {
							if matches!(cmd, Some(XferCmd::PeerCancel)) {
								close.close();
								return Outcome::Cancelled(CancelBy::Peer);
							}
							queue.extend(sender.cancel());
							false
						}
					};
					if !sent && !sender.state().is_terminal() {
						close.close();
						return link_lost(&mut job.cmds).await;
					}
				}
				Effect::ReadChunk { offset, len } => {
					let source = source.clone();
					let read = tokio::task::spawn_blocking(move || source.read(offset, len)).await;
					match read {
						Ok(Ok(data)) => queue.extend(sender.chunk_read(data)),
						_ => queue.extend(sender.read_failed()),
					}
				}
				Effect::CheckSource => {
					let source = source.clone();
					let same = tokio::task::spawn_blocking(move || source.unchanged())
						.await
						.unwrap_or(false);
					queue.extend(sender.source_checked(same));
				}
			}
		}
		if sender.state().is_terminal() {
			break;
		}
		if sender.sent() >= reported + PROGRESS_STEP {
			reported = sender.sent();
			job.ctx.progress(reported).await;
		}
		tokio::select! {
			received = link.rx.recv() => match received {
				Ok(msg) => queue.extend(sender.on_message(msg)),
				Err(_) => {
					close.close();
					return link_lost(&mut job.cmds).await;
				}
			},
			cmd = job.cmds.recv() => match cmd {
				Some(XferCmd::PeerCancel) => {
					queue.extend(sender.on_message(TransferMsg::Cancel));
					close.close();
					return Outcome::Cancelled(CancelBy::Peer);
				}
				_ => queue.extend(sender.cancel()),
			},
			Ok(()) = job.rate.changed() => link.tx.pacer().set_rate(*job.rate.borrow()),
		}
	}
	let _ = tokio::time::timeout(LINK_LOST_GRACE, link.tx.shutdown()).await;
	close.close();
	outcome(sender.state(), None)
}

/// Everything the receiving task needs.
pub struct RecvJob {
	/// Actor link.
	pub ctx: TaskCtx,
	/// The offer.
	pub offer: FileOffer,
	/// Waits for the sender's data link (taken when the task starts).
	pub pending: Option<PendingLink>,
	/// The state machine, already accepted.
	pub receiver: Box<Receiver>,
	/// Effects of `accept` that belong on the data link (the credit).
	pub initial: Vec<Effect>,
	/// Where the file goes.
	pub sink: Box<dyn FileSink + Send>,
	/// Commands from the actor.
	pub cmds: mpsc::Receiver<XferCmd>,
}

/// Runs the receiving side to its end and reports.
pub async fn run_recv(mut job: RecvJob) {
	let result = drive_recv(&mut job).await;
	job.ctx.finish(result).await;
}

async fn drive_recv(job: &mut RecvJob) -> Outcome {
	let Some(pending) = job.pending.take() else {
		return Outcome::Failed(FailureReason::Protocol);
	};
	let waited = tokio::select! {
		linked = pending.wait() => linked,
		cmd = job.cmds.recv() => {
			job.receiver.cancel(&mut *job.sink);
			return Outcome::Cancelled(cmd.map_or(CancelBy::Local, cancel_by));
		}
	};
	let Ok(mut link) = waited else {
		job.receiver.cancel(&mut *job.sink);
		return Outcome::Failed(FailureReason::Protocol);
	};
	let close = link.close_handle();
	let mut queue: VecDeque<Effect> = std::mem::take(&mut job.initial).into();
	let mut reported = 0u64;
	loop {
		while let Some(effect) = queue.pop_front() {
			// Accept and Reject travelled on the control link; the rest goes to the sender.
			if let Effect::Send(msg) = effect
				&& !matches!(msg, TransferMsg::Accept | TransferMsg::Reject)
				&& link.tx.send(&msg).await.is_err()
				&& !job.receiver.state().is_terminal()
			{
				job.receiver.cancel(&mut *job.sink);
				close.close();
				return link_lost(&mut job.cmds).await;
			}
		}
		if job.receiver.state().is_terminal() {
			break;
		}
		let received = job.receiver.received();
		if received >= reported + PROGRESS_STEP {
			reported = received;
			job.ctx.progress(received).await;
		}
		tokio::select! {
			message = link.rx.recv() => match message {
				Ok(msg) => queue.extend(step(job, msg).await),
				Err(_) => {
					job.receiver.cancel(&mut *job.sink);
					close.close();
					return link_lost(&mut job.cmds).await;
				}
			},
			cmd = job.cmds.recv() => match cmd {
				Some(XferCmd::PeerCancel) => {
					queue.extend(step(job, TransferMsg::Cancel).await);
				}
				_ => queue.extend(job.receiver.cancel(&mut *job.sink)),
			},
		}
	}
	let _ = tokio::time::timeout(LINK_LOST_GRACE, link.tx.shutdown()).await;
	close.close();
	let saved = job.receiver.committed_as().map(str::to_owned);
	outcome(job.receiver.state(), saved)
}

/// Feeds one message to the state machine on a blocking worker (it writes and syncs).
async fn step(job: &mut RecvJob, msg: TransferMsg) -> Vec<Effect> {
	let mut receiver = std::mem::replace(&mut job.receiver, Box::new(Receiver::new(&job.offer)));
	let mut sink = std::mem::replace(&mut job.sink, Box::new(NullSink));
	let moved = tokio::task::spawn_blocking(move || {
		let effects = receiver.on_message(msg, &mut *sink);
		(receiver, sink, effects)
	})
	.await;
	match moved {
		Ok((receiver, sink, effects)) => {
			job.receiver = receiver;
			job.sink = sink;
			effects
		}
		Err(_) => Vec::new(),
	}
}

/// A sink that refuses everything; only ever present while the real one is on a worker.
struct NullSink;

impl FileSink for NullSink {
	fn free_space(&mut self) -> Result<u64, relay_messages::transfer::SinkError> {
		Err(relay_messages::transfer::SinkError("busy".to_owned()))
	}
	fn create_temp(
		&mut self,
		_: &relay_messages::SafeFileName,
		_: u64,
	) -> Result<(), relay_messages::transfer::SinkError> {
		Err(relay_messages::transfer::SinkError("busy".to_owned()))
	}
	fn write(&mut self, _: &[u8]) -> Result<(), relay_messages::transfer::SinkError> {
		Err(relay_messages::transfer::SinkError("busy".to_owned()))
	}
	fn sync(&mut self) -> Result<(), relay_messages::transfer::SinkError> {
		Err(relay_messages::transfer::SinkError("busy".to_owned()))
	}
	fn commit(&mut self) -> Result<String, relay_messages::transfer::SinkError> {
		Err(relay_messages::transfer::SinkError("busy".to_owned()))
	}
	fn abort(&mut self) {}
}
