# Adversarial security review: relay-net, channels and peer manager

**This is an AI-performed review. It does NOT close the "Independent security review of peer
security and channels" gate** in `docs/gates.md` (cross-cutting release gates) or the Noise
composition gate under WP-04. That gate requires a named independent human reviewer, a stated
scope, findings and a closing revision. Treat this document as input for that reviewer, not as a
substitute.

- Revision reviewed: `main` at `859ca50` plus the uncommitted working tree present at review time.
- Method: read `docs/net.md`, `docs/channels.md`, `docs/architecture.md`, `docs/gates.md` and the
  `.codesight` map, then read the code listed below end to end and tried to break it.
- Evidence labels: **CONFIRMED** means I traced the exact code path, or wrote a throwaway program
  (kept outside the repository, not committed) that reproduced the behaviour. **PLAUSIBLE** means
  the reasoning is sound from the code but I did not execute it or it depends on platform
  behaviour I did not verify.
- Nothing under `crates/` or `apps/` was modified.

## Scope

Read: `crates/relay-net/src/{handshake,session,record,control,media,discovery,identity,address,
connect,noise,limits,channel}`, `crates/relay-protocol/src/{packet_number,seq,...}`,
`crates/relay-core/src/channel`, `crates/relay-engine/src/{peers,channels}`, plus the call sites
they feed (`media/system_calls.rs`, `media/recv.rs`, `media/net.rs`, `daemon/glue.rs`,
`relay-core/src/state/peer.rs`) and the avatar validation path in `relay-config`.

Not covered: `relay-api` / local gRPC and its auth, `relay-config` persistence and the OS
credential store, the SQLite channel store, the audio stack, the UI crates, the `fuzz/` targets
(not built or run), `snow` / `spake2` / `argon2` / `ed25519-dalek` internals (only API behaviour
that I tested), timing side channels, and any live two-host or hostile-network test. Findings
about third-party crates are limited to what I observed from their source or behaviour.

## Summary

| Severity | Count |
|---|---|
| Critical | 0 |
| High | 1 |
| Medium | 7 |
| Low | 9 |
| Info | 7 |

All counts are of distinct findings below. Every High and Medium finding is CONFIRMED; the
PLAUSIBLE items are all Low or Info and are marked.

Top five, in the order I would fix them:

1. F-01 (High): a membership proof is sent to every trusted peer and carries the channel id in
   plaintext, so any pinned non-member learns every private channel id.
2. F-02 (Medium): a replayed (not forged) advertisement tag makes members dial the attacker with
   no pinning, which reveals the fingerprint and display name of hidden-mode members.
3. F-03 (Medium): pre-authentication handshake slots (global 8, no per-source limit, 5 s hold)
   can be held by one host, which blocks all inbound links.
4. F-04 (Medium): the accept loop spins on accept errors, and the number of authenticated but
   untrusted links is unbounded, so fd exhaustion is reachable and turns into a hot loop.
5. F-05 (Medium): any trusted peer can evict the call-to-session table and fill the early
   `MediaReady` table, which breaks signaling for calls with other peers.

## Findings

### F-01 High, CONFIRMED: membership proof leaks the channel id to every trusted peer

- Where: `crates/relay-net/src/channel/cert.rs:303` (`MembershipCert::to_bytes` writes
  `self.channel` first), `crates/relay-net/src/channel/proof.rs:20-46` (`make_proof`),
  `crates/relay-engine/src/channels/inbound.rs:51` (`link_up` calls `send_proofs(peer, false,
  None)`), `crates/relay-engine/src/channels/proofs.rs:22,42` (`send_proofs`, `announce_channel`
  iterate every link).
- Failure: `docs/channels.md` treats `ChannelId` as a discovery secret and says "a party that
  merely knows the id gets nothing" and that non-members cannot recognise members. The proof
  protocol has the prover send its certificate first, unconditionally, on every trusted link,
  whether or not the peer is a member. The certificate begins with the 16-byte channel id in the
  clear (inside Noise, but to the peer). I built a channel, called `make_proof`, and found the
  16 id bytes verbatim in the encoded message.
