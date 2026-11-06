//! Starting the peer manager: listener, discovery and the actor task.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;
use relay_net::discovery::{AdvertMode, Advertisement, Discovery, DiscoveryEvent, DiscoveryTable};
use relay_net::handshake::{AcceptBackoff, HandshakeGate, UntrustedLinks};
use relay_net::identity::{Fingerprint, Identity};
use relay_net::session::Role;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};

use super::handle::{
	AdvertSink, AdvertSpec, Command, PeerConfig, PeerError, PeerHandle, PeerOutput,
};
use super::hints::Hint;
use super::link::{LinkEvent, LinkParams, establish};
use super::manager::Actor;
use super::profile::{LocalProfile, Profiles};
use super::sessions::SessionTable;
use super::trust::SharedTrust;

const COMMAND_DEPTH: usize = 256;
const OUTPUT_DEPTH: usize = 256;

/// Spawns the manager on the current Tokio runtime.
pub async fn spawn(
	config: PeerConfig,
	identity: Arc<Identity>,
	trust: SharedTrust,
	incarnation: [u8; 16],
) -> Result<(PeerHandle, mpsc::Receiver<PeerOutput>), PeerError> {
	let listener = TcpListener::bind(config.listen)
		.await
		.map_err(PeerError::Bind)?;
	let local_addr = listener.local_addr().map_err(PeerError::Bind)?;
	let (commands, command_rx) = mpsc::channel(COMMAND_DEPTH);
	let (output, output_rx) = mpsc::channel(OUTPUT_DEPTH);
	let (link_tx, link_rx) = mpsc::channel(COMMAND_DEPTH);
	let snapshot = Arc::new(ArcSwap::from_pointee(Vec::new()));
	let sessions = Arc::new(Mutex::new(SessionTable::default()));
	let local = identity.fingerprint();
	let params = Arc::new(LinkParams {
		identity: Arc::clone(&identity),
		display_name: arc_swap::ArcSwap::from_pointee(config.display_name.clone()),
		incarnation,
		timing: config.timing,
	});
	let ids = Arc::new(AtomicU64::new(1));
	let accept = tokio::spawn(accept_loop(
		listener,
		Arc::clone(&params),
		config.manual_policy(),
		Arc::clone(&ids),
		link_tx.clone(),
	));
	let initial = LocalProfile {
		display_name: config.display_name.clone(),
		avatar: config.avatar,
	};
	let profiles = Profiles::new(initial, config.avatars.clone());
	let (profile_tx, profile_rx) = watch::channel(profiles.local().clone());
	if let Some(setup) = &config.discovery {
		spawn_discovery(
			setup,
			&config,
			local,
			local_addr.port(),
			commands.clone(),
			profile_rx,
		)?;
	}
	let key_changes = super::key_changes::KeyChanges::default();
	let actor = Actor {
		config,
		local,
		params,
		trust: trust.clone(),
		key_changes: key_changes.clone(),
		links: HashMap::new(),
		canonical: HashMap::new(),
		wanted: HashMap::new(),
		dialling: HashMap::new(),
		addresses: HashMap::new(),
		table: DiscoveryTable::new(),
		probed: HashMap::new(),
		untrusted: UntrustedLinks::new(),
		pending: HashMap::new(),
		sessions: Arc::clone(&sessions),
		waiters: HashMap::new(),
		ids,
		output,
		link_tx,
		commands: commands.clone(),
		snapshot: Arc::clone(&snapshot),
		accept: accept.abort_handle(),
		profiles,
		remote_profiles: HashMap::new(),
		profile_tx,
	};
	tokio::spawn(actor.run(command_rx, link_rx));
	let handle = PeerHandle {
		commands,
		local_addr,
		local,
		snapshot,
		sessions,
		trust,
		key_changes,
	};
	Ok((handle, output_rx))
}

