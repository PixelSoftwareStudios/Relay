# Dependency and reinvention audit

Scope: read-only sweep of `crates/*`, `apps/*`, `benches/`, `fuzz/` (about 97k lines of Rust) against the directive "use maintained libraries unless a from-scratch version is faster or domain-specific". Method: read `docs/architecture.md` and `.codesight`, grep for hand-rolled patterns, read the hits, run `cargo info`, `cargo tree`, `cargo machete` and check `deny.toml` and the local RustSec database. Versions and licenses below come from `cargo info` (the local index; it does not print release dates, so freshness is judged from the version gap and advisories, not dates). Nothing under `crates/`, `apps/`, `benches/` or any `Cargo.toml` was changed.

Constraints used to judge fit: no allocation, lock or logging on audio callbacks; injectable clocks for deterministic tests; `relay-core`, `relay-protocol` and `relay-messages` stay sans-IO; cargo-deny allows only permissive licenses (Apache-2.0, MIT, BSD-2/3, ISC, Zlib, 0BSD, CC0, BSL-1.0, Unicode-3.0, Unlicense); no networked features.

## 1. Summary

| # | Candidate | Verdict | Lines removed (est.) | New crates |
|---|---|---|---|---|
| 1 | `subtle` for the three hand-rolled constant-time compares | REPLACE | ~12 | 0 (already in tree) |
| 2 | std `IpAddr`/`SocketAddr` parse in the diagnostics redactor (also fixes a leak) | REPLACE | ~12 | 0 |
| 3 | `base64` for the export encoder/decoder | REPLACE | ~45 | 0 (0.22.1 already in tree via tonic) |
| 4 | `hex` for ~10 hand-rolled hex encode/decode helpers | REPLACE | ~70 | 0 (already compiled elsewhere) |
| 5 | std `str::floor_char_boundary` for five char-boundary loops | REPLACE | ~25 | 0 |
| 6 | One `ReplayWindow` instead of two | REPLACE (internal) | ~90 | 0 |
| 7 | One counting allocator for tests and benches | REPLACE (internal) | ~110 | 0 |
| 8 | `backon` (not `backoff`) for the approved backoff swap | CHANGE PLAN | n/a | see 3.1 |
| 9 | Drop unused dependencies (machete) | REPLACE | 0 code, 9 manifest lines | 0 (removes 9 manifest entries) |
| 10 | `parking_lot` (or one shared lock helper) for poison-ignoring `lock()` boilerplate | MAYBE | ~150 | +4 small |
| 11 | `ipnet` for `Subnet` | MAYBE | ~35 | +1 |
| 12 | Collapse `uuid::new_v4()` used only as random bytes into `relay-io` entropy | MAYBE | ~0, drops `uuid` from 4 crates | -1 per crate |
| 13 | `clap_complete` + `clap_mangen` (feature gap) | MAYBE | adds ~30 | +2 |
| 14 | `notify-rust`, `auto-launch`, `single-instance` (feature gaps) | MAYBE | adds | see 6 |

Ranking rule: security-sensitive parsers and comparators first, then deleted lines, then dependency cost.

Overall impression: the codebase already uses libraries well (snow, ed25519-dalek, argon2, blake3, rustix, fs4, directories, toml, rusqlite, clap, ratatui, comfy-table, rtrb, rubato, cpal, mdns-sd, `global_hotkey::HotKey::from_str`). The reinvention is concentrated in small helpers that were copied between crates. Most larger hand-rolled pieces (RTP header, replay window, jitter, token bucket, lockout limiter, transfer framing, image header inspection, sanitizers, counting allocator) are domain-specific and are verdict KEEP-OURS below, with reasons.

## 2. REPLACE, ranked

### 2.1 `subtle` for constant-time comparison (security, REPLACE)

Where: three independent open-coded implementations.
- `crates/relay-net/src/channel/ids.rs:39` `ct_eq` (4 lines), used by `pake.rs:195`, `proof.rs:47`, `tag.rs:207`, `join.rs:262`.
- `crates/relay-api/src/middleware.rs:162` `constant_time_eq` (7 lines), compares the bearer token of the loopback TCP transport.
- `crates/relay-config/src/identity/secret.rs:30` `impl PartialEq for Secret32` (7 lines, inline fold).
- Related: `crates/relay-net/src/transfer/token.rs:46` compares blake3 digests; `blake3::Hash`'s `PartialEq` is already constant-time, so that one is fine.

Problem: the `fold(0, |a, (x, y)| a | (x ^ y)) == 0` idiom has no optimization barrier, so the compiler is free to short-circuit it. `subtle` uses `black_box`-style barriers and is the audited default. The three copies also disagree on length handling (one returns early on length mismatch, one XORs lengths, one `zip`s and silently ignores a length difference, which would compare a prefix as equal if lengths differed; `Secret32` is fixed-size so it is safe in practice).

Replacement: `subtle::ConstantTimeEq` (`a.ct_eq(b).into()`), one helper in `relay-net`, and `Secret32::eq` via `ct_eq`.

- License: BSD-3-Clause (in the `deny.toml` allow list). Version 2.6.1.
- Weight: zero. It is already in the normal tree (`snow` -> `aes-gcm` -> `subtle`, also `argon2`, `ed25519-dalek`).
- Unsafe: none of its own surface that we touch; it is `#![no_std]`.
- Fit: no allocation, sans-IO safe. `relay-api` and `relay-config` gain one direct dependency each.
- Verdict: REPLACE. Cost is minimal, benefit is a hardening of every secret comparison.

### 2.2 Diagnostics redactor IP detection is wrong; use std parsing (security bug, REPLACE)