- Exploit: a person you have pinned (TOFU, which is the normal first-contact flow) who is not in
  your private channel connects. On link-up, and again whenever you create or gain a channel, your
  engine sends them the id of every channel you hold a certificate for. They can then (a)
  recompute `blind(id, epoch, instance_id)` and recognise or track every member's advertisement,
  hidden mode included, (b) compute valid `channel_ref` values and probe admitters, and (c) mount
  the lockout in F-12 against the channel. It also defeats the "outsiders cannot link" property
  of D3 for that peer.
- Fix: prove first, reveal later. Send only the keyed `channel_ref` in the first message and put
  the certificate in a second message that is sent only after the peer's own `channel_ref` for
  that channel has verified (the reply is already `is_reply`-gated). Or strip the plaintext
  `channel` from the wire certificate and have the verifier reconstruct it from the matched
  record (the signature already covers it). Add a test that a link to a non-member produces no
  message containing the id.

### F-02 Medium, CONFIRMED: replayed tags make members dial the attacker unpinned

- Where: `crates/relay-engine/src/channels/advert.rs:88-150` (`on_advert`; the dial is at
  `:139`, `peers.dial(address, None)`), `crates/relay-net/src/channel/tag.rs:179`.
- Failure: the design accepts that a holder of the channel id can forge a tag. But a tag is
  `keyed_blake3(.., epoch || instance_id)` and the `instance_id` is itself in the advertisement,
  so an outsider with no id can passively capture a member's TXT record (`id`, `ce`, `ct`) and
  rebroadcast it from their own address. Any other member recognises the tag (I verified a
  copied TXT map is recognised by another member), and `on_advert` dials the advertised address
  with `expected = None`. A Noise XX initiator reveals its static key in message 3 and then sends
  `Hello` with its display name.
- Exploit: an outsider on the LAN learns the fingerprint and display name of devices belonging to
  a hidden channel and that they share it with the replayed device, without knowing the id. This
  breaks the hidden-mode claim that "who" and "which groups" are hidden. The 30 s redial limit
  is per `instance_id`, so the attacker repeats every 30 s. Tags stay valid for epoch +-1, about
  3 h.
- Fix: do not dial on a bare tag from an address that is not already associated with that
  instance. Options: (a) dial only addresses previously seen with a pinned fingerprint for that
  peer, (b) put an authenticated element in the advert (for example a MAC over the advertised
  address and port keyed by the channel key, which an outsider cannot reuse from another
  address), (c) require the peer to have been seen at that IP in a previous Proven session.
  At minimum, bind the tag to the advertised address and port.

### F-03 Medium, CONFIRMED: one host can starve all inbound handshakes

- Where: `crates/relay-engine/src/peers/start.rs:239,247` (`HandshakeGate::new()`,
  `try_enter`), `crates/relay-net/src/limits.rs:12,14`, `crates/relay-net/src/handshake/mod.rs`
  (`respond`).
- Failure: the gate is 8 permits, global, with no per-source accounting. A permit is held for the
  whole of `establish`, which is the 5 s handshake deadline plus the 5 s `Hello` deadline. A peer
  that connects and sends nothing holds a permit for up to the deadline. When the gate is full
  the connection is silently dropped.
- Exploit: one LAN host opens 8 sockets and reopens each as it times out (about 1.6 connections
  per second sustained). Legitimate inbound links, including a call arriving from a pinned peer,
  are dropped essentially all the time. Outbound dials are unaffected.
- Fix: cap pending handshakes per source IP (for example 2) and per /64 for IPv6, give the first
  preface bytes a short deadline (about 1 s), evict the oldest pending slot rather than refusing
  new arrivals, and log a rate-limited warning.

### F-04 Medium, CONFIRMED: accept loop hot-spins on error; untrusted links are unbounded

