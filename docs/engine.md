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
- `RegisterClient` grants `observe` and `call-control` (both when none requested; `call-control`
  covers every mutation and the push-to-talk stream); handlers check the capability. An observer cannot mutate (`PERMISSION_DENIED`).
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
  releases every press (final sequence `u64::MAX`, so the source is retired) with
  `immediate = true`, so it skips the release delay; an ordinary `RELEASE` is held.
- Push-to-talk release delay (`ptt.release_delay_ms`, 0 to 2000, default 200): seeded from
  the stored preference into the core at start (`service/runtime.rs`), changed live with
  `SetGlobalControls.ptt_release_delay_ms` (domain command, then persisted by the
  `ServiceOp::Controls` adapter step), shown as `Settings.ptt_release_delay_ms`. The media
  send path needs no change: the core publishes the gate that stays open for the hold and
  the closing `GateUpdate` at the deadline (the actor ticks at `next_deadline()`), and every
  safety close publishes in the same outcome as its cause, so no packet leaves after the
  gate closes (`tests/media_release_delay.rs`). `Call.transmit.effective_transmitting`
  stays true during the hold and `Call.transmit.releasing` is true; `blocked_reason` is
  empty until the hold ends. Telemetry has no separate "releasing" meter.

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
tests and embedding. `relay daemon` is a thin call to `run`.

Start-up order: one single-writer lock (shared by preferences and trust), preferences, identity
(OS credential store unless the preferences say otherwise; no silent fallback), trust store,
avatar cache; then the client endpoint is claimed (a second engine fails here, before touching
the network or audio); then `assemble`: peer manager (listener, optional mDNS), media system
(device thread, capture and render workers), glue tasks; then the control service; then serving.

Wiring (`daemon/assemble.rs`, `glue.rs`):

- `PeerAdapter` is the `PeerControl` port, `MediaSystem` is `MediaControl` and `DeviceControl`.
- `MediaConfig::gates` is the actor's `GateReader`, so send paths read the published table.
- `EventSink` never blocks (unbounded channel) and a task maps `MediaEvent::System/Peer` to
  `ControlHandle::system/peer`; `OutputLost` becomes a warning.
- `PeerOutput::Core` goes to `ControlHandle::peer`, `Media` to `MediaSystem::handle_signal`.
- `TrustRequired` and `KeyChanged` flag the peer (`Peer.trust` UNKNOWN or KEY_CHANGED) and raise
  warnings `TRUST_REQUIRED` / `PEER_KEY_CHANGED` through `ControlHandle::attention`, which
  appear as `StateEvent.warning` and in `Snapshot.warnings`. `AvatarRejected` is a warning
  (`AVATAR_REJECTED`). `Profile`, `AvatarReady` and link changes refresh the peer list.
  Pinning goes through `DecidePeerTrust` (`PeerAdapter::decide_trust`).
- The device list is polled every second and published when it changes. The saved device
  selection is re-applied once hardware is listed; a missing device raises
  `SAVED_DEVICE_UNAVAILABLE`, never a substitute.
- Listen port: `network.listen_port` (default 47800, `0` = ephemeral) so a manual `ip:port`
  stays valid. A port in use is an error, not a silent change.
- Switches: `RunOptions::network` (`NetworkOptions::loopback()` for several engines on one
  host, no discovery), `RunOptions::audio` (`AudioSetup::Cpal` or a fake `Backend`),
  `RunOptions::transport` (Unix socket or loopback TCP profile) and `RunOptions::adapters`
  (replace the real wiring with fakes, for service-layer tests).
- Overlay addresses (100.64.0.0/10): `network.allow_overlay_manual` (default false) applies to
  `PeerConfig::manual_policy()` only, used for user-typed addresses (`ManualLocalAddress`,
  `ManualConnectionString`) and the links that follow. Discovery, peer-offered candidates and
  the media policy never get the flag; discovered addresses are re-checked against the plain
  policy when they enter the table. Inbound links are checked with the manual policy too, so an
  opted-in user can be dialled over an overlay.
- Manual targets: `ManualConnectionString` (`relay://ip:port#fingerprint`) names the peer's key,
  so the call starts at once and the dial pins that key. A bare `ManualLocalAddress` of an
  unknown peer starts a dial and answers `FAILED_PRECONDITION`; retry after the handshake.
- Microphone gain is applied after the suppressor; noise suppression accepts `off` only (other
  backends need a qualified model and report `UNAVAILABLE`).
- Media over an overlay: signaling and the control link work over an opted-in overlay address,
  but media candidates a peer offers are checked with the plain LAN policy (peer-supplied, so
  never overlay). An overlay-only pair therefore connects and rings but sends no audio until
  media paths are derived from the authenticated link; that was left out deliberately.

