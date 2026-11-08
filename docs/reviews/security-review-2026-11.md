# Adversarial security review, November 2026

**This is an AI-performed review. It does NOT close the "Independent security review" gate in `docs/gates.md`.** Treat it as input for a named human reviewer.

- Revision: `main` at `5048949` plus the uncommitted working tree present on 2026-11-08 (other agents were editing localizations, schema versions and daemon start-up at the same time).
- Method: read code (not only docs) for every listening socket, parser and untrusted-input path; read `docs/reviews/security-net-channels.md` and `dependency-audit.md` first and did not repeat their verified items. Where it was safe I proved behaviour with throwaway programs in the session scratchpad, against loopback or pure functions only. Nothing under `crates/` or `apps/` was modified. The user's running `relayd`, its socket, the phone and the network were not touched. No packets were sent to any other host.
- Evidence labels: **CONFIRMED** = traced in code and/or reproduced by a program or tool output shown here. **PLAUSIBLE** = sound from code but not executed, or depends on a platform/firewall I could not exercise.

## 1. Executive summary

| # | Question | Verdict |
|---|---|---|
| 1 | Is it really LAN-only and unreachable from outside? | **Unreachable from the public internet by address class: yes. "LAN-only" is weaker than it sounds: it means "private-address-only", not "on-link".** Public sources are dropped after `accept` and public/loopback/overlay addresses are refused as peers (tested, section 5). The daemons bind `0.0.0.0` and filter by RFC 1918/ULA/link-local class, so any routed private network (VPN, campus, VLAN without ACL, carrier network that gives phones 10.x addresses without client isolation) reaches the pre-auth surface (SR-01). The desktop/daemon code contains no outbound internet client of any kind. **The Android APK does**: it bundles Google ML Kit with the Firelog/`CCTDestination` upload path and holds `INTERNET`, contradicting the documented "no network" claim (SR-02). |
| 2 | Can a LAN attacker get RCE, memory corruption, arbitrary file read/write or escalate? | **No such path found.** The attack surface is small and memory-safe: about 30 `unsafe` lines in 9 files (audio priority, AAudio, macOS/Windows platform glue, JNI entry, a test allocator), none in networking, protocol, config, messages or engine crates; every length is bounded before allocation; the receive state machine verifies before it commits; file receipt writes only to an exclusive temp file and renames without replacement; image headers are validated by hand-written parsers (3 million mutated inputs, 0 panics) and then decoded under allocation limits. What a LAN attacker can do is nuisance and availability (ring harassment by key rotation, SR-04), trust-prompt social engineering (SR-06) and the local-app attacks on Android (SR-03). |
| 3 | General practices | **Good on code hygiene, weak on release engineering.** `cargo deny` (advisories, bans, sources, licenses) passes; `cargo audit` finds 0 vulnerabilities on all three lock files and only "unmaintained" notices. Secrets use the OS credential store / Android Keystore, with an explicit insecure-file opt-in. No SQL built by `format!`. The shipped Android build is a **debuggable, debug-signed APK** (SR-05), CI actions are pinned by tag or branch rather than commit (SR-09), the daemon log is world-readable (SR-07) and Windows local-API hardening is unmet (SR-08). |

Counts: Critical 0, High 0, Medium 5, Low 7, Info 7. No finding is a remote-code-execution or memory-safety issue.

### Top findings, in fix order

1. **SR-01 (Medium)**: listeners bind `0.0.0.0` and admit any RFC 1918/ULA source, not only on-link ones, so "LAN-only" includes every routed private network. The Android listener also listens on cellular.
2. **SR-02 (Medium)**: the Android APK bundles ML Kit + Firelog (Google usage logging) and `INTERNET`, contradicting `docs/android.md` ("no network").
3. **SR-03 (Medium)**: Android `MainActivity` is exported and trusts `FROM_CALL`/`ANSWER` extras, so any app can answer a ringing call and wake the screen over the lock screen.
4. **SR-04 (Medium)**: ring harassment: the invite limiter is per key, default policy is `Anyone`, and keys are free.
5. **SR-05 (Medium)**: the released APK is `application-debuggable` and signed with a throwaway debug key.

## 2. Threat model

