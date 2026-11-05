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
  flag only widens where a person may point the dialer. Proptests cover that no network path
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
- Typical flow: `handshake::initiate/respond` on a TCP stream, `control_channel(stream, Arc<session>)`,
  send `new_hello(...)` first, evaluate trust, then signaling. Media: `MediaEndpoint::bind`, run `recv()`
  in one task, `bind_stream` after Accept/MediaReady, `validate_path` for each validated candidate.
- Rekey: new handshake, announce with `session::switch::announce`, `MediaEndpoint::switch_epoch` on
  both sides, then retire the old control link.
- Not included: avatar/bulk send queue prioritisation, link election for simultaneous dialling,
  persistent trust (relay-config), routed-subnet allowlist, a replay window for the control lane
  beyond strict ordering.
- Live mDNS test is opt-in: `cargo test -p relay-net --test discovery -- --ignored`.