- Where: `crates/relay-engine/src/peers/start.rs:241` (`let Ok(..) = listener.accept().await else
  { continue; }`), `crates/relay-engine/src/peers/manager.rs:280-337` (`on_up` stores every
  authenticated link in `links` and emits `TrustRequired`, with no cap).
- Failure: (1) any `accept()` error, in particular `EMFILE`/`ENFILE`, is retried immediately
  without a sleep, which pins a core while the error persists, because the pending connection
  stays in the backlog. (2) The gate only bounds pending handshakes. After `Up` the permit is
  released, and an unlimited number of distinct Noise identities can each complete the handshake
  and `Hello`, then sit in `links` as untrusted entries kept alive by their own keepalives
  (idle limit 15 s, reset by any inbound record). Minting identities is free.
- Exploit: connect a few hundred identities, exhaust the process fd limit (soft limit 256 by
  default on macOS for many launch contexts), and the daemon spins at 100% CPU, cannot open new
  sockets, SQLite files or the media UDP socket. Each untrusted link also raises a `TRUST_REQUIRED`
  attention item to the user.
- Fix: cap total untrusted links (and per source IP), drop the oldest untrusted link when full,
  sleep with backoff on accept errors, and coalesce or cap `TRUST_REQUIRED` attention items.

### F-05 Medium, CONFIRMED: a trusted peer can break other peers' calls via two unbounded-by-peer tables

- Where: `crates/relay-engine/src/peers/signaling.rs:21,60-77,115,124,185` (`SessionTable`,
  256 entries, FIFO eviction, written by every inbound `Invite` before the domain sees it and
  before `channels.screen`), and `crates/relay-engine/src/media/system_calls.rs:42`
  (`early_ready.len() < 16`, entries only removed when a call with that id is prepared or retired).
- Failure (a): a trusted peer sends 257 `Invite` messages with random call ids. `remember` is
  called for each one in `decode`, so the oldest entries, which are the longest-running real
  calls with other peers, are evicted. Later `Accept`, `Reject` and `End` for those calls hit
  `with_session` and are silently `Ignored`: the remote's hangup never reaches the domain, and a
  remote `Accept` is lost.
- Failure (b): the same peer sends 16 `MediaReady` messages with random call ids. They are parked
  in `early_ready` and are never aged out. Afterwards a legitimate early `MediaReady` that arrives
  before the local call is prepared is dropped (`return` with no retry), and that call never
  activates.
- Exploit: a person you have pinned, or whose unverified TOFU you accepted, can degrade your
  calls with every other contact. No rate limit exists at this layer (`relay-core` caps live calls
  but only after the table write).
- Fix: key the table per peer with a per-peer cap, reject `Accept`/`Reject`/`End`/`MediaReady`
  whose recorded owner peer differs from the sender (the table already stores the peer but
  `get`/`with_session` do not compare it), expire `early_ready` entries after a few seconds and
  bound them per peer, and rate-limit inbound `Invite` per peer before touching any table.

### F-06 Medium, CONFIRMED: display-name sanitization misses invisible and lookalike characters, which defeats key-change detection

- Where: `crates/relay-net/src/discovery/advert.rs:170,186` (`sanitize_display_name`,
  `is_invisible_or_bidi`), `crates/relay-net/src/identity/trust.rs:63-79` (`evaluate_peer` keys
  `KeyChanged` on exact name equality), `crates/relay-core/src/channel/name.rs:36`
  (`ChannelName` has the same gaps and fewer entries).
- Failure: I ran the function. These survive and render blank or identical: U+3164, U+FFA0
  (Hangul fillers), U+206A..U+206F (deprecated format controls; the filter covers only
  U+2060..2064 and U+2066..2069), U+FE0F (variation selector), U+034F (combining grapheme
  joiner), U+E0041 (tag characters), U+2800 (braille blank), U+180E. A Cyrillic "e" also passes.
  Because `evaluate_peer` only warns when the claimed name equals a trusted record's name
  exactly, an attacker named `Alice` + U+3164 (or `Ali` + U+034F + `ce`, or Cyrillic) evaluates
  as `New` and shows as a stranger who looks identical to your pinned `Alice`. I confirmed
  `evaluate_peer` returns `KeyChanged` for plain `Alice` and `New` for every variant above.