| Actor | Capability assumed | Where it is stopped |
|---|---|---|
| (1) Internet | Can route packets to a host with a public address (VPS, IPv6 with permissive CPE) | `check_socket(from)` after `accept`; no IPv6 listener; mDNS addresses filtered; no UPnP/NAT-PMP/STUN code |
| (2) LAN stranger / rogue mDNS / MITM | Any L2/L3 position on the Wi-Fi | Noise XX with pinned keys; pre-auth gate (8 global / 2 per source, 5 s); 16 KiB frame cap; display-name sanitizer; first-use trust only on a user decision |
| (2b) Known contact, or one whose key changed | Authenticated link | Messages only from trusted links; KeyChanged refusal; rate limits |
| (3) Local user / process | Same machine, other uid; or same uid | Per-user 0700 socket directory and `SO_PEERCRED` uid match; 0600 files. Same-uid processes are explicitly out of scope of the API (capabilities are self-declared at `RegisterClient`) |
| (4) Malicious Android app | Installed on the same phone | Mostly non-exported components, `FLAG_IMMUTABLE` PendingIntents, `allowBackup=false`; exceptions SR-03, SR-05 |
| (5) Supply chain / CI | Compromised action, crate, Maven artifact, PR | cargo-deny, `--locked`, pinned toolchain; weak spots SR-09, SR-10 |

## 3. Findings

Sorted by severity. "Exploitable today" means reachable on the current build by the named actor; "hardening" means a defence-in-depth gap with no demonstrated attack.

| ID | Sev | Title | Where | Exploitable today |
|---|---|---|---|---|
| SR-01 | Medium | Wildcard listeners and class-based (not on-link) source filter | `relay-engine/src/daemon/assemble.rs` (`config.listen`, `start_messaging`), `relay-engine/src/media/system.rs` (`bind_ip`), `relay-engine/src/peers/start.rs` `accept_loop`, `relay-net/src/address.rs` | Yes, from any routed private network |
| SR-02 | Medium | Android APK ships ML Kit with Google Firelog upload code and `INTERNET` | `apps/relay-android/app/build.gradle.kts`, `AndroidManifest.xml`, `docs/android.md` | Plausible (code present, upload not observed) |
| SR-03 | Medium | Exported `MainActivity` trusts `EXTRA_ANSWER` / `EXTRA_FROM_CALL` | `MainActivity.kt` `handleCallIntent`, manifest | Yes, by a local app that can start an activity |
| SR-04 | Medium | Ring harassment: per-key invite limit, default `Anyone`, no global or per-IP budget | `relay-engine/src/peers/sessions.rs`, `relay-config/src/prefs/model.rs` | Yes, from the LAN |
| SR-05 | Medium | Released APK is debuggable and debug-signed | `justfile` `android-apk` (`assembleDebug`), README | Yes (with USB debugging authorised, or by anyone re-signing) |
| SR-06 | Low | A pasted or scanned `relay://` string makes the device Verified with no confirmation of name or fingerprint | `relay-engine/src/service/rpc/connect_trust.rs`, `relay-ui/src/controller.rs` | Yes (needs social engineering) |
| SR-07 | Low | Daemon log is created 0644 in a 0755 directory and records peer fingerprints and call ids | `relay-io/src/proc.rs` `open_rotating_log` | Yes, to other local users who can reach the user's home |
| SR-08 | Low | Windows: bearer-token profile in `%TEMP%\relay` with no owner or ACL check | `relay-engine/src/daemon/transport.rs`, `relay-io/src/fs/private.rs` | Untested (needs shared or pre-created TEMP) |
| SR-09 | Low | CI/release: third-party actions pinned by tag or branch; publish job inherits unpinned build inputs | `.github/workflows/*.yml` | Hardening |
| SR-10 | Low | Android Gradle dependencies unverified; Android and fuzz lock files not covered by CI `cargo deny` | `apps/relay-android/gradle`, `ci.yml` | Hardening |
| SR-11 | Low | Address policy accepts cloud-metadata and directed-broadcast addresses outside the media-candidate path | `relay-net/src/address.rs` `parse_manual`, `check_ip` | Yes, limited (LAN-side dial gadget) |
| SR-12 | Low (PLAUSIBLE) | `mdns-sd` binds wildcard UDP 5353 and does not check the source of a datagram is on-link | `mdns-sd 0.21.7` `handle_read`, `relay-net/src/discovery/service.rs` | Only if the host firewall exposes 5353 |
| SR-13 | Info | Release profile keeps overflow checks off; wire-derived `u64` arithmetic wraps silently in release | `Cargo.toml` `[profile.release]` | Hardening |
| SR-14 | Info | Fuzz coverage gaps | `fuzz/fuzz_targets` | Hardening |
| SR-15 | Info | Capabilities are self-declared, so they are not a security boundary against same-uid processes | `relay-api` registry, `docs/api.md` | By design |
| SR-16 | Info | Blocklist is keyed on the key digest, so an attacker evades it with a new key | `relay-config/src/blocklist.rs` | By design |
| SR-17 | Info | Diagnostics redactor leaves display names, MAC addresses and short hex | `relay-config/src/diagnostics/redact.rs` | Hardening |
| SR-18 | Info | Vendored, patched `winit` in the Android build is outside cargo-deny's registry checks | `apps/relay-android/third_party/winit` | Hardening |
| SR-19 | Info | Two display-name sanitizers with different strength | `relay-messages/src/sanitize.rs` vs `relay-net/src/discovery/names.rs` | Hardening |

