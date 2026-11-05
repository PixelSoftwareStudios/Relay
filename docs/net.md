# relay-net

LAN discovery, device identity and the secure peer transport. Status: implemented, **independent
security review still open**; the composition is a proposal, not an audited protocol.

## Modules

| Module | Role |
|---|---|
| `address` | LAN-only policy: private, link-local, ULA accepted; public and (by default) loopback refused; IPv4-mapped IPv6 unwrapped. Manual `ip:port` / `[v6%scope]:port` only, no DNS, no URLs. |
| `discovery` | `mdns-sd` browse/advertise of `_praxis-relay._tcp.local.`, interface policy (loopback, tunnel/VPN/container names excluded unless named), sanitized names, bounded table (256). Output is untrusted hints. |
| `identity` | X25519 static key (`Identity`), BLAKE3 `Fingerprint` (full and short forms), `TrustStore` trait and in-memory impl. |
| `handshake` | 24-byte `RLH1` preface with fresh 128-bit epoch, then `Noise_XX_25519_ChaChaPoly_BLAKE2s` with prologue `RelayPeer/v1\0 || major || epoch`. 4 KiB frames, 5 s deadline, pending-handshake gate (8). |
| `session` | `SecureSession`: `snow::StatelessTransportState`, explicit 64-bit nonces, per-lane counters, 24 h / 2^32 lifetime. `switch` validates `EpochSwitch`. |
| `record` | 32-byte public header, inner header copy, kind registry (`0x01/02/03` media lane, `0x80` control lane). |
| `control` | TCP framing (u32 length, <= 16 KiB record), envelope validation, Hello-first rule, duplicate-id handling. |
| `channel` | Named channels (see `docs/channels.md`): blinded mDNS tags, SPAKE2 join inside the Noise link, signed membership certificates, invites, attempt limiter, `ChannelStore` trait. Wired into the engine by `relay-engine::channels`; independent review open. |
| `connect` | `relay://<ip>:<port>#<64 hex fingerprint>` codec. `ConnectionString::parse` applies the LAN-only `AddressPolicy` (no DNS, no credentials/paths) and requires the fingerprint; the result is an address plus `expected`, which the caller passes to `handshake::initiate(.., Some(&expected))` so a different key is refused before our static key is sent. Pinning is only as good as the channel the string travelled over and does not set `Verified`. `encode` ranks eligible local addresses: private IPv4 on physical-looking interfaces, then ULA IPv6, then IPv4 link-local; excludes tunnel/VPN interface names, public, loopback, 100.64.0.0/10 and link-local IPv6 (its scope id is not portable). |
| `transfer` | Per-transfer data links for session-messaging file transfers (see below): `TransferHub` (listener plus dialer), `TransferToken`, `Expectation`, `DataLink` / `DataTx` / `DataRx`, `Pacer`. |
| `media` | UDP datagrams (<= 1200 B), replay window (1024), epoch overlap (<= 2 epochs, <= 2 s), path challenge/response, `MediaEndpoint`. |

## Invariants enforced here

- One cipher suite, no negotiation, no plaintext mode; preface or major mismatch fails.
- Nonce = per-lane counter from the session (media high bit 0, control high bit 1), never derived
  from RTP. Counters are never reset or reused; exhaustion stops sending.
- Header is authenticated by an inner copy compared after decrypt; lane, kind and epoch must agree.
- Receive order: length, header, epoch lookup, read-only replay check, decrypt, structure,
  call/stream binding, and only then replay commit. Failed records leave no state behind.
- Control errors are terminal for the channel; the trust store is only written by explicit calls
  to `TrustStore::put`, never by the handshake or the channel.
- Media is sent only to a path validated by an authenticated challenge from that endpoint; a new
  source address never changes the destination. A rekey validates the path under the new epoch
  before sending on it.
- Nothing here starts capture or a call: `Hello` and handshake completion are identity facts only.

## Trust labelling

`evaluate_peer` returns `Verified`, `UnverifiedFirstUse`, `New` or `KeyChanged`. UIs must show
first-use trust as unverified and never as MITM-proof; verification compares `Fingerprint::full()`.
The short form (80 bits) only tells devices apart. `Hello.public_key_sha256` carries the crate's
32-byte BLAKE3 fingerprint despite its schema name.

## Integration notes

