# relay-platform

Headless global hotkeys for the CLI/TUI and the desktop app. No GUI dependency. Turning key
edges into push-to-talk (PTT) leases is `relay_client::ptt::PttWorker` (`docs/client.md`). Status: implemented and unit-tested with fakes; **real key delivery is an
unmet manual gate** (below).

## Modules

| Module | Role |
|---|---|
| `hotkey` | `HotkeyBackend` trait (`probe`, `start`, `stop`), `BackendStatus` (`Available`, `NeedsPermission(kind)`, `Unsupported(reason)`), `detect_backend()`. `start` refuses unless the probe says `Available`. |
| `hotkey::GlobalHotkeyBackend` | macOS, Windows and X11 through `global-hotkey`. |
| `hotkey::PortalBackend` | Wayland via the XDG GlobalShortcuts portal (`ashpd`), Linux only, cargo feature `wayland`. |
| `hotkey::macos` | Main-thread check, run-loop pump, Accessibility / Input Monitoring status. |
| `session` | `SessionEvent`, macOS screen-lock monitor, `lock_detection()` coverage. |
| `testing` | `FakeBackend`/`FakeKeyboard`. |

## Event-loop rules

- Windows: the manager is created on a dedicated `relay-hotkey` thread that pumps the win32 message queue.
- X11: `global-hotkey` runs its own grab thread; our worker only forwards events.
- macOS: the manager must live on the **main thread**, and the main run loop must be serviced.
  `start` returns `NotMainThread` otherwise. A CLI should run tokio on other threads and call
  `hotkey::run_main_loop(|| keep_going)` on the main thread.
- Wayland: the portal backend owns a current-thread runtime on its own thread. Probing connects to
  the session bus and fails honestly (`Unsupported`) when the interface is absent. Without the
  `wayland` feature, a Wayland session reports `Unsupported` (X11 grabs would see nothing under
  Wayland). The compositor may ask the user to confirm or rebind the shortcut; the preferred
  trigger is only a hint.

## Permissions (macOS)

`hotkey::macos::permissions()` reports Accessibility (`AXIsProcessTrusted`) and Input Monitoring
(`CGPreflightListenEventAccess`) without prompting. The backend is conservative: if Input
Monitoring is not granted the probe says `NeedsPermission(InputMonitoring)` and `start` refuses,
even though Carbon hotkeys may work without it on some releases. Permissions attach to the
hosting terminal app, not the `relay` binary.

## From key edges to a lease

The backend's `HotkeyStream` is consumed by whoever owns the key: the CLI (`apps/relay-cli/src/hotkey`) and the
desktop app (`apps/relay-desktop/src/hotkey.rs`) both press and release one `relay_client::ptt::PttWorker`
(`docs/client.md`, "Push-to-talk worker"). There is no second lease loop here any more.

`detect_backend()` is for a terminal program that pumps the run loop itself. `detect_app_backend()` is for a
GUI app whose event loop owns the main thread (eframe on macOS and Windows): on macOS the key is a Carbon
`RegisterEventHotKey` hotkey delivered through that application's event loop, which needs neither Input
Monitoring nor Accessibility (only the media-key path of `global-hotkey` uses an event tap), so the probe does
not ask for them there. Registering and unregistering happen on the main thread; the desktop app calls them
from its frame logic.

## Lock / suspend detection

| OS | Lock | Suspend |
|---|---|---|
| macOS | polled (250 ms) via `CGSessionCopyCurrentDictionary` | not detected |
| Linux, Windows | not implemented | not implemented |

Where undetected, a key-up lost to lock/suspend leaves the lease to lapse within `lease_ms`.
`session::lock_detection()` says which case applies.

## Tests

`cargo test -p relay-platform` (tests/hotkey.rs): honest status for unavailable fakes, the fake backend's edges,
the detected backend's probe, and lock-detection coverage. The lease tests moved to `relay-client` with the worker.
`--features wayland` on Linux and the Windows target were only type-checked from macOS.

## Unmet gates

- **Manual: real key delivery.** Press/release delivery through `global-hotkey` (macOS, Windows,
  X11) and the Wayland portal (GNOME, KDE) has not been exercised; headless CI cannot do it.
  Check key-up delivery, held-key behaviour, permission prompts and compositor rebinding by hand.
- Runtime test of the `wayland` feature and the Windows message pump (compile-checked only).
- Lock/suspend detection on Windows and Linux; suspend on macOS.
- End-to-end test against a real engine `InputControl` handler (engine side not built yet).
