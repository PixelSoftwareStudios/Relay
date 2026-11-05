# Architecture

Relay is a LAN-only voice tool. One per-user engine process (`relayd`) owns devices, identity, peers and call sessions. Clients (CLI, TUI, desktop) control it over a local gRPC API and never carry media. Peers talk through an authenticated signaling channel plus encrypted UDP media.

## Layout

| Path | Role |
|---|---|
| `crates/relay-core` | Sans-IO domain: typed IDs, single-owner state, call lifecycle, routing, PTT leases |
| `crates/relay-protocol` | Wire formats: RTP profile, jitter planner, sequence extension |
| `crates/relay-audio` | Devices (cpal), ring buffers, resampling, Opus, mixer, noise suppression adapters |
| `crates/relay-net` | mDNS discovery, trust, Noise transport, peer signaling |
| `crates/relay-config` | TOML preferences, identity, avatars, diagnostics |
| `crates/relay-api` | Local gRPC contract and server/client bindings |
| `crates/relay-engine` | Daemon assembly of the crates above |
| `crates/relay-platform` | Global hotkeys and push-to-talk driver |
| `crates/relay-client` | Request vocabulary, gRPC wire, endpoint and meter logic, and the `relayd` launcher (`daemon` module), shared by the CLI and desktop clients |
| `apps/relay-daemon` | `relayd` binary: the engine as a standalone per-user process (see `docs/packaging.md`) |
| `apps/relay-cli` | `relay` binary: scriptable CLI and TUI; `relay daemon start/stop/...` manage `relayd`, never embed it |
| `crates/relay-ui` | Shared egui UI (view models, state application, theme, responsive layout, widgets, screens) behind a `Shell` trait; no windowing or runtime (a workspace member, not a default build) |
| `apps/relay-desktop` | Thin native shell over `relay-ui`: eframe window, tray, file dialog, tokio runtime (a workspace member, not a default build) |
| `assets/` | Shared fonts and images |

## Boundaries

- Local IPC is control only; PCM never crosses gRPC.
- Remote peers cannot invoke local engine commands or authorize a microphone.
- Capture only starts after explicit per-call transmit consent.
- Audio callbacks do not allocate, lock, log or perform I/O.
