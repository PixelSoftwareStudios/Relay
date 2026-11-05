//! Named channels: blinded discovery tags, password and invite admission, and signed
//! membership proof, all running inside an already Noise-authenticated control link.
//!
//! A channel is a scope for discovery, visibility and call authorization. It is not a
//! conference: nothing here knows about calls or audio. Channel proof never replaces
//! fingerprint trust, and never grants microphone consent or call acceptance.
//! See `docs/channels.md` for the design, threat model and open review gate.

pub mod binding;
pub mod cert;
mod create;
pub mod ids;
pub mod invite;
pub mod join;
pub mod limiter;
pub mod limits;
pub mod pake;
pub mod proof;
pub mod store;
pub mod tag;
pub mod wire;

use thiserror::Error;

pub use binding::SessionBinding;
pub use cert::{Authority, CertError, Delegation, MembershipCert, Revocation};
pub use create::{CreateError, new_channel};
pub use invite::{InviteError, InviteRecord, InviteToken};
pub use join::{AdmissionService, AdmitSession, Begin, JoinClient, JoinError};
pub use limiter::{AttemptLimiter, Locked};
pub use pake::{Credential, PakeError};
pub use proof::{ProofOutcome, check_proof, check_ref, make_proof, make_ref};
pub use relay_core::channel::{ChannelId, ChannelName, Policy, Role};
pub use store::{ChannelRecord, ChannelStore, InMemoryChannelStore, StoreError};
pub use tag::{ChannelAdvert, Tag};

/// Errors that cross the module boundary. Join failures seen by a remote peer are always the
/// single uniform `Denied` message; these are for the local caller only.
#[derive(Debug, Error)]
pub enum ChannelError {
	#[error(transparent)]
	Store(#[from] StoreError),
	#[error(transparent)]
	Join(#[from] JoinError),
	#[error(transparent)]
	Create(#[from] CreateError),
}