### SR-01 Medium: "LAN-only" is "private-address-only"

**Scenario.** A phone on cellular whose carrier hands out 10.x addresses without client isolation, a laptop on a campus or corporate network, a laptop on a VPN that routes RFC 1918, or a host on a multi-VLAN network without ACLs. An attacker on any other routed private network connects to TCP 47800 (or the media/transfer ports) and runs the pre-authentication handshake, then, as an authenticated stranger, can ring the user (SR-04) and send a Hello (name, avatar).

**Evidence.**
- `assemble.rs`: `config.listen = network.listen.unwrap_or_else(|| ([0, 0, 0, 0], listen_port).into())`; `start_messaging` binds the transfer hub to `listen.ip()` (also `0.0.0.0`); `MediaConfig::bind_ip` defaults to `0.0.0.0`; `PeerSocket::open` binds `SocketAddr::new(bind_ip, 0)`.
- `accept_loop` calls only `policy.check_socket(from)`. `check_ip` accepts any `is_private() || is_link_local()` IPv4 or any ULA/link-local IPv6 (`is_lan_ip`), so it classifies by address range, not by whether the source is on one of the host's own subnets.
- The subnet of the receiving interface is already computed (`Subnet::of_interface`) but is used only to filter media candidates (`signaling.rs`), not inbound control or data links.
- Android: `RelayRuntime.kt` registers Wi-Fi/Ethernet callbacks for locks only; there is no `bindProcessToNetwork` or per-network socket, so the wildcard listener is reachable through the cellular interface too.

**Not reachable from the internet** (see section 5): a public source is closed immediately after `accept` (TCP SYN/ACK still completes, so a scanner sees the port open on a public-IP host).

**Fix.** After `accept`, require the peer IP to be inside the subnet of the interface that owns `stream.local_addr()` (reuse `Subnet::of_interface`; for IPv6 link-local require the same scope id, for ULA the same /64). Apply the same check to the transfer hub and to the UDP media source. On Android, bind the process to the Wi-Fi/Ethernet `Network` and refuse cellular. Optionally bind per eligible interface instead of `0.0.0.0` (`InterfacePolicy` already knows which are eligible). Add the opposite test to `handshake_dos.rs` (a private but off-subnet source is refused).

### SR-02 Medium: Android ships Google telemetry code

**Scenario.** The QR scanner uses `com.google.mlkit:barcode-scanning:17.3.0`. The built APK contains `MlKitComponentDiscoveryService`, `MlKitInitProvider`, `TransportBackendDiscovery` with `backend:...CctBackendFactory`, `JobInfoSchedulerService`, `CCTDestination` and `enableFirelog` classes, plus `GoogleApiActivity`, and the app holds `INTERNET`. ML Kit uses this path to upload usage and performance events to Google. `docs/android.md` states "ML Kit's bundled QR model (no Play services, no network)", which this build does not support.

**Evidence (CONFIRMED that the code and permission are present).**
```
$ aapt2 dump xmltree --file AndroidManifest.xml dist/android/relay-0.1.0-android-arm64-v8a.apk
  ... MlKitComponentDiscoveryService, MlKitInitProvider, TransportBackendDiscovery,
      backend:com.google.android.datatransport.cct.CctBackendFactory, JobInfoSchedulerService
$ strings classes*.dex | grep -i firelog      -> enableFirelog, firelogEventType, CCTDestination
```
I did not run the app or capture traffic (no device or emulator was used), so the actual upload is **PLAUSIBLE**, not observed.

**Fix.** Either replace ML Kit with an offline decoder (ZXing core, or `zxing-cpp`), or strip the transport and component services with `tools:node="remove"` in the manifest and verify with a network capture on the emulator that nothing leaves the device. Update `docs/android.md` either way, and add a CI check that fails if `datatransport` or `firebase` reappears in the merged manifest.

### SR-03 Medium: any app can answer a ringing call

**Scenario.** `MainActivity` is `exported="true"` (it is the launcher). `handleCallIntent` honours `EXTRA_FROM_CALL=true` by showing over the lock screen and turning the screen on, and `EXTRA_ANSWER=true` by calling `RelayRuntime.whenReady { it.answer() }`. A malicious app that can start an activity (it is in the foreground, or holds an overlay or similar privilege) sends an explicit intent with those two extras while a call rings and the call is answered without the user touching Relay; the microphone then opens according to the default call mode.

