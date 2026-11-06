# Desktop client

Living notes for `apps/relay-desktop`: an optional, pure-Rust native window (egui via eframe, `glow` backend) over the local engine API. The app is a thin shell: the UI, view models, supervisor and mock live in `crates/relay-ui` (see `docs/ui.md`). It is a controller and display only. There is no WebView, no gRPC-Web, no audio, no PCM and no inference in the client; the daemon owns all of that. egui and eframe are MIT/Apache-2.0, compatible with this project's Apache-2.0 licence (Slint was ruled out on licensing).

## Build isolation
`apps/relay-desktop` and `crates/relay-ui` are default workspace members, so a plain `cargo build` and `cargo test` include the desktop app, and `cargo run` launches it (`default-run` in its manifest). The build also produces `relayd` next to it, which the desktop starts when no daemon is running. Run the terminal client with `cargo run -p relay-cli`. The terminal-only crates (`relay-cli`, `relay-daemon`) never depend on the GUI crates; build just those with `cargo build -p relay-cli -p relay-daemon`. To run the desktop app:

| Task | Command |
|---|---|
| Build / run | `cargo run` (starts `relayd` if none is running) |
| UI without a daemon | `cargo run -p relay-desktop -- --mock` (or `RELAY_DESKTOP_MOCK=1`) |
| Tests | `cargo test -p relay-ui -p relay-desktop` |
| Lints | `cargo clippy -p relay-ui -p relay-desktop --all-targets --no-deps` |

Dependencies come from `[workspace.dependencies]` (a commented GUI section holds `eframe`, `tray-icon`, `async-trait`, `qrcode`, `rfd`). The endpoint is chosen as in the CLI: `RELAY_TCP_PROFILE`, `RELAY_SOCKET`, else the per-user default, re-resolved on every connect attempt.

## Layout
The desktop crate holds only what needs a window or the OS; everything else is in `crates/relay-ui`.
- `src/main.rs`: tracing, the tokio runtime (2 workers), window icon and viewport, wiring of hub, controller, emitter and supervisor (or the mock), eframe `run_native`.
- `src/app.rs`: `DesktopApp`, the `eframe::App` wrapper. It owns the runtime and the tray, hides to the tray on close, applies tray updates, and calls `RelayApp::begin_frame` / `RelayApp::ui`.
- `src/host.rs`: `DesktopShell`, the `relay_ui::shell::Shell` implementation (egui clipboard and viewport commands, the `rfd` dialog with the size check before reading).
- `src/host.rs` also implements the messaging dialogs: `pick_directory` (download folder), `save_as` (export), `pick_file` (file offer) with `rfd`, returning paths only, and `open_url` (egui `OpenUrl`, after the link confirmation).
- `src/tray.rs`: `tray-icon` menu, three glyph states, Show / Quit.
- Images used here come from `assets/icon/`: window icon (`relay-mark-256.png`; on macOS `relay-app-512.png`, the same mark inside the macOS icon grid with its transparent margin and shadow, so the Dock icon is not oversized) and tray glyphs `tray-{idle,call,muted}[@2x].png`. The header logo and `default-avatar.png` are embedded by `relay-ui`. Fonts come from `assets/font` (Noto Sans and CJK/Devanagari subsets, about 3.1 MB, OFL 1.1), embedded and installed by `relay-ui` (`fonts::install`, see `docs/ui.md`); the desktop app adds nothing.
- Tests in `apps/relay-desktop/tests`: tray glyphs and the picker's bounded read. All logic and UI tests live in `crates/relay-ui/tests`.

