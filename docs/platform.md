# relay-platform

Headless global hotkeys and push-to-talk (PTT) lease driving for the CLI/TUI. No GUI
dependency. Status: implemented and unit-tested with fakes; **real key delivery is an
unmet manual gate** (below).

## Modules

| Module | Role |
|---|---|
| `hotkey` | `HotkeyBackend` trait (`probe`, `start`, `stop`), `BackendStatus` (`Available`, `NeedsPermission(kind)`, `Unsupported(reason)`), `detect_backend()`. `start` refuses unless the probe says `Available`. |
| `hotkey::GlobalHotkeyBackend` | macOS, Windows and X11 through `global-hotkey`. |
| `hotkey::PortalBackend` | Wayland via the XDG GlobalShortcuts portal (`ashpd`), Linux only, cargo feature `wayland`. |
| `hotkey::macos` | Main-thread check, run-loop pump, Accessibility / Input Monitoring status. |
| `ptt` | `PttDriver`, `LeaseSink` (+ `GrpcLeaseSink` over `RelayClient`), `PttState`, `PttTarget`. |
| `session` | `SessionEvent`, macOS screen-lock monitor, `lock_detection()` coverage. |
| `testing` | `FakeBackend`/`FakeKeyboard`, `FakeLeaseSink` with scripted replies. |

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

## PTT driver

`PttDriver::new(sink, client_context, cfg)` then `run(hotkey_stream, inputs)`; observe
`watch::Receiver<PttState>`. Inputs: `PttInput::Target(Option<PttTarget>)` (from
`PttTarget::from_call`, only when this client is the call's controller),
`PttInput::Session(SessionEvent)`, `Shutdown`.

- Press sends `INPUT_ACTION_PRESS` with `requested_lease_ms` (300 default; core caps at 500).
  Renewal is sent at one third of the granted lease; release on key up.
- `input_sequence` starts at 1 and rises by one per message for a per-driver random `source_id`
  (one driver per `InputControl` stream).
- Acks must echo `source_id` and sequence, else the driver fails with `Protocol`.
- Accepted but `effective_transmitting == false` is `Gated` (lease kept, not claimed as sending).
- Rejection: `Rejected{code}`, no retry until the key is released and pressed again.
- No ack before the lease would expire: `Expired`, a best-effort `RELEASE_ALL`, and the held key is
  suppressed until re-pressed.
- Target change, lock/suspend, focus loss: `RELEASE_ALL`; a held key must be released and pressed
  again. Presses while locked or without a target are `Blocked`.
- Channel error, engine disconnect, hotkey backend death: best-effort release, state `Failed`,
  `run` returns an error. The engine's own lease expiry is the final backstop.
- `GrpcLeaseSink::open(&mut RelayClient)` wraps `raw().input_control(..)`, since `RelayClient`
  has no helper for it. There is no `WatchInputs` RPC; `InputControl` acks carry the outcome.
  Note the api doc lists `InputControl` stream semantics as engine-side and open.

## Lock / suspend detection

| OS | Lock | Suspend |
|---|---|---|
| macOS | polled (250 ms) via `CGSessionCopyCurrentDictionary` | not detected |
| Linux, Windows | not implemented | not implemented |

Where undetected, a key-up lost to lock/suspend leaves the lease to lapse within `lease_ms`.
`session::lock_detection()` says which case applies.

## Tests

`cargo test -p relay-platform` (tests/): press/release, renewal with monotonic sequence, expiry
on stall, rejection, gated, no target, lock, focus and target change, disconnect, sink error,
backend death, and honest status for unavailable fakes. Time is paused, so they are deterministic.
`--features wayland` on Linux and the Windows target were only type-checked from macOS.

## Unmet gates

- **Manual: real key delivery.** Press/release delivery through `global-hotkey` (macOS, Windows,
  X11) and the Wayland portal (GNOME, KDE) has not been exercised; headless CI cannot do it.
  Check key-up delivery, held-key behaviour, permission prompts and compositor rebinding by hand.
- Runtime test of the `wayland` feature and the Windows message pump (compile-checked only).
- Lock/suspend detection on Windows and Linux; suspend on macOS.
- End-to-end test against a real engine `InputControl` handler (engine side not built yet).
