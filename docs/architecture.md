# Architecture

Relay is a LAN-only voice tool. One per-user engine process (`relayd`) owns devices, identity, peers and call sessions. Clients (CLI, TUI, desktop) control it over a local gRPC API and never carry media. Peers talk through an authenticated signaling channel plus encrypted UDP media.

## Layout

| Path | Role |
|---|---|
| `crates/relay-core` | Sans-IO domain: typed IDs, single-owner state, call lifecycle, routing, PTT leases |
| `crates/relay-protocol` | Wire formats: RTP profile, jitter planner, sequence extension |
| `crates/relay-audio` | Devices (cpal), ring buffers, resampling, Opus, mixer, microphone processing (sonora noise suppression, AGC2, AEC3) |
| `crates/relay-net` | mDNS discovery, trust, Noise transport, peer signaling |
| `crates/relay-io` | OS I/O helpers: atomic and private files, advisory locks, detached spawn and logs, real clock and entropy, per-user dirs, the `FileSink` for received files. No sockets, no audio, no engine/net dependencies, `#![forbid(unsafe_code)]` |
| `crates/relay-config` | TOML preferences, identity, avatars, diagnostics |
| `crates/relay-api` | Local gRPC contract and server/client bindings |
| `crates/relay-engine` | Daemon assembly of the crates above |
| `crates/relay-platform` | Global hotkeys and push-to-talk driver |
| `crates/relay-client` | Request vocabulary, gRPC wire, endpoint and meter logic, and the `relayd` launcher (`daemon` module), shared by the CLI and desktop clients |
| `apps/relay-daemon` | `relayd` binary: the engine as a standalone per-user process (see `docs/packaging.md`) |
| `apps/relay-cli` | `relay` binary: scriptable CLI and TUI; `relay daemon start/stop/...` manage `relayd`, never embed it |
| `crates/relay-ui` | Shared egui UI (view models, state application, theme, responsive layout, widgets, screens) behind a `Shell` trait; no windowing or runtime (a workspace member, not a default build) |
| `apps/relay-desktop` | Thin native shell over `relay-ui`: eframe window, tray, file dialog, tokio runtime (a workspace member, not a default build) |
| `benches/` (`relay-benches`) | Benchmarks and neural-candidate evaluation (tract, FFT); a workspace member, not a default build: use `-p relay-benches` |
| `assets/` | Shared fonts and images |

## Boundaries

- Local IPC is control only; PCM never crosses gRPC.
- Remote peers cannot invoke local engine commands or authorize a microphone.
- Capture only starts after explicit per-call transmit consent. Clients may start the daemon; starting grants nothing.
- A daemon advertises and rings only while a client is attached (an open `WatchState` stream, debounced 2 s); with none it declines invites as busy.
- A call is owned by the controller-lease holder and ends 3 s after its owner detaches (re-attach or an attached takeover keeps it); observers own nothing.
- Audio callbacks do not allocate, lock, log or perform I/O.

## I/O boundary

All OS I/O helpers live in `relay-io`: filesystem (atomic replace with fsync and directory fsync, owner-only 0600/0700 creation, symlink and owner checks, bounded reads, temp files, no-overwrite rename, free space), advisory locks, detached process launch and log files, wall/monotonic clocks, OS entropy (`entropy::random_bytes`/`random_hex`, used for tokens, keys and operation ids; UUIDs only where an id is genuinely a UUID), poison-tolerant locking (`LockExt`/`RwLockExt`: a panicked worker must not wedge the daemon) and per-user directories. Consumers call it and keep no private copies. Pure crates (`relay-core`, `relay-protocol`, `relay-messages`) only declare ports (`Clock`, `IdSource`, `FileSink`); `relay-io` supplies the real implementations where it can depend on them (`relay-messages`), and the engine adapts the rest (`relay-core` has no OS code and `relay-io` does not depend on it). Sockets, mDNS and protocol code stay in `relay-net` and `relay-api` (the socket bind and peer-credential check remain in `relay-api`); audio devices stay in `relay-audio`. `relay-io` contains no `unsafe`: syscalls go through `rustix`, free space through `fs4`. Windows owner-only ACLs are an unmet gate (see `docs/portability.md`).