- Exploit: trust-prompt spoofing. The user sees "Alice is waiting for a trust decision" and
  approves a different key. The fingerprint is still the only identity, but the UI presents a
  name the user already trusts, and the key-change alarm that exists for exactly this case does
  not fire.
- Fix: sanitize by allow-list on Unicode general category (reject `Cf`, `Mn`/`Me` runs at the
  start, `Zl`, `Zp`, `Co`, `Cn`, unassigned, tag block, variation selectors, Hangul fillers,
  braille blank), normalise to NFKC, and compare names for `KeyChanged` on a confusable skeleton
  (UTS 39) rather than raw string equality. Also show the short fingerprint beside every name
  in trust prompts. Apply the same filter to `ChannelName`.

### F-07 Medium, CONFIRMED (code path); impact is OS-dependent: the channel clock never resyncs and is suspend-blind

- Where: `crates/relay-engine/src/channels/clock.rs:29,42` (`unix()` is `base_unix` sampled once
  plus `Instant::elapsed()`).
- Failure: certificate expiry, invite expiry, delegation expiry, revocation `forget_at` and the
  hourly tag epoch all use this clock. It is read from the wall clock once at process start and
  then advances with `Instant`. On Linux (`CLOCK_MONOTONIC`) and macOS (`CLOCK_UPTIME_RAW`)
  `Instant` does not advance while the machine sleeps (PLAUSIBLE for the exact OS behaviour; I
  did not measure it, but the standard library documents that `Instant` is allowed to behave this
  way). A laptop that sleeps for a week believes a week less has passed, so certificates and
  invites outlive their expiry by the sleep time, and tag epochs drift beyond the +-1 window
  that peers accept, which also breaks recognition. A daemon started before time sync has a
  wrong base for its whole lifetime, and a rolled-back or spoofed time source at start (NTP on a
  hostile LAN) pins it there.
- Exploit: an expired membership certificate is accepted as valid, or an expired invite is
  redeemable, for as long as the verifier's accumulated sleep. A revocation window lapses late or
  never. This is a fail-open on expiry.
- Fix: use wall-clock time for certificate, invite and epoch decisions, clamped to never move
  backwards past the last observed value, and keep the monotonic clock only for lockout pacing
  (which is the stated reason for the current design). Re-read the wall clock periodically and
  log large steps.

### F-08 Medium, CONFIRMED: discovery table can be filled, which silences discovery and channel recognition

- Where: `crates/relay-net/src/discovery/table.rs:18-25` (`TableFull` at 256, no eviction),
  `crates/relay-engine/src/peers/manager.rs:156-177` (`on_discovered` returns before emitting
  `ChannelEvent::Advert` when `upsert` fails).
- Failure: records are keyed by service full name, so an attacker announcing 256 distinct
  instance names fills the table. New legitimate peers are then never listed or dialled by
  discovery, and tag recognition (which runs only for admitted records) stops. Entries leave
  only on a `Removed` event or goodbye; an attacker who keeps announcing keeps them.
- Fix: evict the least recently seen entry that has no pinned or wanted association, cap entries
  per source address, and expire entries by TTL rather than relying on explicit removal.

### F-09 Low, CONFIRMED: trust-decision key cache can be evicted by identity churn

- Where: `crates/relay-engine/src/peers/trust.rs:15,56-67` (`SEEN_KEYS_MAX = 64`),
  `manager.rs:283` (`observe` runs for every authenticated link including untrusted ones),
  `trust.rs:143-148` (`put` fails with `Unknown` if the key is not in the cache).
- Failure: a new legitimate peer connects and waits for the user. If 64 other identities connect
  before the user answers, the legitimate key is evicted and `DecidePeerTrust` fails with
  `UnknownKey`. Annoyance and a targeted denial of pairing, not a bypass.
