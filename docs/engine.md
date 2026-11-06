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
  covers every mutation and the push-to-talk stream) and `admin` only when named; `Shutdown` is the
  one admin call. Handlers check the capability. An observer cannot mutate (`PERMISSION_DENIED`).
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
- Consent: the per-call transmit mode is the consent. `SetTransmitMode` to `UNMUTED` or
  `PUSH_TO_TALK` is the explicit consent, in that one request (there is no consent flag);
  `MUTED` withdraws it. Starting a call or accepting one never authorizes the microphone by
  itself; a new call starts in `calls.default_transmit_mode` (default `push_to_talk`, a
  user setting applied live when the call connects, through the same route change and gate
  rules, so no audio captured before the change is ever sent). `ptt.enabled = false`
  rejects `PUSH_TO_TALK` (`PTT_DISABLED`), turns a push-to-talk default into `muted`
  (`Settings.effective_default_transmit_mode`) and mutes calls already in push-to-talk.
  Audio is verified end to end by `tests/audio_flow.rs`: two whole engines on loopback with
  real sockets, Noise links, Opus and RTP, a synthetic tone as the microphone and a recording
  speaker, after a manual dial plus trust on first use. UDP path challenges are sent again
  every 250 ms until answered, so one lost datagram does not stall a call.
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
tests and embedding. The `relayd` binary (`apps/relay-daemon`) is a thin `clap` front end for `run`; no client embeds the engine. `relayd` takes `--socket`, `--tcp` / `--tcp-profile`, `--home` (config and data under a directory), `--log-level`, `--listen-port` and, hidden for tests, `--no-discovery` and `--identity-store os|file|memory`.

`Shutdown` (admin only) sets a flag the host watches; `run` then stops the same way as on SIGTERM: calls end, the microphone is released, the endpoint is removed, the process exits 0. It is idempotent.

Build and API revision: `relay_api::BUILD_REVISION` is the git commit the binary was built from (`build.rs` of `relay-api`; `-dirty` when tracked files had uncommitted changes, `<version>-dev` without git) and `relay_api::API_REVISION` is an integer bumped when the API changes in a way clients depend on. The daemon reports both in `GetInfo` (`EngineInfo.build_revision`/`api_revision`), in the `RegisterClient` reply (`ClientSession.daemon_build_revision`/`daemon_api_revision`), in `GetSettings` (`SettingsDump.build_revision`/`api_revision`) and in every `Settings` state message (`daemon_build_revision`/`daemon_api_revision`, so one snapshot shows a stale daemon). A daemon built before revisions reports an empty build and `0`. The embedded commit does not change for further uncommitted edits, so restart a daemon after rebuilding from a dirty tree (the desktop app does this at launch, below).

Presence and ownership: `service/presence.rs` is a pure state machine over the clients that hold an open `WatchState` (`PresenceLink`, one `Watcher` guard per stream). It debounces attach and detach by `presence_debounce` (2 s, injectable) and feeds `SystemEvent::TrackPresence` and `Watching` to core, which gates adverts (`AdvertSpec::present`, `PeerControl::presence_changed`) and declines invites as busy with nobody attached, and ends a call `owner_grace` (3 s) after its owner is gone (`EndReason::OwnerGone`). Owner detach closes the gate at once, so the push-to-talk release delay never outlives it. `relayd --idle-exit <secs>` (default off; `RunOptions`) stops the daemon after that long with no client attached and no call. First-run defaults: `privacy.advert_mode` visible, inbound `anyone`, `clients.autostart_daemon` true (`SetGlobalControls.autostart_daemon`).

Start-up order: one single-writer lock (shared by preferences and trust), preferences; then the
client endpoint is claimed and **served at once** (a second engine fails here, before touching
the network, audio or the secure storage); then, in the background, the identity (OS credential
store unless the preferences say otherwise; no silent fallback), trust store, avatar cache;
then `assemble`: peer manager (listener, optional mDNS), media system (device thread, capture
and render workers), glue tasks; then the control service replaces the start-up answers on the
socket. `DaemonHandle::begin(options)` returns a `Booting` that is already serving;
`Booting::ready()` opens the identity and finishes (and stays up as `Failed` instead of
returning an error); `DaemonHandle::start` is `begin` plus a first-failure-is-an-error
`ready`, as embedders (Android) and most tests use it. `run` uses `begin`/`ready`.

### Start-up state, the keychain prompt and the identity lifecycle

Opening the OS keychain can wait minutes for the user's password (macOS) or fail (denied,
locked, interrupted). Before this was handled the daemon opened the keychain before it served
its socket, so a slow answer made the launcher give up ("did not become ready") and the stuck
daemon was lost. Now:

- `relay_api::EngineSwitch` is on the socket from the first moment. Until the engine is
  installed it answers `GetInfo` with `EngineInfo.startup` (`STARTING` + `WAITING_FOR_KEYCHAIN`,
  or `FAILED` + `KEYCHAIN_UNAVAILABLE` / `IDENTITY_UNREADABLE` with the system's text in
  `detail`), refuses everything else with `UNAVAILABLE` (`RegisterClient` included: `RelayClient`
  reports it as `ApiError::Starting`), and always routes `RetryKeychain`, `UseFileStore` and
  `ResetIdentity` to `daemon/boot.rs`. A ready engine reports no `startup`.
