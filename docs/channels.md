# Named channels (private groups on a shared LAN)

Status: design record plus implementation in `relay-core::channel`, `relay-net::channel` and the
live driver `relay-engine::channels` (see "Engine driver" below).
**The independent security review gate stays open**: the composition below is a proposal, not an
audited protocol, and `spake2` itself is unaudited.

## What this is, and is not

A channel is a **named scope for discovery, visibility and call authorization** on a shared network
(dorm, office) where not everyone should see or call everyone. A device is in zero or more
channels. Members of a channel see each other and may call each other; non-members cannot see the
members of a private channel.

It is **not a conference bridge**. Calls stay independent 1:1 sessions (`CallId` is one local
device and one remote peer). A channel is metadata attached to *peers*, never to calls: no channel
type holds a `CallId`, no audio path reads channel state, and the engine must never mix one remote
party's audio into another party's stream. "Everyone in the channel" is N separate calls, each
under the ordinary per-call consent rules.

Non-negotiables inherited from the project:

- LAN only; no plaintext mode; no cipher negotiation; no accounts or cloud.
- Everything below runs **inside** the already Noise-authenticated control link. Channel proof
  never replaces fingerprint identity trust and never substitutes for `evaluate_peer`.
- Channel membership never grants microphone consent, never auto-accepts a call, never starts
  capture. The only thing it can change is whether an inbound invite may *ring* (see
  `RingDecision`, which has no accept variant).

## Concepts

| Term | Meaning |
|---|---|
| `ChannelId` | 16 bytes, `blake3::derive_key(.., owner_pub)[..16]`. Self-certifying: it pins the owner key. Treated as a **discovery secret**: knowing it lets you recognise members' tags, not join. |
| Owner | Holds the channel's Ed25519 signing key (generated at creation, stored in the `ChannelStore`). |
| Admin | A device holding an owner-signed `Delegation` for its own Ed25519 key; may admit members and revoke. |
| Member | Holds an owner/admin-signed `MembershipCert` for its **Noise static key**. |
| Policy | `listed` (name advertised), `password`, `invite_only`. Not listed means hidden. |
| Credential | What a joiner must prove to an admitter: nothing but the id (open), a password key, an invite secret, or both. |

## Decisions

### D1. Password: SPAKE2 inside Noise, not HKDF

A channel password is low entropy. Feeding it through a KDF to make a key (or a MAC) lets a passive
sniffer test guesses offline; the project rules forbid presenting that as pairing. We use a
balanced PAKE so a passive observer learns nothing testable and an active attacker gets one guess
per run, which we rate limit.

Crates evaluated (checked on crates.io, 2026-11):

| Crate | License | State | Verdict |
|---|---|---|---|
| `spake2` 0.4.0 | MIT/Apache-2.0 | RustCrypto/PAKEs, 1.6 M downloads, `forbid(unsafe)`; 0.5 pre-release is in progress; **no external audit** | **chosen**, pinned to 0.4.0; revisit 0.5 |
| `cpace` 0.1.0 | BSD-3 | one maintainer, last release 2020, ~2k downloads | rejected: stale |
| `opaque-ke` 4.1.0-pre.2 | MIT/Apache-2.0 | audited design (NCC, earlier version), actively developed, pre-release | rejected: asymmetric (needs a registered server verifier; members are peers), heavy dependency tree, pre-release |
| hand-rolled CPace/SPAKE2 | n/a | n/a | rejected: no own crypto |

Use of `spake2`:

- Ed25519 group, asymmetric `start_a` (joiner) / `start_b` (admitter).
- PAKE identities are `blake3(domain || session binding || channel id)` for both sides, so the
  exchange cannot be replayed into another session, with other keys, or for another channel.
- The crate's output key is **not trusted by itself**. We add explicit key confirmation: two
  keyed-BLAKE3 MACs (one per direction, distinct labels) over a transcript hash covering the
  binding (epoch, both static public keys, roles), channel id, invite id and both PAKE messages.
  Confirmation is sent only after the other side's is verified (B first, then A), and the grant is
  released only after both.