Recovery notices from `relay-config` appear as `Snapshot.warnings`. Shutdown stops the actor
first (detaching every client and stopping every live call), lets the hangups leave, stops the
peer manager, then the server (2 s drain), then removes the socket files.

## Profile

The advertised display name is the stored preference, or the computer's hostname (via
`gethostname`) when the preference is empty or still the factory default; either way it passes
`relay_net::discovery::sanitize_display_name`. `SetProfile` sanitizes and persists through
`relay-config`. `UploadAvatar` goes through `relay_config::avatar::process_avatar`
(size/dimension-capped decode, centre crop, 256x256 lossless WebP encode, blake3 hash; accepts PNG, JPEG, WebP, GIF, BMP and TIFF) and the avatar
cache. `Settings.display_name` and `Settings.avatar_sha256` carry both in snapshots and
settings events. The peers adapter learns the profile through `PeerControl::profile_changed`
(called at start-up and on every change) and advertises/transfers it.

## Avatar, connection info, privacy

- `GetAvatar` returns the cached canonical lossless WebP (`image`, `media_type`, 256x256) for
  the local avatar or one received from a peer. Unknown hash is `NOT_FOUND`, a malformed hash
  `INVALID_ARGUMENT`, anything over 1 MiB or not a canonical WebP is refused. Needs `observe`.
- `GetConnectionInfo` lists the eligible addresses of this machine, the listen port, the key
  digest and the best `relay://ip:port#fingerprint`. It uses `relay_net::connect::encode` with the
  plain LAN policy and the default interface policy: physical private IPv4 first, then ULA IPv6,
  then IPv4 link-local; no tunnel, VPN, container, loopback, public, link-local IPv6 or
  100.64.0.0/10 address, whether or not typed overlay entry is enabled. Interfaces are listed
  with `if-addrs` (injectable through `NetworkOptions::interfaces`).
- `SetPrivacy` changes `privacy.inbound_policy` (`anyone`, `shared_channel_only`) and
  `privacy.advert_mode` (`visible`, `hidden`); each is optional and an empty request is refused.
  Both are persisted, shown in `Settings` and pushed as a settings event. A hidden mode flips
  `AdvertSpec::hidden`, so discovery advertises neither name nor key hint (re-announced at once).
  The inbound policy is enforced by the channel driver (below).

## Telemetry and the microphone test

- The media workers publish levels and counters as plain atomics (`media/meters.rs`): microphone
  level after gain and suppression, capture overruns, playback underruns, and per call the
  received level, a speaking flag (audible within the last 300 ms) and the receive counters.
  Nothing is allocated or locked on the audio threads; a reader samples them.
- `WatchTelemetry` samples at most 20 times a second (default 10, `max(meters_hz, stats_hz)`),
  one task per stream, filtered by `call_ids` (at most 8). A stream keeps 8 samples and drops
  the oldest when its client is slow (`DropOldest`); `telemetry_samples_dropped` counts them and
  `sample_sequence` shows the gaps. Round-trip time, jitter and mouth-to-ear estimates are
  unset (`latency_estimate_method` is `unmeasured`) until the media layer measures them.
- The microphone test is local only. It meters the capture path and, with
  `local_playback` (which needs `confirmed_feedback_warning`), plays the microphone on the
  user's own output through a one-frame-deep monitor queue. It adds no sender and touches no
  gate, so it cannot transmit even during a call. It needs a selected microphone (there is no
  default), ends after 30 s (`RunOptions::mic_test_max`), on `StopMicTest`, when the starting
  client goes away (idle detach) and at shutdown, and then releases the microphone unless a call
  is using it. The test opens the microphone under a reserved capture generation that the domain
  never sees; if a call opens capture meanwhile the test shares that stream.

## Channels

`src/channels/` is the live driver for named channels; `docs/channels.md` has the design,
decisions and the full behaviour table. Shape:

- `ChannelHub` (cheap to clone) owns the `ChannelStore` (`SqliteChannelStore` in the daemon),
  the `AdmissionService`, per-link state and the operation ledger. The daemon builds it in
  `assemble` with the peer handle, the control handle, the settings store, a `Clock` and the
  shared `AdvertSpec` sender; `RunOptions::channel_vault` and `channel_clock` replace the secret
  vault and the time source (tests).
- The peer manager reports `PeerOutput::Channel(ChannelEvent)`: link up (after trust), link
  down, validated channel messages and discovery records. `glue::peer_output` hands those to
  `ChannelHub::on_event` and nothing else, and passes every `PeerOutput::Core` event through
  `ChannelHub::screen` first. Channel messages never reach `decode`.
- `Msg::Channels` (via `ControlHandle::channels_changed`) replaces the channel view in the
  `Directory`; that yields `Snapshot.channels`, `StateEvent.channel` and
  `Peer.shared_channel_ids` (proven channels only).
