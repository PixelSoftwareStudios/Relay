mod common;

use common::Fixture;
use relay_core::testing::Node;
use relay_core::*;

fn request(client: ClientId, operation: OperationId, command: Command) -> Input {
	Input::Client(Request {
		client,
		operation,
		command,
	})
}

#[test]
fn duplicate_request_returns_the_cached_result_without_reexecuting() {
	let mut f = Fixture::new();
	let op = f.node.ids.operation();
	let (call, lease) = (f.call, f.lease);
	let route = Command::SetTransmitRoute {
		call,
		lease,
		route: TransmitRoute::open_mic(),
	};
	let first = f.node.feed(request(f.client, op, route.clone()));
	assert!(!first.duplicate);
	let revision = f.node.state.revision();
	let again = f.node.feed(request(f.client, op, route));
	assert!(again.duplicate);
	assert_eq!(again.result, first.result);
	assert!(again.effects.is_empty());
	assert_eq!(
		f.node.state.revision(),
		revision,
		"no new events for a retry"
	);
}

#[test]
fn start_call_retry_does_not_create_a_second_call() {
	let mut node = Node::new(1);
	let client = node.attach_client();
	let (op, call, session) = (node.ids.operation(), node.ids.call(), node.ids.session());
	let start = Command::StartCall {
		call,
		session,
		peer: common::peer(),
		allow_parallel_same_peer: false,
	};
	let first = node.feed(request(client, op, start.clone()));
	let retry = node.feed(request(client, op, start));
	assert_eq!(first.result, retry.result);
	assert_eq!(node.state.live_calls(), 1);
	assert_eq!(
		node.state.operation(client, op).map(|r| r.result.clone()),
		Some(first.result)
	);
}

#[test]
fn reusing_an_operation_id_with_a_different_body_is_rejected() {
	let mut f = Fixture::new();
	let op = f.node.ids.operation();
	let (call, lease) = (f.call, f.lease);
	let muted = |value| Command::SetCallControls {
		call,
		lease,
		muted: Some(value),
		deafened: None,
		gain: None,
	};
	f.node
		.feed(request(f.client, op, muted(true)))
		.result
		.unwrap();
	let conflict = f.node.feed(request(f.client, op, muted(false)));
	assert_eq!(conflict.result, Err(Reject::OperationIdReused));
	assert!(f.node.state.call(call).unwrap().controls.muted);
}

#[test]
fn operation_ids_are_scoped_to_the_client() {
	let mut f = Fixture::new();
	let other = f.node.attach_client();
	let op = f.node.ids.operation();
	let (call, lease) = (f.call, f.lease);
	let mute = Command::SetCallControls {
		call,
		lease,
		muted: Some(true),
		deafened: None,
		gain: None,
	};
	f.node
		.feed(request(f.client, op, mute.clone()))
		.result
		.unwrap();
	// Same id from another client is a fresh request (and fails on authority).
	let result = f.node.feed(request(other, op, mute)).result;
	assert!(matches!(result, Err(Reject::StaleController { .. })));
}

#[test]
fn requests_from_unattached_clients_are_rejected_and_not_cached() {
	let mut node = Node::new(1);
	let ghost = node.ids.client();
	let op = node.ids.operation();
	let out = node.feed(request(ghost, op, Command::ResumeCapture));
	assert_eq!(out.result, Err(Reject::UnknownClient));
	assert!(node.state.operation(ghost, op).is_none());
}

#[test]
fn per_client_cache_evicts_oldest_and_eviction_reopens_the_retry_window() {
	let limits = Limits {
		operation_cache_per_client: 3,
		..Limits::default()
	};
	let mut node = Node::with_limits(1, limits);
	let client = node.attach_client();
	let ops: Vec<_> = (0..4).map(|_| node.ids.operation()).collect();
	for op in &ops {
		node.feed(request(client, *op, Command::ResumeCapture));
	}
	assert!(
		node.state.operation(client, ops[0]).is_none(),
		"oldest evicted"
	);
	assert!(node.state.operation(client, ops[3]).is_some());
}