Where: `crates/relay-config/src/diagnostics/redact.rs:27` `looks_like_ip` (about 12 lines) plus `is_hex_run`/`looks_like_path` helpers.

Problem found by reading, not by running: IPv4 is a hand split on `.`; IPv6 is "three or more colons and only hex digits and colons". That misses compressed addresses with fewer than three colons, which are the common LAN forms: `fe80::1`, `fd00::1`, `::1`, `fd12::5`. It also never matches bracketed `[fe80::1]:7000` or `%scope` forms. A support bundle is exactly where a user's LAN addresses must not leak ("when unsure, redact" is the stated rule).

Replacement: strip surrounding brackets and a `%scope` suffix, then `s.parse::<IpAddr>().is_ok() || s.parse::<SocketAddr>().is_ok()`. No crate needed.

- Cost: zero dependencies; the std parser is strict and well tested.
- Verdict: REPLACE (and add a unit test for `fe80::1`, `[fd00::1]:7000`, `::1`).

### 2.3 `base64` crate for export (REPLACE)

Where: `crates/relay-messages/src/store/base64.rs`, 45 lines, used by `store/export.rs:149,188` for the session export JSON (`data_base64`). It is also an untrusted-input parser: imports decode attacker-influenced files.

Problem: the decoder looks up each character with `ALPHABET.iter().position(...)` (O(64) per char, and not constant time, which does not matter here), accepts non-canonical trailing bits, and is a bespoke parser of untrusted text.

Replacement: `base64::engine::general_purpose::STANDARD.encode/decode` (padding required by default; `with_decode_allow_trailing_bits(false)` is the default, so canonical input is enforced).

- License: MIT OR Apache-2.0. Version 0.22.1 is already in `Cargo.lock` (tonic uses it); `cargo info` shows 0.23.1 as latest, but pick 0.22.1 to add zero new crates now. `RUSTSEC-2017-0004` is old and patched.
- Sans-IO: `base64` is `no_std` + `alloc`, fine for `relay-messages`.
- Lines removed: about 45.
- Verdict: REPLACE.

### 2.4 `hex` crate in place of about ten hand-rolled helpers (REPLACE)

`hex 0.4.3` (MIT OR Apache-2.0) is already used by `relay-api`, `relay-config`, `relay-engine`, `relay-ui`, `relay-cli`. Other crates re-implement it:

| File | Helper | Lines |
|---|---|---|
| `crates/relay-net/src/channel/tag.rs:229-241` | `hex`, `unhex<N>` | ~14 |
| `crates/relay-net/src/discovery/advert.rs:40-57, 95, 118` | `to_hex`, `from_hex`, `decode_hex<N>`, inline `format!("{b:02x}")` | ~25 |
| `crates/relay-net/src/connect.rs:94` | inline collect | 5 |
| `crates/relay-net/src/identity/fingerprint.rs:51-76` | `parse` digit loop, `group_hex` (uppercase grouped) | ~25 (keep the grouping, use `hex::encode_upper` + chunks) |
| `crates/relay-messages/src/message.rs:19-34` | `hex` | 3 |
| `crates/relay-messages/src/store/export.rs:195` | inline | 1 |
| `crates/relay-core/src/id.rs:113`, `channel/mod.rs:47` | `Display` with `{byte:02x}` | keep (no allocation, no_std-friendly) |
| `benches/audio/neural/model.rs:17`, `benches/audio/real.rs:63` | `hex`, `sha256_hex` | 6 |
| `crates/relay-config/src/diagnostics/redact.rs:14` | `is_hex_run` | keep (a predicate) |

Also note `format!("{b:02x}")` per byte allocates a String per byte; `hex::encode` allocates once. `hex::decode_to_slice` into a `[u8; N]` replaces the const-generic `unhex`/`decode_hex` helpers and enforces length and ASCII.

- Weight: zero new crates; adds `hex` as a direct dependency of `relay-net`, `relay-messages` (and benches).
- Fit: `hex` is `no_std` + `alloc`; fine for sans-IO.
- Lines removed: about 70, plus a class of off-by-one risks (two copies of `text[i*2..i*2+2]` slicing that rely on a prior `is_ascii` check).
- Verdict: REPLACE. Keep the `Display` impls in `relay-core` that write directly to the formatter.

### 2.5 std `str::floor_char_boundary` (REPLACE)

Five copies of "walk `end` back until `is_char_boundary`":
- `crates/relay-messages/src/sanitize.rs:97` `truncate_chars`
- `crates/relay-messages/src/markdown/blocks.rs:43`
- `crates/relay-net/src/discovery/names.rs:79` `truncate`
- `crates/relay-net/src/discovery/advert.rs:168` `truncate` (identical to the previous)

`str::floor_char_boundary` is stable on the pinned toolchain (verified by compiling with rustc 1.99.1). Each helper becomes `s[..s.floor_char_boundary(max)].to_owned()`. About 25 lines removed, zero dependencies. Verdict: REPLACE. Note: neither the old nor the new truncation is grapheme-aware, so a display name cut at the byte limit can end in a combining mark or inside a ZWJ emoji; `unicode-segmentation` (1.13.3, MIT OR Apache-2.0, already in the tree through comfy-table) would fix that but is cosmetic and the names are re-sanitized, so it stays a note, not a recommendation.

### 2.6 Two replay windows (internal duplicate, REPLACE)

- `crates/relay-protocol/src/seq/replay.rs` (109 lines): generic `ReplayWindow<const WORDS>` with `check`/`accept`.
- `crates/relay-net/src/media/replay.rs` (98 lines): the same algorithm fixed to `MEDIA_REPLAY_WINDOW`, with `check`/`commit`.