**Evidence.** `MainActivity.kt` `handleCallIntent`, and the manifest. The legitimate sender is `IncomingCallNotifier.openApp`, which uses a `FLAG_IMMUTABLE` PendingIntent. Nothing in the activity distinguishes the two senders.

**Fix.** Do not take the answer from an extra on an exported component. Route notification actions through a non-exported trampoline (`CallActionReceiver` already exists and is not exported), or put a per-process random token in the PendingIntent extra and check it. `setShowWhenLocked`/`setTurnScreenOn` should also depend on that token. Also consider `VISIBILITY_PRIVATE` for the caller name on the lock screen (currently `VISIBILITY_PUBLIC`).

### SR-04 Medium: ring harassment by an unauthenticated LAN device

**Scenario.** Any device that completes a Noise handshake (no trust needed) can send `Invite`; the user's phone rings, shows the attacker's chosen name and avatar, and can show a full-screen intent. The per-peer limiter is `INVITES_PER_WINDOW = 16` per 10 s per peer key (`SessionTable::admit_invite`). A key costs nothing, so the attacker rotates keys and rings without bound. The default is `InboundPolicy::Anyone`.

**Evidence.** `sessions.rs` (limit keyed by `PeerId`), `relay-core/src/state/peer.rs` `on_invite` (a cap on live calls, none on call history rate), `prefs/model.rs` (`Anyone` is `#[default]`). Not executed; the limiter is a pure function of the peer key, so the bypass follows from reading it.

**Fix.** Add a global ring budget and a per-source-IP budget for untrusted callers, and make unknown callers silent by default (notification only, no ringtone, no full-screen intent) until the user opts in. Consider default `SharedChannelOnly` or "contacts only" with an explicit "allow unknown callers" switch.

### SR-05 Medium: the shipped APK is debuggable and debug-signed

**Evidence.** `justfile` `android-apk` runs `./gradlew assembleDebug` and copies `app-debug.apk`; the release workflow calls `just android-apk`. On the built file:
```
$ aapt2 dump badging dist/android/relay-0.1.0-android-arm64-v8a.apk | grep debuggable
application-debuggable
```
README says "the APK is debug-signed" (known). Consequences: anyone with `adb` access to an unlocked phone can `run-as dev.relay.app` and read the app's private files, which defeats much of the Keystore sealing; the debug key is generated per build machine, so successive releases carry different signing keys, users cannot pin the publisher, and an update path cannot be trusted; debuggable also loosens JIT and native-debug protections.

**Fix.** Build `assembleRelease` with a project-owned release key held as a CI secret (or at least a stable key committed as a public certificate fingerprint in the README), `debuggable=false`, and publish the signer certificate SHA-256 beside `SHA256SUMS.txt`.

### SR-06 Low: scan or paste makes a device Verified with no confirmation

**Evidence.** `ConnectAndTrust` dials the string with its fingerprint pinned and records the peer as `Verified` with `out_of_band_verified: true` (`connect_trust.rs`); `Controller::connect_and_trust` forwards it with no step that shows the resulting name and fingerprint. A QR sticker or a pasted message therefore creates a Verified contact whose name the other side chose, and Verified is the prerequisite for auto-answer.

**Fix.** Show "Verify X, fingerprint ab12 cd34 ..., on 192.168.1.20" with explicit Confirm before storing; never let the scanned string alone create a Verified contact whose name collides with an existing contact (the name-skeleton check exists in `peers/trust.rs`).

### SR-07 Low: daemon log world-readable

**Evidence (CONFIRMED on this machine).**
```
drwxr-xr-x  ~/Library/Logs/Relay
-rw-r--r--  ~/Library/Logs/Relay/relayd.log       (36 lines contain 32+ hex characters)
```
`open_rotating_log` uses `fs::create_dir_all` and `OpenOptions::new().create(true).append(true)` (umask default) and follows symlinks. INFO lines carry peer fingerprints, call ids and "incoming call answered automatically". Where the home directory is group- or world-readable (some Linux distributions, shared Windows profiles), others learn who calls whom and when.

**Fix.** Create the directory 0700 and the file 0600 (`create_private_file` already exists in `relay-io::fs`), open with `O_NOFOLLOW`, and log fingerprints only at DEBUG.

### SR-08 Low: Windows loopback API is unhardened (not tested)

`transport.rs` places `engine.tcp` (address + bearer token) in `%TEMP%\relay` using `create_dir_all`, with no owner check; `relay-io/src/fs/private.rs` states the explicit owner-only DACL is not applied. If `%TEMP%` is shared (a service account, a Terminal Server profile redirect, or `C:\Windows\Temp`), a pre-created directory lets another user read the token, and the token grants `admin` and `messages`. Token comparison is constant-time and loopback-only, which is right. **Fix.** Place the profile under `%LOCALAPPDATA%\Relay` and set an explicit owner-only DACL; or switch to a named pipe with an owner-only DACL. I could not run Windows.

