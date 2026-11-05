# relay-engine: service and daemon

Living notes for `crates/relay-engine/src/service/` (control actor and local API) and
`src/daemon/` (process host). Media and peer adapters live in `src/media/` and `src/peers/`
and plug in through the port traits below.

## Shape

```
 clients ── UDS ── GuardLayer ── EngineService (rpc/) ──ControlHandle──▶ Actor ─▶ relay_core::Engine
                                                                          │            │
                                   media thread ◀── GateReader (arc-swap) ◀┘      Effects ─▶ Ports
```

- One task (`actor.rs`) owns `relay_core::Engine`. Inputs: RPC requests, `ControlHandle`
  events from adapters, and ticks scheduled from `State::next_deadline()`.
- Every effect leaves through `Ports` (`MediaControl`, `PeerControl`, `DeviceControl`,
  `SettingsStore`; see `ports.rs`). Methods are synchronous and must not block; completions
  come back as `ControlHandle::system/peer/peers_changed/devices_changed`. In-memory fakes:
  `service::fakes`. `Ports::unwired()` reports `Unsupported` for everything.
- Gate updates are published into an `ArcSwap<GateTable>` before the request that caused them
  is answered, so an acknowledged mute/release/takeover is already closed for the media
  thread. `GateReader::load()` is lock-free.
- `Applied` failures of the actor inbox: ordinary requests get `RESOURCE_EXHAUSTED` when the
  inbox is full; inputs that only close gates wait for space.
- IDs: `Ids` over an `Entropy` adapter. A `StartCall` derives its call and session ids from
  `(client, operation)` with a process-secret key, so a retry is the same call.

## API behaviour

- `ClientContext` in the body must equal the `relay-client-id` header
  (`INVALID_ARGUMENT`); a foreign incarnation is `FAILED_PRECONDITION`.
- `RegisterClient` grants `observe`/`control`/`input` (all when none requested); handlers check
  the capability. An observer cannot mutate (`PERMISSION_DENIED`).
- Domain mutations (calls, controller, route, controls, global controls) use core's
  idempotency cache. Adapter-executed mutations (devices, noise, mic test, profile, avatar,
  trust, microphone gain) use `Ledger`, keyed by `(client, operation)` plus a hash of the
  full request. Both keep 4096 records per client and 10 minutes. Same id with a different
  body is `ALREADY_EXISTS` / `OPERATION_ID_REUSED`. `Unsupported` adapter errors are
  `UNIMPLEMENTED` and are not recorded.
- `GetOperation` looks in the ledger, then in core. Domain rejections are returned as a
  `FAILED` operation with a stable `Problem.code`; nothing is `ACCEPTED` for long because the
  actor applies synchronously.
- Naming boundary: core `Connecting`/`Active` are `NEGOTIATING`/`CONNECTED` on the wire
  (`convert/view.rs`).
- Consent: `SetTransmitRoute` to push-to-talk or open mic requires
  `explicit_microphone_consent`; starting a call never authorizes the microphone.
- The controller lease generation is core's `ControllerLeaseId`; `lease_id` is that number
  widened to 16 bytes. Leases do not expire (`expires_in_ms = 0`); a client that goes quiet
  with no open stream for `idle_client_ttl` (default 5 min) is detached and releases what it
  held.
- `InputControl`: one stream is one input source. Closing the stream or `RELEASE_ALL`
  releases every press (final sequence `u64::MAX`, so the source is retired).

## Watching

The service keeps its own gapless event sequence (`feed.rs`): core lifecycle events plus peer,
device and settings changes, each numbered once. Core attach/detach events yield no API event.
`WatchState` without `after_revision` starts with an atomic snapshot; with it, the engine
replays from a bounded window (1024 events, backlog at most the subscriber queue) or answers
`ResyncRequired` and ends the stream. A subscriber that lags a 256-item broadcast queue also
gets `ResyncRequired`. Filters (`call_filter`, `include_peers`) may skip revisions. Streams
hold a `StreamLimiter` slot (96 by default).

## Daemon

`daemon::run(RunOptions)` serves until SIGINT/SIGTERM; `DaemonHandle::start` is the same for
tests. Start-up order: one `WriterLock` (shared by preferences and the trust store), preferences,
identity (OS credential store unless the preferences say otherwise; there is no silent
fallback), trust store, avatar cache, then the control service, then the socket. Recovery
notices from `relay-config` appear as `Snapshot.warnings`. Adapters are built by
`RunOptions::adapters` from an `Environment` (paths, identity, trust store, `FileSettings`,
`ControlHandle`, `GateReader`); without it only settings are wired. Shutdown stops the actor
first (detaching every client and stopping every live call), then the server (2 s drain),
then removes the socket.

## Profile

The advertised display name is the stored preference, or the computer's hostname (via
`gethostname`) when the preference is empty or still the factory default; either way it passes
`relay_net::discovery::sanitize_display_name`. `SetProfile` sanitizes and persists through
`relay-config`. `UploadAvatar` goes through `relay_config::avatar::process_avatar`
(size/dimension-capped decode, centre crop, 128x128 PNG re-encode, blake3 hash) and the avatar
cache. `Settings.display_name` and `Settings.avatar_sha256` carry both in snapshots and
settings events. The peers adapter learns the profile through `PeerControl::profile_changed`
(called at start-up and on every change) and advertises/transfers it.

## Not done / owned elsewhere

- `WatchTelemetry` is `UNIMPLEMENTED` (media owns telemetry).
- `InputControlMessage.transmit_route_generation` is not validated; the core binds a press to
  the route generation internally.
- `MutationContext.expected_revision` is honoured for `SetGlobalControls` only (settings
  revision).
- The local avatar hash is not persisted by `relay-config` (no schema field), so it is
  re-created from the cache only on upload.
- Operation events are not streamed; clients poll `GetOperation`.
- Windows named pipe transport is not wired here.

## Tests

`cargo test -p relay-engine --test service_clients --test service_operations --test service_watch
--test service_gate --test service_daemon`: CLI and GUI clients on a real socket with fake
adapters, takeover and stale controllers, duplicate/conflicting/expired operations, snapshot
and watch without gaps, replay and resync, bounded streams, push-to-talk gate behaviour,
config persistence, writer lock, idle clients and shutdown.
