# relay-core

Sans-IO, deterministic domain for Relay: typed IDs, a single-owner state
machine, call lifecycle, transmit consent, controller and push-to-talk
leases, mute/deafen precedence, bounded queues and caches, and a
snapshot + watch contract with no gaps. No I/O, no clock reads, no Tokio.

## Shape

`State::apply(Input, MonoTime) -> Outcome { result, revision, duplicate, effects }`

- Presence and ownership (`state/owner.rs`): after `SystemEvent::TrackPresence`, `SystemEvent::Watching { client, watching }` says which clients hold an open state watch. A call is owned by the client holding its controller lease. A live call whose owner is not attached gets a grace timer (`Limits::owner_grace`, default 3 s, at most 60 s, folded into `next_deadline`); if it is still orphaned when it fires the call ends like a hangup (`EndReason::OwnerGone`: End or Decline{busy} to the peer, media stopped, capture closed, route reset). Re-attaching, or an attached client taking the lease over, keeps the call; other clients' calls are untouched; observers own nothing. A ringing incoming call has no owner and is declined once nobody at all is attached. While nobody is attached, an inbound invite is declined busy (`Reject::NotAttached`), creates no call and opens no capture. Without `TrackPresence` (tests, simple hosts) every client counts as attached and none of this applies.
- `Input`: `Client(Request)`, `Disconnected`, `Peer(PeerEvent)`, `System(SystemEvent)`, `Tick`.
- `Request { client, operation, command }`: deduplicated per `(client, operation)`.
- `Effect`: adapter work (`GateUpdate`, `ReceiveMix`, `OpenCapture`/`CloseCapture`,
  `EnsureLink`, `Peer{..}`, `PrepareMedia`/`StopMedia`). Gate updates always come first
  in an outcome, so a deny is published before anything is acknowledged.
- `Engine` wraps `State` with the bounded control queue (256). Inputs that only close
  gates (`Input::is_safety_deny`: disconnect, link down, lock/suspend, device loss,
  PTT release, mute/deafen on) bypass the queue.
- `State::snapshot()` and `State::watch(Cursor)` give snapshot + events. A watcher that
  is behind the replay window or per-client lag bound, ahead of us, or from another
  incarnation gets `Watch::Resync(snapshot)`. `Snapshot::apply` folds events and fails
  on any revision gap.

Modules: `id` (newtypes, never-wrapping counters), `limits`, `call` (aggregate, state
table, route/controls), `gate` (pure policy), `command`, `effect`, `event`, `outcome`,
`ops` (idempotency cache), `queue`, `engine`, `state/*` (owner logic), `testing`
(fakes).

## Invariants worth knowing

- Push-to-talk release delay: an ordinary key-up on an open gate keeps the lease alive
  in a *hold* until `now + delay` (`SetGlobalControls.ptt_release_delay`, default
  `Limits::ptt_release_delay` = 200 ms, hard max 2000 ms = `PTT_RELEASE_DELAY_MAX`; 0 =
  immediate). The hold is the lease's own expiry, so `next_deadline()` and `Tick` close it
  exactly at the deadline, and `CallView.releasing` says so. A new press by the source
  replaces the hold; a second key-up never extends it. Only a normal key-up holds: a
  release with `PttUpdate.immediate` (source retirement, stream closure), a key-up while
  the gate is already closed, and every fail-closed cause (disconnect, takeover/release of
  the controller, route change or withdrawal, lock/suspend, capture or device loss, link
  loss, call end) drop the lease and any hold in the same outcome. Global or per-call mute
  or deafen cancel holds without touching held keys. A press that stalls out (missed
  refresh) also cancels the call's pending holds. Invariant (property-tested): `releasing`
  implies the gate is open.
- **The transmit mode is the consent.** A call has one mode: `Muted`, `Unmuted` (open mic) or
  `PushToTalk` (`call::RouteMode`, carried by `TransmitRoute`). Choosing `Unmuted` or
  `PushToTalk` through `Command::SetTransmitMode` is the user's explicit consent to send the
  microphone to that call; `Muted` withdraws it. There is no separate consent flag, so mode
  and consent cannot disagree. Every change takes a new route generation, closes the old
  leases and publishes the gate with the new route in the same outcome, so the gate opens only
  after the change was applied and audio captured earlier is never sent (RT-02 still holds;
  `routing` and `defaults` tests). A call is muted until a mode applies; capture is opened
  only while a Connecting/Active call has a transmitting mode and the OS session allows it.
- **Default mode.** `SystemEvent::CallDefaults { mode, ptt_enabled }` (sent at start and when
  `calls.default_transmit_mode` or `ptt.enabled` change) makes each call that becomes Active
  start in that mode unless a mode was chosen explicitly since it last (re)connected
  (`Call::route_chosen`). The default is advance consent by the user's own setting, applied
  as a normal route change with a new generation. With push-to-talk disabled, choosing it is
  rejected (`Reject::PttDisabled`), a default of push-to-talk applies as `Muted` (muted is the
  safe fallback: it can never open the microphone by surprise) and calls already in
  push-to-talk are muted at once, their leases closed.
- Gate = conjunction of independent conditions (`gate::evaluate`); every deny wins.
  Global mute/deafen are stored separately from per-call state.
- Any change of controller, route generation, session lock/suspend, device loss, or
  call end/interruption drops all PTT leases. Leases expire at `now + ttl`
  (default 300 ms, hard max 500 ms); sequences per `(client, source)` must increase.
- Takeover and controller loss reset the route to muted unless it is
  background open-mic. Interruption and link loss always reset it.
- Capture generation changes on every open and every invalidation; stale completions are
  rejected. Device loss needs an explicit `ResumeCapture`; no device is auto-selected.
- Glare: lower `blake3(initiator_key || call_id)` wins; the loser becomes an alias of the
  survivor (controller moves with it); nobody is auto-accepted. `allow_parallel_same_peer`
  skips coalescing.
- Terminal calls are retained (64) so duplicate hangup/accept answers stay idempotent.

## Integrator notes

- Supply IDs (call, session, operation, client, incarnation) from an entropy adapter;
  the domain never generates them.
- Call `apply(Input::Tick, now)` at or before `State::next_deadline()`. Expiry of leases
  and timers is also evaluated at the start of every `apply`.
- Completions (`CaptureReady`, `MediaPrepared`, `MediaStopped`, ...) echo the generation or
  session they were issued under; a mismatch returns `Reject::StaleGeneration/StaleSession`.
- Media send path: drop any frame whose gate `route`/`capture` generation is older than the
  last `GateUpdate`, and check the gate immediately before packet emission.
- `Limits::validate` enforces that runtime config only tightens design ceilings.

## Not covered here

Device-budget-based admission (distinct from the call cap), capture-sample cutoffs for
no-pre-roll, peer protocol/DTO mapping, and persistence belong to other crates.

## Tests

`cargo test -p relay-core`: lifecycle/glare/admission, routing and precedence, PTT lease
traces, idempotency and bounds, watch no-gap, and proptest interleavings (privacy
invariants after every step, watch fold equals snapshot, deterministic replay).