`relay-net` does not depend on `relay-protocol` today (checked in `Cargo.toml`), which is probably why the copy exists. No crate fits: the semantics (read-only `check`, `commit` only after authentication, `TooOld`/`Duplicate`/`Fresh`) are domain-specific and a bitset crate (`bitvec`, `bit-set`) would not give the sliding shift for free and adds allocation. Verdict on a crate: KEEP-OURS. But keep one copy: move it to a tiny pure module both can use (for example `relay-core` or `relay-protocol`) and delete the other. About 90 lines removed.

### 2.7 One counting allocator (internal duplicate, REPLACE)

`crates/relay-audio/tests/common/counting.rs` (58 lines) and `crates/relay-net/tests/counting/mod.rs` (58 lines) are byte-identical; `benches/tests/neural_allocs.rs` (83 lines) is a third variant. Crate candidates: `stats_alloc` 0.1.10 (MIT) and `dhat` 0.3.3 (MIT OR Apache-2.0). Neither fits: ours counts per thread with a thread-local arm flag, which is what makes "zero allocations on the audio callback while other test threads run" assertable; `stats_alloc` counts process-wide and `dhat` is a heap profiler with a global session. Verdict on a crate: KEEP-OURS. Share one file (`#[path]` include or a `publish = false` test-support crate) and delete two copies. About 110 lines removed.

### 2.8 Unused dependencies (cargo-machete 0.9.2, `--with-metadata`)

Confirmed by grepping the source:

| Crate | Unused | Verdict |
|---|---|---|
| `relay-engine` | `anyhow`, `bytes` | remove (no `anyhow`/`bytes` references in `crates/relay-engine`) |
| `apps/relay-cli` | `tracing`, `tracing-subscriber` | remove (no `tracing` references in `apps/relay-cli/src`) |
| `relay-protocol` | `criterion` (dev) | remove (benches live in `benches/`) |

False positives (do not remove): `relay-api` `tonic-prost`, `protoc-bin-vendored`, `tonic-prost-build`, and `relay-net` `protoc-bin-vendored`, `tonic-prost-build`. They are used by `build.rs` and by generated code; machete does not see that.

## 3. Changes to the already-approved swaps

### 3.1 `backoff` is unmaintained: use `backon`

`backoff 0.4.0` (MIT/Apache-2.0) carries `RUSTSEC-2025-0012` (informational, "unmaintained", dated 2025-03-04), which recommends `backon`. `deny.toml` does not list that advisory in `ignore`, and `[advisories] version = 2` treats unmaintained crates as errors for workspace dependencies, so adding `backoff` would fail `cargo deny check`. Alternatives from `cargo info`: `backon 1.6.0` (Apache-2.0, async-friendly retry builder) and `exponential-backoff 2.1.1` (MIT OR Apache-2.0, a plain iterator of delays with jitter, no async runtime, no retry executor).

Recommendation: our call sites only need the delay sequence (`peers/backoff.rs` 40 lines, `handshake/backoff.rs` 30 lines, `relay-ui/supervisor.rs:21`, `relay-ui/autostart.rs:149`, plus the fixed-retry reconnect loops in `apps/relay-cli/src/client/feed.rs:78,124`, which are not in the approved list but belong with it). `exponential-backoff` fits a delay-iterator use best and keeps the injectable-clock rule (it never sleeps). `backon` fits if we want its retry combinators. Either is acceptable; `backoff` is not.

### 3.2 `governor` is heavier than the code it replaces

