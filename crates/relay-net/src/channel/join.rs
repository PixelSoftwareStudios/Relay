//! Joining a channel: a SPAKE2 exchange with key confirmation, then a certificate grant.
//!
//! ```text
//! joiner                                     admitter (owner or admin)
//!   JoinStart{ref, invite_id, spake_A}  ──▶
//!                                       ◀──  JoinReply{spake_B, confirm_B}
//!   JoinConfirm{confirm_A}              ──▶  (limiter, invite spent, certificate issued)
//!                                       ◀──  JoinGrant{name, policy, owner key, cert}
//! ```
//!
//! Every failure the peer can observe is the one [`ChannelJoinDenied`]: unknown channel,
//! wrong password, unknown or spent invite, lockout and full channel look the same, and an
//! unknown channel or invite still runs a complete exchange against a random credential.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use relay_core::channel::{ChannelId, ChannelName, Policy, Role};
use thiserror::Error;

use super::binding::SessionBinding;
use super::cert::{CertError, MembershipCert};
use super::ids::{channel_id, ct_eq, reference};
use super::limiter::{AttemptLimiter, UNKNOWN_CHANNEL};
use super::limits::{CERT_RENEW_GRACE, JOINS_CONCURRENT_MAX};
use super::pake::{self, Confirmations, Credential, PakeContext, PakeError, PakeRole, PakeState};
use super::store::{ChannelRecord, ChannelStore};
use crate::identity::Fingerprint;
use crate::proto::channel_message::Body;
use crate::proto::{
	ChannelJoinConfirm, ChannelJoinDenied, ChannelJoinGrant, ChannelJoinReply, ChannelJoinStart,
	ChannelMessage,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum JoinError {
	#[error("message does not fit the join state")]
	Unexpected,
	#[error(transparent)]
	Pake(#[from] PakeError),
	#[error("the admitter refused the join")]
	Denied,
	#[error("grant is invalid")]
	BadGrant,
	#[error(transparent)]
	Certificate(#[from] CertError),
}

fn message(body: Body) -> ChannelMessage {
	ChannelMessage { body: Some(body) }
}

fn denied() -> ChannelMessage {
	message(Body::JoinDenied(ChannelJoinDenied {}))
}

/// The joining side.
pub struct JoinClient {
	channel: ChannelId,
	stage: ClientStage,
}

enum ClientStage {
	AwaitReply(Box<PakeState>),
	AwaitGrant,
	Done,
}

impl JoinClient {
	/// Starts a join for `channel` with whatever credential the user supplied. Returns the
	/// first message to send.
	pub fn start(
		binding: &SessionBinding,
		channel: ChannelId,
		credential: &Credential,
		invite_id: Option<[u8; 8]>,
	) -> (Self, ChannelMessage) {
		let context = PakeContext {
			binding: *binding,
			channel,
			invite_id,
		};
		let (state, pake_message) = pake::start(PakeRole::Client, &context, credential);
		let start = ChannelJoinStart {
			channel_ref: reference(&channel, binding.digest(), binding.local_is_initiator())
				.to_vec(),
			invite_id: invite_id.map(|i| i.to_vec()).unwrap_or_default(),
			pake_message,
		};
		(
			Self {
				channel,
				stage: ClientStage::AwaitReply(Box::new(state)),
			},
			message(Body::JoinStart(start)),
		)
	}

	/// Handles the admitter's reply; on success returns the confirmation to send.
	pub fn on_reply(&mut self, reply: &ChannelMessage) -> Result<ChannelMessage, JoinError> {
		match &reply.body {
			Some(Body::JoinDenied(_)) => return Err(JoinError::Denied),
			Some(Body::JoinReply(_)) => {}
			_ => return Err(JoinError::Unexpected),
		}
		let Some(Body::JoinReply(ChannelJoinReply {
			pake_message,
			confirmation,
		})) = &reply.body
		else {
			return Err(JoinError::Unexpected);
		};
		let ClientStage::AwaitReply(state) = std::mem::replace(&mut self.stage, ClientStage::Done)
		else {
			return Err(JoinError::Unexpected);
		};
		let confirmations: Confirmations = state.finish(pake_message)?;
		if !confirmations.verify(confirmation) {
			// Wrong secret or a peer that is not the admitter: nothing further is sent.
			return Err(JoinError::Denied);
		}
		self.stage = ClientStage::AwaitGrant;
		Ok(message(Body::JoinConfirm(ChannelJoinConfirm {
			confirmation: confirmations.to_send().to_vec(),
		})))
	}

	/// Handles the grant and returns the record to persist. The owner key must hash to the id
	/// we asked for and the certificate must name our own authenticated key.
	pub fn on_grant(
		self,
		grant: &ChannelMessage,
		local: &Fingerprint,
		now: u64,
	) -> Result<ChannelRecord, JoinError> {
		if !matches!(self.stage, ClientStage::AwaitGrant) {
			return Err(JoinError::Unexpected);
		}
		accept_grant(&self.channel, grant, local, now)
	}
}

fn accept_grant(
	expected: &ChannelId,
	grant: &ChannelMessage,
	local: &Fingerprint,
	now: u64,
) -> Result<ChannelRecord, JoinError> {
	let g = match &grant.body {
		Some(Body::JoinGrant(g)) => g,
		Some(Body::JoinDenied(_)) => return Err(JoinError::Denied),
		_ => return Err(JoinError::Unexpected),
	};
	let owner: [u8; 32] = g
		.owner_public
		.as_slice()
		.try_into()
		.map_err(|_| JoinError::BadGrant)?;
	if channel_id(&owner) != *expected {
		return Err(JoinError::BadGrant);
	}
	let cert = MembershipCert::from_bytes(&g.cert)?;
	cert.verify(&owner, local, now)?;
	if cert.channel != *expected {
		return Err(JoinError::BadGrant);
	}
	Ok(ChannelRecord {
		id: *expected,
		name: ChannelName::new(&g.name).map_err(|_| JoinError::BadGrant)?,
		policy: Policy {
			listed: g.listed,
			password: g.password,
			invite_only: g.invite_only,
		},
		role: Role::Member,
		owner_public: owner,
		authority: None,
		password: None,
		cert: Some(cert),
		issued: Vec::new(),
		revocations: Vec::new(),
		invites: Vec::new(),
	})
}

/// Renewal: a member whose certificate is near or just past expiry asks an admitter again.
pub fn renewal_request(record: &ChannelRecord, binding: &SessionBinding) -> Option<ChannelMessage> {
	let cert = record.cert.as_ref()?;
	Some(message(Body::Renew(crate::proto::ChannelRenew {
		channel_ref: reference(&record.id, binding.digest(), binding.local_is_initiator()).to_vec(),
		cert: cert.to_bytes(),
	})))
}

/// Applies a renewal grant to the stored record.
pub fn apply_renewal(
	record: &mut ChannelRecord,
	grant: &ChannelMessage,
	local: &Fingerprint,
	now: u64,
) -> Result<(), JoinError> {
	let fresh = accept_grant(&record.id, grant, local, now)?;
	if fresh.owner_public != record.owner_public {
		return Err(JoinError::BadGrant);
	}
	record.cert = fresh.cert;
	Ok(())
}

/// Whether issuing a certificate spends an invite use.
#[derive(Clone, Copy)]
enum Spend {
	Join(Option<[u8; 8]>),
	Nothing,
}

struct Inner {
	store: Arc<dyn ChannelStore>,
	limiter: Mutex<AttemptLimiter>,
	in_flight: AtomicUsize,
}

/// The admitting side: owns the attempt limiter and the bound on concurrent exchanges. One
/// instance serves every link of the device.
#[derive(Clone)]
pub struct AdmissionService {
	inner: Arc<Inner>,
}

/// Result of [`AdmissionService::begin`].
pub enum Begin {
	Reply(Box<AdmitSession>, ChannelMessage),
	Denied(ChannelMessage),
}

const UNKNOWN: ChannelId = UNKNOWN_CHANNEL;

impl AdmissionService {
	pub fn new(store: Arc<dyn ChannelStore>) -> Self {
		Self {
			inner: Arc::new(Inner {
				store,
				limiter: Mutex::new(AttemptLimiter::new()),
				in_flight: AtomicUsize::new(0),
			}),
		}
	}

	fn limiter(&self) -> std::sync::MutexGuard<'_, AttemptLimiter> {
		self.inner.limiter.lock().unwrap_or_else(|p| p.into_inner())
	}

	/// Finds the admitting channel whose reference the sender computed on this session.
	fn find(&self, binding: &SessionBinding, channel_ref: &[u8]) -> Option<ChannelRecord> {
		let records = self.inner.store.list().ok()?;
		let sender_is_initiator = !binding.local_is_initiator();
		records
			.into_iter()
			.filter(ChannelRecord::can_admit)
			.find(|r| {
				ct_eq(
					&reference(&r.id, binding.digest(), sender_is_initiator),
					channel_ref,
				)
			})
	}

	/// Handles a `JoinStart` from `remote` (the link-authenticated fingerprint).
	pub fn begin(
		&self,
		binding: &SessionBinding,
		remote: &Fingerprint,
		start: &ChannelMessage,
		mono: Duration,
		now: u64,
	) -> Begin {
		let Some(Body::JoinStart(ChannelJoinStart {
			channel_ref,
			invite_id,
			pake_message,
		})) = &start.body
		else {
			return Begin::Denied(denied());
		};
		let invite: Option<[u8; 8]> = invite_id.as_slice().try_into().ok();
		let Some(guard) = SlotGuard::acquire(&self.inner) else {
			return Begin::Denied(denied());
		};
		// A locked-out peer is refused before any store read or record clone.
		if self.limiter().check(mono, remote, &UNKNOWN).is_err() {
			return Begin::Denied(denied());
		}
		let found = self.find(binding, channel_ref);
		let key = found.as_ref().map_or(UNKNOWN, |r| r.id);
		if self.limiter().check(mono, remote, &key).is_err() {
			return Begin::Denied(denied());
		}
		// Charge the attempt now: a guesser who sees a bad confirmation simply stops, so
		// counting only failed confirmations would let them guess for free. A completed join
		// refunds it. Open channels have a public credential and are not charged.
		let charged = found.as_ref().is_none_or(|r| r.policy.is_protected());
		if charged {
			self.limiter().record_failure(mono, remote, &key);
		}
		let credential = found
			.as_ref()
			.and_then(|r| credential_for(r, invite, now))
			.unwrap_or_else(Credential::dummy);
		let context = PakeContext {
			binding: *binding,
			channel: found
				.as_ref()
				.map_or_else(|| ChannelId::from_bytes(super::ids::random()), |r| r.id),
			invite_id: invite,
		};
		let (state, own_message) = pake::start(PakeRole::Server, &context, &credential);
		let Ok(confirmations) = state.finish(pake_message) else {
			return Begin::Denied(denied());
		};
		let reply = message(Body::JoinReply(ChannelJoinReply {
			pake_message: own_message,
			confirmation: confirmations.to_send().to_vec(),
		}));
		let session = AdmitSession {
			service: self.clone(),
			_guard: guard,
			remote: *remote,
			key,
			found: found.is_some(),
			charged,
			invite,
			confirmations,
		};
		Begin::Reply(Box::new(session), reply)
	}

	/// Handles a renewal request. Returns a grant with a fresh certificate or `Denied`.
	pub fn renew(
		&self,
		binding: &SessionBinding,
		remote: &Fingerprint,
		request: &ChannelMessage,
		mono: Duration,
		now: u64,
	) -> ChannelMessage {
		let Some(Body::Renew(renew)) = &request.body else {
			return denied();
		};
		let Some(record) = self.find(binding, &renew.channel_ref) else {
			self.limiter().record_failure(mono, remote, &UNKNOWN);
			return denied();
		};
		if self.limiter().check(mono, remote, &record.id).is_err() {
			return denied();
		}
		let valid = MembershipCert::from_bytes(&renew.cert).is_ok_and(|c| {
			c.channel == record.id
				&& c.verify_with_grace(
					&record.owner_public,
					remote,
					now,
					CERT_RENEW_GRACE.as_secs(),
				)
				.is_ok()
		});
		if !valid {
			self.limiter().record_failure(mono, remote, &record.id);
			return denied();
		}
		self.issue(&record.id, remote, Spend::Nothing, now)
			.unwrap_or_else(|refused| refused)
	}

	/// Atomically (in the store) spends the invite, if any, and issues the certificate.
	fn issue(
		&self,
		channel: &ChannelId,
		remote: &Fingerprint,
		spend: Spend,
		now: u64,
	) -> Result<ChannelMessage, ChannelMessage> {
		let mut outcome: Option<ChannelMessage> = None;
		let applied = self.inner.store.update(channel, &mut |record| {
			let mut next = record.clone();
			if let Spend::Join(invite) = spend
				&& next.policy.invite_only
				&& !invite.is_some_and(|i| next.consume_invite(&i, now))
			{
				return;
			}
			let Some(cert) = next.admit(remote, now) else {
				return;
			};
			outcome = Some(message(Body::JoinGrant(ChannelJoinGrant {
				name: next.name.as_str().to_owned(),
				listed: next.policy.listed,
				password: next.policy.password,
				invite_only: next.policy.invite_only,
				owner_public: next.owner_public.to_vec(),
				cert: cert.to_bytes(),
			})));
			*record = next;
		});
		match (applied, outcome) {
			(Ok(()), Some(grant)) => Ok(grant),
			_ => Err(denied()),
		}
	}
}

/// The credential an admitter expects, or `None` when the invite is missing or unusable (the
/// caller then substitutes a random one so the failure is not observable).
fn credential_for(record: &ChannelRecord, invite: Option<[u8; 8]>, now: u64) -> Option<Credential> {
	let password = record.password.as_ref();
	let invite_cred = if record.policy.invite_only {
		let id = invite?;
		Some(
			record
				.invites
				.iter()
				.find(|i| i.id == id && i.is_usable(now))?
				.credential(),
		)
	} else {
		None
	};
	match (record.policy.password, password, invite_cred) {
		(true, Some(p), Some(i)) => Some(p.combine(&i)),
		(true, Some(p), None) => Some(p.clone()),
		(false, _, Some(i)) => Some(i),
		(false, _, None) => Some(Credential::open(&record.id)),
		(true, None, _) => None,
	}
}

/// Decrements the in-flight counter when an exchange ends, however it ends.
struct SlotGuard(Arc<Inner>);

impl SlotGuard {
	fn acquire(inner: &Arc<Inner>) -> Option<Self> {
		let previous = inner.in_flight.fetch_add(1, Ordering::AcqRel);
		if previous >= JOINS_CONCURRENT_MAX {
			inner.in_flight.fetch_sub(1, Ordering::AcqRel);
			return None;
		}
		Some(Self(Arc::clone(inner)))
	}
}

impl Drop for SlotGuard {
	fn drop(&mut self) {
		self.0.in_flight.fetch_sub(1, Ordering::AcqRel);
	}
}

/// An exchange waiting for the joiner's confirmation. Dropping it abandons the join.
pub struct AdmitSession {
	service: AdmissionService,
	_guard: SlotGuard,
	remote: Fingerprint,
	key: ChannelId,
	found: bool,
	charged: bool,
	invite: Option<[u8; 8]>,
	confirmations: Confirmations,
}

impl AdmitSession {
	/// Verifies the joiner's confirmation and, only then, spends the invite and grants. The
	/// attempt was already charged in [`AdmissionService::begin`].
	pub fn on_confirm(self, confirm: &ChannelMessage, now: u64) -> ChannelMessage {
		let verified = matches!(&confirm.body, Some(Body::JoinConfirm(c)) if self.confirmations.verify(&c.confirmation));
		if !verified || !self.found {
			return denied();
		}
		match self
			.service
			.issue(&self.key, &self.remote, Spend::Join(self.invite), now)
		{
			Ok(grant) => {
				if self.charged {
					self.service
						.limiter()
						.record_success(&self.remote, &self.key);
				}
				grant
			}
			Err(refused) => refused,
		}
	}
}