- Fix: keep the key on the held `Established` link and pin from that, not from a global FIFO.

### F-10 Low, CONFIRMED: no contributory-behaviour or canonical-encoding check on remote static keys

- Where: `crates/relay-net/src/handshake/mod.rs:79-83` (`remote_key`), snow 0.10.0 default X25519
  resolver.
- Failure: snow's `dh` does not reject low-order points: I confirmed `dh(zero point)` returns
  `Ok` with an all-zero output. It also ignores bit 255 of the public key: I confirmed
  `dh(pub) == dh(pub | 1<<255)` while the BLAKE3 fingerprint of the two encodings differs. So (a)
  one private key can present as two fingerprints (identity multiplicity, which sidesteps a
  per-fingerprint block, ban or limiter bucket), and (b) a static key that is a low-order point
  is a "valid" identity that authenticates nobody. Neither lets an attacker impersonate an
  existing key, so this is not a break of the pinning check.
- Fix: after the handshake reject a remote static key with bit 255 set, and reject it when
  `dh(local_static, remote_static)` is all zero. Do the same check on the key stored in the
  trust file.

### F-11 Low, CONFIRMED: listed-channel names resolve to ids through an unauthenticated, last-writer-wins map

- Where: `crates/relay-engine/src/channels/advert.rs:95-103`, `joining.rs:104`
  (`listed_seen` keyed by name).
- Failure: any device can advertise a listed channel with the same name as a real one but its own
  id. The map is overwritten per advertisement, so "join by name" can resolve to the attacker's
  channel (self-certifying ids prove only who owns an id, not that the name is the one you meant).
  Joining proceeds only through a trusted linked admitter, so the attacker must be a pinned peer
  and the effect is joining the wrong channel, plus one password guess if the user types the real
  password.
- Fix: bind a listed name to the first id seen for that name and flag a conflict to the user,
  or require the id (join code) rather than the name.

### F-12 Low, CONFIRMED: attempt limiter lets pinned attackers lock legitimate joiners out; scales with identities

- Where: `crates/relay-net/src/channel/limiter.rs:110-131`, `join.rs:299-300` (charged at
  `begin`), limits in `limits.rs:49-61`.
- Measurement: I simulated one channel for 24 h with attacker identities retrying whenever the
  limiter allowed. A fresh legitimate joiner is locked out 1.0% of the time with 1 attacker
  identity, 3.1% with 2, 29.2% with 4, 47.9% with 8 and 84.4% with 16. This matches the
  documented DoS trade-off, but note two things the document does not say. Channel messages are
  accepted only on links the admitter has trusted, so each attacker identity must first be
  pinned by the owner (a real brake, and it makes the F-01 id leak the practical enabler).
  Second, the per-peer escalation throttles one identity but not many.
- Fix: document that only pinned peers can reach admission; do not charge `JoinStart` to the
  channel budget until the PAKE message is well-formed, and consider charging the channel
  budget only on a failed confirmation by an identity that already holds a prior grant or a
  verified fingerprint.

### F-13 Low, CONFIRMED: proven-membership state outlives certificate expiry

- Where: `crates/relay-engine/src/channels/state.rs:67` (`scopes`),
  `proofs.rs:79-90`, `drop_revoked` at `proofs.rs:171`.
- Failure: a peer is marked `Proven` once, when its proof is checked, and stays so until the link
  drops or a revocation is learned. There is no re-check at certificate expiry. With
  `SharedChannelOnly` screening, a member whose certificate expired days ago, or a member removed
  without the revocation reaching you, still rings. Links normally live as long as the daemon.
- Fix: store the proof's `expires_at` with the evidence and have the 1 s upkeep tick demote
  expired proofs; request a re-proof near expiry.

### F-14 Low, CONFIRMED: media candidates from a trusted peer make the engine send UDP probes to arbitrary LAN hosts

- Where: `crates/relay-net/src/address.rs:143-164` (`candidate_from_proto`),
  `media/endpoint.rs:134-161` (`validate_with`).
