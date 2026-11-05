# Desktop client

Living notes for `apps/relay-desktop`: an optional, pure-Rust native window (egui via eframe, `glow` backend) over the local engine API. It is a controller and display only. There is no WebView, no gRPC-Web, no audio, no PCM and no inference in the client; the daemon owns all of that. egui and eframe are MIT/Apache-2.0, compatible with this project's Apache-2.0 licence (Slint was ruled out on licensing).

## Build isolation
`apps/relay-desktop` is a normal workspace member but is deliberately **not** in `default-members`, so a plain `cargo build` / `cargo test` stays free of windowing dependencies. Build it explicitly:

| Task | Command |
|---|---|
| Build / run | `cargo run -p relay-desktop` (daemon running) |
| UI without a daemon | `cargo run -p relay-desktop -- --mock` (or `RELAY_DESKTOP_MOCK=1`) |
| Tests | `cargo test -p relay-desktop` |
| Lints | `cargo clippy -p relay-desktop --all-targets` |

Dependencies come from `[workspace.dependencies]` (a commented GUI section holds `eframe`, `tray-icon`, `async-trait`). The endpoint is chosen as in the CLI: `RELAY_TCP_PROFILE`, `RELAY_SOCKET`, else the per-user default, re-resolved on every connect attempt.

## Layout
Logic is kept apart from drawing so it runs window-less in `tests/`.
- Logic: `engine.rs` (port traits), `relay_link.rs` (adapter over `RelayClient`), `supervisor.rs`, `apply.rs` / `convert.rs` / `model.rs` (wire to view model), `hub.rs`, `controller.rs` (validation), `emitter.rs`, `format.rs` (text, transmit status), `viewmodel.rs` (cards and rows), `mock.rs`.
- Drawing: `ui/` (`mod.rs` window glue, `sections.rs`, `widgets.rs`), `shell.rs` (tokio to egui glue, `Actions`), `tray.rs`, `main.rs`.
- Shared client logic (wire commands, operation settling, endpoint resolution, meter scaling) lives in `crates/relay-client`; see `docs/client.md`.
- `assets/icon/default-avatar.png` is embedded (`avatar.rs`) and drawn for any profile, local or peer, without an avatar hash; it is never sent over the network. The view model resolves this (`Avatar::Placeholder`). The logo `assets/icon/relay-mark.png` (source `assets/icon/relay-mark.svg`) is embedded with `include_bytes!` and used for the window, tray and header. egui's built-in fonts are used; no fonts are bundled.

## Threading and repaint
A tokio runtime (2 worker threads) runs the supervisor, the emitter and every controller call. The UI thread never blocks on the engine: buttons spawn a controller future, and failures land in `Shared` and are shown in a banner. Writers call `egui::Context::request_repaint`; the UI never requests one itself and has no animations, so an idle app is not redrawn.

`emitter::run` ticks at 25 Hz (40 ms, under the 30 Hz ceiling) and publishes the newest state and meters, only if changed, so bursts coalesce. `Hub::update` marks state dirty only when it actually changed, so a daemon that stays down (retry every 5 s) causes no repaints. Meters are produced only while the window is visible: minimized or hidden windows set `Hub::set_visible(false)`, and the hub then drops meter writes. Tray text refreshes at most once a second and only when it changes.

## State flow and resync
`supervise` connects (`RelayClient::connect`, register with `observe` + `call-control`, `snapshot_and_watch`, bounded 256-item watch, optional telemetry at 10 Hz meters / 1 Hz stats). Any watch error, stream end, engine `Resync`, revision gap, queue overflow or incarnation change drops the session, clears engine data (stale calls are never shown as live) and reconnects with 250 ms to 5 s exponential backoff, then replaces everything from a fresh snapshot. Link states: `connecting`, `connected`, `reconnecting`, `offline`. Missing telemetry shows "no signal data", never zero.

## Controller rules
`Controller` validates before anything reaches the engine: id lengths and hex, known call / peer / available device, finite gains in 0..4, display name 1 to 64 printable characters, avatar PNG or JPEG up to 1 MiB (the engine decodes and validates for real). Transmit modes other than listen-only are refused unless consent is in the same request; accepting a call never enables the microphone. Call mutations use the lease generation from state; if this client does not hold the call, `AcquireController` runs first without takeover.

Peer and local avatars arrive from `GetAvatar` as lossless WebP (`AvatarImage.media_type`); `imaging::decode_for_texture` decodes them with the `image` crate (WebP enabled), under the same dimension and allocation limits as other images. The upload picker still offers PNG and JPEG only, although the engine also accepts WebP, GIF, BMP and TIFF.

Not implemented: peer trust decisions, mic test, noise-suppression selection, the `InputControl` stream, latency figures. The profile image is chosen by typing a file path (no native file dialog dependency); there is no preview.

## Window, tray, daemon
- With a tray icon (macOS, Windows) closing the window hides it; the tray menu has Show and "Quit this app (daemon keeps running)". Tray quit exits the process directly.
- On Linux no tray is built (`tray-icon` needs a GTK main loop this client does not run), so closing the window quits the app. The UI states which behaviour applies.
- Quitting the app never stops the daemon or ends calls. The API has no shutdown call, so this client cannot stop the daemon; the UI says so.

## Push-to-talk and hotkeys (honest state)
A call is shown as transmitting only when the engine reports `effective_transmitting`. Push-to-talk shows "armed" until then, with the engine's blocked reason. This app registers no global hotkey (the daemon side owns input so PTT works with the window closed) and has no on-screen talk button. Wayland note for any future in-app hotkey: compositors generally refuse global key grabs from ordinary clients; it would need the XDG GlobalShortcuts portal.

## Verified here (macOS, Rust 1.99.1)
- `cargo check`, `cargo clippy -p relay-desktop --all-targets` clean; `cargo test -p relay-desktop`: 42 tests (controller validation and leases, supervisor reconnect and resync, emitter rate bounds and visibility, conversion and bounds, view models, transmit text, mock mode).
- The release binary starts and stays up.

## Unmet gates
- **Resource gate: unmet.** Not measured against a real workload. Only an indicative sample exists: release build, no daemon (offline), window not interacted with, `ps` on macOS, about 42 MB RSS and roughly 0.01 s CPU over 10 s after start. Idle with a daemon, and active with calls and meters at the ceiling, are unmeasured; so are GPU, startup time and Linux/Windows. Required before claiming: process CPU and RSS idle (connected, no calls) and active (one connected call, visible), on each platform.
- No GUI interaction, screenshot or accessibility audit was done; the window, tray and close-to-tray were not observed by a person.
- `RelayLink` is tested only on the unreachable-daemon path, not against a real engine process.
- Linux and Windows builds are untried. Windows uses the loopback profile until the `relay-api` named pipe exists.
- Peer avatars are not shown (the API carries a hash, not bytes); peers show the default avatar.
- Root `Cargo.lock` additions for eframe are not committed by this work package (the file is being edited by other agents).