- The identity is read on a blocking thread (`spawn_blocking`), one read at a time per process.
  Nothing that touches the network (peers, discovery, advertising, listeners) is built before the
  identity is loaded, so a daemon never advertises while it waits.
- `Failed` keeps the daemon up, holds the writer lock and waits for a choice: `RetryKeychain`
  reads again; `UseFileStore` saves `identity.key_storage = insecure_file` and retries (the only
  way the file store is ever used: never a silent fallback; a new identity is created if that
  file holds none, so contacts re-verify); `ResetIdentity` deletes the stored identity
  (see below). While `STARTING` these calls are refused with `FAILED_PRECONDITION`.
- `relay_config::Keychain::os` is one cached instance per process: identity and channel secrets
  share the single credential-store item and it is read once per start (a failed read is not
  cached, so a retry asks again); readers queue behind the read in flight. Only the daemon opens
  the OS store: `relay` and the desktop app never do.

Identity lifecycle. `IdentityStore::load` has three outcomes: found, `Ok(None)` (the store
definitively says no item exists) and an error (denied, timeout, interrupted, locked, ACL
refusal, corrupt or newer-version bundle). A new identity is generated **only** on `Ok(None)`
and is written with `IdentityStore::create`, create-if-absent (one locked read-modify-write of
the keychain bundle; an exclusive link for the file store), so two first starts end up with one
identity and an existing item is never overwritten. An error never generates or writes anything:
the daemon goes to `Failed` and leaves the item byte-identical. Every creation is logged at info
with the fingerprint. The identity changes only when: the user runs `relay identity reset --yes`
(`ResetIdentity`; a bundle too damaged to read is removed whole), the user deletes the keychain
item, the user chooses the file store with none stored, or on Android the app data is wiped.
A changed identity makes every contact see a new key and verify this device again.

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
- Media over an overlay: with `network.allow_overlay_manual` on, a link whose authenticated
  remote socket IP is in 100.64.0.0/10 lets media use exactly that IP (`PeerConfig::overlay_peer`,
  `LinkInfo.overlay_peer`). The offered candidate is accepted only when its IP equals the link's
  remote IP (its port comes from the signed offer), and the peer's `MediaEndpoint` is bound with
  `AddressPolicy::with_overlay_peer(ip)` so path validation and challenge replies accept that one
  address. The IP is never taken from advertised, discovered or offered data. With the setting off
  (default), or for any other overlay address, behaviour is unchanged: candidates are refused and
  an overlay-only pair rings without audio. Covered by `peers_overlay_media` (no real overlay
  interface exists in tests, so the endpoint side is covered by the `relay-net` address tests).

Recovery notices from `relay-config` appear as `Snapshot.warnings`. Shutdown stops the actor
first (detaching every client and stopping every live call), lets the hangups leave, stops the
peer manager, then the server (2 s drain), then removes the socket files.

## Profile

The advertised display name is the stored preference; when the preference is empty or still the factory
default it is `RunOptions::default_display_name` (additive, `None` on desktop; a phone passes its device
name because an Android hostname is `localhost`), and when that is `None` or blank after sanitising, the
computer's hostname (via `gethostname`). A name the user set always wins. Whatever is chosen passes
`relay_net::discovery::sanitize_display_name`. The default is never written to the preferences, so a later
change of the device name is picked up until the user chooses one. Tests: `daemon::settings::tests`. `SetProfile` sanitizes and persists through
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

- `ClearAvatar` (`ServiceOp::ClearAvatar`): `SettingsStore::clear_avatar` drops the `state` row
  that pins the local avatar (`AvatarCache::clear_local`; the bytes stay cached and age out),
  the actor clears `Aux.avatar` and calls `profile_changed`, which sends every trusted link a
  `Profile` with an empty avatar hash. The receiver publishes that profile with no avatar, so its
  state feed shows an empty `Peer.avatar_sha256`.