- Failure: any private, ULA or link-local address passes the policy, so a trusted peer can offer
  up to 8 candidates naming other LAN hosts, the gateway, or `169.254.169.254` (accepted, and
  relevant when running in a cloud VM). The engine sends one authenticated challenge datagram
  (97 bytes) to each. No amplification (the response path is rate limited and same-size),
  but it is a UDP probe primitive from a trusted peer.
- Fix: accept only candidates in the same subnet as the interface the link arrived on, or equal
  to the link's remote IP; refuse `169.254.169.254` and subnet broadcast addresses.

### F-15 Low, PLAUSIBLE: rekey and epoch switch are implemented but never invoked, so sessions hard-stop at 24 h

- Where: `crates/relay-net/src/session/mod.rs:112` (`needs_rekey`), `media/endpoint.rs:121`
  (`switch_epoch`); no call site exists in `relay-engine` or `relay-client` (grep).
- Failure: `seal` returns `Exhausted` at 24 h (`REKEY_MAX_AGE`), which terminates the control
  link; the link is redialled with a new handshake. `PeerMedia` in `media/system.rs` holds the
  session it was created with and is created once per peer, so I expect a call that spans a
  reconnect to keep sealing with the dead session (not run). Availability, not confidentiality.
- Fix: wire `needs_rekey` into the link supervisor, or document the 24 h limit and rebuild
  `PeerMedia` when the session changes.

### F-16 Low, PLAUSIBLE: CPU and store amplification on channel messages from a trusted peer

- Where: `crates/relay-engine/src/channels/proofs.rs:56-62` (`check_proof` runs while the state
  lock is held; up to 16 Ed25519 `verify_strict` per message), `join.rs:255-267` (`find` calls
  `store.list()` and clones every record, including secrets, on every `JoinStart` before the
  limiter check).
- Failure: with the channel id (see F-01) a trusted peer can flood proofs with 16 invalid
  revocations each, or flood `JoinStart` for an unknown channel, at control-lane speed. Not
  measured.
- Fix: rate-limit channel messages per link, run signature checks outside the state lock, and
  check the limiter before `store.list()`.

### F-17 Low, CONFIRMED: the invite token is retained in plaintext in the operation ledger

- Where: `crates/relay-engine/src/channels/ledger.rs:23` (`Stored.invite:
  Option<CreateInviteResponse>`, 10 minutes, `#[derive(Debug)]`).
- Failure: the bearer secret sits in an unzeroized `String` for the retention window and would
  be printed by any `{:?}` of a `Stored`. I found no such log call (the log statements in
  `channels/` and `relay-net` do not format secrets), so this is hygiene.
- Fix: zeroize on expiry, hold it as `Zeroizing<String>` and give `Stored` a redacting `Debug`.

### F-18 Info: overlay (100.64.0.0/10) manual entries pass the parser and are then refused everywhere else

- Where: `address.rs:68-84` (`check_ip` always refuses the range), `peers/manager.rs:100,121`
  and `start.rs:244` (all call `check_socket`).
- Observation: with `allow_overlay_manual`, `parse_manual` and `ConnectionString::parse` accept
  `100.64.0.1:80` (tested), but `Command::Dial`, `Command::AddAddress`, the accept loop and media
  candidates all reject it, so the typed entry silently does nothing and no call can complete.
  This fails closed, so it is not a security issue, but it contradicts the "still
  Noise-authenticated and pinned" integration note in `docs/net.md`. The boundary itself held:
  network-sourced paths (discovery, candidates, media, `encode`) never admit the range.

### F-19 Info: listed channels publish the channel id in clear

- Where: `crates/relay-net/src/channel/tag.rs:119-126`.
- Observation: a listed channel's `chl` TXT value is `hex(id):name`. Since tags are keyed by the
  id, anyone on the LAN can compute and track every device's tag for a listed channel, so the
  "anonymous hint" description in `docs/channels.md` D5 is optimistic. In Open mode the mDNS
  `id` is a per-install constant, and in hidden mode the IP address and TCP port remain stable;
  the document already concedes the latter.