fn spawn_discovery(
	setup: &super::handle::DiscoverySetup,
	config: &PeerConfig,
	local: Fingerprint,
	port: u16,
	commands: mpsc::Sender<Command>,
	mut profile: watch::Receiver<LocalProfile>,
) -> Result<(), PeerError> {
	let mut backend = match &setup.sink {
		Some(sink) => Backend::Sink(Arc::clone(&sink.0)),
		None => Backend::Mdns(
			Discovery::start(&setup.interfaces, config.policy)
				.map_err(|e| PeerError::Discovery(e.to_string()))?,
		),
	};
	let advertise = setup.advertise;
	let mut hints = setup.hints.as_ref().and_then(super::HintSource::take);
	let policy = config.policy;
	let mut spec = config.advert.clone();
	// What goes out, if anything. The name is sanitized and truncated by
	// relay-net; it is a hint. Without an attached client nothing goes out.
	let plan = move |profile: &LocalProfile, spec: &AdvertSpec| {
		(advertise && spec.present).then(|| Advertisement {
			instance_id: spec.instance_id,
			display_name: profile.display_name.clone(),
			port,
			fingerprint: local,
			capabilities: String::new(),
			mode: if spec.hidden {
				AdvertMode::Hidden
			} else {
				AdvertMode::Open
			},
			extra_txt: spec.extra_txt.clone(),
		})
	};
	let current = |spec: &Option<watch::Receiver<AdvertSpec>>| {
		spec.as_ref()
			.map(|rx| rx.borrow().clone())
			.unwrap_or_default()
	};
	let mut published = current(&spec);
	backend.show(plan(&profile.borrow().clone(), &published))?;
	tokio::spawn(async move {
		loop {
			tokio::select! {
				event = backend.next_event() => {
					let Some(event) = event else { break };
					let command = match event {
						DiscoveryEvent::Found(peer) => Command::Discovered(peer),
						DiscoveryEvent::Removed(name) => Command::Removed(name),
					};
					if commands.send(command).await.is_err() {
						break;
					}
				}
				hint = next_hint(&mut hints) => {
					let command = match hint {
						Some(Hint::Found(record)) => {
							match relay_net::discovery::parse_platform_record(&record, &policy) {
								Some(peer) => Command::Discovered(Box::new(peer)),
								None => continue,
							}
						}
						Some(Hint::Lost(name)) => Command::Removed(name),
						None => {
							hints = None;
							continue;
						}
					};
					if commands.send(command).await.is_err() {
						break;
					}
				}
				changed = profile.changed() => {
					if changed.is_err() {
						break;
					}
					let now = profile.borrow_and_update().clone();
					let _ = backend.show(plan(&now, &published));
				}
				changed = spec_changed(&mut spec) => {
					if changed.is_err() {
						break;
					}
					published = current(&spec);
					let _ = commands.try_send(Command::Visibility);
					let now = profile.borrow().clone();
					let _ = backend.show(plan(&now, &published));
				}
			}
		}
		backend.shutdown();
	});
	Ok(())
}

/// Where advertisements go and where browse results come from.
enum Backend {
	Mdns(Discovery),
	Sink(Arc<dyn AdvertSink>),
}

impl Backend {
	fn show(&mut self, advert: Option<Advertisement>) -> Result<(), PeerError> {
		match self {
			Self::Mdns(discovery) => {
				let _ = discovery.withdraw();
				match advert {
					Some(advert) => discovery
						.advertise(&advert)
						.map_err(|e| PeerError::Discovery(e.to_string())),
					None => Ok(()),
				}
			}
			Self::Sink(sink) => {
				sink.publish(advert.as_ref());
				Ok(())
			}
		}
	}

	async fn next_event(&mut self) -> Option<DiscoveryEvent> {
		match self {
			Self::Mdns(discovery) => discovery.next_event().await,
			Self::Sink(_) => std::future::pending().await,
		}
	}

	fn shutdown(self) {
		match self {
			Self::Mdns(discovery) => discovery.shutdown(),
			Self::Sink(sink) => sink.publish(None),
		}
	}
}

async fn next_hint(hints: &mut Option<mpsc::UnboundedReceiver<Hint>>) -> Option<Hint> {
	match hints {
		Some(rx) => rx.recv().await,
		None => std::future::pending().await,
	}
}

async fn spec_changed(
	spec: &mut Option<watch::Receiver<AdvertSpec>>,
) -> Result<(), watch::error::RecvError> {
	match spec {
		Some(rx) => rx.changed().await,
		None => std::future::pending().await,
	}
}

async fn accept_loop(
	listener: TcpListener,
	params: Arc<LinkParams>,
	policy: relay_net::AddressPolicy,
	ids: Arc<AtomicU64>,
	events: mpsc::Sender<LinkEvent>,
) {
	let gate = HandshakeGate::new();
	let mut backoff = AcceptBackoff::new();
	loop {
		let (stream, from) = match listener.accept().await {
			Ok(accepted) => {
				backoff.reset();
				accepted
			}
			Err(error) => {
				// Persistent errors (fd exhaustion) must not spin on the backlog.
				let delay = backoff.on_error();
				tracing::warn!(%error, ?delay, "accept failed");
				tokio::time::sleep(delay).await;
				continue;
			}
		};
		if policy.check_socket(from).is_err() {
			continue;
		}
		let Ok(permit) = gate.try_enter(from.ip()) else {
			continue;
		};
		let (params, events) = (Arc::clone(&params), events.clone());
		let id = ids.fetch_add(1, Ordering::Relaxed);
		tokio::spawn(async move {
			let result = establish(
				stream,
				Role::Responder,
				None,
				&params,
				id,
				events.clone(),
				Some(&permit),
			)
			.await;
			drop(permit);
			if let Err(error) = result {
				let _ = events
					.send(LinkEvent::Failed {
						link: id,
						error: error.to_string(),
					})
					.await;
			}
		});
	}
}
