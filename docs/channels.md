# Named channels (private groups on a shared LAN)

Status: design record plus implementation in `relay-core::channel` and `relay-net::channel`.
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

`tag = keyed_blake3(tag_key, "tag" || epoch_le || instance_id)[..8]`, with
`tag_key = derive_key(domain, channel_id)` and `epoch = unix_secs / 3600`.

- Members recompute the tag for each of their channels, for epochs `e-1..=e+1`, and for the
  advertiser's instance id, and compare in constant time. Outsiders lack `channel_id`, so they
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
- Honest limit: the device is still visible on the LAN (its IP, port, mDNS responses). Hidden mode
  hides *who* and *which groups*, not that something answers. No anonymity claim.

### D4. Attempt limiting and lockout

Per admitter, sans-IO (`AttemptLimiter`, injected monotonic clock):

- per (peer fingerprint, channel): 5 failures, then lockout `60 s * 2^n`, capped at 1 h;
- per channel, all peers: 20 failures per hour, then a 15 min cool-down. Per-peer limits alone are
  bypassable because a LAN attacker can mint unlimited Noise identities; the channel cap is the
  real defence and is a deliberate DoS trade-off (an attacker can lock out legitimate joiners);
- per peer across channels: 30 failures per hour;
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
- Listed channels expose their name and an anonymous "this device is in it" hint; joining still
  follows the policy.
- Inbound policy `SharedChannelOnly` (opt-in) lets the engine drop invites from peers that share
  no proven channel. `Anyone` is the default and today's behaviour. Neither ever accepts.
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
| Channel wire message | 2 KiB |

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
2. On each discovery update, `ChannelAdvert::recognise` gives `Evidence::Candidate` per channel;
   dial and send `make_proof` (not before trust evaluation of the peer has run).
3. Incoming `ChannelMessage`: `Proof` -> `check_proof` over `store.list()`; on `proven`, note
   `Evidence::Proven`, persist `new_revocations`, answer with `make_proof(.., is_reply = true)`
   once. `JoinStart` -> `AdmissionService::begin`; `JoinConfirm` -> `AdmitSession::on_confirm`;
   `Renew` -> `AdmissionService::renew`.
4. Joiner: `Credential::{open, from_password, from_invite_secret}` (combine when both), then
   `JoinClient::{start, on_reply, on_grant}`; persist the returned `ChannelRecord`.

## Known gaps and open review items

- The independent security review gate is **open**; `spake2` is unaudited; Ed25519 and Argon2
  come from maintained crates but the composition has not been reviewed.
- Admin enrollment wire flow (see D2) and group-wide revocation guarantees are not implemented.
- Owner key loss means the channel cannot admit or revoke any more (create a new one).
- No rate limit on tag recognition work beyond the 256-entry discovery table.
- The engine does not yet advertise in hidden mode, rotate the pseudonymous instance id, or
  route channel messages.

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

`crates/relay-net/tests/channel_*.rs` plus unit tests in `relay-core::channel`: PAKE success,
wrong password, lockout, transcript binding (other session, other keys, other channel), invite
expiry/single-use/forgery, certificate forgery/expiry/revocation/delegation, tag recognition and
outsider non-linkability, visibility rules, malformed-input proptests.