- Address policy and 100.64.0.0/10 (RFC 6598 shared space: CGNAT, Tailscale and similar
  overlays). It is not a LAN address, since the path may cross the internet through a
  coordination service. Boundary: **user-typed only, opt-in**. `AddressPolicy::with_overlay_manual(true)`
  (default off) makes `parse_manual` and `ConnectionString::parse` accept an address the user typed
  or pasted. Discovery (`parse_resolved`), remote-offered candidates (`candidate_from_proto`),
  `check_ip`/`check_socket` (media destinations) and `connect::encode` always refuse it with
  `AddressError::OverlayNetwork`, whatever the flag, and are never advertised. With the flag off a
  manual entry fails with `AddressError::OverlayManualDisabled`, whose message tells the user how
  to enable it. A call over an overlay is still Noise-authenticated and fingerprint-pinned; the
  flag only widens where a person may point the dialer. For media, `with_overlay_peer(ip)` admits
  exactly one shared-space address in `check_ip`/`check_socket`: the remote IP of an authenticated
  control link, supplied by the engine only when the opt-in is on; nothing else changes. Proptests cover that no network path
  yields such an address.
- `SecureSession::handshake_hash()` exposes the Noise `h` (commits to prologue, epoch, both
  static keys and all handshake messages). Upper layers bind proofs to it; it is not a key.
- Discovery: `Advertisement.mode` (`Open` or `Hidden`: no name, no fingerprint hint),
  `Advertisement.extra_txt` (<= 8 short ASCII entries, core keys protected) and
  `DiscoveredPeer.txt` (bounded raw TXT, never interpreted by discovery).
- `Envelope.channel = 20` carries `ChannelMessage` (see `docs/channels.md`); the engine ignores
  bodies it does not handle, so channel messages cannot reach call state.
- Resource limits before trust: `HandshakeGate::try_enter(source_ip)` allows 2 pending handshakes
  per source (IPv4 address or IPv6 /64) of 8, evicts the oldest slot of the busiest source when
  full (`respond_gated` abandons an evicted handshake), the 24-byte preface must arrive within
  1 s, `AcceptBackoff` paces accept errors (20 ms doubling to 2 s), and `UntrustedLinks` keeps at
  most 16 authenticated-but-untrusted links (2 per source), evicting the oldest.
- Names: `sanitize_display_name` NFKC-normalises and removes every invisible, format, tag,
  private-use and excess combining character; `name_skeleton` (UTS 39 skeleton, lowercase, no
  spaces) is what `TrustStore::find_by_name` implementations must compare, so lookalikes of a
  trusted contact raise `KeyChanged`.
- Remote static keys with bit 255 set or a low-order point are rejected after the handshake
  (`identity::check_remote_key`).
- `DiscoveryTable` caps entries per source address (8), evicts the oldest of the busiest source
  when full instead of refusing newcomers, and `expire(ttl)` ages entries out.
- `candidate_from_proto` also refuses cloud metadata addresses (`169.254.169.254`,
  `fd00:ec2::254`). Manual `ip:port` parsing is strict (digits only, scope only on link-local,
  mapped literals normalised) and `check_manual_socket` is the check dial commands must use.
- Session messages: `Envelope.session_message = 21` carries `SessionMessage` (chat, acks, file offers,
  previews, transfer control, permissions). `control::session_msg::validate` checks structure on send
  and receive (text 1..=4096 bytes, inline image <= 15 KiB, preview <= 15 KiB, offer name <= 255,
  offer/preview ids 16 bytes, hash 32 bytes, size <= 4 GiB, enums in range, seq >= 1). Content policy
  (character classes, permissions, budget, per-sender limits) stays in `relay-messages` and is answered
  with `ChatAck`, never by closing the link. Unknown protobuf fields are ignored; unknown oneof members
  and enum values are refused. `ControlSender::send` now refuses an invalid envelope **without**
  poisoning the half (nothing was sealed), so a too-large message is a recoverable error.
  `ControlReceiver` applies a flood guard to session messages (`SessionRateLimit`, a
  `relay_messages::rate::TokenBucket` at 4x the per-sender 10/s burst 20); exceeding it is
  `ControlError::RateLimited` and terminal. A 16 KiB control record is why inline images and
  previews are capped at 15 KiB: larger images travel as file offers.
- Typical flow: `handshake::initiate/respond` on a TCP stream, `control_channel(stream, Arc<session>)`,
  send `new_hello(...)` first, evaluate trust, then signaling. Media: `MediaEndpoint::bind`, run `recv()`
  in one task, `bind_stream` after Accept/MediaReady, `validate_path` for each validated candidate.
- Rekey: new handshake, announce with `session::switch::announce`, `MediaEndpoint::switch_epoch` on
  both sides, then retire the old control link.
- Not included: avatar/bulk send queue prioritisation, link election for simultaneous dialling,
  persistent trust (relay-config), routed-subnet allowlist, a replay window for the control lane
  beyond strict ordering.
- Live mDNS test is opt-in: `cargo test -p relay-net --test discovery -- --ignored`.

## Per-transfer data links (`transfer`)

Each file transfer uses its own TCP connection and Noise XX handshake, so a saturated file link
cannot head-of-line block signaling on the control link and a transfer can be cancelled by
closing a socket.