- `SetFavorites` (`ServiceOp::SetFavorites`, `rpc/favorites.rs`): `SettingsStore::set_favorites` (the daemon's `FavoriteBook` in `daemon/favorites.rs` maps peer ids, the key fingerprints, to the trust public keys of the `favorites` table, `docs/config.md`) validates and stores the whole list; then `Directory::set_favorites` sets `Peer.favorite`/`favorite_position` and publishes a `Peer` event for each peer whose place changed (the existing peer-update path, no new event kind). `ForgetPeer` and `DecidePeerTrust` re-read the list afterwards, because the table drops the favorite of a forgotten peer itself. Tests: `tests/service_favorites.rs`.
- `ForgetPeer` / `DecidePeerTrust(UNKNOWN)` (`ServiceOp::ForgetPeer`): `PeerControl::forget_peer`
- Key changes (`peers/key_changes.rs`, `peers/replacing.rs`): when `on_up` refuses a key under a pinned name, `PeerHandle` remembers it in shared memory: offending key -> previous pinned record, its name, the presented name and when (at most 32; a newer offending key for the same pinned key replaces the older entry; it is dropped when the pinned key is forgotten, replaced or seen again, or the offending key is pinned). `PeerInfo::key_change` carries the pinned digest to the directory (the glue sets it on the flagged peer), so `Peer` has `key_changed`, `previous_peer_id`, `previous_fingerprint_short`; `peer_id` and `fingerprint_short` stay those of the new key. `forget` accepts either key (an id or an 8+ digit prefix of the offending key resolves through the map to the pinned record; an unrelated id is still `NotFound`), removes the record, the entry and the retry state of both keys, and the service drops the flagged card and the `PEER_KEY_CHANGED` warning. `ReplacePinnedKey` (`ServiceOp::ReplaceKey`, `rpc/replace_key.rs`, `PeerHandle::replace_pinned_key`): only with a remembered key change for `peer_id`, `verified = true` and `expected_fingerprint` (64 hex, separators and case ignored) equal to the offending key's full fingerprint, `KeyedTrustStore::replace` swaps the record in one transaction (`relay-config` `TrustStore::replace_key`: the new key is verified, keeps the name and first-seen time, the `favorites` row moves to the new key with its place, then the old row goes; call history keeps its copies), the entry is cleared and the manager dials the new key (addresses of the old key plus the scanned string's). A mismatch, a short fingerprint, a missing entry or `verified = false` is refused with nothing changed. `ConnectAndTrust` of a string whose fingerprint equals a refused key (`PeerControl::key_change_for`, also after the dial itself revealed the change) takes the same path with the scanned fingerprint as the proof. Never automatic, never on a name alone. Tests: `tests/peers_key_change.rs`, `peers_key_change_scan.rs`, `service_key_changed.rs`.
- `ConnectAndTrust` (`rpc/connect_trust.rs`, `PeerControl::connect_pinned`, `ConnectFailure`): parses the string with the manual-address policy (as `StartCall` does), dials with `PeerHandle::dial(addr, Some(expected))` from a blocking thread (the handshake refuses another key before our static key is revealed, so a wrong device learns nothing and no record is created), and on a match pins `Verified` with `PeerHandle::pin` (the path `DecidePeerTrust(VERIFIED, out_of_band)` uses), then a `ServiceOp::Trust` refreshes the directory. Strength: the string carries the whole 64-hex-digit digest, so the comparison is exact (256 bits); shorter hints are refused, never compared. Failures are FAILED operations with `CONNECT_STRING_INVALID`, `CONNECT_FINGERPRINT_MISMATCH` or `CONNECT_UNREACHABLE` and leave the trust store untouched. Tests: `tests/connect_trust.rs`.
  resolves the selector against the trust store (`peers/forgetting.rs`: id, hex fingerprint
  prefix of 8+ digits, or exact case-insensitive name; fingerprint first; several matches are
  `PortError::Ambiguous`, problem `PEER_AMBIGUOUS`, and nothing changes), removes the record
  under the trust lock and tells the manager (`Command::Forgotten`) to drop the link and stop
  redialling. The directory emits `Removed{kind:"peer"}` and re-reads the peer list.
  `Peer.fingerprint_short` is `Fingerprint::short()` of the peer id.

## Settings by key

`GetSettings` returns `SettingsStore::all()` (`relay_config::prefs::flat::all`: every settable
key, strings unquoted, `config_version` left out). `SetSettings` (`rpc/settings.rs`) needs
`call-control`; the whole call is `PERMISSION_DENIED` (`SETTING_RESTRICTED: <key>`) when it
would change a key in `service/settings_keys.rs`'s sensitive list (`identity.*`,
`diagnostics.record_raw_audio`, `network.allow_wan`, `network.allow_overlay_manual`); a sensitive
key sent with its current value is a no-op so an export can be imported again. The entries (at
most 64, keys 128 bytes, values 1 KiB) go to the actor in sorted order (`Msg::Settings`,
`actor_settings.rs`): `SettingsStore::apply` validates them one after another on a copy of the
preferences (`flat::set`, which refuses unknown keys, bad values and out-of-range numbers) and
saves once. Then the live keys are applied: display name, privacy and ring keys, autostart and
microphone gain in the actor (the microphone gain falls back to `SAVED_NEEDS_RESTART` when the
media path refuses it); `audio.playback_gain` and `ptt.release_delay_ms` as a domain
`SetGlobalControls` (under an operation id derived from the request, so the caller's own id never
collides with it); `messages.*` and the messaging half of `limits.*` through the messaging actor. A follow-up that fails turns its
keys into `SAVED_NEEDS_RESTART`. Everything else (devices, noise backend, `network.*`,
`hotkeys.*`, `diagnostics.*`, the other `privacy.*`) is saved and read at the next start.
`SetSettings` is not tracked by operation id: repeating a request just saves the same values
again.

**Limits** (`limits.*`, `docs/limits.md`). `FileSettings::apply` uses `flat::set_many` and, when
a limit changed, puts it into force itself: `AvatarCache::set_limit` evicts at once and
`CallHistory::set_limits` prunes at once (both are shared with the other clones the daemon holds).
The messaging half (`limits.messages_session_mib`, `limits.messages_retained_mib`,
`limits.messages_retained_calls`, `limits.transfer_concurrency`) goes to the messaging actor as `Hub::set_limits(MessagingLimits)`
(`messaging/storage.rs`): every transcript, running or ended, takes the new per-session budget
(nothing is deleted; a chat over its budget refuses new data until it is under), ended
transcripts over a smaller retained budget or call count are removed oldest first with `HistoryEvicted`, and
the transfer count applies from the next offer or accept. At start the saved values are read
(`MessagingLimits::from_prefs`; `MessagingOptions.limits` overrides them for tests that need a
budget below the allowed minimum). If the messaging actor is not running the keys are
`SAVED_NEEDS_RESTART`. `GetSettings` also fills `SettingsDump.usage`: avatar cache bytes
(`SettingsStore::avatar_cache_bytes`), bytes of ended chats and of the largest running chat
(`Hub::usage`); a limit whose usage is not cheap to know has no entry. Tests:
`tests/service_limits.rs`, `tests/limits_defaults.rs`.

