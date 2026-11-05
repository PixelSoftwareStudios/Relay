# Desktop client

Living notes for `apps/relay-desktop`: an optional, pure-Rust native window (egui via eframe, `glow` backend) over the local engine API. The app is a thin shell: the UI, view models, supervisor and mock live in `crates/relay-ui` (see `docs/ui.md`). It is a controller and display only. There is no WebView, no gRPC-Web, no audio, no PCM and no inference in the client; the daemon owns all of that. egui and eframe are MIT/Apache-2.0, compatible with this project's Apache-2.0 licence (Slint was ruled out on licensing).

## Build isolation
`apps/relay-desktop` and `crates/relay-ui` are normal workspace members but is deliberately **not** in `default-members`, so a plain `cargo build` / `cargo test` stays free of windowing dependencies. Build it explicitly:

| Task | Command |
|---|---|
| Build / run | `cargo run -p relay-desktop` (daemon running) |
| UI without a daemon | `cargo run -p relay-desktop -- --mock` (or `RELAY_DESKTOP_MOCK=1`) |
| Tests | `cargo test -p relay-ui -p relay-desktop` |
| Lints | `cargo clippy -p relay-ui -p relay-desktop --all-targets --no-deps` |

Dependencies come from `[workspace.dependencies]` (a commented GUI section holds `eframe`, `tray-icon`, `async-trait`, `qrcode`, `rfd`). The endpoint is chosen as in the CLI: `RELAY_TCP_PROFILE`, `RELAY_SOCKET`, else the per-user default, re-resolved on every connect attempt.

## Layout
The desktop crate holds only what needs a window or the OS; everything else is in `crates/relay-ui`.
- `src/main.rs`: tracing, the tokio runtime (2 workers), window icon and viewport, wiring of hub, controller, emitter and supervisor (or the mock), eframe `run_native`.
- `src/app.rs`: `DesktopApp`, the `eframe::App` wrapper. It owns the runtime and the tray, hides to the tray on close, applies tray updates, and calls `RelayApp::begin_frame` / `RelayApp::ui`.
- `src/host.rs`: `DesktopShell`, the `relay_ui::shell::Shell` implementation (egui clipboard and viewport commands, the `rfd` dialog with the size check before reading).
- `src/tray.rs`: `tray-icon` menu, three glyph states, Show / Quit.
- Images used here come from `assets/icon/`: window icon `relay-mark-256.png` and tray glyphs `tray-{idle,call,muted}[@2x].png`. The header logo and `default-avatar.png` are embedded by `relay-ui`. No fonts are bundled.
- Tests in `apps/relay-desktop/tests`: tray glyphs and the picker's bounded read. All logic and UI tests live in `crates/relay-ui/tests`.