### SR-09 Low: CI and release supply chain

- All actions are pinned by tag or branch, not commit SHA: `dtolnay/rust-toolchain@master` (a moving branch, used in every job), `Swatinem/rust-cache@v2`, `taiki-e/install-action@v2`, `orhun/git-cliff-action@v4`, `gradle/actions/setup-gradle@v4`, `EmbarkStudios/cargo-deny-action@v2`, `actions/*@vN`.
- The `publish` job holds `contents: write`, `id-token: write` and `attestations: write`, but it publishes artifacts produced by earlier jobs that ran those unpinned actions. A compromised action taints the artifacts, and `attest-build-provenance` then attests the tainted files. Provenance is real but attests "built by this workflow", not "built from audited inputs".
- `SHA256SUMS.txt` is uploaded in the same release as the files it covers, so it detects accidental corruption only; it is not signed.
- `pip install dmgbuild==1.6.7` pins the top package but not its dependencies; `taiki-e/install-action` installs `cargo-about` and `just`/`cargo-ndk` at unpinned versions; `cargo install cargo-fuzz --locked` is unpinned.
- No workflow interpolates attacker-controlled text into `run:` (inputs go through `env:`), PRs from forks run with read-only tokens, and `release.yml` verifies `artifacts-run` is a completed push/dispatch run of the exact tagged commit. These are good.

**Fix.** Pin every third-party action to a full commit SHA (Dependabot can bump them), pin `rust-toolchain` by SHA and a fixed toolchain input, sign `SHA256SUMS.txt` (cosign or minisign) and publish the signer, and pin Python tool dependencies with `--require-hashes`.

### SR-10 Low: dependency coverage gaps

CI runs `cargo deny` for the root workspace only. `apps/relay-android/Cargo.lock` (621 crates) and `fuzz/Cargo.lock` are separate; I ran `cargo audit` on both (0 vulnerabilities; `basic-toml` RUSTSEC-2026-0374 unmaintained on the Android lock). Gradle resolves Maven Central and Google artifacts with no `gradle/verification-metadata.xml`, so transitive Android dependencies are not hash-verified. **Fix.** Run `cargo deny --manifest-path apps/relay-android/Cargo.toml` in CI and enable Gradle dependency verification.

### SR-11 Low: asymmetric address policy

**PoC output** (`AddressPolicy::lan_only().parse_manual`, scratch program):
```
169.254.169.254:80     OK        169.254.170.2:80   OK      [fd00:ec2::254]:80  OK
192.168.1.255:47800    OK        10.255.255.255:47800 OK    [::ffff:8.8.8.8]:80 ERR NotLan
8.8.8.8:80             ERR NotLan  127.0.0.1:47800 ERR Loopback  0.0.0.0:47800 ERR NotLan
relay://192.168.1.1:80#<fp>  OK(192.168.1.1:80)   relay://8.8.8.8:80#<fp> ERR NotLan
```
The cloud-metadata block (`is_reserved_infrastructure`) is applied only in `candidate_from_proto`. A pasted or scanned string, and a device found through mDNS, can therefore make the app dial `169.254.169.254` (and any LAN host and port), sending the 24-byte binary preface. Impact is small (no response content is returned, preface is not HTTP) but it is an SSRF-shaped gadget on cloud and some embedded hosts, and a LAN port-scan primitive for a rogue mDNS responder. Directed-broadcast addresses (`x.x.x.255`, `10.255.255.255`) are also accepted. **Fix.** Apply `is_reserved_infrastructure` inside `check_ip`; refuse the all-ones host of the local subnet and `.255`/`.0` of a /24 in manual entries; restrict discovered and typed ports to the 1024-65535 range.

### SR-12 Low, PLAUSIBLE: wildcard mDNS socket

`mdns-sd` binds `0.0.0.0:5353` / `[::]:5353` and takes only the receiving interface from the packet-info, not the source address (read in `service_daemon.rs` `handle_read`). If a host's firewall exposes UDP 5353 (a public IPv4 host, or a global IPv6 address behind a permissive router), a remote source can answer queries and inject discovery entries. The address filter then limits injected addresses to private ranges, so the effect is discovery-table pollution and the SR-11 dial gadget, not a breach. Not tested: injecting on the user's own interface would disturb their mDNS. **Fix.** Drop mDNS datagrams whose source is not on-link (check against interface subnets, or TTL 255 and source-in-subnet as RFC 6762 requires) with a wrapper, or contribute the check upstream.

### Info items