## Call history

`service/recorder.rs` follows each call from its first `CallUpdated` event to its terminal one; the actor feeds it from `sync_feed`, the single place calls end (`actor_history.rs`). A call is recorded only if the user saw it: an outgoing call (the user started it) or an incoming one that reached `IncomingRinging`. An invite the domain refused before a call existed (no client attached, call cap, policy rejection, unauthenticated link) never produces an event, so it leaves nothing; a call that lost the simultaneous-invite tie-break (`GlareLost`) is superseded and not recorded. Outcome: connected at any point (`Active`) is `completed` with the duration from the first `Active` to the end (the monotonic actor clock), even if it then failed; otherwise `Declined` is `declined` (a decline caused by the window going away, `OwnerGone`, on an incoming call counts as `missed`); an outgoing call that ended or was cancelled before connecting is `cancelled`, any other outgoing failure `failed`; an incoming call never accepted is `missed`, one the user accepted that then failed is `failed`. The name comes from the directory when the call is seen (the fingerprint hint stands in when none is known). The wall clock (`ServiceConfig::wall_clock`, default system time) dates the entry and prunes the table; tests inject one.

Writing goes through `SettingsStore::record_call` (default `Unsupported`; `FileSettings` writes to `relay_config::history::CallHistory`, `MemorySettings` keeps a list). Nothing is written while `history.enabled` is off: the key is a live key, `actor_settings.rs` copies the stored value into `Directory.aux.history_enabled`. After a write and after a clear the actor bumps `Aux.history_revision` and republishes the `Settings` event (`Settings.history_enabled`, `Settings.history_revision`), which is how every watcher and the UI know to read the list again; the revision is not stored (it restarts at 0 with the daemon, so clients also read the list on connect). `ListCallHistory` (needs `observe`) reads through `SettingsStore::call_history` on a blocking thread, asks for one row more than the limit to set `more`, and caps the page at `MAX_LIST`; `ClearCallHistory` (needs `call-control`) goes through the actor (`Msg::ClearHistory`). Tests: `tests/service_history.rs`.

## Telemetry and the microphone test

- The media workers publish levels and counters as plain atomics (`media/meters.rs`): microphone
  level after gain and suppression, capture overruns, playback underruns, and per call the
  received level, a speaking flag (audible within the last 300 ms) and the receive counters.
  Nothing is allocated or locked on the audio threads; a reader samples them.
- `WatchTelemetry` samples at most 20 times a second (default 10, `max(meters_hz, stats_hz)`),
  one task per stream, filtered by `call_ids` (at most 8). A stream keeps 8 samples and drops
  the oldest when its client is slow (`DropOldest`); `telemetry_samples_dropped` counts them and
  `sample_sequence` shows the gaps. `rtt_ms` is the
  smoothed round trip of the control link's own keepalives (`peers/rtt.rs`): the writer stamps
  each `Keepalive{nonce}`, the reader times the peer's echo of the latest nonce (a stale,
  replayed or unsolicited echo is ignored), all inside the Noise session. The first sample
  appears one keepalive interval (5 s) after the link comes up, and while there is none
  `latency_estimate_method` is `unmeasured`; with one it is `control_keepalive_rtt`. It is a TCP
  probe that queues behind signaling, so it overstates the UDP media path and is never turned
  into a mouth-to-ear figure: jitter and `estimated_mouth_to_ear_ms` stay unset.
- The microphone test is local only. It meters the capture path and, with
  `local_playback` (which needs `confirmed_feedback_warning`), plays the microphone on the
  user's own output through a one-frame-deep monitor queue. It adds no sender and touches no
  gate, so it cannot transmit even during a call. It uses the selected microphone or the system default, ends after 30 s (`RunOptions::mic_test_max`), on `StopMicTest`, when the starting
  client goes away (idle detach) and at shutdown, and then releases the microphone unless a call
  is using it. The test opens the microphone under a reserved capture generation that the domain
  never sees; if a call opens capture meanwhile the test shares that stream.
  When the microphone cannot be opened (device missing, macOS permission refused or still
  undetermined, device busy) the daemon raises the warning `MICROPHONE_UNAVAILABLE`
  (`recovery_action = check_microphone_access`) whose message tells the user where to allow access
  (macOS: System Settings > Privacy & Security > Microphone). The first open on macOS triggers the
  system prompt; a signed bundle with a usage description is still an open gate (`docs/gates.md`). A failed open leaves capture `Lost` for the rest of the call; the next call retries it, and `SystemEvent::InputAccessChanged` (a hosted daemon is told with `DaemonHandle::input_access_changed`, the Android shell does when the permission arrives) retries it inside the running call. `MICROPHONE_UNAVAILABLE` is withdrawn with a `Removed{kind: "warning", id: code}` event when capture opens. A device lost mid-call still waits for an explicit `ResumeCapture`.