- The password is stretched once on the **joiner** with Argon2id (m=19 MiB, t=2, p=1, salt derived
  from the channel id) into a 32-byte `pw_key`. Admitters store only `pw_key`, so a leaked store is
  not a plaintext password (it is still a PAKE-equivalent credential; stated honestly).
- Admitters do not run Argon2 per attempt, so an attacker cannot make them burn memory.

### D2. Invite-only: capability tokens plus signed membership certificates

Evaluated:

1. **Bearer capability token redeemed at an admitter** (chosen for admission). 256-bit secret, an 8
   byte id, expiry, `uses_left`. The admitter stores a derived credential, never the token string.
   The token is used as a PAKE secret, so presenting it to a wrong or rogue peer discloses nothing.
   Single-use is enforced by the admitter's store (`consume_invite` is atomic); expiry by an
   injected clock. Revocation of an unspent token is deleting the record.
2. **Owner-signed membership certificate bound to the member's static key** (chosen for ongoing
   membership). Verifiable offline by any member who knows the owner public key; useless to anyone
   who copies it because the Noise handshake already proved the holder owns the key. Carries an
   expiry (default 14 days, max 30).
3. A shared "membership key" delivered after join (rejected): trivially cloned by a malicious
   member, no expiry, no per-member revocation.

Revocation and expiry story, honestly:

- A certificate is valid until `expires_at`. There is no online CRL. Removal = (a) the owner or an
  admin signs a `Revocation` for the subject fingerprint; revocations are exchanged in the
  `ChannelProof` message and members refuse proofs from revoked subjects; (b) renewal is refused
  by admitters; (c) the certificate lapses by itself within 30 days.
- A revoked member keeps knowing the `ChannelId` (so can still recognise tags) and keeps any
  profile data already received. A member that the revocation never reaches (offline) keeps
  being accepted by peers that also never saw it until expiry. To cut a member off with certainty,
  create a new channel (new owner key, new id) and re-invite. There is no "rekey in place".
- Delegations (admins) are not revocable before their expiry (max 30 days); keep admin counts low.
- Admission needs an admitter (owner or admin) online and reachable. In v1 the crypto and
  verification for admin delegations exist (`Authority::delegate`, chain checks), but the wire
  flow that enrols an admin (moving a delegation and the stored password credential to another
  device) is **not implemented**; today only the owner device admits. If none is, nobody new joins;
  existing members still prove membership offline of the owner.

### D3. mDNS must not leak private channels

Per channel the advertisement carries only a **blinded tag**:

`tag = keyed_blake3(tag_key, "tag" || epoch_le || instance_id || fingerprint)[..8]`, with
`tag_key = derive_key(domain, channel_id)` and `epoch = unix_secs / 3600`.

- Members recompute the tag for each of their channels, for epochs `e-1..=e+1`, the advertiser's
  instance id and each **already known device fingerprint** (the trust store), and compare in
  constant time. The match names the fingerprint the advertiser must present, and the dial pins
  it (`initiate(.., Some(&fp))`). A copied advertisement therefore only ever leads to a dial that
  fails before our static key is revealed (review F-02); a member we have not pinned is not
  recognised, so first contact between two members goes through the normal trust flow (or the
  other side dials). Dials triggered by tags must additionally be rate limited by the caller. Outsiders lack `channel_id`, so they
  cannot recognise, link, or enumerate; the tag differs per device and per epoch, so two devices'
  tags in the same channel are not even equal.
- Tags are padded with random decoys to 4 or 8 slots so the number of channels is hidden coarsely.
- A tag is **unauthenticated**: anyone who knows the id can forge one. It only says "probably a
  member"; the UI shows a tag match as `Candidate` until a `ChannelProof` succeeds on a link, and
  only `Proven` counts as shared for any decision.
- Hidden mode (`AdvertMode::Hidden`, recommended once a device joins a private channel): the mDNS
  record carries no display name and no fingerprint hint and uses a rotating pseudonymous instance
  id (`keyed_blake3(instance_secret, epoch)[..16]`). Names and profile are exchanged only over the
  authenticated link after proof. Listed channels advertise their name in clear (`chl=`), by choice.