**Authorisation.** The *listening* side (recommended: the downloader, which has just accepted)
calls `TransferHub::expect(Expectation { offer_id, session_id, peer, role, size }, ttl)`. That mints
a `TransferToken` (256 random bits; digest kept, token zeroized and never logged) bound to the
offer, the session and the expected peer fingerprint, valid 30 s by default (max 120 s), and
registers it (at most 16 outstanding). The engine sends `TransferControl.data_link =
DataLinkGrant { token, port, ttl_ms }` over the authenticated control link. The grant carries no
address: the dialer uses the control link's remote IP, so a peer cannot redirect the dial. The
dialer calls `TransferHub::dial(addr, &expected_fingerprint, &Ticket, role, size)`.

**Handshake.** Same pattern, preface and key checks as the control handshake but with prologue
domain `RelayData/v1\0`, so a control handshake never completes as a data link. The dialer
pins the listener key after message 2, before revealing its own key; the 68-byte ticket
(`RDT1 || session_id || offer_id || token`) is the Noise payload of message 3, encrypted. The
listener checks, in this order: caps (global link cap, per-source cap) so a refused link does not
burn the token, then the authenticated peer key equals the bound key (a stranger can neither use
nor burn the token), then session id and token digest (constant-time hash compare; 3 wrong tries
by the right peer revoke it), expiry, and finally consumes it (single use). Every refusal is a
silent close; the dialer sees `Closed` on its first receive (the initiator finishes its handshake
before the listener decides).

**Frames.** Sealed with `RecordKind::Data = 0x81` on the control lane of the data link's own session
(strict sequence, replay rejected). Tag byte then fixed fields: `Chunk` (offset u64, 1..=32 KiB)
and `Done` sender to receiver; `Credit` (u64) and `Result` (verdict 0..=3) receiver to sender;
`Cancel` both ways. The messages are `relay_messages::transfer::TransferMsg`, so the engine pumps
state-machine effects straight in and out; chunks above 32 KiB (the state machine emits 64 KiB)
are split on send. Credit stays a receiver-to-sender frame because the `Receiver` state machine
enforces its window; the draft's control-link `credit_up_to`/`done` were dropped. Each half
validates: direction, chunk offset equals next expected, never past the declared size, `Done`
only after every byte, credit monotonic and <= size, nothing after `Done`/`Result` except
`Cancel`, record length bounded before allocation. Any violation is terminal and bounded (one
frame in memory). Idle limit 30 s without a frame or write progress (`DataError::Idle`).

**Pacing and priority.** `Pacer` is a `relay_messages::rate::TokenBucket` per link (default
`BULK_RATE_BYTES_PER_SEC` = 16 MiB/s, burst max(rate/20, 64 KiB)) plus an optional hub-wide
`SharedLimit`; both can change at runtime (`DataTx::pacer().set_rate`, `TransferHub::set_global_rate`),
which is how the engine slows files during a call. Time is `tokio::time`, so tests use a paused
clock. Sends yield between frames. `Priority::Background` (default) sets DSCP CS1 via
`TcpSocket::set_tos_v4` / `set_tclass_v6` on listener and dialer sockets (accepted sockets inherit);
Linux/macOS honour the call, Windows ignores it without QoS policy, and routers or Wi-Fi may
ignore the mark. No socket2 needed. It is a hint; the pacer and separate tasks are the protection.

**Caps.** `HubConfig`: 8 established links (`DATA_LINKS_MAX`) both directions together, 4 per
source, `HandshakeGate` (8 pending, 2 per source, oldest of the busiest evicted) before any
crypto, address policy on accepted peers and dialed addresses, `AcceptBackoff`. There is no
untrusted-but-authenticated state: a connection is either unauthenticated (gate) or already
holds a redeemed token. Frame validation and `CloseHandle::close()` (also unblocks a send stuck
on the pacer or on TCP backpressure) give prompt cancellation; dropping both halves closes the
socket and frees the slots.

Measured (loopback, unpaced, 16 MiB, Noise + framing + TCP, this development Mac): about
158 MiB/s in release; the debug build is about 5 MiB/s because the crypto is unoptimised. This is
CPU/loopback, not a Wi-Fi figure.

Tests: `transfer_link` (0, 1, 32 KiB, 32 KiB+1, 3 MiB files; wrong, reused, expired, revoked tokens;
wrong peer key; pin mismatch; cancel both ways; close unblocking the pacer; link drop; link cap),
`transfer_hostile` (out-of-order, repeated, oversized, empty, past-end, early `Done`, wrong
direction, backwards or excessive credit, unknown tag, wrong record kind, tampered, replayed,
absurd length prefixes, idle timeout, proptest on arbitrary bodies), `transfer_pacing` (virtual
clock: 4 MiB at 1 MiB/s within 0.1 s of expectation, global cap, runtime rate change),
`transfer_throughput`.