## Call statistics (`WatchCallStats`)

A stuck call is diagnosed with `WatchCallStats(call_id)`, a server stream of `CallStats` about
once a second (capability `observe`, one stream slot each). It is separate from the state stream
on purpose: the numbers change every second and would flood it, so a client opens it only
while it shows them. The media layer keeps plain atomics and one path record per call
(`media/diag.rs`, `media/stats.rs`); the stream task turns a raw reading (`service/call_stats.rs`,
`CallRaw`) into the wire message and keeps the windows. Nothing is allocated or locked on the
audio threads.

- **Media path.** `path_state` is `WAITING` (waiting for the remote's candidates, or for a
  candidate to answer the path challenge; `path_reason` names it), `CONNECTED` (`local_addr`,
  `remote_addr`, `transport` = `lan` or `overlay`) or `FAILED` (`path_reason` holds the cause:
  stream binding refused, workers busy, or the last validation error such as a timeout).
  A call without media yet reports `WAITING` with "media is not prepared".
- **Activity.** `sending` / `receiving` are true when a packet was sent / arrived within the last
  500 ms; `packets_sent` and `packets_received` count them (received includes packets the jitter
  buffer refused as late). `mic_level_dbfs` is the microphone level while capture is open.
- **Jitter** is RFC 3550 interarrival jitter, `J += (|D| - J) / 16` over the arrival and RTP
  timestamps, so `jitter_ms` is the mean change of delay between neighbouring packets.
- **Loss** comes from the sequence numbers: expected = highest wrap-extended sequence minus the
  first plus one; lost = expected - received. `loss_percent_total` is since the first packet,
  `loss_percent_5s` over the last five seconds (deltas between two 1 Hz readings). A reordered
  packet never counts as new, a 16-bit wrap is not loss.
- **Round trip.** `rtt_ms` is the control link's smoothed keepalive round trip (`peers/rtt.rs`,
  see Telemetry above); `rtt_min_ms` / `rtt_max_ms` are the extremes of the readings of the last
  10 s. It is a TCP probe and overstates the UDP path.
- **Buffers.** `jitter_buffer_depth_ms` is audio queued in the jitter buffer,
  `jitter_buffer_target_ms` the margin the controller wants, `playout_delay_ms` the playback FIFO
  fill plus the latency the audio backend reports, `capture_buffer_*` and `playback_buffer_*`
  the callback or buffer size of the open device streams in frames and milliseconds,
  `clock_drift_ppm` the drift correction applied to playback.
- **Codec.** `codec_bitrate_bps` is the encoder setting, `packet_bytes` the size of the last
  sealed datagram.
- **Mouth-to-ear** is an estimate, not a measurement: `capture buffer + encode (12.5 ms: one
  10 ms frame plus about 2.5 ms of lookahead) + rtt/2 + jitter buffer depth + decode (5 ms: half
  of the 10 ms render tick) + playout delay`. It is unset while there is no round trip, and it
  assumes the one-way delay is half the control link's round trip.
- The first packet sent, the first packet received and the outcome of the path validation are
  logged once per call at `info` (`first media packet sent`, `first media packet received`,
  `media path validated` or the `warn` `no media path could be validated`).

Tests: `tests/call_stats_sim.rs` (a simulated link with 5 % loss and 20 ms of jitter plus a fake
clock for the windows) and `tests/call_stats_loopback.rs` (two whole engines).

## Tones

Ring, ringback and call blips are played by the daemon, so they work for any
client. `service/tones.rs` holds `ToneDriver`: the actor calls it after each
message and tick (only when the domain revision, the attached flag or the ring
preferences changed, or a preview runs) and sends the resulting `ToneCommand`s to
`MediaControl::tone`; the real media system forwards them to the render worker's
`TonePlayer` (see `docs/audio.md`, Tones). `StoredSettings.ring` /
`SettingsChange::Ring` / `Aux.ring` carry `audio.ringtone_enabled`,
`audio.ringtone_volume`, `audio.ringback_enabled` and `audio.call_sounds_enabled`;
`SetGlobalControls` changes them (`ServiceOp::Controls.ring`), persists, publishes
`Settings` and the next sync applies the change to a playing tone. `PreviewTone`
(`ServiceOp::PreviewTone`) arms a 2 s test ring; the actor wakes at its deadline.
The actor stops any tone at shutdown. Tests: `tone_driver`, `service_tones` (fake
media records tone commands: no ring unattended, ring with a client, stop on
remote cancel, deafen, live volume and disable, ringback, preview),
`media_tone_playback` (fake audio backend: tone only while ringing, no call
needed, deafen, zero packets, peer hears silence, no underrun in the first 500 ms
on an output that takes 250 ms to open).

The ring does not start on a cold stream: an outgoing call sends
`ToneCommand::Prewarm` while it prepares (the output opens and is held 3 s), and the
device thread prefills the playout FIFO with silence and attaches the output to the
render worker before it starts the stream (`docs/audio.md`, "Start-up of a ring").

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
  the tag epoch, refreshes authority certificates, drops abandoned admissions, demotes proofs
  whose certificate expired (re-asking shortly before) and asks for renewals.
