//! The front door of the messaging actor: closures run on the task that owns the state.

use tokio::sync::{mpsc, oneshot};

use super::hub::Hub;

/// Work for the actor.
pub(super) type Work = Box<dyn FnOnce(&mut Hub) + Send>;

/// Inbox depth; senders wait when it is full.
pub(super) const INBOX: usize = 1024;

/// Why the actor could not be reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the messaging service has stopped")]
pub struct Stopped;

/// Cloneable handle to the messaging actor.
#[derive(Clone)]
pub struct MessagingHandle {
	pub(super) tx: mpsc::Sender<Work>,
}

impl MessagingHandle {
	/// Runs `work` on the actor without waiting for it.
	pub async fn post(&self, work: impl FnOnce(&mut Hub) + Send + 'static) -> Result<(), Stopped> {
		self.tx.send(Box::new(work)).await.map_err(|_| Stopped)
	}

	/// Non-waiting variant for callers that must not block; false when the inbox is full.
	pub fn try_post(&self, work: impl FnOnce(&mut Hub) + Send + 'static) -> bool {
		self.tx.try_send(Box::new(work)).is_ok()
	}

	/// Runs `work` on the actor and returns its result.
	pub async fn ask<T: Send + 'static>(
		&self,
		work: impl FnOnce(&mut Hub) -> T + Send + 'static,
	) -> Result<T, Stopped> {
		let (reply, answer) = oneshot::channel();
		self.post(move |hub| {
			let _ = reply.send(work(hub));
		})
		.await?;
		answer.await.map_err(|_| Stopped)
	}
}