- `rpc/channels.rs` serves the six RPCs; secret fields are moved out of the request into
  zeroizing buffers before anything else happens.
- A 1 s upkeep task (`spawn_maintenance`, `ChannelHub::tick`) follows the privacy mode, rotates
  the tag epoch, refreshes authority certificates, drops abandoned admissions and asks for
  renewals.
- Engines that share a host also share a hostname, and trust is keyed by name: give each a
  distinct display name (the channel tests do) or the second pairing reports `PEER_KEY_CHANGED`.

## Not done / owned elsewhere

- Discovered devices have no peer id until their first handshake, so only peers with a known key
  are listed. mDNS is not exercised by the loopback tests (`relay-net` has an opt-in live test).
- `InputControlMessage.transmit_route_generation` is not validated; the core binds a press to
  the route generation internally.
- `MutationContext.expected_revision` is honoured for `SetGlobalControls` only (settings
  revision).
- The local avatar hash is persisted in the settings `state` table, but its bytes live in the
  disposable `avatars.db` cache; if that file is wiped the local avatar reads as unset until
  uploaded again.
- Operation events are not streamed; clients poll `GetOperation`.
- Windows named pipe transport is not wired here.

## Tests

`cargo test -p relay-engine --test service_clients --test service_operations --test service_watch
--test service_gate --test service_daemon`: CLI and GUI clients on a real socket with fake
adapters, takeover and stale controllers, duplicate/conflicting/expired operations, snapshot
and watch without gaps, replay and resync, bounded streams, push-to-talk gate behaviour,
config persistence, writer lock, idle clients and shutdown. `service_overlay` covers the overlay
setting; `service_e2e` runs two complete engines (separate homes, loopback sockets, fake audio):
trust gating, call, consent, audio both ways, withdrawal and hangup.
`service_profile` covers avatar download, connection info and privacy; `service_telemetry` the
telemetry stream and the microphone test (including release on client loss). The mic test during
a connected call, and speaking indicators, are in `service_e2e`.
`cargo test -p relay-cli --test real_engine` runs the CLI against the real service layer.
`channels_e2e`, `channels_calls` and `channels_secrets` cover the channel driver on whole engines
(see `docs/channels.md`, test map).

## Media and peers

Code: `src/peers/` (links, trust, signaling, profile) and `src/media/` (workers, devices,
per-call pipelines, admission). Both implement the service ports: `PeerAdapter` is a
`PeerControl`; `MediaSystem` is a `MediaControl` and `DeviceControl`. Completions leave through
`MediaEvent` (`EventSink`, a non-blocking closure) and `PeerOutput` (a channel); the daemon
glue maps them to `ControlHandle::system/peer`, routes `PeerOutput::Media` to
`MediaSystem::handle_signal`, and sets `MediaConfig::gates` to the `GateReader` so send paths
read the actor's table directly (`GateBoard::publish` exists for hosts without it).

### Peers

- `spawn(config, identity, trust, incarnation)` binds the TCP listener, optionally runs mDNS,
  and returns a `PeerHandle` plus the `PeerOutput` receiver. One actor owns all links.
- Link setup: Noise handshake (`expected` fingerprint when dialling a known peer), `Hello`
  both ways, then trust evaluation. Trusted links (`Verified` / `UnverifiedFirstUse`) emit
  `LinkUp`. An authenticated but new key yields `TrustRequired`; the link is held and every
  message except keepalives is dropped until `PeerHandle::pin` (an explicit user decision) writes
  the trust store. A different key under a pinned name yields `KeyChanged` and the link is
  closed; nothing is replaced. `ConfigTrust` adapts the persistent `relay-config` store to
  relay-net's `TrustStore`; it can only pin keys authenticated in the current process.
- Simultaneous dialling: `elect` keeps the link dialled by the lower fingerprint (then the
  higher epoch); both sides compute it independently. The loser is closed without `LinkDown`.
- Reconnect: wanted peers (`ensure_link`) are redialled with jittered exponential backoff
  (`BackoffConfig`) using discovery and manual addresses. Signaling sent before the link is up is
  queued (32 per peer) and flushed on `LinkUp`. Keepalives every 5 s, idle timeout 15 s.
- Signaling: `signaling.rs` maps `PeerMessage` and `PeerEvent` to `relay.peer.v1`. The wire
  has no session id, so outbound invites record the domain's session and inbound ones derive
  one from the call id (`derived_session`). Media negotiation (`EndpointOffer`, `MediaReady`)
  is `MediaSignal`.
