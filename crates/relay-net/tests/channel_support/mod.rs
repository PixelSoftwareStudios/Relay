#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use relay_net::channel::{
	AdmissionService, Begin, ChannelId, ChannelRecord, ChannelStore, Credential,
	InMemoryChannelStore, JoinClient, JoinError, Policy, SessionBinding, new_channel,
};
use relay_net::identity::{Fingerprint, Identity};
use relay_net::proto::ChannelMessage;
use relay_net::session::LinkEpoch;

pub const NOW: u64 = 1_800_000_000;

pub fn mono(secs: u64) -> Duration {
	Duration::from_secs(secs)
}

/// A joiner (initiator) and an admitter (responder) with their session bindings.
pub struct Pair {
	pub joiner: Identity,
	pub admitter: Identity,
	pub joiner_binding: SessionBinding,
	pub admitter_binding: SessionBinding,
}

impl Pair {
	pub fn new() -> Self {
		let joiner = Identity::generate().unwrap();
		let admitter = Identity::generate().unwrap();
		Self::with(joiner, admitter, LinkEpoch::random())
	}

	pub fn with(joiner: Identity, admitter: Identity, epoch: LinkEpoch) -> Self {
		// Stands in for the Noise handshake hash: commits to epoch and both keys.
		let mut hasher = blake3::Hasher::new();
		hasher.update(epoch.as_bytes());
		hasher.update(joiner.public().as_bytes());
		hasher.update(admitter.public().as_bytes());
		let joiner_binding =
			SessionBinding::from_handshake_hash(hasher.finalize().as_bytes(), true);
		let admitter_binding = joiner_binding.peer_view();
		Self {
			joiner,
			admitter,
			joiner_binding,
			admitter_binding,
		}
	}

	pub fn joiner_fp(&self) -> Fingerprint {
		self.joiner.fingerprint()
	}
}

pub struct World {
	pub store: Arc<InMemoryChannelStore>,
	pub service: AdmissionService,
	pub channel: ChannelId,
}

/// An owner device holding one channel with the given policy. Passwords use Argon2 only when
/// `password` is given.
pub fn world(policy: Policy, password: Option<&str>, owner_fp: &Fingerprint) -> World {
	let record = new_channel("Floor 3", policy, password, owner_fp, NOW).unwrap();
	let channel = record.id;
	let store = Arc::new(InMemoryChannelStore::new());
	store.put(record).unwrap();
	let service = AdmissionService::new(store.clone() as Arc<dyn ChannelStore>);
	World {
		store,
		service,
		channel,
	}
}

pub const OPEN: Policy = Policy {
	listed: false,
	password: false,
	invite_only: false,
};

/// Runs a whole join over the given bindings; returns what the joiner ends up with.
pub fn run_join(
	pair: &Pair,
	world: &World,
	credential: &Credential,
	invite_id: Option<[u8; 8]>,
	at: Duration,
) -> Result<ChannelRecord, JoinError> {
	let (mut client, start) =
		JoinClient::start(&pair.joiner_binding, world.channel, credential, invite_id);
	let reply =
		match world
			.service
			.begin(&pair.admitter_binding, &pair.joiner_fp(), &start, at, NOW)
		{
			Begin::Reply(session, reply) => (session, reply),
			Begin::Denied(_) => return Err(JoinError::Denied),
		};
	let confirm: ChannelMessage = client.on_reply(&reply.1)?;
	let grant = reply.0.on_confirm(&confirm, NOW);
	client.on_grant(&grant, &pair.joiner_fp(), NOW)
}