## Threading and repaint
A tokio runtime (2 worker threads, owned by `main`/`DesktopApp`; `relay-ui` only gets a `Handle`) runs the supervisor, the emitter and every controller call. The UI thread never blocks on the engine: buttons spawn a controller future, and failures land in `Shared` and are shown in a banner. Writers call `Shell::request_repaint` (egui's `request_repaint` on desktop); the UI never requests one itself and has no animations, so an idle app is not redrawn.

`emitter::run` ticks at 25 Hz (40 ms, under the 30 Hz ceiling) and publishes the newest state and meters, only if changed, so bursts coalesce. `Hub::update` marks state dirty only when it actually changed, so a daemon that stays down (retry every 5 s) causes no repaints. Meters are produced only while the window is visible: minimized or hidden windows set `Hub::set_visible(false)`, and the hub then drops meter writes. Tray text refreshes at most once a second and only when it changes.

## State flow and resync
`supervise` connects (`RelayClient::connect`, register with `observe` + `call-control`, `snapshot_and_watch`, bounded 256-item watch, optional telemetry at 10 Hz meters / 1 Hz stats). Any watch error, stream end, engine `Resync`, revision gap, queue overflow or incarnation change drops the session, clears engine data (stale calls are never shown as live) and reconnects with 250 ms to 5 s exponential backoff, then replaces everything from a fresh snapshot. Link states: `connecting`, `connected`, `reconnecting`, `offline`. Missing telemetry shows "no signal data", never zero.

## Controller rules
`Controller` validates before anything reaches the engine: id lengths and hex, known call / peer / available device, finite gains in 0..4, display name 1 to 64 printable characters, avatar PNG or JPEG up to 1 MiB (the engine decodes and validates for real). Transmit modes other than listen-only are refused unless consent is in the same request; accepting a call never enables the microphone. Call mutations use the lease generation from state; if this client does not hold the call, `AcquireController` runs first without takeover.

Peer and local avatars arrive from `GetAvatar` as lossless WebP (`AvatarImage.media_type`); `imaging::decode_for_texture` decodes them with the `image` crate (WebP enabled), under the same dimension and allocation limits as other images. The upload picker still offers PNG and JPEG only, although the engine also accepts WebP, GIF, BMP and TIFF.

Not implemented: peer trust decisions, mic test, noise-suppression selection, the `InputControl` stream, latency figures.

## Window, tray, daemon
- With a tray icon (macOS, Windows) closing the window hides it; the tray menu has Show and "Quit this app (daemon keeps running)". Tray quit exits the process directly.
- The tray glyph has three states: idle, in call (ringing or connected), and muted (global mute, shown only while connected; it wins over in-call). They are monochrome template images; macOS gets the `@2x` set and tints them, other platforms the 1x set. The glyph and tooltip are updated only when the tray summary changes, at most once a second.
- On Linux no tray is built (`tray-icon` needs a GTK main loop this client does not run), so closing the window quits the app. The UI states which behaviour applies.
- Quitting the app never stops the daemon or ends calls. The API has no shutdown call, so this client cannot stop the daemon; the UI says so.

## Connection panel
Collapsed by default. While open it asks the daemon once (`GetConnectionInfo`) and shows the connection string (selectable, with a Copy button via `Shell::copy_text`), addresses, port and fingerprint, and a QR code. The QR is encoded with `qrcode` (no image feature) and uploaded as a texture only while the panel is open; closing it drops the texture and the cached string. A paste field calls a `relay://` string: `relay_client::connection::check` rejects obvious typos locally (scheme, `#fingerprint` of 64 hex, literal `ip:port`), and the engine applies the real LAN-only policy and key pinning. A connection string grants no trust and never turns a microphone on.

## Avatars
Peer and own avatars are fetched lazily with `GetAvatar` the first time a hash is drawn (`Textures::texture`), decoded once off the UI thread with size and allocation limits (`imaging.rs`, scaled to at most 256 px), and kept in a 64-entry LRU texture cache. Pending and failed hashes show `default-avatar.png`; a failure is remembered so the engine is not asked again until the entry is evicted. The own profile uses the shell's `FilePicker` (the native `rfd` dialog on desktop, xdg-portal on Linux), checks the size before reading, previews the picked image, and uploads only on confirmation. The engine has no request to remove an avatar, so "Discard" only drops a pending choice.

## Channels and privacy
The channel list comes from `Snapshot.channels` and `StateEvent.channel`/`Removed(kind=channel)`. The UI can create (name, listed, password, invite only), join (invite token, or channel id plus password), leave (two-step), mint invites (owner/admin; lifetime and uses from fixed choices; the token is shown until dismissed with a Copy button), and revoke a proven member (two-step; the peer id is used as the member fingerprint). Peers show badges for channels with proven shared membership. Privacy: inbound policy (anyone / only members of my channels) and network visibility (visible / hidden). Passwords and invite tokens are `relay_client::Secret` values: `Debug` prints `Secret(<redacted>)`, they live in form fields only until submitted, and the view model and logs never contain them. The channel security review gate in `docs/channels.md` stays open.

## Push-to-talk and hotkeys (honest state)
A call is shown as transmitting only when the engine reports `effective_transmitting`. Push-to-talk shows "armed" until then, with the engine's blocked reason. This app registers no global hotkey (the daemon side owns input so PTT works with the window closed) and has no on-screen talk button. Wayland note for any future in-app hotkey: compositors generally refuse global key grabs from ordinary clients; it would need the XDG GlobalShortcuts portal.

## Verified here (macOS, Rust 1.99.1)
- `cargo check`, `cargo clippy -p relay-ui -p relay-desktop --all-targets --no-deps` clean; `cargo test -p relay-ui -p relay-desktop`: 89 tests (controller and channel validation, secrets absent from `Debug`, supervisor reconnect and resync, emitter rate bounds, visibility and tray state, texture cache bounds, bounded image decode, QR layout, tray glyphs, view models, mock mode). `cargo test -p relay-client` covers the new wire commands against a fake engine.
- The debug binary in `--mock` mode starts and stays up for several seconds.

## Unmet gates
- **Resource gate: unmet.** No new measurement was taken for this version. The earlier indicative sample (about 42 MB RSS, roughly 0.01 s CPU in 10 s, release build, no daemon, window untouched) predates the avatar cache, QR, channels and tray states and is not a gate result. Required: process CPU and RSS idle (connected, no calls) and active (one connected call, visible, meters at the cadence ceiling) on each platform, plus texture-cache growth with many peers.
- No GUI interaction, screenshot or accessibility audit; the window, the native file dialog, the clipboard, the QR, the three tray glyphs and close-to-tray were not observed by a person. macOS template tinting of the `@2x` glyphs is unverified. `rfd` on macOS and Wayland dialogs from a tokio task are untested.
- Tests use a fake engine and the mock; nothing was run against a real engine for GetAvatar, GetConnectionInfo or the channel RPCs (the engine side was still being built). `RelayLink` itself is covered only on the unreachable-daemon path.
- The engine offers no way to clear a stored avatar. Channel changes reach the UI only through snapshot and channel events, so counts such as shared peers can lag until the engine emits one.
- Revoke assumes the peer id is the member fingerprint the engine expects; confirm against the engine.
- Linux and Windows builds are untried. Windows uses the loopback profile until the `relay-api` named pipe exists.
- Root `Cargo.lock` changes for `qrcode` and `rfd` are not committed by this work package (the file is being edited by other agents).