- Profile and avatar: after a link becomes trusted each side sends `Profile` (sanitized display
  name, avatar hash and size). Names and avatars are display-only and never confer trust. The
  receiver treats size and hash as claims: size > 1 MiB is ignored; one fetch per peer, only
  from a trusted authenticated link, never from a discovery hint; chunks must arrive in order,
  within the announced size; the assembled bytes go through
  `AvatarCache::put_untrusted` (`validate_canonical`) and the stored hash must equal the
  announced one, otherwise `AvatarRejected` and that hash is not requested again. We only serve
  the avatar we advertise. Avatar chunks (8 KiB) use a separate low-priority queue that the
  writer serves only when the signaling queue is empty, so call signaling and keepalives wait
  behind at most one chunk. The advertised mDNS name is the sanitized local display name and is
  re-published on `set_profile`. `PeerSnapshot` lists discovered devices (`id: None`, hint and
  name only) before any connection.

### Media

- Threads: `relay-devices` (owns the `AudioBackend` and stream handles), `relay-capture`
  (shared microphone chain, suppressor, one encoder per call), `relay-render` (per-call jitter
  buffer and decoder, mixer, playback FIFO). Commands arrive through bounded `rtrb` queues
  (`Inbox`); received packets through one bounded queue per peer socket.
- Send: capture reader -> suppressor -> frame assembler -> `CallSender` per call (Opus, RTP
  with contiguous sequence and a marker per talk spurt, seal, UDP). Each frame is stamped with
  the capture generation and the call's route generation when it leaves the reader. The
  sender reads the call's latest gate before encoding and again immediately before the packet
  is sealed, and drops frames whose generations differ or whose gate is closed.
- Receive: `MediaEndpoint::recv` (relay-net authenticates and replay-checks) -> `Ingress` ->
  `CallReceiver::ingest`, which accepts a packet only if the sealed call id, stream generation,
  RTP SSRC and the socket it came through all match that call's negotiated stream. Then jitter
  buffer, `CallDecoder` (decode / PLC with fade), mixer slot, `RenderSink`.
- Drift: `RenderSink` feeds `DriftEstimator` the playback FIFO fill in device samples every
  500 ms and applies the correction (sign inverted, since the FIFO is the resampler's output)
  to the output resampler. Tested by simulation only (`media_sink`).
- Devices: selection is explicit; the microphone has no default fallback. A lost device
  makes the worker drop the stream and send `CaptureLost` (or `OutputLost`); the domain closes
  the gate, and only `ResumeCapture` opens a device again.
- Admission: `AdmissionLimits::admit` refuses a call at the call cap or when the projected
  per-frame time of the capture or render worker (measured EWMA plus one more call's worth)
  exceeds the budget (default 50% of 10 ms, assumed first-call cost 400 us; both are
  design defaults, not measurements). A refusal returns before anything is installed, so
  running calls are untouched.

### Tests

`cargo test -p relay-engine --test peers_pure --test peers_links --test peers_trust --test
peers_profile --test media_gate --test media_canary --test media_sink --test media_admission
--test media_end_to_end`: election, backoff, signaling round trips, trust gating and
persistence, loopback links (hold until pin, key change, reconnect, simultaneous dial,
keepalive), profile/avatar abuse cases, gate staleness and concurrent close, per-call canary
tones on a network that delivers every packet to every receiver, drift simulation, admission,
and two in-process engines completing a call with consent, mute, withdrawal, hangup and
microphone loss.

### Known gaps

- Real hardware: all audio tests use fake devices; nothing here measures latency, wake-up
  jitter, glitch rate or CPU, and the worker timing and admission defaults are unvalidated.
- No steady-state allocation guarantee end to end: relay-net allocates the sealed datagram
  (send) and the payload `Vec` (receive) per packet, and the UDP send is driven with
  `Handle::block_on` from the capture thread. Codec, jitter buffer, mixer and RTP
  packetization use preallocated buffers.
- A link replaced by election after media was bound keeps its media endpoint on the old
  session (no `switch_epoch` wiring); elections normally finish before any call starts.
- `prepare_media` failures after admission (no link, socket bind, playback open) log and retire the
  call without a completion event; the domain's path timeout ends it.
- `resolve_address` answers from known addresses only; an unknown one starts a background dial
  and reports `Unavailable`. A discovered device has no `PeerId` until its first handshake, so
  `PeerControl::peers()` lists only peers with a known key.
- Microphone test, microphone gain and non-`off` noise suppression return `Unsupported`.
- `relay-audio`'s `StreamResampler` is bypassed when device and internal rates are both 48 kHz
  and then ignores drift correction, so the drift loop only acts on devices running at another
  rate; a 48 kHz device with a drifting clock is uncorrected until the resampler can run
  in-place. `media_sink` simulates a 44.1 kHz device for that reason.
- Avatar bulk-priority is by construction (biased writer); no timing test exists.