### F-20 Info: channel wire size limit is not enforced on the live path

- `channel::wire::decode` (2 KiB cap, `wire.rs:26`) has no caller; `control::validate_body` calls
  only `wire::validate` (field-level bounds), so channel messages travel under the 16 KiB
  control cap. A full 16-revocation proof would exceed 2 KiB, so enforcing it as written would
  reject honest traffic. Update the document or the limit.

### F-21 Info: duplicated and unused protocol primitives

- `relay-protocol::packet_number` (`PacketNumber::nonce`, `PacketNumberSpace`, `PacketReplay`) is
  not referenced by `relay-net` or `relay-engine`; `relay-net` has its own nonce construction and
  its own `ReplayWindow`. I model-checked the `relay-net` window (below) but not the
  `relay-protocol` one. Two implementations of a security primitive invite drift; delete the
  unused one or have one depend on the other.

### F-22 Info: parser leniency in manual addresses

- `192.168.1.1:+80` and `[fe80::1%+3]:80` are accepted (Rust integer parsing), a scope id is
  accepted on non-link-local and IPv4-mapped addresses (`[fd00::1%5]`, `[::ffff:192.168.1.1%2]`),
  and an IPv4-mapped IPv6 literal is returned as an IPv6 `SocketAddr`, which will not compare
  equal to the same host's IPv4 form in `addresses.contains`. Cosmetic; normalise on parse.

### F-23 Info: epoch is chosen by the initiator alone

- `LinkEpoch` is generated by the dialler and adopted by the responder. Session keys are still
  fresh (Noise ephemerals), so nonce safety does not depend on the epoch being unique. But no
  code may treat the epoch as a freshness or uniqueness proof: a hostile dialler can reuse an
  epoch across its own links, and `MediaReceiver::receive` selects the first slot with an equal
  epoch. The link election tie-break (`election.rs`) also reads a value the dialler chose, which
  only matters between two links of the same peer.

### F-24 Info: Hello and avatar claims are display data only

- A peer can claim another trusted peer's name or avatar hash (a hash already in the cache is
  marked ready without a transfer, `profile.rs:136-144`). This is cosmetic duplication of
  display data and is the same class as F-06; the fingerprint remains the identity.

## What I checked and found sound

Handshake and session:

- One suite (`Noise_XX_25519_ChaChaPoly_BLAKE2s`), no negotiation, no plaintext mode. The
  prologue binds the domain, the protocol major and the epoch, and the preface major and zero
  flags are checked before any Noise work, so a downgrade or a stripped preface fails.
- Frame length is validated before allocation (4 KiB), payloads must be empty, the whole
  handshake runs under a 5 s deadline, and `expected` is compared after message 2 and before our
  static key is sent in message 3. The check is applied whenever an expected fingerprint is supplied. The
  engine supplies one for wanted peers and `relay://` strings; manual `ip:port` dials and
  tag-driven dials pass `None` by design (see F-02).
- Nonces: separate counters per lane, lane bit derived from the lane (cannot be chosen
  independently), counters taken with `fetch_add` and never reset, hard stop at 2^32 records
  and 24 h, `needs_rekey` at half. Direction keys are separate: I ran a record sealed by A against
  A and it failed, against B and it opened. Flipping the lane field or the nonce lane bit is
  rejected (tested).
- Receive order is as documented: length, header, epoch lookup, read-only replay check, AEAD
  open, inner-header comparison, kind/lane agreement, binding, then commit. A forged high counter
  was rejected with `Authentication` and left the window unchanged, the legitimate record was
  still accepted afterwards, and its replay was rejected (tested). `ReplayWindow` agreed with a
  reference model on 1,000,000 randomised operations including jumps, wraps over word
  boundaries and out-of-window values (0 mismatches).