Status: not yet added. `governor 0.10.4` (MIT) default features are `std`, `dashmap`, `jitter`, `quanta`; `std` pulls `futures-timer`, `futures-util`, `futures-sink`, `parking_lot`. Our consolidated target is `relay-messages::rate::TokenBucket` (47 lines, integer millisecond clock, `try_take(n)`, `available`). The transfer pacer (`relay-net/src/transfer/pacer.rs`) needs variable-cost takes, a live-changeable rate (`SharedLimit::set_rate`) and `tokio::time::Instant` for paused-clock tests; governor's `Quota` is immutable and its `Clock` trait would need a custom adapter. The `AttemptLimiter` in `relay-net/src/channel/limiter.rs` is a lockout policy (windows, strikes, cooldown), not a rate limiter, so governor's keyed limiter does not model it. Recommendation: consolidate on the single in-house `TokenBucket` (delete `pacer.rs`'s `Limiter` wrapper duplication only where it overlaps), and use governor only if a per-IP keyed limiter is ever needed (`default-features = false` plus `dashmap`).

### 3.3 Confirmed status of the other approved swaps

- pulldown-cmark, linkify, idna: already imported by `relay-messages/src/markdown/{blocks,inline,url}.rs`. Done.
- `biquad` for the voice high-pass: imported in `relay-audio/src/dsp/filters.rs`. Done.
- `audio_thread_priority`: imported in `relay-audio/src/priority/mod.rs`. Done (the working tree has `priority/windows.rs` and `priority/macos.rs` still carrying 13 `unsafe` sites; those are the remaining footprint if the shim is partly kept).
- `rusqlite_migration` (2.6.0, Apache-2.0, rust-version 1.95): not yet used; `relay-config/src/db/migrate.rs` is 87 lines. Compatible with rust 1.99.
- `atomic-write-file` (0.3.1, BSD-3-Clause) or `tempfile::persist`: not yet used; `relay-io/src/fs/atomic.rs` 128 lines and `fs/temp.rs` 81 lines. `tempfile 3.27.0` is already a workspace dependency and already in the tree; `NamedTempFile::persist` would reuse it. Note the architecture rule that `relay-io` owns the fsync + directory fsync sequence: check that `persist` does the directory fsync (it does not), so a wrapper stays either way.

## 4. MAYBE

### 4.1 Poison-ignoring `lock()` boilerplate: `parking_lot`

Where: 91 sites of `.lock().unwrap_or_else(|e| e.into_inner())` across 38 files, plus about a dozen per-type `fn lock(&self) -> MutexGuard` helpers (`relay-net/src/media/endpoint.rs:110`, `channel/store.rs:204`, `relay-engine/src/media/gate.rs:153`, `media/live.rs:42`, `service/presence.rs:36`, `service/clients.rs:76`, `relay-ui/src/autostart.rs:78`, and others). It is the right policy (a panicked worker must not wedge the daemon), but it is repeated 91 times.

Options: `parking_lot::Mutex` (no poisoning; 0.12.5 in the lock file but only on the GUI/other targets, not in the daemon tree, so it adds `parking_lot`, `parking_lot_core`, `lock_api`, `scopeguard`, `smallvec`) or one tiny workspace helper (a `LockExt` trait on `std::sync::Mutex` in `relay-io` or `relay-core`). `std::sync::nonpoison::Mutex` is still unstable on 1.99.1 (compile test: E0658). Fit: not used on audio callbacks (the architecture forbids locks there), so either choice is neutral for real-time. Verdict: MAYBE. A shared extension trait removes the repetition with zero dependencies; take `parking_lot` only if the faster uncontended lock is wanted.

### 4.2 `ipnet` for `Subnet`

Where: `crates/relay-engine/src/peers/subnet.rs`, 59 lines (`same_prefix` bit twiddling, netmask-to-prefix via `leading_ones`). `ipnet 2.12.2` (MIT OR Apache-2.0) gives `IpNet::new`, `contains`, `with_netmask`. It is not in the daemon's normal tree today (a `cargo tree -i ipnet` finds nothing), so it adds one dependency-free crate. About 35 lines removed. The existing code is correct and tested; verdict MAYBE (low value). Do not adopt `ipnet` for the address policy: `is_lan_ip` (`relay-net/src/address.rs:222`) already uses `Ipv4Addr::is_private`/`is_link_local`, and the extra checks (100.64/10 overlay space, the cloud metadata address, IPv4-mapped IPv6, `fe80::/10`, `fc00::/7`) are policy, not generic classification. `Ipv6Addr::is_unique_local` and `is_unicast_link_local` are stable in std on this toolchain and would replace the two bit masks in `is_lan_ip`/`is_v6_link_local` (3 lines); worth doing with the touch.

### 4.3 `uuid` used as a random-bytes source

Six production call sites use `Uuid::new_v4().as_bytes()` only to get 16 random bytes: `relay-cli/src/client/real.rs:111`, `relay-platform/src/ptt/driver.rs:52`, `relay-api/src/client_ext.rs:27`, `relay-api/src/registry.rs:28,45`, `relay-client/src/wire.rs:37`. `relay-api/src/tcp.rs:73` builds the loopback TCP bearer token by concatenating two UUIDv4 strings. The architecture says OS entropy belongs in `relay-io` (`entropy.rs`); these sites bypass that rule. Replacing them with `relay-io::entropy` (or `rand::random::<[u8; 16]>()`, which `rand 0.10` already provides) lets `uuid` (with the `serde` feature) leave `relay-api`, `relay-client`, `relay-platform`, `relay-ui` and `relay-cli`. For the bearer token use 32 random bytes hex-encoded, which is cleaner than two UUIDs. Value is hygiene, not lines. MAYBE.

### 4.4 `secrecy` and the three `Secret` types

There are three redacting secret wrappers: `relay-client/src/command.rs:11` `Secret(String)` (Debug redacted, **no zeroize on drop**), `apps/relay-cli/src/secret.rs:16` `Secret(String)` (redacted, zeroized on drop), and `relay-config/src/identity/secret.rs:9` `Secret32(Zeroizing<[u8; 32]>)`. `secrecy 0.10.3` (Apache-2.0 OR MIT) provides `SecretString`/`SecretBox` with redacted Debug and zeroize-on-drop, and `zeroize` is already a dependency. Why it is only MAYBE: `Command` derives `Clone` and `PartialEq` and `OperationCache` compares commands for exactly-once replay (`relay-core/src/ops.rs`); `secrecy` deliberately omits `PartialEq` and requires an opt-in for `Clone`. And every `expose().to_owned()` (`wire.rs:256-270`, `manage.rs:167-190`) copies the secret into a plain `String` that goes into a prost message, which no wrapper can zeroize.

Recommendation: do not add a crate; instead (a) delete the duplicate by making the CLI use `relay_client::Secret` (the CLI already depends on `relay-client`), and (b) give that one type the zeroize-on-drop and a constant-time `PartialEq` (via `subtle`). That closes a real inconsistency (one wrapper wipes, the other does not).

### 4.5 `lru` for the avatar texture cache

`crates/relay-ui/src/avatar.rs:58` `TextureCache<T>` is a hand-written LRU (`used` counter, scan to evict) with extra `Pending`/`Missing`/`Ready` states and a pure-model design so it tests without egui. `lru 0.18.5` (MIT) is already in the tree (via ratatui) and has advisories `RUSTSEC-2026-0002` (patched 0.16.3) and `RUSTSEC-2026-0253` (patched 0.18.2), both satisfied by 0.18.5. The scan is O(n) over a small capacity; swapping would save about 25 lines and keep the states. MAYBE, low value.

### 4.6 Image header inspection (`relay-messages/src/image.rs`, 309 lines)

Header-only PNG/JPEG/GIF/WebP inspection that also counts animation frames and enforces pixel and decoded-bytes limits before any decoder runs. `imagesize 0.15.0` (MIT) returns dimensions only, no frame count or APNG/animated-WebP awareness, and the policy checks would remain ours. Verdict: KEEP-OURS (security policy parser, covered by `fuzz` targets and proptests). The same holds for `relay-config/src/avatar/webp.rs` (83 lines), which checks the canonical RIFF/VP8L form byte for byte.

### 4.7 macOS Core Foundation FFI in `relay-platform`

`crates/relay-platform/src/session/macos.rs` (79 lines, 250 ms polling loop over `CGSessionCopyCurrentDictionary`) and `hotkey/macos.rs` (82 lines: `AXIsProcessTrusted`, `CGPreflightListenEventAccess`, `CFRunLoopRunInMode`, `pthread_main_np`) hand-declare `extern "C"` bindings with about ten `unsafe` blocks. Candidates: `objc2-core-foundation 0.3.2` (Zlib OR Apache-2.0 OR MIT; already in the lock file through the GUI stack, so no new crate on macOS) for the `CFString`/`CFDictionary`/`CFBoolean` handling, and `macos-accessibility-client 0.0.2` (Apache-2.0, tiny) for the Accessibility check. This would remove the manual `CFRelease` bookkeeping in `screen_locked`. Not a clear win: the permission query for Input Monitoring is not covered by a maintained crate, and the lock check could move to a distributed notification instead of polling only through `objc2-foundation`. Verdict: MAYBE (reduces unsafe count; modest).

## 5. KEEP-OURS (checked, with reasons)

| Area | Where | Why we keep it |
|---|---|---|
| Length-prefixed handshake framing | `relay-net/src/handshake/framing.rs` (31 lines) | `tokio_util::codec::LengthDelimitedCodec` (`tokio-util 0.7.20`, MIT, already in the tree) needs a `Framed` adapter and its own max-length config for a 4-byte prefix with a zero-length reject. The hand version validates length before allocating, is 20 lines of logic, and is fuzzed. Not worth the indirection. |
| Transfer data frames | `relay-net/src/transfer/frame.rs` (124 lines) | Tag byte + fixed fields, not a stream codec; `bytes::Buf` would not shorten it. |
| RTP header / sequence extension | `relay-protocol/src/rtp/header.rs` | Cross-checked in tests against `rtp` and `webrtc-util`; deliberately no_std-ish and fuzzed. |
| Jitter statistics | `relay-protocol/src/jitter/*` | RFC 3550 estimator, domain-specific (stated as fine). |
| Lockout limiter | `relay-net/src/channel/limiter.rs` | Windows + strikes + cooldown, not a rate limiter. |
| Operation cache | `relay-core/src/ops.rs` | Exactly-once replay with per-client and global bounds. |
| Early-ready table | `relay-engine/src/media/early.rs` | 16-entry TTL map with per-peer caps; `moka`/`lru` would not express the per-peer bound. |
| Sample conversion | `relay-audio/src/convert/sample.rs` (74 lines) | `dasp_sample 0.11.0` is in the lock file but has no NaN sanitization; our `clean()` is a deliberate guard (see the AGC NaN fix in git history). |
| RMS/dBFS meter | `relay-engine/src/media/meters.rs` | 8 lines of math on every 10 ms frame; `ebur128` is a LUFS meter for mastering, far heavier. |
| Speaking indicator | `relay-engine/src/media/recv.rs` + `meters.rs` | Energy threshold plus 300 ms hold on already decoded audio. `webrtc-vad`/`earshot` are options only if false positives show up. `sonora` (already an evaluation dependency) also contains a VAD. |
| Limiter / mixer | `relay-audio/src/mixer/*` | Real-time, allocation-free; no crate is tuned for this. |
| Rotating log | `relay-io/src/proc.rs:44` (`open_rotating_log`) | Rotates once at spawn for a detached child whose stdout is redirected to the file. `tracing-appender` rotates inside the process by time and cannot take over a redirected fd; it would be a different feature. |
| Log/runtime directories | `relay-io/src/paths.rs` | `directories::ProjectDirs` has no log dir and no macOS runtime dir; ours is injectable for tests. |
| Preferences get/set | `relay-config/src/prefs/flat.rs` (130 lines) | Already built on `toml`; `figment`/`config` solve layered sources we do not have. One small cut: the "unknown setting" check in `portable.rs` could be `#[serde(deny_unknown_fields)]`, about 8 lines. |
| Hotkey parsing | `relay-platform/src/hotkey/global.rs:69` | Already `global_hotkey::HotKey::from_str`. |
| Table output | `apps/relay-cli/src/commands/table.rs` | Already `comfy-table`; TTY via `std::io::IsTerminal`, `NO_COLOR` read by hand (5 lines); `anstream` or `supports-color` would be heavier than the check. |
| Signals | `relay-engine/src/daemon/mod.rs:246` | `tokio::signal` is enough for SIGINT/SIGTERM; `signal-hook` adds nothing. |
| Device polling (1 s) | `relay-engine/src/daemon/glue.rs:17` | `cpal 0.18.2` has no device add/remove event API (grep of its source finds only `ErrorKind::DeviceChanged` for the default device). Polling is the only portable option today. |
| `async-trait` | `relay-ui/src/engine.rs` | Used for `dyn` traits (`Arc<dyn Operations>`, `Arc<dyn Engine>`); native async fn in traits is not dyn compatible. Keep. |
| Counting allocator | see 2.7 | Per-thread arming is the point. |
| Sanitizers | `relay-messages/src/sanitize.rs` | `sanitize-filename 0.8.0-beta` (MIT) is a beta and drops idempotence guarantees that our proptests check; the display-name and text sanitizers are Unicode-policy code on top of `unicode-security`. |
| Media type guess | `relay-messages/src/sanitize.rs:guess_media_type`, `relay-ui/src/format.rs:media_type_for` | Advisory only. `mime_guess` (MIT) would pull a large table. One duplicate to merge (the UI copy recognizes fewer extensions), a few lines. |
| Short ids | `apps/relay-cli` `chars().take(8)` (4 sites) | Hex strings are ASCII; replace with `&hex[..8]` if touched. Trivial. |
| Time and size formatting | none found | No hand-rolled duration, byte-size or timestamp formatting exists (timestamps are integer milliseconds in the export JSON), so `humantime`, `bytesize`, `time`/`jiff` have nothing to replace. |
| CLI testing | `apps/relay-cli/tests/*` | Uses `env!("CARGO_BIN_EXE_relay")` and ratatui's `TestBackend`; `insta` (Apache-2.0, 1.49.0) would make the 20 `contains` assertions in `tui_render.rs` reviewable as snapshots; optional. `assert_cmd` (2.2.2) would replace about 10 lines of `Command` setup; not worth a dependency. |
| Fuzz | `fuzz/fuzz_targets/*` | Targets take `&[u8]` into parsers; `arbitrary` would only help for structured inputs, which the proto decoders already cover through `prost`. |

## 6. Feature gaps where a crate is the right tool (not swaps)

Checked by grep; none of these exist yet.

- Shell completions and man page: `clap_complete 4.6.11` (MIT OR Apache-2.0, already in the registry cache) plus `clap_mangen 0.3.3`. About 30 lines (a `relay completions <shell>` subcommand and a build or xtask step for the man page). Reasonable; also good for packaging (`docs/packaging.md`).
- Desktop notification for an incoming call: `notify-rust 4.18.2` (MIT OR Apache-2.0). It pulls platform stacks (zbus on Linux), so check against the existing `zbus` advisory ignores in `deny.toml` before adding. The daemon owns ringing; a notification belongs in the desktop shell, not the daemon.
- Launch at login: no launchd/systemd/Run-key writing exists. `auto-launch 0.6.0` (MIT) is the crate; the "autostart" code in `relay-ui` (`autostart.rs`) means starting the daemon, not login items.
- Single GUI instance: `single-instance 0.3.3` (MIT); alternatively reuse `relay-io`'s advisory lock (`lock.rs`, 62 lines, `fs4`), which already exists and avoids a new dependency. Prefer the existing lock.
- Panic reports: `human-panic 2.0.8` (MIT OR Apache-2.0); optional and local-only, no network.
- File watching: nothing polls files, so `notify 8.2.0` has no use today.

## 7. Dependency health

### 7.1 Duplicate versions (`cargo tree -d`, all features)

| Duplicate | Pulled by | Note |
|---|---|---|
| `objc2` 0.5.3 vs 0.6.5, `block2`, `objc2-foundation`, `objc2-app-kit`, `core-foundation` 0.9 vs 0.10, `bitflags` 1.3.2 | `eframe` -> `egui-winit` -> `accesskit_winit`/`winit` pin the older Apple stack; `keyring` -> `security-framework` uses `core-foundation` 0.10 | GUI only; not in the headless default build. Waits on the winit/egui release cadence. |
| `rand_core` 0.6.4 and `getrandom` 0.2.17 | `curve25519-dalek` 4.1.3 -> `ed25519-dalek` 2.2.0 | We use `rand` 0.10 / `rand_core` 0.10; unavoidable until dalek 3 ships. |
| `chacha20` 0.9.1 vs 0.10.2 | `chacha20poly1305` 0.10 -> `snow` 0.10 | Crypto stack, small. |
| `sha2` 0.10.9 vs 0.11.1 | `ed25519-dalek` 2.2 (0.10), `relay-benches` and others (0.11) | The workspace pins `sha2 = "0.10"` for `relay-benches` only; moving benches to 0.11 removes nothing because dalek still needs 0.10. |
| `syn` 2.0.119 vs 3.0.9 | `async-trait`, `tonic` on 3.x | Compile-time only. |
| `hashbrown` 0.16.1 vs 0.17.1, `itertools` 0.14 vs 0.15, `miniz_oxide` 0.8.9 vs 0.9.1 (`png`), `thiserror` 1.0.69 vs 2.0.21 | assorted | Small; `thiserror` 1.x comes from a transitive dependency, not ours. |

None of the duplicates is caused by our own manifests. `deny.toml` has `multiple-versions = "warn"`, so these show as warnings, not failures.

### 7.2 Weight (unique normal-dependency crates per top-level dependency, host target)

`relay-daemon` builds about 355 crates and `relay-cli` about 380, `relay-messages` 65, `relay-core` 13. Largest subtrees: `ratatui` 83 (brings `ratatui-widgets`, `kasuari`, `compact_str`, `lru`, `strum`, `time`; default features include a calendar widget that pulls `time`, so `default-features = false, features = ["crossterm"]` is worth trying and measuring), `tonic` 81 (needed), `idna` 43 (approved swap; this is the ICU/`idna_adapter` weight, accepted for IDN safety), `image` 41, `snow` 40, `comfy-table` 33, `rubato` 25, `cpal` 22.

Worth reconsidering:
- `image` features `png, jpeg, webp, gif, bmp, tiff` (workspace `Cargo.toml`): `relay-config/src/avatar/process.rs:36-41` accepts all six for local uploads, but the UI pre-check only offers PNG and JPEG (`relay-ui/src/format.rs`). Dropping `bmp`, `tiff`, and probably `gif` removes three decoders (and their untrusted-input surface) and compile time. This is a product decision, not a swap; the canonical stored form is lossless WebP regardless.
- `relay-benches` is in `default-members` (`Cargo.toml`), so `cargo build` compiles its dependencies (`hound`, `claxon`, `sha2`, `cpu-time`, `criterion` for dev). The heavy ones (`tract-onnx`, `realfft`, `sonora`, `nnnoiseless`) are optional, which is good. Moving `relay-benches` out of `default-members` would speed up the everyday build.
- `rusqlite` with `bundled` compiles SQLite from C; expected and fine.
- `keyring` 4.2.0 (11 crates on macOS) pulls `zbus` on Linux, which accounts for the three RustSec ignores in `deny.toml`; no change available from our side.
- `criterion 0.8.2` in `relay-protocol` dev-dependencies is unused (see 2.8); `divan` would not be a better benchmark harness here and the benches are not worth migrating.

### 7.3 Licenses and cargo-deny fit of every recommendation

| Crate | License (`cargo info`) | In allow list |
|---|---|---|
| subtle 2.6.1 | BSD-3-Clause | yes |
| base64 0.22.1 | MIT OR Apache-2.0 | yes |
| hex 0.4.3 | MIT OR Apache-2.0 | yes |
| backon 1.6.0 | Apache-2.0 | yes |
| exponential-backoff 2.1.1 | MIT OR Apache-2.0 | yes |
| backoff 0.4.0 | MIT/Apache-2.0 | yes, but unmaintained (RUSTSEC-2025-0012) |
| governor 0.10.4 | MIT | yes |
| rusqlite_migration 2.6.0 | Apache-2.0 | yes |
| atomic-write-file 0.3.1 | BSD-3-Clause | yes |
| ipnet 2.12.2 | MIT OR Apache-2.0 | yes |
| secrecy 0.10.3 | Apache-2.0 OR MIT | yes |
| lru 0.18.5 | MIT | yes (patched past RUSTSEC-2026-0002/-0253) |
| objc2-core-foundation 0.3.2 | Zlib OR Apache-2.0 OR MIT | yes |
| macos-accessibility-client 0.0.2 | Apache-2.0 | yes |
| clap_complete 4.6.11, clap_mangen 0.3.3 | MIT OR Apache-2.0 | yes |
| notify-rust 4.18.2 | MIT OR Apache-2.0 | yes (check zbus advisories) |
| auto-launch 0.6.0, single-instance 0.3.3 | MIT | yes |
| human-panic 2.0.8 | MIT OR Apache-2.0 | yes |
| insta 1.49.0 | Apache-2.0 | yes |
| tokio-util 0.7.20 | MIT | yes |
| unicode-segmentation 1.13.3 | MIT OR Apache-2.0 | yes |
| parking_lot 0.12.5 | MIT OR Apache-2.0 | yes |

## 8. Suggested order of work

1. Manifest cleanup: remove the five unused dependencies (2.8).
2. Security batch: `subtle` (2.1), redactor IP fix with tests (2.2), `base64` (2.3).
3. Mechanical batch: `hex` (2.4), `floor_char_boundary` (2.5), `Ipv6Addr::is_unique_local`/`is_unicast_link_local` in `address.rs`.
4. De-duplication batch: one `ReplayWindow` (2.6), one counting allocator (2.7), one `Secret` type with zeroize and constant-time equality (4.4), one media-type helper.
5. Swap batch (approved): switch the backoff choice to `exponential-backoff` or `backon` before touching `Cargo.toml` (3.1), then `rusqlite_migration`, then the atomic write.
6. Decide: lock helper vs `parking_lot` (4.1), `uuid` removal (4.3), `image` feature trim (7.2), `ratatui` default features (7.2).
7. Product work using libraries from day one: completions and man page (6), login autostart, incoming-call notifications.

## 9. Part 1 outcome (relay-net, relay-messages, relay-protocol, relay-config, relay-platform, benches, fuzz, relay-testkit)

Done:
- 2.1 `subtle::ConstantTimeEq` in `relay-net` `channel/ids.rs::ct_eq` and `Secret32::eq`.
- 2.2 `looks_like_ip` rewritten on `IpAddr`/`SocketAddr` parsing (`diagnostics/ip.rs`), handling compressed IPv6, brackets, ports, IPv4-mapped and `%zone`; `peer:<ip>` words are also checked. Regression tests in `relay-config/tests/diagnostics.rs`. Note: std is strict, so a malformed dotted quad such as `999.1.1.1` or `010.0.0.1` is no longer redacted.
- 2.3 `base64` 0.22 STANDARD engine; `store/base64.rs` deleted; RFC 4648 vector, hostile-input and proptest cases in `relay-messages/tests/export_base64.rs` (non-canonical trailing bits, whitespace, URL-safe alphabet and misplaced padding are all refused).
- 2.4 `hex` in `relay-net` (tag, advert, connect, fingerprint via `encode_upper`, one shared fixed-width `fixed_hex::decode` over `hex::decode_to_slice`), `relay-messages` and `benches`.
- 2.5 `floor_char_boundary` in four loops (the audit counted five; the grep finds four).
- 2.6 One `ReplayWindow`: `relay-net` now depends on `relay-protocol` (which has only `thiserror`) and aliases `ReplayWindow<16>`; the copy is deleted and `commit` is now `accept`.
- 2.7 `crates/relay-testkit` (member, not a default member, `publish = false`) holds `CountingAlloc`, `allocations_in` (per-thread arming) and `total_allocations` (process-wide, for `benches/tests/neural_allocs.rs`). Test binaries install it with a two-line `#[global_allocator]`. No `relay-engine` test has a counting allocator, so nothing is deferred there.
- 2.8 `relay-protocol` `criterion` removed. `cargo machete --with-metadata` still lists the build-script false positives (`relay-api`, `relay-net`).
- 4.2 (the std part) `Ipv6Addr::is_unicast_link_local`/`is_unique_local` in `relay-net/src/address.rs`.

Kept, with reasons:
- 3.1 `AcceptBackoff` (`relay-net/src/handshake/backoff.rs`, 12 lines of logic) stays. `exponential-backoff` 2.1.1 would fit the "plain delay, never sleeps" rule (set jitter 0.0 for determinism, call `Backoff::next(attempt)`), but the accept loop wants unbounded retries, no jitter and a `reset`, so the crate would wrap a counter we still own, delete no lines and add `fastrand` plus itself. `backon` is an async retry executor and does not fit a loop that must sleep on a caller's own clock. For the engine, UI and CLI call sites that do want jitter, `exponential-backoff` is the pick.
- `rusqlite_migration` 2.6.0 (Apache-2.0) requires `rusqlite` 0.40 while the workspace uses 0.41.1. Two `rusqlite` versions cannot coexist (both link `libsqlite3-sys`, a `links = "sqlite3"` crate), so it cannot be adopted until it releases for 0.41 or we pin 0.40. `db/migrate.rs` is a ten-line loop around the schema text; the refuse-newer check and notices live in `db/open.rs` and are unchanged.

Deferred to part 2 (owned by crates other agents are editing):
- `relay-api` `middleware.rs::constant_time_eq` to `subtle` (2.1); `relay-client` `Secret` zeroize plus constant-time `PartialEq`, and the CLI `Secret` removal (4.4).
- Unused dependencies: `relay-engine` `anyhow`, `bytes`; `apps/relay-cli` `tracing`, `tracing-subscriber` (2.8).
- Backoff swap to `exponential-backoff` for `relay-engine/src/peers/backoff.rs`, `relay-ui` `supervisor.rs`/`autostart.rs` and the CLI `client/feed.rs` loops (3.1).
- MAYBEs: `ipnet` for `relay-engine` `Subnet` (4.2), `lru` for the `relay-ui` avatar cache (4.5), `uuid` as random bytes (4.3), the lock helper (4.1).
- `relay-audio/src/priority*` was not touched; `relay-audio` only gained the `relay-testkit` dev-dependency and its allocation tests were switched.

## 11. Part 2 outcome (apps/relay-cli, crates/relay-ui, apps/relay-desktop)

Done:
- Unused dependencies (`cargo machete --with-metadata`): `apps/relay-cli` `tracing-subscriber` moved to dev-dependencies (only `tests/messages_wire.rs` installs a subscriber; the binary only emits `tracing` events) and `uuid` removed (the PTT source id now uses `relay_io::entropy::random_bytes`). `blake3`, `relay-audio` and `relay-protocol` stay as dev-dependencies and are listed in `[package.metadata.cargo-machete] ignored`: `tests/real_chat.rs` includes the engine's test support by path and that code uses them. `relay-ui` and `relay-desktop` report nothing unused.
- 4.1 `relay_io::LockExt` replaces the hand-written poison-tolerant locks in `relay-ui` (`hub`, `actions`, `inbox`, `messaging/hub`, `autostart`, `controller`; the five local `lock()` helpers are deleted) and is used by the new desktop code; `relay-ui` and `relay-desktop` gained a `relay-io` dependency.
- 6 `clap_complete` 4.6.11 and `clap_mangen` 0.3.3 (both MIT OR Apache-2.0, MSRV 1.85): `relay completions <shell>` and `relay man [--dir DIR]` (`commands/docs.rs`, 35 lines; tests in `tests/docs_cmd.rs` check every shell and that the pages name the subcommands).
- 6 `notify-rust` 4.18.2 (MIT OR Apache-2.0, MSRV 1.89) for incoming-call notifications and `auto-launch` 0.6.0 (MIT) for start at login, both in `apps/relay-desktop` only. The notification carries the caller's name and nothing else, is shown only when the window is hidden or not in front, and defaults on; start at login defaults off and shows the OS's own answer. Behind traits (`Notifier`, `LoginBackend`) so the tests use fakes. `docs/desktop.md` has the behaviour.

Kept, with reasons:
- Backoff (3.1): `relay-ui` `supervisor::backoff` (250 ms doubling to 5 s), `autostart::decide` (1 s doubling per start within 60 s) and the CLI `client/feed.rs` / `session_feed.rs` loops stay hand-written. `exponential-backoff` 2.1.1 is an iterator with its own `fastrand` generator and no injectable sample, bounded by `max_attempts` with a "last attempt" marker. The UI supervisor needs an unbounded attempt counter, `autostart` is a pure function of recorded `Instant`s that its tests drive without a clock, and the CLI loops are a fixed `retry` interval chosen by the caller (not exponential). Jitter has no value for one local daemon, so using the crate would delete no lines, add `fastrand`, and lose the exact-delay assertions in `tests/supervisor.rs`. `backoff` (RUSTSEC-2025-0012) is not used.
- `lru` (4.5) for `avatar::TextureCache` is not adopted: the cache evicts `Pending` entries last (so a slow fetch is not requested twice) and keeps `Missing` markers, which `lru` cannot express, so it would keep the state machine and save only the eviction scan.
- `cargo deny check` with these dependencies: advisories ok, bans ok, licenses ok, sources ok (only the usual duplicate-version warnings). `notify-rust` pulls `zbus` with its default `async-io` runtime on Linux next to the `tokio` zbus already in the tree (no new advisory).
