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
| `channel` | Named channels (see `docs/channels.md`): blinded mDNS tags, SPAKE2 join inside the Noise link, signed membership certificates, invites, attempt limiter, `ChannelStore` trait. Not wired into discovery or the engine yet; independent review open. |
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

- Typical flow: `handshake::initiate/respond` on a TCP stream, `control_channel(stream, Arc<session>)`,
  send `new_hello(...)` first, evaluate trust, then signaling. Media: `MediaEndpoint::bind`, run `recv()`
  in one task, `bind_stream` after Accept/MediaReady, `validate_path` for each validated candidate.
- Rekey: new handshake, announce with `session::switch::announce`, `MediaEndpoint::switch_epoch` on
  both sides, then retire the old control link.
- Not included: avatar/bulk send queue prioritisation, link election for simultaneous dialling,
  persistent trust (relay-config), routed-subnet allowlist, a replay window for the control lane
  beyond strict ordering.
- Live mDNS test is opt-in: `cargo test -p relay-net --test discovery -- --ignored`.