- **SR-13.** `[profile.release]` has `lto = "fat"`, `strip = "symbols"` and panic `unwind` (deliberate, documented) but no `overflow-checks = true`. Wire-derived `u64` offsets are mostly `saturating_add`/checked in the hot paths I read (transfer `on_chunk` uses `saturating_add`), but a release build wraps silently elsewhere (for example `jitter/buffer.rs` range `next_seq + horizon + 1`). Enable overflow checks for `relay-net`, `relay-protocol` and `relay-engine` at negligible cost.
- **SR-14.** Fuzz targets exist for address, avatar decode, channel text and wire, control envelope, media datagram, record header and RTP. Missing: `relay_net::transfer` frame decode, `relay_messages::image::inspect` (four hand-written container parsers), markdown/linkify, mDNS TXT parsing (`discovery/advert.rs`), invite token and certificate parsing, `control::session_msg::validate`. CI only compiles the fuzz crate (`cargo fuzz build`); it never fuzzes. I did not run libFuzzer. My own mutation run: 3,000,000 mutated PNG/JPEG/GIF/WebP headers into `inspect`, 0 panics (448,289 accepted, 2,551,711 rejected).
- **SR-15.** `RegisterClient` lets a caller choose its capabilities (`admin`, `messages`, `call-control`). That is fine for a per-user daemon whose boundary is the uid (verified, section 5), but the capability gate does not protect against a malicious same-uid process, and `SetClientQuiet`/`HOUSE` lease takeover are reachable by any registered client. Documented as out of scope in `tcp.rs`; keep it that way in `docs/api.md`.
- **SR-16.** `BlockPeer` stores a bounded list of 256 key digests. A blocked attacker generates a new identity key and returns. Blocking is a nuisance filter, not an exclusion mechanism; say so in the UI help.
- **SR-17.** The diagnostics redactor handles IPs, paths, 32+ hex runs, `Bearer` tokens and sensitive key names well (it uses std parsers now). It does not touch display names, MAC addresses, short fingerprints (under 32 hex digits) or host names.
- **SR-18.** `apps/relay-android/third_party/winit` is a patched copy of winit 0.30.13 (soft keyboard text). Review its diff on each upgrade; it handles every typed character, including the identity-bearing fields.
- **SR-19.** `relay_messages::sanitize_display_name` (control + format characters only) is weaker than `relay_net::discovery::sanitize_display_name` (NFKC, confusables, marks, markup). Names from the wire go through the stronger one in `peers/link.rs`/`profile.rs`; make the weaker one private or delegate.

## 4. Question 1 in detail: reachability

**Listening sockets (all enumerated by grep for `bind(`, `TcpListener`, `UdpSocket`, `0.0.0.0`, `UNSPECIFIED`):**

| Socket | Bind | Source filter | Note |
|---|---|---|---|
| Control TCP (`peers/start.rs`) | `0.0.0.0:47800` (IPv4 only) | `check_socket(from)` then handshake gate | SR-01 |
| Data/transfer TCP (`TransferHub`) | `listen.ip():transfer_port`, `0.0.0.0` | policy + token + gate, 8 links | SR-01 |
| Media UDP (`MediaEndpoint`) | `0.0.0.0:ephemeral` per link | AEAD under session keys, silent drop | datagram needs a valid key |
| mDNS | wildcard 5353 via `mdns-sd`; loopback only if the dev flag is set | `InterfacePolicy` excludes tunnels, bridges, containers by name | SR-12 |
| Local API | Unix socket `0700` directory + `SO_PEERCRED`, or `127.0.0.1:ephemeral` + 256-bit bearer token (Windows) | uid / token | verified |
| Android | same engine in-process; no extra sockets | n/a | SR-01 on cellular |

**Outbound:** the only outbound connections in Rust are the Noise/TCP dial to a LAN peer (policy-checked), the UDP path challenge to a policy-checked candidate, and mDNS multicast. No `reqwest`, `hyper` client, `ureq`, STUN, UPnP/IGD/NAT-PMP, update check, telemetry or DNS lookup exists in any `Cargo.toml` or source file (grepped; the only `http://` strings are the loopback gRPC URI and the Markdown link parser). Manual addresses are IP literals only ("no DNS lookup is ever performed", tested: `localhost:80` and `metadata.google.internal` are `Malformed`). IPv6 has no listener.

**Overlay (Tailscale):** opt-in `allow_overlay_manual` admits only an address the user typed; discovery, candidates and `encode` refuse 100.64.0.0/10 (tested in `address.rs`, plus my matrix). When enabled, the same wildcard listeners are reachable on the tailnet interface, so the exposure is "everyone your tailnet ACL lets in", still behind Noise.

