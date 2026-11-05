//! Starting the peer manager: listener, discovery and the actor task.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;
use relay_net::discovery::{Advertisement, Discovery, DiscoveryEvent, DiscoveryTable};
use relay_net::handshake::HandshakeGate;
use relay_net::identity::{Fingerprint, Identity};
use relay_net::session::Role;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};

use super::handle::{Command, PeerConfig, PeerError, PeerHandle, PeerOutput};
use super::link::{LinkEvent, LinkParams, establish};
use super::manager::Actor;
use super::profile::{LocalProfile, Profiles};
use super::signaling::SessionTable;
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
		config.policy,
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
	let actor = Actor {
		config,
		local,
		params,
		trust,
		links: HashMap::new(),
		canonical: HashMap::new(),
		wanted: HashMap::new(),
		dialling: HashMap::new(),
		addresses: HashMap::new(),
		table: DiscoveryTable::new(),
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
	let mut discovery = Discovery::start(&setup.interfaces, config.policy)
		.map_err(|e| PeerError::Discovery(e.to_string()))?;
	let advertise = setup.advertise;
	let instance_id = rand::random();
	let announce = move |discovery: &mut Discovery, profile: &LocalProfile| {
		if !advertise {
			return Ok(());
		}
		// The name goes out sanitized and truncated by relay-net; it is a hint.
		let _ = discovery.withdraw();
		discovery
			.advertise(&Advertisement {
				instance_id,
				display_name: profile.display_name.clone(),
				port,
				fingerprint: local,
				capabilities: String::new(),
			})
			.map_err(|e| PeerError::Discovery(e.to_string()))
	};
	announce(&mut discovery, &profile.borrow().clone())?;
	tokio::spawn(async move {
		loop {
			tokio::select! {
				event = discovery.next_event() => {
					let Some(event) = event else { break };
					let command = match event {
						DiscoveryEvent::Found(peer) => Command::Discovered(peer),
						DiscoveryEvent::Removed(name) => Command::Removed(name),
					};
					if commands.send(command).await.is_err() {
						break;
					}
				}
				changed = profile.changed() => {
					if changed.is_err() {
						break;
					}
					let current = profile.borrow_and_update().clone();
					let _ = announce(&mut discovery, &current);
				}
			}
		}
		discovery.shutdown();
	});
	Ok(())
}

async fn accept_loop(
	listener: TcpListener,
	params: Arc<LinkParams>,
	policy: relay_net::AddressPolicy,
	ids: Arc<AtomicU64>,
	events: mpsc::Sender<LinkEvent>,
) {
	let gate = HandshakeGate::new();
	loop {
		let Ok((stream, from)) = listener.accept().await else {
			continue;
		};
		if policy.check_socket(from).is_err() {
			continue;
		}
		let Ok(permit) = gate.try_enter() else {
			continue;
		};
		let (params, events) = (Arc::clone(&params), events.clone());
		let id = ids.fetch_add(1, Ordering::Relaxed);
		tokio::spawn(async move {
			let result =
				establish(stream, Role::Responder, None, &params, id, events.clone()).await;
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