- Each link may send at most 20 channel messages per second; the rest are dropped before any
  verification, hashing or store access. Membership proof is the two-step `MemberRef` then
  `Proof` flow (`docs/channels.md`, D6). Channel time is a clamped wall clock (`Clock`).
- The operation ledger holds an invite as `StoredInvite` (zeroized on drop, redacted `Debug`).
- Engines that share a host also share a hostname, and trust is keyed by name: give each a
  distinct display name (the channel tests do) or the second pairing reports `PEER_KEY_CHANGED`.

## Session messaging

`src/messaging/` is the daemon side of `docs/session-messages.md`; `docs/api.md` has the RPCs.
One actor, `Hub`, is the single owner of every transcript and transfer record. It is a separate
task next to the control actor (the call state machine stays free of file and chat concerns),
fed by closures on a bounded inbox (`MessagingHandle::post/ask`), and wakes for the stores' retry
deadlines (`SessionStore::next_deadline`, `poll`), the "engine tick" for messaging.

- **Sessions follow calls.** After every domain revision change the control actor reports
  `CallFact`s (`Actor::sync_messaging`). A session starts when a call is `Connecting`, `Active` or
  `Interrupted` and ends when the call is over or gone. It is a `relay_messages::SessionStore`
  inside one `HistoryManager` (256 MiB retained budget by default, `limits.messages_retained_mib`, and the last 3 ended calls, `limits.messages_retained_calls` via `HistoryManager::set_max_ended`; whichever is stricter evicts the oldest ended first; a new call with the
  same peer replaces the old transcript). **At most one session per peer**: no call id travels
  on the session lane, so a message is routed by the *authenticated link's peer* to that peer's
  one session. A message for session S can only come from S's peer; a peer with no session gets
  a rate-limited `Ended` ack and nothing is stored. A second parallel call with a peer that
  already has a session gets no messaging.
- **Peer layer.** `PeerOutput::Session(SessionEvent)` carries `LinkUp{peer, remote_ip}`,
  `LinkDown` and `Message` (never `Debug`-printed, only the kind), only from the canonical
  trusted link; `PeerHandle::send_session` sends on that link or drops (the store's outbox
  retries). `relay-net` already validates structure and flood-guards the link (4x the sender limit
  closes it).
- **Permissions on both sides.** `Permissions` live in the store; `SessionStore::receive` refuses
  a disabled kind with a `Disabled` ack no matter what the sender's UI did, compose refuses it
  locally. `messages.enabled` is checked in front of both. The initiator (host) pushes a
  `PermissionUpdate`; only the host's push is applied by the guest. Files off cancels running
  transfers and refuses new accepts. Max offer size is enforced on both sides (`TooLarge` ack).
- **Acks and retries.** Every inbound chat/offer is answered with a `ChatAck` from the store
  result; `Ack` updates delivery state; unacked messages are resent (5 s, doubling, 3 retries, then
  `Failed(timeout)`); a rejection never retries; the user retries with `RetrySessionMessage`.
- **Images** are validated by `relay_messages::image::inspect` (header only, decompression bombs
  and animation limits refused) and stored as the sent bytes, at most 15 KiB on this lane. The
  engine never decodes pixels, so there is no decode worker or allocation limit to enforce here;
  clients decode with their own `max_alloc` and deadline.
- **File transfers.** A `TransferHub` (`relay_net::transfer`) is bound at start on
  `network.transfer_port` (0 = ephemeral) at the peer listener's IP, with the same address
  policy. Its port is told to a peer only in the `DataLinkGrant` on the authenticated control
  link; mDNS and connection strings never carry it. Flow: `OfferFile` (daemon opens the path,
  hashes a BLAKE3 snapshot on a blocking worker, remembers size/mtime/inode) -> offer on the
  control link -> `RespondOffer` -> `OfferResponse` + `TransferHub::expect` token as a grant on
  the control link (the data-link session id is the call id, which both sides share) -> the
  sender dials with the token -> chunks, credit and the verification result on the data link.
  Each running transfer is one task owning its `relay-messages` `Sender`/`Receiver`, the file or
  `FileSink`, and the link; reads (`spawn_blocking`) and every receiver step (write, fsync,
  hash check, rename) run on blocking workers; the actor only records progress (an event per
  256 KiB) and results. The sink is `relay_io::sink::DirSink` (temp file in the destination,
  fsync, verify, rename without replacing); `RunOptions::messaging.sinks` replaces it (tests:
  full or failing disk). Free space is checked by `Receiver::accept`
  (`NO_SPACE`, the offer stays open). Two transfers per session by default (`limits.transfer_concurrency`, 1 to 8), never auto-accepted, never
  auto-opened, no resume; names are sanitized by both sides. The sender re-checks the source
  before the first chunk and before finishing (`source_changed`). Pacing is
  `messages.transfer_rate_limit_mbps` per link, applied live to running transfers. A failure the
  receiver already reported on the data link is not repeated on the control link (the two race
  and would change its meaning); a sender's failure also goes over the control link, because
  closing a data link with unread credit can reset it and lose the last frame.