**Malicious QR / connection string.** Parsed by `ConnectionString::parse`: IP literals only, 256 bytes max, fingerprint mandatory; public, loopback, unspecified, multicast and overlay targets are refused (PoC above). It can still point at any private address and port (SR-11) and creates a Verified contact if the target holds the matching key (SR-06). It cannot reach the internet or loopback.

**Tried and blocked.** Public sources (`8.8.8.8`, `1.1.1.1`), IPv4-mapped public addresses (`::ffff:8.8.8.8`), NAT64 (`64:ff9b::`), 6to4/documentation ranges, loopback, `0.0.0.0`, `::`, multicast, site-local `fec0::`, unscoped link-local, benchmark `198.18/15`, `192.0.0.0/24`, `172.32/12` just outside RFC 1918, leading-zero/hex octets, trailing-dot hosts and ports above 65535 are all refused by `parse_manual`. The three network test suites and the fuzz suite pass in a scratch target dir: `address` 8/8, `handshake_dos` 7/7, `transfer_hostile` 8/8, `fuzz` 12/12.

## 5. Verified SAFE (what was checked and how)

| Area | Result | How |
|---|---|---|
| Public/loopback/overlay peer addresses | Refused | Scratch program over 34 address forms; `tests/address.rs` passes |
| Pre-auth handshake DoS | Per-source and global slots, eviction, 5 s deadline, accept back-off | Read `gate.rs`, `limits.rs`, `start.rs`; ran `handshake_dos` (7 tests) |
| Frame/allocation bounds | Length validated before `vec![0; len]` in handshake (4 KiB), control (16 KiB), data (about 32 KiB) | Read `framing.rs`, `control/receiver.rs`, `transfer/link.rs`; grepped every `with_capacity`/`vec![0; n]` in net/protocol/messages/engine |
| Replay / nonce / order | Strictly increasing sequence checked before open and committed after; separate lanes and direction keys | Read receivers; earlier review's randomized test not repeated |
| Panics on wire data | The only `expect`/`unwrap` in net, protocol, messages and peers sources follow exact length checks (`try_into` on checked slices) | Grepped all, inspected `invite.rs`, `cert.rs`, `frame.rs` |
| File receipt | Name sanitized (separators, controls, bidi, Windows reserved, dots/space trim); exclusive random temp file, hash verified before an atomic no-replace rename; offset must equal bytes received; size bounded by grant; destination chosen by the local user per offer; no auto-accept | Read `sanitize.rs`, `relay-io/src/sink.rs`, `receiver.rs`; grepped for auto-accept (none) |
| Peer images | Header validated by hand-written parsers, dimensions and animation bounded, then decoded with `image::Limits` (max alloc 80 MiB) off the UI thread; only PNG/JPEG/GIF/WebP accepted from peers | Read `image.rs`, `relay-ui/messaging/image.rs`; 3,000,000-input mutation run, 0 panics |
| Avatars | Canonical 256x256 lossless WebP, hash-checked, fully decoded under ceiling | Prior review plus `process.rs` limits |
| Peer text | Controls, bidi and invisible characters refused; display names NFKC + confusable skeleton; links only `http(s)` with IDN warnings; never auto-opened | Read `sanitize.rs`, `names.rs`, `markdown/url.rs` |
| Terminal escape injection | Names, messages, file names and reason codes are stripped of controls before the CLI/TUI sees them; inbound `reason` and `reason_code` are length-checked and never displayed | Read proto validation and consumers |
| Call state machine | Duplicate call id maps to retained outcome; call id owned by first peer; invite while busy gets `Decline(busy)`; glare resolved deterministically; terminal calls pruned | Read `state/peer.rs`, `sessions.rs` |
| Auto-answer | Re-checked at ring time and after 1.5 s: global switch, flagged, `Verified`, live link, no other call, someone home; mic follows the user's default call mode | Read `actor_auto.rs`, `directory.rs::may_auto_answer` |
| Local control socket | Directory 0700 and owner-checked, symlinks and foreign owners refused, stale socket reclaimed only if ours and refusing connections, `SO_PEERCRED` uid check, socket `chmod 0600` | Read `uds.rs`, `relay-io/src/fs/private.rs`; checked live: `/var/folders/.../T/relay-501` is `drwx------` (the socket itself was not touched) |
| Loopback TCP token | 256 bits, constant-time compare, profile 0600 and parsed as loopback only | Read `tcp.rs`, `middleware.rs` |
| Secret storage | OS credential store by default, explicit "INSECURE" file option labelled as such, 0600; zeroization of identity bytes; Android AES-256-GCM in the Keystore (StrongBox when available) with per-slot AAD | Read `identity/store.rs`, `Sealer.kt` |
| SQLite | No SQL assembled with `format!`; DB created 0600 in a 0700 directory | Grepped; read `db/open.rs`; `~/Library/Application Support/Relay.Relay` is `drwx------` |
| Unsafe | about 30 lines in 9 files: audio priority, AAudio slices, macOS AVFoundation/hotkeys, JNI entry; none in network or parsing crates; `undocumented_unsafe_blocks` is denied workspace-wide | Grepped the tree |
| Process spawning | No shell, no `xdg-open` with untrusted text; `open_url` is gated to http/https; the only `Command::new` runs the daemon from its own file | Grepped |
| Android manifest | `allowBackup=false`, `dataExtractionRules` exclude all, services and `CallActionReceiver` not exported, PendingIntents `FLAG_IMMUTABLE`, `BootReceiver` checks the action, no `FileProvider`, no cleartext config needed (no HTTP) | Read manifest and sources; dumped merged manifest from the built APK |
| Dependencies | `cargo deny --offline check advisories bans sources licenses` all ok; `cargo audit --no-fetch` on the three lock files: 0 vulnerabilities, 3 to 4 "unmaintained" (`zvariant`, `zvariant_derive`, `zbus_names`, `basic-toml`), advisory DB dated 2026-11-02 | Ran the tools |
| Ringtone import | Local files only, bounded by bytes and seconds before decode and resample | Read `ringtone/mod.rs` |

