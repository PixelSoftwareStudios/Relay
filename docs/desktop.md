# Desktop client

Living notes for `apps/relay-desktop`: an optional Tauri 2 + Svelte 5 (TypeScript, Vite, pnpm) window over the local engine API. It is a controller and display only. There is no gRPC-Web, no audio, no PCM and no inference in the client; the daemon owns all of that.

## Build isolation
The root `Cargo.toml` lists `apps/relay-desktop` in `exclude`, so `cargo build` / `cargo test` at the root never need a WebView toolchain. `src-tauri` is a standalone package (empty `[workspace]`, own `Cargo.lock`, edition 2024) that depends on `relay-api` by path and pins the same versions as the root `[workspace.dependencies]` (tokio, tonic, serde, uuid, hex, thiserror, tracing). Tauri is the latest stable 2.x (2.12), not the 3.0 alpha.

| Task | Command (in `apps/relay-desktop`) |
|---|---|
| Install | `pnpm install` |
| Frontend build / typecheck / tests | `pnpm build`, `pnpm check`, `pnpm test` |
| Host tests | `cd src-tauri && cargo test` |
| UI without a daemon (browser) | `pnpm dev:mock` |
| UI in the shell without a daemon | `VITE_RELAY_MOCK=1 pnpm tauri dev` |
| Real app | start the daemon, then `pnpm tauri dev` |
| Debug bundle | `pnpm tauri build --debug --bundles app` |

TypeScript is pinned to 6.x because `svelte-check` does not yet run on TypeScript 7 without an extra native-preview alias.

## Layout
- `src/` Svelte UI. Pure logic (`format.ts`, `transmit.ts`, `resync.ts`) is separate from components; `store.svelte.ts` is the reactive store over a `Bridge` (`types.ts`). `tauri.ts` is the real bridge, `mock.ts` the in-memory one, `bridge.ts` picks one (Tauri unless `VITE_RELAY_MOCK=1` or no Tauri runtime).
- `src-tauri/src` Rust host: `engine.rs` (port traits), `relay_link.rs` (adapter over `RelayClient`), `supervisor.rs`, `apply.rs`/`convert.rs`/`model.rs` (wire to view model), `hub.rs`, `controller.rs` (validation), `commands.rs`, `emitter.rs`, `tray.rs`.
- Tests are separate from sources: `tests/` (vitest) and `src-tauri/tests/` (Rust, fake engine in `tests/common`).
- Shared images live in the repo-root `assets/` (`relay-mark.svg`); Vite aliases `$assets` to it and the icon set in `src-tauri/icons` is generated from it with `tauri icon`. No fonts are bundled; the UI uses the system font stack.

## Command surface
The webview can call exactly these commands, enforced by the app manifest in `build.rs` and `capabilities/main.json` (no fs, shell, http or opener plugins; `withGlobalTauri` off; CSP `default-src 'self'`; a test keeps the manifest and capability in step):

`get_state`, `get_meters`, `start_call`, `accept_call`, `decline_call`, `end_call`, `set_transmit`, `set_call_controls`, `set_global_controls`, `select_devices`, `set_profile`, `upload_avatar`, `hide_window`, `quit_app`.

`Controller` validates before anything reaches the engine: id lengths and hex, known call/peer/available device, finite gains in 0..4, display name 1 to 64 printable characters, avatar PNG/JPEG up to 256 KiB (raw bytes over IPC; the engine decodes and validates). Transmit modes other than listen-only are refused locally unless consent is in the same request; accepting a call never enables the microphone. Call mutations use the lease generation from state; if this client does not hold the call, `AcquireController` runs first without takeover. Errors reach the UI as plain strings and are always shown.

Not implemented in this client: peer trust decisions, mic test, noise-suppression selection, the `InputControl` stream, the telemetry-derived latency figures.

## State flow
`supervise` connects (`RelayClient::connect`, register with `observe` + `call-control`, `snapshot_and_watch`, bounded 256-item watch, optional telemetry at 10 Hz meters / 1 Hz stats) and mirrors snapshot and events into `Hub`. Any watch error, stream end, engine `Resync`, revision gap, queue overflow or incarnation change drops the session, clears engine data (stale calls are never shown as live) and reconnects with 250 ms to 5 s exponential backoff, then replaces everything from a fresh snapshot. Link states: `connecting`, `connected`, `reconnecting`, `offline` (daemon unreachable). The endpoint is re-resolved on each attempt (`RELAY_TCP_PROFILE`, `RELAY_SOCKET`, else the per-user default, as in the CLI).

## Events and cadence
`emitter::run` ticks at 10 Hz: `relay://state` (full structural state, only if changed) and `relay://meters` (only if changed), each with a host-assigned sequence number. Bursts coalesce, so event rate is bounded regardless of engine rate. The store drops stale sequences and, on a jump, refetches `get_state`; it also fetches on start. Meters are discarded while the link is not `connected`. Missing telemetry shows "no signal data", never zero.

## Window, tray, daemon
- Closing the window hides it to the tray when a tray icon exists; without a tray (some Linux desktops) closing quits the app.
- Tray menu: status line, Show, "Quit this app (daemon keeps running)". Tooltip and status text refresh at most once per second and only when the text changes.
- Quitting the app never stops the daemon or ends calls. This client cannot stop the daemon (the API has no shutdown call); the UI says so.

## Push-to-talk and hotkeys (honest state)
The UI shows a call as transmitting only when the engine reports `effective_transmitting`. Push-to-talk shows "armed" until the engine says otherwise, with the engine's blocked reason. This app registers no global hotkey (`tauri-plugin-global-shortcut` is not used: the daemon side owns input so PTT works with the window closed, and an in-app registration would be a second, possibly conflicting source) and has no on-screen talk button yet. Wayland note for any future use: compositors generally refuse global key grabs from ordinary clients, so a plugin-based hotkey would silently not fire there; it would need the XDG GlobalShortcuts portal or compositor binding.

## Verified here (macOS, Rust 1.99.1, Node 24, pnpm 12)
- `pnpm install`, `pnpm build`, `pnpm check` (0 errors), `pnpm test` (23 tests).
- `cargo check`, `cargo clippy --all-targets` clean, `cargo test` (28 tests: controller validation and lease handling, supervisor reconnect/resync, emitter rate bounds, view conversion, capability manifest).
- `pnpm tauri build --debug --bundles app` produced `Relay.app`; the window was not launched or inspected.

## Unmet gates
- No GUI run, screenshot, memory, CPU or startup measurement; none are claimed.
- `RelayLink` is covered only for the unreachable-daemon path; no test against a real engine process yet (the engine was still being built).
- Tray behavior, close-to-tray and the IPC avatar upload are untested outside the compiled bundle; Linux and Windows builds are untried.
- Windows named-pipe transport is an unmet `relay-api` gate, so Windows uses the loopback profile.
- Peer avatars are not displayed: the API carries only a hash, not image bytes, so peers show initials; the own-profile preview is local only.
- Accessibility has only basic semantics (roles, labels, live regions); no audit.