- Channel adverts follow presence: with no client attached the daemon advertises nothing, tags included. First-run defaults are `AdvertMode::Visible` and inbound `anyone` (see `docs/engine.md`); the CLI auto-start note says so.
- Honest limit: the device is still visible on the LAN (its IP, port, mDNS responses). Hidden mode
  hides *who* and *which groups*, not that something answers. No anonymity claim.

### D6. Membership proof reveals nothing to a non-member (review F-01)

The proof is two steps. Step 1: each side sends only `ChannelRef`, a keyed hash of the session
binding and the sender's role under the channel key (16 bytes). It proves knowledge of the
channel id and reveals nothing to a peer without it. Step 2: a side sends its certificate and
revocations (`ChannelProof`) only to a peer whose reference for that channel it has verified
(`check_ref`). A pinned peer that is not in the channel never produces a matching reference, so
it never receives the channel id (which starts the certificate encoding), the owner key, a
certificate or a revocation: the whole link-up traffic towards it is opaque references.
`tests/channel_leak.rs` asserts this on the encoded messages of a full run. A peer that merely
learned the id can still obtain our certificate, which is not secret and useless without our
static key. Reflected references fail (the sender's role is part of the hash), as do references
from another link (the binding). Rejected alternative: stripping the channel id from the wire
certificate. It would hide one field but still send owner-signed material to non-members.

### D4. Attempt limiting and lockout

Per admitter, sans-IO (`AttemptLimiter`, injected monotonic clock):

- per (peer fingerprint, channel): 5 failures, then lockout `60 s * 2^n`, capped at 1 h;
- per channel, all peers: 20 failures per hour, then a 15 min cool-down. Per-peer limits alone are
  bypassable because a LAN attacker can mint unlimited Noise identities; the channel cap is the
  real defence and is a deliberate DoS trade-off (an attacker can lock out legitimate joiners);
- per peer across channels: 30 failures per hour;
- reserved lane (F-12): while the channel budget cools down, an identity with no failure on any
  channel in the last hour and no entry for the pair may still attempt, against a separate
  6-attempt budget per cool-down. Each attacker identity can use it once per hour, so 16 pinned
  attackers lock a fresh joiner out about 2 % of the time instead of 84 %; the guessing ceiling
  grows by at most 6 attempts per cool-down, and per-pair lockout, per-peer budget and Argon2
  are unchanged;
- every attempt is **charged when the exchange starts** and refunded when it completes, because a
  guesser who sees a bad confirmation just stops and would otherwise never be counted. Open
  channels (public credential) are not charged;
- uniform failure: unknown channel, unknown/expired/spent token and wrong secret all run the same
  PAKE with a dummy credential and end in one `JoinDenied` after confirmation, so there is no
  existence or validity oracle. A locked-out peer gets the same message, immediately.
- Passwords must be 8 to 128 bytes. A short password under a 20/hour channel budget is still
  guessable over days; recommend invites for anything sensitive.

### D5. Membership and visibility rules (sans-IO, `relay-core::channel`)

- Hidden channel members are visible only to peers that prove membership of a shared channel.
- Listed channels expose their name **and their id** (`chl`). Because tags are keyed by the id,
  anyone on the LAN can compute and track every device's tag for a listed channel: a listed
  channel is public by choice, not an anonymous hint. Joining still follows the policy.
- Inbound policy `SharedChannelOnly` (opt-in) lets the engine drop invites from peers that share
  no proven channel. `Nobody` drops every invite, whatever the sender proved (auto-answer cannot bypass it).
  `Anyone` is the default and today's behaviour. None of them ever accepts.
- Leaving a channel deletes its secrets and certificate from the store and stops advertising its
  tag at the next epoch refresh immediately.

## Limits

| Limit | Value |
|---|---|
| Joined channels per device | 16 |
| Advertised tag slots | 4, or 8 when more than 4 channels |
| Listed channels advertised | 3 |
| Channel name | 1..=48 Unicode scalars, sanitized like display names |
| Members issued per channel (unexpired certs tracked by an admitter) | 64 |
| Admins per channel | 4 |
| Active invites per channel | 16 |
| Invite lifetime | default 24 h, max 7 d; `uses` default 1, max 8 |
| Certificate lifetime | default 14 d, max 30 d; renewal grace 7 d |
| Revocations kept per channel | 128; `ChannelProof` carries at most 16 |
| Concurrent join exchanges per link / per admitter | 1 / 4 |
| Channel wire message (encoded, enforced by `wire::validate`) | 8 KiB |

## Threat model

Adversaries: other LAN devices (passive and active), a malicious channel member, a rogue
non-member that learned the channel name, an attacker with a copy of the local store.

- **Passive sniffer**: sees mDNS and TCP metadata. Cannot read channel traffic (inside Noise), see
  names of hidden channels, test password guesses, or link tags across devices or epochs. Can see
  that devices exist and their (hidden-mode) pseudonymous ids; can see traffic timing and sizes.
- **Active non-member**: can connect to any device and run the join exchange; each run gives one
  password guess, rate limited per D4. Cannot learn whether a channel or token exists. Cannot
  replay a recorded exchange or certificate: PAKE and confirmations are bound to the session
  binding and both static keys, certificates are bound to the subject's static key.
- **Rogue peer enumeration**: without the channel id nothing is recognisable. With the name only,
  nothing: the id is random, not derived from the name.
- **Malicious member**: sees every other member that contacts it and their names; can leak the
  channel id (enabling tag recognition, not admission); can mint nothing; can share its
  certificate uselessly; cannot be stopped from leaking data it already saw. A malicious admin can
  admit anyone for the delegation's lifetime. A member can call other members, still gated by
  consent and acceptance.
- **Store copy**: reveals `pw_key` and invite credentials (PAKE-equivalent) and, on owner
  devices, the signing key. The store implementation should live in the OS credential store or
  encrypted storage; this crate defines the trait only.
- **DoS**: bounded by the limits above; handshake gate (8 pending) remains the outer bound. A
  determined LAN attacker can lock out joiners of password channels and flood tag-bearing
  adverts.
- **Not claimed**: anonymity, deniability, forward secrecy of channel membership, protection from
  a compromised OS or same-user process, protection from a member who screenshots the list.

## Module map

- `relay-core::channel`: `ChannelId`, `ChannelName`, `Policy`, `Memberships`, `Evidence`,
  `PeerScope`, `InboundPolicy`, `RingDecision`, `visible`. No I/O, no clock reads, no crypto.
- `relay-net::channel`: `binding` (session binding), `ids` (derivations), `tag` (blinded tags +
  advert TXT), `pake` (SPAKE2 + confirmation, Argon2id stretching), `cert` (delegations,
  certificates, revocations), `invite`, `limiter`, `store` (`ChannelStore` trait with an atomic
  `update`, in-memory impl, `ChannelRecord`), `create` (`new_channel`), `join` (`JoinClient`,
  `AdmissionService`/`AdmitSession`, renewal), `proof` (`make_proof`/`check_proof`), `wire`
  (`channel.proto` messages, bounded decode).

Driver sketch for the engine's peer adapter (all calls are synchronous and cheap except the
joiner's one-time Argon2 and the store):

1. After the control link is up, `SessionBinding::for_session(&session)`.
2. On each discovery update, `ChannelAdvert::recognise(local, instance, known_fingerprints,
   epoch)` gives `Evidence::Candidate` per channel and the fingerprint to pin; dial with
   `Some(fingerprint)` only (never `None`), rate limited. On a trusted link send one
   `make_ref` per channel with a certificate.
3. Incoming `ChannelMessage`: `MemberRef` -> `check_ref`; on a match send `make_proof` for that
   channel once per link (and our own `make_ref` if not yet sent). `Proof` -> `check_proof` over
   `store.list()`; on `proven`, note `Evidence::Proven` (keep `expires_at` and demote at expiry),
   persist `new_revocations`. `JoinStart` -> `AdmissionService::begin`; `JoinConfirm` -> `AdmitSession::on_confirm`;
   `Renew` -> `AdmissionService::renew`.
4. Joiner: `Credential::{open, from_password, from_invite_secret}` (combine when both), then
   `JoinClient::{start, on_reply, on_grant}`; persist the returned `ChannelRecord`.

## Time

Certificate, invite, delegation and revocation expiry and tag epochs use the wall clock,
sampled per decision (`channel::clock::{UnixClock, SystemClock, ClampedClock, ManualClock}`).
A process-start sample plus a monotonic offset is wrong after a suspend and after a late time
sync (review F-07). Expiry is strict, `ClampedClock` never reports less than it already did so a
rolled-back clock cannot resurrect an expired certificate, and tag epochs tolerate one hour
either way. The monotonic clock remains the right source for lockout pacing only. The engine's
`Clock` (`channels/clock.rs`) reads a `ClampedClock<SystemClock>` for `unix()` and `Instant`
only for `mono()`; tests move both with `advance`.

## Known gaps and open review items

- An AI security review is in `docs/reviews/security-net-channels.md`; it does not close the
  independent human review gate. The epoch of a link is chosen by the dialler alone (F-23), so
  nothing here treats it as freshness; freshness comes from the Noise ephemerals and the PAKE.
- Only trusted (pinned) links can reach admission, which bounds who can burn the channel-wide
  attempt budget. While that budget cools down, a reserved lane (6 attempts per cool-down) admits identities with a clean record, so a few pinned attackers no longer lock out a fresh joiner; roughly 64 pinned identities still can (F-12).
- The independent security review gate is **open**; `spake2` is unaudited; Ed25519 and Argon2
  come from maintained crates but the composition has not been reviewed.
- Admin enrollment wire flow (see D2) and group-wide revocation guarantees are not implemented.
- Owner key loss means the channel cannot admit or revoke any more (create a new one).
- Tag recognition work is bounded by the discovery table (per-source and total caps, 120 s expiry), the trust store size and the 4-dials-per-10-s pace.
- Joining needs a trusted, already linked admitter (owner device). The driver does not dial
  for a join; pair first, then join. A join waits one second for a link that is coming up.
- A recognised tag makes the engine dial the advertised address, pinned to the recognised
  fingerprint (at most 2 addresses per advert, once per 30 s per fingerprint, 4 per 10 s overall).
  The link was already trusted (only known fingerprints are recognised), so the channel proof
  starts at once.
- Listed channel names that are not ASCII are not advertised (mDNS TXT is ASCII only here);
  the channel is still joinable by id.
- The driver keeps the decrypted records (owner signing key included) in process memory while
  running; they are loaded from the store, whose secrets sit in the OS credential store.
- Concurrent identical requests that race before the first finishes can both run; the operation
  ledger deduplicates retries, not parallel duplicates.
- `RelayClient::create_invite` and the desktop client do not yet read
  `CreateInviteResponse.operation`; a failed invite arrives there with an empty `invite`.

## Channels on your network

A channel is *listed* when its creator chose so (`Policy::listed`). A listed channel advertises its
name and id in clear in the mDNS record, so every device on the LAN can read them. That is the
creator's choice: an unlisted channel advertises only blinded tags. Relay shows the listed channels
of other devices in **Channels on your network** (UI), `relay channel nearby` (CLI) and
`Snapshot.nearby_channels` (API) so a name is something you can pick instead of typing.

- What is shown: the name, how many distinct devices (by advertised instance id) advertise it, and
  whether it is *ambiguous*. The id is not exposed; joining goes through `JoinChannel{name}`.
  Seeing a channel here proves nothing about it and grants nothing: joining still needs the
  password or invite the channel asks for, and the channel's admitter must accept.
- Liveness follows presence. Discovery reports a record when it appears or changes and when it is
  withdrawn (goodbye or record expiry), not repeatedly while it is unchanged. A device is
  identified by its mDNS full name; `ChannelEvent::AdvertGone` (from the discovery task in
  `peers/start.rs`) drops it at once, and a name with no device left disappears. A changed record
  (new epoch) of the same device does not count twice; a hidden device's rotated instance id is a
  new name and the old one goes with its own withdrawal. As a safety net for a missed withdrawal a
  device not heard from for 10 minutes (`LISTED_TTL`, checked by the 1 s upkeep tick) is dropped.
  Time is the channel `Clock`, so tests move it.
- Excluded: channels this device is already in.
- Ambiguity rule: a name seen with two different ids, or equal to the name of a channel this
  device is in, is shown as ambiguous and **cannot be joined by name** (`CHANNEL_DENIED`); it is
  never silently bound to the first id seen. It stays ambiguous while any device advertises it
  (anyone can therefore make a name unjoinable by name, but cannot make you join their channel).
  Join such a channel with its id (join code) or an invite. Names are not authenticated: a joinable
  name is only the one id seen so far, and a device that advertises a name before the real one can
  still be the one you reach by name, which is why the password or invite check, not the name, is
  what protects a channel.
- Bounds: at most 32 names and 64 devices per name are remembered; further names are ignored
  until old ones expire.
- Privacy mode: hearing is independent of the device's own advertising mode. A hidden device lists
  what it hears and, as before, still advertises listed names of its own channels in clear (by
  its owner's choice) while hiding its own name and fingerprint hint.

## Engine driver (`crates/relay-engine/src/channels/`)

`ChannelHub` is the one object that implements the four-step driver sketch above inside the
running engine. The daemon builds it in `assemble`, hands it to the glue task that consumes
`PeerOutput`, and to the RPC layer (`ServiceBuilder::with_channels`). Without it (fake adapters)
the channel RPCs stay `UNIMPLEMENTED`.

| Concern | Where | Behaviour |
|---|---|---|
| Trust first | `peers/manager.rs` | `PeerOutput::Channel(LinkUp)` is emitted only for a link that passed `evaluate_peer` (trusted, canonical). `Body::Channel` is routed to the driver and never to `decode`, so channel traffic cannot reach call state. |
| Proof | `channels/proofs.rs` | Two steps (D6). On `LinkUp`, and on gaining a channel, the driver sends `make_ref` for every channel it holds a certificate for. A matching `MemberRef` (`check_ref`) is answered with `make_proof` (certificate and revocations, at most once per 60 s per channel and link) and, for the first reference seen for that channel, with our own reference. A valid `Proof` records `Evidence::Proven` together with the certificate's `expires_at`; the 1 s tick demotes a proof whose certificate lapsed and, two minutes before that, asks the peer again (it answers with a renewed certificate if it has one). Signatures are checked on a stripped copy of the records (no authority keys, password credentials, issued certificates or invites), outside the state lock. |
| Join (admitter) | `channels/inbound.rs` | `JoinStart`/`JoinConfirm` run through `AdmissionService` (limiter fed with a monotonic clock). One exchange per link; a new start replaces an abandoned one, which is dropped after 15 s so it cannot hold one of the four global slots. |
| Join (joiner) | `channels/joining.rs` | Argon2 runs on a blocking thread. The joiner tries up to 8 linked devices (invite issuer first). Every failure is `CHANNEL_DENIED`; only the device's own pacing (5 failures, doubling lockout, `AttemptLimiter` keyed by local fingerprint) answers `CHANNEL_LOCKED_OUT`. |
| Renewal | `channels/proofs.rs` | A member whose certificate has under 3.5 days left asks a peer that proved the channel to renew; the answer is applied with `apply_renewal`. Authority holders refresh their own certificate on the 1 s upkeep tick. |
| Advertising | `channels/advert.rs` | `ChannelAdvert::build` per joined channel into `AdvertSpec::extra_txt`; hidden mode (from `SetPrivacy`) uses `hidden_instance_id` and rotates it per tag epoch. Rebuilt only when the epoch, mode or channel set changes. A device in no channel publishes no channel TXT at all. |
| Hints | `channels/advert.rs` | `ChannelAdvert::recognise(.., known_fingerprints, ..)` is given the trusted fingerprints (`PeerHandle::trusted`) and returns the fingerprint to pin. The engine dials `Some(fp)` only, once per fingerprint per 30 s, at most 4 dials per 10 s over all adverts, and never dials a device it already has a link to. A copied advertisement can only produce a pinned dial that fails at message 2. A hit makes the peer a `Candidate`; nothing treats a candidate as shared. Listed names are tracked in `channels/nearby.rs` (see "Channels on your network"): a second id for the same name makes it ambiguous and joining by that name is refused (use the id). |
| Ringing | `channels/screen.rs` | `InboundPolicy::decide` runs before an invite reaches the control actor. A dropped invite is answered with a plain `Decline` (indistinguishable from the user's). Only `Proven` counts. |
| Nearby listed channels | `channels/nearby.rs` | See "Channels on your network" below. `Snapshot.nearby_channels` and one `StateEvent.nearby_channels` (whole list) report them. |
| Views | `service/directory.rs` | `Snapshot.channels`, `StateEvent.channel` (new), `Peer.shared_channel_ids` (proven only) and `Removed{kind: "channel"}`. |

API behaviour:

- `CreateChannel`, `JoinChannel`, `LeaveChannel`, `RevokeChannelMember` return an `Operation`;
  failures are `FAILED` operations with `CHANNEL_DENIED`, `CHANNEL_LOCKED_OUT`, `CHANNEL_LIMIT` or
  `CHANNEL_NAME_INVALID`. Malformed input (wrong id length, a password that does not match the
  policy or is outside 8 to 128 bytes, out-of-range invite lifetime) is `INVALID_ARGUMENT`;
  a failing store is `FAILED_PRECONDITION` with a generic message.
- `CreateInvite` returns `CreateInviteResponse` whose new `operation` field is `FAILED` with
  `CHANNEL_DENIED` for a non-owner, an unknown channel and a channel without invites. That is a
  response, not a gRPC error. Revoke by a non-admitter is `CHANNEL_DENIED` the same way.
- Operations use a driver-local ledger (256 per client, 4096 total, 10 minutes). The request
  print is a keyed hash, so a stored print is no password oracle. `GetOperation` falls back to it.
- Secrets: request bodies are moved into zeroizing buffers, nothing logs them, and the only
  place an invite leaves the engine is the `CreateInviteResponse` (and its idempotent replay).
- Membership grants nothing about calls: the driver has no call ids, never touches the media
  or the gates, and `screen` can only turn a ring into a decline.

## Proposed API and wire changes (drafted, NOT wired)

### Peer wire and discovery (done in `relay-net`)

- `channel.proto` is compiled with `peer.proto`; `Envelope.channel = 20` carries a
  `ChannelMessage`, and `control::validate_body` checks it with `channel::wire::validate`.
  The engine's body matches all have wildcard arms, so channel messages are ignored there until
  the peer adapter routes them; they must never reach call state.
- `Advertisement` has `mode: AdvertMode` (`Hidden` omits name and fingerprint hint) and
  `extra_txt`; `DiscoveredPeer.txt` holds the bounded raw TXT. `ChannelAdvert::{to_txt,
  from_map}` use keys `ce` (epoch), `ct` (tags) and `chl0..chl2` (listed channels). In hidden
  mode use `tag::hidden_instance_id(secret, epoch)` as the instance id and re-advertise when the
  epoch changes.
- `SessionBinding` is now derived from `SecureSession::handshake_hash()`
  (`SessionBinding::for_session(&session)`), replacing the epoch-plus-keys construction.

### Local gRPC (`relay.engine.v1`, additive)

```proto
rpc CreateChannel(CreateChannelRequest) returns (Operation);   // name, policy, optional password
rpc JoinChannel(JoinChannelRequest) returns (Operation);       // join_code or invite, password
rpc LeaveChannel(LeaveChannelRequest) returns (Operation);     // channel_id
rpc ListChannels(ListChannelsRequest) returns (ListChannelsResponse);
rpc CreateInvite(CreateInviteRequest) returns (CreateInviteResponse); // channel_id, ttl_s, uses
rpc RevokeChannelMember(RevokeChannelMemberRequest) returns (Operation); // channel_id, fingerprint

message ChannelPolicy { bool listed = 1; bool password = 2; bool invite_only = 3; }
message CreateChannelRequest { MutationContext context = 1; string name = 2; ChannelPolicy policy = 3;
  string password = 4; /* never echoed, logged or stored in events */ }
message JoinChannelRequest { MutationContext context = 1; bytes channel_id = 2; string name = 3;
  string password = 4; string invite = 5; /* RLYI1... token */ }
message CreateInviteRequest { MutationContext context = 1; bytes channel_id = 2; uint32 ttl_seconds = 3;
  uint32 uses = 4; }
message CreateInviteResponse { string invite = 1; uint64 expires_unix = 2; }
message ChannelInfo { bytes channel_id = 1; string name = 2; ChannelPolicy policy = 3;
  string role = 4; /* owner|admin|member */ uint32 shared_peers = 5; uint64 cert_expires_unix = 6; }
// Snapshot.channels (repeated ChannelInfo); Peer.shared_channel_ids (repeated bytes);
// Settings.inbound_policy (ANYONE | SHARED_CHANNEL_ONLY); Settings.advert_mode.
// Stable problem codes: CHANNEL_DENIED (uniform for every join failure), CHANNEL_LOCKED_OUT,
// CHANNEL_LIMIT, CHANNEL_NAME_INVALID.
```

The share string for a hidden password or open channel is the join code: `RLYC1` + base32 of
`channel_id` (the id pins the owner key, so nothing else is needed). Invites use `RLYI1` +
base32 of `channel_id || token_id || secret || issuer_hint || checksum` (implemented in
`channel::invite`). `channel::invite` has the parser; the join-code codec is left to the
integrator because it is presentation only.

Passwords and invite secrets travel only in the request body on the local socket, are zeroized
after use, and appear in no event, snapshot, log or `Operation` result.

## Test map

Engine: `cargo test -p relay-engine --test channels_e2e --test channels_calls --test
channels_secrets --test channels_hardening --test peers_sessions --test peers_hardening`. Real daemons, loopback sockets, SQLite store, in-memory vault, fake audio:
invite plus password join with uniform denials, single-use invites, non-owner invite, joiner
lockout, limits, idempotent operations, revocation, `SharedChannelOnly` screening by proof,
a forged tag naming an unpinned device that causes no dial and never counts as shared, a recognised tag that re-links a restarted
member, epoch and hidden-mode re-advertising, certificate renewal, three engines in one channel
with simultaneous calls and canary tones (no cross-audio, no auto-accept, no consent), and a
captured-tracing check that no password or invite appears in logs, events, snapshots or
operation results. `channels_hardening` covers listed-name binding, demotion of an expired proof
and invite redaction; `peers_sessions` and `peers_hardening` the per-peer call table, candidate
subnet filter, untrusted-link cap, key cache churn and look-alike names (review F-05, F-09,
F-11, F-13, F-14, F-17, F-24). `channels_nearby` (`cargo test -p relay-engine --test channels_nearby`) covers the nearby list: a listed channel seen by another device, withdrawal, the safety-net expiry with the injected clock, per-device counting, joined and same-name exclusion, ambiguity, the 32-name bound, a hidden device and the event stream. UI: `relay-ui` `channels_nearby`, `nearby_state`; CLI: `relay-cli` `channel_nearby`.

`crates/relay-net/tests/channel_*.rs` plus unit tests in `relay-core::channel`: PAKE success,
wrong password, lockout, transcript binding (other session, other keys, other channel), invite
expiry/single-use/forgery, certificate forgery/expiry/revocation/delegation, tag recognition and
outsider non-linkability, visibility rules, malformed-input proptests.