- Control lane: strict increasing sequence checked before open and committed only after full
  acceptance, `Hello`-first with its digest bound to the authenticated key, duplicate message ids
  with a different body are fatal, errors are terminal, the retention map is bounded.
- Media: unbound `(call, generation)` never reaches the engine, at most 64 streams are bound,
  path validation requires an authenticated response from the challenged address under the
  challenged epoch, challenge responses are rate limited and same-size, epoch overlap is bounded
  to 2 epochs and 2 s.
- No panic or unbounded allocation found in the record, header, handshake framing, control
  validation or media parsers on hostile input. Indexing in `SecureSession::open` is guarded by
  the minimum-length check.

Addresses and connection strings (tested with about 40 inputs): IPv4-mapped loopback, public and
overlay addresses are unwrapped before classification; `::192.168.1.1`, NAT64, 6to4, site-local,
multicast, `0.0.0.0` and `::` are refused; link-local IPv6 requires a numeric non-zero scope;
leading-zero octets, URLs, credentials, paths, fragments and non-ASCII input are refused; a
multi-byte character at the scheme boundary returns an error, not a panic; the fingerprint
requires exactly 64 hex digits; `100.64.0.0/10` (including the mapped form) is refused unless the
user-typed opt-in is on, and `candidate_from_proto`, `check_ip`, `check_socket` and `encode`
refuse it regardless.

Trust: the handshake never writes the trust store; a held untrusted link is not canonical for
signaling, channels or media (`on_message` drops everything from it); `KeyChanged` rejects the
new link and does not replace the record; a verified record is not downgraded.

Channels and PAKE: the PAKE identities and transcript cover the session binding (derived from
the Noise handshake hash, which includes the epoch and both static keys), the channel id, the
invite id and both messages, with distinct labels per direction; the joiner verifies the
admitter's confirmation before revealing its own, so a rogue admitter gets one guess per run and
no offline test; unknown channel, unknown or spent invite and wrong secret run a full exchange
against a random credential; invite redemption is an atomic store update, so concurrent
redemption cannot double-spend; certificates use `verify_strict`, are domain-separated, bound to
the subject's fingerprint (a copied certificate is useless), expiry-checked against both the
certificate and the delegation, and self-certifying ids stop an admitter from substituting the
owner key (`accept_grant`); `admit` refuses revoked subjects and bounds issued certificates.
Channel messages are only processed from trusted links, are routed to the channel driver and
never to `decode`, and `RingDecision` has no accept variant.

Microphone, accept, cross-call audio: I found no path by which a remote peer can open the
transmit gate (the gate cell is created closed and is opened only by the domain snapshot),
cause capture, accept a call, or forward audio between calls. Inbound `Accept`/`Reject`/`End` map
only to domain events, `StreamState` and `MediaFeedback` are ignored, and audio is bound to
`(source, call, generation, SSRC)` per peer endpoint (`recv.rs:115-130`). The caveat is F-05:
the session table does not compare the sender with the recorded owner of a call id; call ids are
128-bit keyed-hash outputs, so I found no way to learn another call's id from the wire.

Names, avatars, logs: display names are length-bounded before and after sanitization; avatars
are accepted only as canonical 256x256 lossless WebP, size-bounded, hash-checked and fully
decoded under an allocation ceiling on a blocking thread before they are cached; no log
statement in `relay-net` or `relay-engine/src/{peers,channels}` formats a password, invite,
key or certificate (searched all `tracing` calls), and the operation print is a keyed hash.

## Items I could not verify, for the independent reviewer

- Behaviour of `std::time::Instant` across suspend on each shipped OS (F-07).
- Whether the real SQLite `ChannelStore` and the OS credential store keep the same atomicity and
  secrecy as the in-memory store the tests use.
- Whether `spake2` 0.4.0 performs small-subgroup checks on the peer element in the way the
  composition assumes (I relied on its API contract, not its internals).
- Coverage-guided fuzzing: the `fuzz/` targets were not built or run by me.
- Any interaction with the local gRPC surface, which is outside this scope.