## 6. Limits of this review

- **Windows** (loopback token profile, TEMP permissions, MSI): not run; SR-08 is from code only. **Named pipes** are not used, so there is nothing to review there.
- **Real devices.** I did not use the phone (serial `R5CY14EJX6R`) or an emulator. SR-02 (telemetry upload), SR-03 (intent from another app) and the cellular-interface exposure in SR-01 are from the built APK and source, not from observation.
- **No hostile network.** No packet was sent to any other host; no live two-host attack, no ARP/mDNS spoofing, no real scan. Reachability from outside is shown by the code path and address tests, not by an external probe.
- **Crypto.** I relied on `snow`, `spake2`, `ed25519-dalek`, `argon2` and `blake3` as libraries; no formal proof, no review of their internals, no timing measurements. Constant-time comparison is by `subtle` in the API guard; I did not check every fingerprint compare.
- **Fuzzing.** No libFuzzer campaign, only the repository's test suites (passing) and my 3-million-input header mutation run.
- **`mdns-sd` behaviour on the wire** (SR-12) was read, not exercised.
- **Not covered:** the UI crates beyond the message and image paths, the audio DSP/Opus bindings (no unsafe in the workspace code; the C Opus library and `sonora` were not audited), the update ZIP (no in-app updater exists: there is no update endpoint to attack, the ZIP is a manual download), macOS signing and notarization (ad-hoc, documented), and the `docs/gates.md` independent-review gate.
- The working tree was changing underneath me; line numbers are deliberately omitted and findings reference files and symbols.

## 7. Status of fixes

Findings above are left as written. Fixed items (invisible on a home or office LAN on Wi-Fi or Ethernet; tests listed in `docs/net.md` and `docs/engine.md`):

| # | Status | Commit | What changed |
|---|---|---|---|
| SR-01 | Fixed in Rust (Android shell: nothing required) | `15d1337` | Control accept loop, transfer hub and UDP path challenge admit a source only on an eligible interface (`InterfacePolicy`: no tunnel, VPN, container or cellular names) and inside that interface's subnet; checked per connection against `stream.local_addr()`, so Wi-Fi plus Ethernet works. Overlay stays on its own opt-in; loopback only through `lan_and_loopback` (tests). Optional hardening left: bind the Android process to the Wi-Fi `Network`. |
| SR-04 | Partly fixed (the invisible part) | `3793f84` | Per-source (6 per 60 s) and global (24 per 60 s) invite budgets for untrusted callers and a 15 s cooldown after a decline; contacts are never throttled. Default `Anyone` policy and ringing are unchanged: silent unknown callers remain a product decision. |
| SR-11 | Fixed | `15d1337` | `check_ip` refuses cloud-metadata addresses for every input; manual entries refuse broadcast and network addresses; dialed ports from manual, QR, discovery and remote candidates are 1024-65535. |
| SR-12 | Mitigated | `d522293` | `mdns-sd` has no source hook; resolved records keep only addresses on-link with the interface that heard them. Residual: the library still caches datagrams from off-link sources, and an on-link neighbour address can still be named. |

PoC after the fix (`AddressPolicy::lan_only().parse_manual`): `169.254.169.254:80`, `169.254.170.2:80`, `[fd00:ec2::254]:80` are `Reserved`; `192.168.1.255:47800` and `10.255.255.255:47800` are `Broadcast`; `192.168.1.5:80` is `PortRange`; `8.8.8.8:80`, `127.0.0.1:47800` and `[::ffff:8.8.8.8]:80` are unchanged.