- **Ownership and privacy.** Capability `messages` (never default, not implied by `admin`) gates
  every RPC and the event stream. A client holding it may read and send in any live session of
  this daemon and read/export/clear any retained transcript; the rationale is that transcripts
  are private to the people in the call and the daemon serves one user, so "who may read" is the
  question of which program the user trusted with `messages`, not which one holds the call lease.
  Clients without it (observers, call controllers, admins) get `PERMISSION_DENIED` and no event.
  Transcripts are RAM only; they vanish when the daemon exits (including idle-exit); `ClearSession`
  (a client command such as `relay messages clear`) drop them earlier; export writes only where the client says.
- **Logging.** Nothing logs message text, file names or paths. The only `tracing` calls in
  `messaging/` carry ids and error classes, `Debug` of peer events and API requests is redacted,
  and `messages_secrets` captures TRACE from two whole daemons and greps it.
- **Idempotency.** Mutations keep their last 1024 outcomes by (client, operation id) in the actor
  (same request: stored outcome; different request: `ALREADY_EXISTS`). Not time limited.
- **Known limits.** `RespondOffer` accept creates the temp file in the actor (`Receiver::accept`
  needs the sink synchronously for `NO_SPACE`): one `statvfs` and one `create`. Previews are read
  from the live file at approval, not from the offer snapshot. A hostile peer that holds a real
  call can still fill its session with up to 64 MiB; the budget and the per-peer rate limit are
  the bound.

Tests (`cargo test -p relay-engine --test 'messages_*'`, real daemons on loopback with fake audio
unless noted): `messages_text` (round trips, acks, idempotency, pins, search, paging, gapless
watch), `messages_files` (happy path with hash and no overwrite, reject, receiver and sender
cancel, source changed, refused paths and size limits), `messages_limits` (full and failing disk
through fake sinks, concurrency 2, files off mid-transfer, previews), `messages_permissions`
(receiver enforcement, host push, master switch, capability gate, images, history budget, rate
limit), `messages_isolation` (A with B and C plus B-C: no cross-call text), `messages_hostile` (a
raw trusted peer: out-of-session chat, forged control messages, flood), `messages_retry` (a raw
peer that controls its acks: resend with the same sequence, rejection, user retry),
`messages_history` (retention, replacement, retained-budget eviction, clear, export, nothing on
disk), `messages_secrets` (TRACE grep).

## Not done / owned elsewhere

- Discovered devices have no peer id until their first handshake, so only peers with a known key
  are listed. The manager therefore dials discovered devices that carry a key hint, are not this
  device and are not linked (`peers/probing.rs`). A probe is an ordinary control-link handshake: it
  shows the responder this device's static public key and display name (and its IP address, which
  it sees anyway) and nothing else; no audio, no call and no trust. It is made only while this
  device is itself visible, that is discovery advertising is on, a client is attached (presence)
  and `privacy.advert_mode` is not `hidden`; when either stops, no new probe starts (established
  links stay) and the per-instance memory is dropped, and when both hold again the current
  discoveries are probed at once (`Command::Visibility`, sent when the advert spec changes). At
  most 8 probes run at once and an instance is probed again only after 30 s. The responder holds
  the resulting link untrusted: it raises the usual trust notice, never auto-trusts, and the
  inbound policy (who may ring) still applies before any call reaches call state. Anyone on the
  LAN who advertises can trigger a probe; being private (hidden) is what stops this device from
  dialling out. mDNS is not exercised by the loopback tests (`relay-net` has an opt-in live test).
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
`peers_sessions` (call table ownership and bounds, candidate subnet), `peers_hardening`
(untrusted-link cap, key cache churn, look-alike names) and `channels_hardening` (listed
names, proof expiry, invite redaction) cover the review fixes. `channels_e2e`, `channels_calls`
and `channels_secrets` cover the channel driver on whole engines
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
- Inbound hardening: the accept loop uses `HandshakeGate::try_enter(source_ip)` (2 pending
  handshakes per source, oldest of the busiest source evicted, `respond_gated` abandons an
  evicted one), sleeps `AcceptBackoff` (20 ms to 2 s) on accept errors, and `UntrustedLinks`
  caps held untrusted links at 16 (2 per source), closing the oldest when exceeded. Dial and
  AddAddress apply `check_manual_socket` (overlay allowed only with the typed-address opt-in);
  discovery uses `check_socket`. Discovery records expire after 120 s without a refresh (5 s
  upkeep tick). `PeerConfig` is unchanged.
- Call-to-session table (`sessions.rs`): each call records the peer that owns it. Accept, Reject,
  End and MediaReady count only from that peer, an Invite replaying another peer's call id is
  ignored, capacity is 32 calls per peer (a full table evicts from the peer holding most), and
  inbound Invite and MediaReady are limited to 16 per 10 s per peer before touching the table.
  MediaReady for a call no peer-owned entry exists for never reaches media. Media candidates
  from `EndpointOffer` are kept only when equal to the link's remote IP or inside the subnet
  of the local interface it arrived on (`subnet.rs`, from `if_addrs` netmasks; unknown
  interface means remote IP only).