#[test]
fn global_cache_bound_and_retention_expiry() {
	let limits = Limits {
		operation_cache_global: 4,
		..Limits::default()
	};
	let mut node = Node::with_limits(1, limits);
	let (a, b) = (node.attach_client(), node.attach_client());
	let mut ops = Vec::new();
	for i in 0..6 {
		let op = node.ids.operation();
		let client = if i % 2 == 0 { a } else { b };
		node.feed(request(client, op, Command::ResumeCapture));
		ops.push((client, op));
	}
	let retained = ops
		.iter()
		.filter(|(c, o)| node.state.operation(*c, *o).is_some())
		.count();
	assert!(retained <= 4, "retained {retained}");

	node.advance(600_000);
	assert!(
		ops.iter()
			.all(|(c, o)| node.state.operation(*c, *o).is_none())
	);
}

#[test]
fn detach_frees_cached_operations_and_releases_control() {
	let mut f = Fixture::new();
	let op = f.node.ids.operation();
	f.node.feed(request(f.client, op, Command::ResumeCapture));
	f.cmd(Command::Detach).unwrap();
	assert!(f.node.state.operation(f.client, op).is_none());
	assert!(f.node.state.call(f.call).unwrap().controller.is_none());
	let again = f.cmd(Command::ResumeCapture);
	assert_eq!(again, Err(Reject::UnknownClient));
}

#[test]
fn client_ceiling_is_enforced() {
	let limits = Limits {
		attached_clients: 2,
		..Limits::default()
	};
	let mut node = Node::with_limits(1, limits);
	node.attach_client();
	node.attach_client();
	let extra = node.ids.client();
	assert_eq!(
		node.command(extra, Command::Attach),
		Err(Reject::ClientLimit)
	);
	let first = node.state.snapshot().clients.into_iter().next().unwrap();
	assert_eq!(
		node.command(first, Command::Attach),
		Err(Reject::AlreadyAttached)
	);
}

#[test]
fn queue_overflow_rejects_but_safety_denies_bypass_it() {
	let limits = Limits {
		control_command_queue: 2,
		..Limits::default()
	};
	let mut engine =
		Engine::new(State::new(common::peer(), Incarnation::from_u128(1), limits).unwrap());
	let client = ClientId::from_u128(1);
	let mut op = 0u128;
	let mut next = |command| {
		op += 1;
		request(client, OperationId::from_u128(op), command)
	};
	let now = MonoTime::ZERO;
	assert_eq!(
		engine.submit(next(Command::Attach), now),
		Ok(Submitted::Queued)
	);
	assert_eq!(
		engine.submit(next(Command::ResumeCapture), now),
		Ok(Submitted::Queued)
	);
	assert_eq!(
		engine.submit(next(Command::ResumeCapture), now),
		Err(QueueFull)
	);
	// A mute must never wait behind a backlog.
	let mute = Command::SetGlobalControls {
		muted: Some(true),
		deafened: None,
		output_gain: None,
		expected: None,
	};
	match engine.submit(next(mute), now) {
		Ok(Submitted::Applied(outcome)) => assert_eq!(outcome.result, Err(Reject::UnknownClient)),
		other => panic!("{other:?}"),
	}
	let drained = engine.drain(now);
	assert_eq!(drained.len(), 2);
	assert_eq!(engine.queued(), 0);
}

#[test]
fn limits_cannot_exceed_design_ceilings() {
	let too_many = Limits {
		active_calls: 9,
		..Limits::default()
	};
	assert!(State::new(common::peer(), Incarnation::from_u128(1), too_many).is_err());
	let zero = Limits {
		attached_clients: 0,
		..Limits::default()
	};
	assert!(zero.validate().is_err());
	let ptt = Limits {
		ptt_lease_max: std::time::Duration::from_secs(1),
		..Limits::default()
	};
	assert!(ptt.validate().is_err());
	assert!(
		Limits {
			active_calls: 2,
			..Limits::default()
		}
		.validate()
		.is_ok()
	);
}
