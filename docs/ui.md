# Shared UI (`relay-ui`)

`crates/relay-ui` holds the Relay screens and everything behind them so more than one shell can reuse them: the desktop app today, an Android app (eframe through `android-activity`) later. It depends on `egui` (no `eframe`, `tray-icon` or `rfd`, no windowing), `relay-client`, `relay-api`, `tokio` (a `Handle` only, never a runtime), `image` and `qrcode`. It is a workspace member but not a default member, like `relay-desktop`.

| Task | Command |
|---|---|
| Tests | `cargo test -p relay-ui` |
| Lints | `cargo clippy -p relay-ui --all-targets --no-deps -- -D warnings` |

## What lives here
- Logic, no toolkit: `engine` (port traits), `relay_link` (adapter over `relay-client`), `supervisor`, `apply` / `convert` / `model` (wire to view model), `hub`, `controller` and `channels` (validation), `emitter` (bounded cadence, tray summary), `format`, `viewmodel`, `avatar` (default image, bounded cache), `imaging` (bounded decode), `qr`, `inbox`, `mock`. The engine link stays here (not in `relay-client`) because it implements the UI's `Engine` port; dependency direction is `relay-ui -> relay-client -> relay-api`.
- Shell glue: `shell` (the `Shell` and `FilePicker` traits), `actions` (`Shared`, `ShellSink`, `Actions`), `tasks` (avatar fetch, connection info, invite, picker jobs on the shell's runtime handle).
- Look and layout: `theme` (colour, spacing and type tokens, light and dark styles; egui's default `ThemePreference::System` follows the OS hint), `layout` (`LayoutClass`, `Page`, touch size).
- Drawing: `ui/` with `RelayApp`, sections (status, calls and call cards, peers, audio, this app), channels and privacy, connection panel with QR texture, profile, textures (avatar cache), `nav` (bottom bar).

## Shell contract
A shell supplies a tokio `Handle` and an `Arc<dyn Shell>`:

| Method | Purpose |
|---|---|
| `pick_image` (`FilePicker`) | Native image chooser; the shell checks the size (1 MiB) before reading and returns `PickedFile { name, bytes }`, or `Ok(None)` on cancel |
| `copy_text` | Clipboard |
| `request_repaint` | Called by writers after new data, never per frame |
| `window_visible` | Hint polled once per frame; meters are produced only while true |
| `show_window`, `hide_window`, `quit` | Window actions; quitting never stops the daemon. Mobile shells may ignore show/hide |
| `hides_to_tray` | Whether closing hides the window, so the UI can say so |

Wiring (see `apps/relay-desktop/src/main.rs`): build `Hub`, `SessionSlot`, `Controller`, `Shared`, then `Actions { controller, hub, shared, runtime, shell }`; spawn `emitter::run` with a `ShellSink` and either `supervisor::supervise(RelayLink::new(resolve_endpoint), ..)` or `mock::run`. Per frame call `RelayApp::begin_frame(ctx)` then `RelayApp::ui(ui)`.

## Responsive layout
`LayoutClass::for_width` picks compact below 600 points (phone: one column, bottom navigation with Calls, People, Channels, Settings, 44 point minimum touch targets and a larger body size) and regular otherwise (desktop: one scrolling column of all sections). The class is chosen in `begin_frame` from the content width; the style is reinstalled only when the class changes (one repaint, no per-frame work). Tests in `crates/relay-ui/tests` cover the breakpoint, touch sizes, themes, every compact page, and run whole frames with a fake `Shell` and no window.

## Rules kept
- Idle zero repaint: the UI never requests a repaint itself; an idle screen asks for none (tested at phone and desktop widths).
- Bounded cadence: emitter at 25 Hz, tray text at most once a second, state marked dirty only when changed.
- Secrets (`relay_client::Secret`) stay redacted in `Debug`; UI-only state is deliberately not `Debug`.

## Not done
- No Android shell exists yet; the touch layout is untested on a device or with a real touch screen. `FilePicker` has no Android implementation (content URIs rather than paths are the expected shape: return the bytes).
- No on-device or screenshot check of the compact layout; only window-less frames run.
- The mobile resource gate (CPU, RSS, battery) is unmeasured.

## Push-to-talk release delay

The Settings section has a slider for the push-to-talk release delay (0 to 2000 ms, step 10, shown as `N ms`; `release_delay_label` renders 0 as "Off (closes at once)"). Dragging keeps the value in `Local::release_delay_edit` and sends one `Controller::set_ptt_release_delay` on release (a `SetGlobalControls` with only `ptt_release_delay_ms`); values above 2000 are refused locally. `Settings.ptt_release_delay_ms` comes from the engine, and `describe_transmit` shows "Releasing" only while the engine reports both `effective_transmitting` and `releasing`. The mock engine round-trips the value. View-model tests: `tests/format.rs`, `tests/controller.rs`, `tests/mock.rs`.