- Trust decisions pin from the key held on the link, not from the store's 64-key cache, so
  identity churn cannot make `DecidePeerTrust` fail. `ConfigTrust::find_by_name` compares
  `name_skeleton`. Names are display data: when another key (trusted record or live link) claims
  a look-alike name, the shown name gets the short fingerprint appended (`names.rs`).
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
  to the output resampler. The estimator thresholds are scaled to the device rate. The fill
  target is `max(20 ms, 1.5 x the largest device callback seen + half a frame)` (the callback
  publishes it with a relaxed `fetch_max`), and a fill that stays more than half a callback plus
  half a frame above target for 200 ms sheds one frame at a time. Simulation only
  (`media_sink`, `media_sink_adapt`); no device was measured.
- Send gate (RT-02): a frame is sent only if the gate was open at the start of its poll and at
  the previous poll, the worker was not late by more than 20 ms, and its audio is not older than
  the last unclean poll (plus conversion and suppressor delay). A press therefore costs about
  one poll (2 ms) of onset and a stall drops its backlog rather than sending it late. Residual:
  a close and re-open of the gate inside one poll interval cannot be seen by polling; closing
  that needs an open epoch in `GateSnapshot` (relay-core), not done.
- Supervision (RT-01): both workers run under `media/supervise.rs`. A panicking step is caught,
  the worker is reset (capture: stream dropped, suppressor replaced by pass-through, domain told
  `CaptureLost`; render: the call being rendered is removed), `MediaEvent::WorkerFailed` is
  emitted and the daemon shows `MEDIA_WORKER_FAILED`. The fourth panic within 60 s is final.
  A watchdog reports a worker with no heartbeat for 3 s, but only a worker that is expected to run
  (`Supervised::active`: render has a call, a tone or the monitor; capture has a microphone stream).
  An idle worker is never judged, and a check that fires much later than its period (the process
  was frozen: phone asleep, app cached, laptop suspended) restarts every deadline instead of
  reporting. The log line `media worker is not responding` carries the worker, the silence and the
  expectation. The user text is plain (`Audio from the other side stopped. Relay restarted it.`).
  Idle workers sleep 500 ms and are woken by their command queue. The UDP send is `try_send_to`: a full socket buffer drops the packet
  and counts it (`SendStats::send_dropped`).
- Devices: nothing has to be chosen. Both directions start as `FollowDefault` (the OS default
  input and output, `AAudioBackend` on Android reports its one routed device as default);
  `SelectDevices` with an empty id returns to it, and a named id pins. The device thread
  re-scans the hardware every 2 s while idle (and at each open and select), so a new default or
  a plugged-in device reaches the device list and `WatchState` (`Device` events, plus a
  `Settings` event when the derived `*_device_name` / `*_pinned_missing` change). A pinned device
  that is absent when a stream opens is replaced by the default for that open only and
  `Settings.*_pinned_missing` says so (restoring the saved choice at startup also raises
  `SAVED_DEVICE_UNAVAILABLE`); no other device is ever picked. Streams already open are never
  restarted mid-call: a changed default applies at the next open (next call, next test, the
  playback stream reopens after tones or calls stop). A pinned device unplugged at startup stays on
  the default until the next `SelectDevices` or restart. A lost open device makes the worker drop the stream and send `CaptureLost` (or `OutputLost`); the domain closes
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
- Allocation: the audio crate, codec, jitter buffer, mixer, RTP packetization, `RenderSink` and
  now the relay-net media send/receive path (`seal_audio_into`, `try_send_audio`,
  `recv_into`) allocate nothing per packet in steady state (`relay-net/tests/media_alloc.rs`,
  `relay-audio/tests/allocations.rs`). Real cpal threads are still not counted.
- Session lifetime (review F-15): `media/rekey.rs` compares each peer's link session with the one
  its media endpoint runs on every 500 ms and, when the link was replaced (24 h hard stop and
  redial, or an election winner), calls `MediaEndpoint::switch_epoch` on the validated path;
  the remote does the same independently and a switch that times out is retried. The old epoch
  is accepted for at most the 2 s overlap. Not done: nothing starts a rekey proactively when
  `needs_rekey` turns true (it only logs); a new link needs a redial initiated by the peers
  layer. `tests/media_rekey.rs` replaces the link mid-call between two real engines.
- Early media negotiation (review F-05 b): `media/early.rs` keeps remote `Ready` messages for
  calls not yet prepared for 10 s, at most 4 per peer and 16 overall.
- Real-time (review `docs/reviews/realtime-audio.md`): the cpal `realtime` feature is on and
  the workers call `relay_audio::priority::promote_current_thread` (macOS QoS user-interactive;
  Windows/Linux through `audio_thread_priority`, without D-Bus so no rtkit); the outcome is
  logged, a denial is not an error. None of it has been measured on Windows or Linux. The
  Windows 15.6 ms timer tick is not raised, so a 2 ms capture poll may stretch there; macOS
  App Nap assertions are not taken. Not measured: any wake jitter, in either direction.
- `prepare_media` failures after admission (no link, socket bind, playback open) log and retire the
  call without a completion event; the domain's path timeout ends it.
- `resolve_address` answers from known addresses only; an unknown one starts a background dial
  and reports `Unavailable`. A discovered device has no `PeerId` until its first handshake, so
  `PeerControl::peers()` lists only peers with a known key.
- Avatar bulk-priority is by construction (biased writer); no timing test exists.