## Threading and repaint
A tokio runtime (2 worker threads, owned by `main`/`DesktopApp`; `relay-ui` only gets a `Handle`) runs the supervisor, the emitter and every controller call. The UI thread never blocks on the engine: buttons spawn a controller future, and failures land in `Shared` and are shown in a banner. Writers call `Shell::request_repaint` (egui's `request_repaint` on desktop); the UI never requests one itself and has no animations, so an idle app is not redrawn.

`emitter::run` ticks at 25 Hz (40 ms, under the 30 Hz ceiling) and publishes the newest state and meters, only if changed, so bursts coalesce. `Hub::update` marks state dirty only when it actually changed, so a daemon that stays down (retry every 5 s) causes no repaints. Meters are produced only while the window is visible: minimized or hidden windows set `Hub::set_visible(false)`, and the hub then drops meter writes. Tray text refreshes at most once a second and only when it changes.

## State flow and resync
`supervise` connects (`RelayClient::connect`, register with `observe` + `call-control`, `snapshot_and_watch`, bounded 256-item watch, optional telemetry at 10 Hz meters / 1 Hz stats). Any watch error, stream end, engine `Resync`, revision gap, queue overflow or incarnation change drops the session, clears engine data (stale calls are never shown as live) and reconnects with 250 ms to 5 s exponential backoff, then replaces everything from a fresh snapshot. Link states: `connecting`, `connected`, `reconnecting`, `offline`. Missing telemetry shows "no signal data", never zero.

## Controller rules
`Controller` validates before anything reaches the engine: id lengths and hex, known call / peer / available device, finite gains in 0..4, display name 1 to 64 printable characters, avatar of PNG, JPEG, WebP, GIF, BMP or TIFF, recognised from the bytes (not the file name), up to 1 MiB and 2048 x 2048, with the engine's refusal wording (`imaging::avatar_check`; the engine decodes and validates for real). Transmit modes other than listen-only are refused unless consent is in the same request; accepting a call never enables the microphone. Call mutations use the lease generation from state; if this client does not hold the call, `AcquireController` runs first without takeover.

Peer and local avatars arrive from `GetAvatar` as lossless WebP (`AvatarImage.media_type`); `imaging::decode_for_texture` decodes them with the `image` crate (WebP enabled), under the same dimension and allocation limits as other images. The upload picker offers all six formats the engine accepts; the host reads the file only after the size check, then sniffs it.

Not implemented: peer trust decisions (a peer's short fingerprint is shown on the incoming-call prompt and in the peer list, and `ClearAvatar` is wired; there is no verify or `ForgetPeer` button), mic test, noise-suppression selection, the `InputControl` stream, latency figures.

## Window, tray, daemon
- With a tray icon (macOS, Windows) closing the window hides it; the tray menu has Show and "Quit this app (daemon keeps running)". Tray quit exits the process directly.
- The tray glyph has three states: idle, in call (ringing or connected), and muted (global mute, shown only while connected; it wins over in-call). They are monochrome template images; macOS gets the `@2x` set and tints them, other platforms the 1x set. The glyph and tooltip are updated only when the tray summary changes, at most once a second.
- On Linux no tray is built (`tray-icon` needs a GTK main loop this client does not run), so closing the window quits the app. The UI states which behaviour applies.
- Quitting the app never stops the daemon, but it hangs up the calls this app owns first (bounded to 2 s); calls other clients own are untouched. With live calls the app asks first: "Quit this app? Your active call will be hung up." Closing the window only hides it (calls keep going). Offline, the status banner offers "Start daemon" (the launcher in `relay-client` runs `relayd`; the `relay` binary is not needed); connected, Settings has "Quit daemon..." behind a confirmation, which registers a separate `admin` client for `Shutdown` and is distinct from "Quit this app". `Shell` gains `can_manage_daemon`, `start_daemon` and `stop_daemon` (phones answer no).

## Daemon auto-start
`crates/relay-ui/src/autostart.rs` is the policy; `supervisor::supervise(engine, hub, slot, shell, autostart)` applies it. When no daemon answers and the shell can manage one, the supervisor shows `Link::Starting` ("Starting the Relay daemon") and calls `Shell::start_daemon`. A daemon that dies is restarted with backoff (1, 2 s); three starts within 60 s end it: the banner says the daemon keeps stopping with the launcher's reason and log tail, and only the "Start daemon" button resumes. "Quit daemon..." marks the session user-quit, so nothing restarts it until "Start daemon". The "Start the daemon automatically" checkbox (Settings, this app) is `clients.autostart_daemon`, stored through `SetGlobalControls`; while no daemon is reachable the default (on) applies. `RELAY_NO_AUTOSTART=1` overrides it. Auto-start never grants capture or consent. The app holds a `WatchState` stream while connected, which is what makes the daemon advertise and ring; hidden to the tray it still counts as attached.

## Connection panel
Collapsed by default. While open it asks the daemon once (`GetConnectionInfo`) and shows the connection string (selectable, with a Copy button via `Shell::copy_text`), addresses, port and fingerprint, and a QR code. The QR is encoded with `qrcode` (no image feature) and uploaded as a texture only while the panel is open; closing it drops the texture and the cached string. A paste field calls a `relay://` string: `relay_client::connection::check` rejects obvious typos locally (scheme, `#fingerprint` of 64 hex, literal `ip:port`), and the engine applies the real LAN-only policy and key pinning. A connection string grants no trust and never turns a microphone on.

## Avatars
Peer and own avatars are fetched lazily with `GetAvatar` the first time a hash is drawn (`Textures::texture`), decoded once off the UI thread with size and allocation limits (`imaging.rs`, scaled to at most 256 px), and kept in a 64-entry LRU texture cache. Pending and failed hashes show `default-avatar.png`; a failure is remembered so the engine is not asked again until the entry is evicted. The own profile uses the shell's `FilePicker` (the native `rfd` dialog on desktop, xdg-portal on Linux), checks the size before reading, previews the picked image, and uploads only on confirmation. "Discard" only drops a pending choice; "Remove picture" (shown when an avatar is stored and nothing is pending) calls `ClearAvatar`, after which trusted peers are told to stop showing it.

## Channels and privacy
The channel list comes from `Snapshot.channels` and `StateEvent.channel`/`Removed(kind=channel)`. The UI can create (name, listed, password, invite only), join (invite token, or channel id plus password), leave (two-step), mint invites (owner/admin; lifetime and uses from fixed choices; the token is shown until dismissed with a Copy button), and revoke a proven member (two-step; the peer id is used as the member fingerprint). Peers show badges for channels with proven shared membership. Privacy: inbound policy (anyone / only members of my channels) and network visibility (visible / hidden). Passwords and invite tokens are `relay_client::Secret` values: `Debug` prints `Secret(<redacted>)`, they live in form fields only until submitted, and the view model and logs never contain them. The channel security review gate in `docs/channels.md` stays open.

## Message times

Message rows show local time (`HH:MM:SS`). `DesktopShell` does not override `Shell::time_zone`, so the system zone is used (read when the Messages panel is created; a zone change while the app runs shows after a restart). The hover text shows the full local date and offset plus the UTC clock.

## Push-to-talk and hotkeys (honest state)
A call is shown as transmitting only when the engine reports `effective_transmitting`. Push-to-talk shows "armed" until then, with the engine's blocked reason. This app registers no global hotkey (the daemon side owns input so PTT works with the window closed) and has no on-screen talk button. The hotkey itself is edited in Settings, "Push-to-talk hotkey" (`docs/ui.md`): it is saved through `SetSettings` and takes effect after a restart of whatever registers it. Wayland note for any future in-app hotkey: compositors generally refuse global key grabs from ordinary clients; it would need the XDG GlobalShortcuts portal.

## Verified here (macOS, Rust 1.99.1)
- `cargo check`, `cargo clippy -p relay-ui -p relay-desktop --all-targets --no-deps` clean; `cargo test -p relay-ui -p relay-desktop`: 89 tests (controller and channel validation, secrets absent from `Debug`, supervisor reconnect and resync, emitter rate bounds, visibility and tray state, texture cache bounds, bounded image decode, QR layout, tray glyphs, view models, mock mode). `cargo test -p relay-client` covers the new wire commands against a fake engine.
- The debug binary in `--mock` mode starts and stays up for several seconds.

## Unmet gates
- **Resource gate: unmet.** No new measurement was taken for this version. The earlier indicative sample (about 42 MB RSS, roughly 0.01 s CPU in 10 s, release build, no daemon, window untouched) predates the avatar cache, QR, channels and tray states and is not a gate result. Required: process CPU and RSS idle (connected, no calls) and active (one connected call, visible, meters at the cadence ceiling) on each platform, plus texture-cache growth with many peers.
- No GUI interaction, screenshot or accessibility audit; the window, the native file dialog, the clipboard, the QR, the three tray glyphs and close-to-tray were not observed by a person. macOS template tinting of the `@2x` glyphs is unverified. `rfd` on macOS and Wayland dialogs from a tokio task are untested.
- Tests use a fake engine and the mock; nothing was run against a real engine for GetAvatar, GetConnectionInfo or the channel RPCs (the engine side was still being built). `RelayLink` itself is covered only on the unreachable-daemon path.
- Channel changes reach the UI only through snapshot and channel events, so counts such as shared peers can lag until the engine emits one.
- Revoke assumes the peer id is the member fingerprint the engine expects; confirm against the engine.
- Linux and Windows builds are untried. Windows uses the loopback profile until the `relay-api` named pipe exists.
- Root `Cargo.lock` changes for `qrcode` and `rfd` are not committed by this work package (the file is being edited by other agents).

## Incoming-call notifications and start at login
Both are desktop-only switches in Settings, "This app", drawn from `relay_ui::host_options::HostOptions` (the shell supplies them; phones show nothing).
- **Notify on incoming calls** (default on, stored in `desktop.toml` in the per-user config directory; a damaged file means the defaults). `notify::NotifySink` wraps the emitter's sink: when a call newly enters `IncomingRinging`, the link is connected, the switch is on and the window is hidden, minimized or not focused (`DesktopShell::in_front`), `notify-rust` shows "Incoming Relay call" with the caller's display name (control characters removed, 64 characters at most, "Unknown caller" if empty) and nothing else: never message text, never an address. A call is announced once; a call that rang while the window was in front is not announced after the user looks away. The OS call runs on its own short-lived thread. When the system refuses, the reason is kept and the Settings text says so (calls still ring through the daemon and show in the window and the tray tooltip); the next success clears it. `Notifier` is a trait, so the tests use a recording fake (`tests/notify.rs`).
- **Start this app at login** (default off, `auto-launch`: a LaunchAgent on macOS, the `Run` key on Windows, an XDG autostart entry on Linux). The shown status is what the OS reports after each change (`LoginItem::refresh`), including `Unsupported(reason)` and `Failed(reason)`; it never claims more than the OS said. It starts the app (its window opens), not the daemon. Not verified on a real login here (`tests/login.rs` uses a fake backend).

## Session messaging
`main` spawns `relay_ui::messaging::supervisor::supervise` (own connection, `messages` capability only, opt-in) next to the state supervisor, or `messaging::mock::run` with `--mock`. The Messages panel is a right side panel at 600 points and wider and a bottom-nav page below; the default window is 720 pt wide, so it starts in the regular layout (narrowing it below 600 pt switches to the page form). See `docs/ui.md` ("Session messaging UI"). Not verified by a person: native folder/save/file dialogs, clipboard paste (`ViewportCommand::RequestPaste`) and opening links in a browser; only window-less frames and the mock were run. Link opening is the one place `open_url` is called, after the host confirmation.

## Language
The language is a desktop-only preference stored with the notification switch in `desktop.toml` (`language = "de"`; absent or unknown follows the system language via `sys-locale`, English if unsupported; the picker's first entry "System (<detected>)" removes the key and detects again). `DesktopOptions::new` takes a locale reader (`SystemLocales`) so tests inject it, and calls it again when "System" is picked; `main` sets `i18n::set_active` before the tray is built. `DesktopShell` reports it through `HostOptions::language()` (with `language_choice()`, `system_language()` and `set_language_choice(Option<Language>)`); the UI applies a change on the next frame, the tray relabels its menu and tooltip when the next `TraySummary` (which carries the language) arrives, and notification title and unknown-caller text are worded in the language active when the call rings. Native dialog titles and the file-error text use `tr!` too. See `docs/i18n.md`.


## App icon (macOS bundle)

`assets/icon/relay.icns` holds every size macOS asks for (16 to 512 pt at 1x and 2x, so 16 to 1024 px), built from `assets/icon/relay-app-macos.svg` by `assets/icon/make-icns.sh` (needs `iconutil` and `resvg`). The packaged `.app` bundle uses it as `CFBundleIconFile`; `cargo run` uses the padded PNG set at runtime. Keep the SVG as the source and regenerate both outputs with the script.
