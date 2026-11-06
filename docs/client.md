# Shared client crate

Living notes for `crates/relay-client`. It holds what `apps/relay-cli` and `apps/relay-desktop` genuinely share over `relay-api`, and nothing toolkit-specific (no clap, ratatui, egui or tray). It depends only on `relay-api`, so it builds and tests without the engine.

| Module | Role |
|---|---|
| `command` | `Command` (every engine mutation), `CallTarget`, `CallAction`. Call-bound commands carry the controller generation; microphone consent is a field of `SetTransmit`, i.e. in the same request |
| `wire` | The one place commands become RPCs: `send`, `mutation` (fresh idempotent operation id), `acquire_generation` (lease, optional explicit takeover), `open_telemetry`, `connect_registered` |
| `operation` | `settle` polls until an operation leaves `ACCEPTED`, over the `OperationSource` trait (a client, a client behind a lock taken per poll, or a test fake) |
| `endpoint` | `RELAY_TCP_PROFILE`, `RELAY_SOCKET`, per-user default, `describe`. Flags stay in the CLI |
| `meter` | dBFS to 0..1 bar fill (60 dB floor) |
| `daemon` | `ensure_running(&Endpoint, &StartOptions)` starts the daemon unless one answers: an advisory file lock at `<endpoint>.start.lock` makes concurrent callers start one; `daemon stop` in this process sets a flag so a later `ensure_running` refuses (`EnsureError::StoppedByRequest`); `StartOptions::autostart()` passes `--idle-exit` (300 s or `RELAY_IDLE_EXIT_SECS`); failures carry the log tail. Launcher for the separate `relayd`: `locate` (explicit, `RELAY_DAEMON_BIN`, next to the executable, `PATH`), `start` (detached: own process group, or `DETACHED_PROCESS` on Windows; output to a log file; waits for the endpoint with a bounded timeout; returns `Launched`, `AlreadyRunning`, `BinaryNotFound` or `FailedToStart` with the log tail), `stop` (registers with `admin`, calls `Shutdown`, waits for the endpoint to vanish), `reachable`, `log_tail`. No dependency on `relay-engine`. |

## What stays in the apps
- Validation differs by design (CLI: finite gain 0 or more, non-empty name; desktop: gain 0..4, 1 to 64 printable characters, avatar size and type, known ids), so it is not forced into one rule set.
- Lease policy: the CLI acquires before every call mutation (optional `--takeover`); the desktop reuses the generation from state and never takes over.
- Error mapping and exit codes (CLI `CliError`) versus `EngineError` (desktop).
- Supervisors: the CLI feed forwards `Update`s with a fixed retry and the `Control`/`Subscribe` traits for fakes; the desktop mirrors state into a view model with exponential backoff and `Operations`/`Engine` ports. Their consumers differ, so only the wire and settle code below them is shared.
- Name and id resolution (`commands/resolve.rs`, `names.rs`), view models, transmit text, and hotkey/PTT status (the desktop has no hotkey surface) are single-client.

## Daemon revision and replacing a stale daemon
`RelayClient::daemon_revision()` (relay-api) returns the daemon's build and API revision from `GetInfo` as `relay_api::Revision`; `Revision::local()` is this binary's, `is_current()` compares. `relay_client::refresh` decides whether to replace a running daemon: `refresh(&mut impl Daemon, Policy)` is the pure decision (tested with a fake), `refresh_endpoint` the real one. It probes the daemon (registers with `observe`, reads the snapshot: a call that is preparing, ringing, connecting, connected or interrupted makes it busy), and with `Policy::StaleOnly` (the CLI) replaces only a daemon whose build or API revision differs, with `Policy::AnyIdle` (the desktop app at launch) any idle daemon. A busy daemon is never touched (`Outcome::KeptBusy`). Stop and start run under the per-endpoint start lock that `ensure_running` uses, so a client that autostarts at the same moment waits for the replacement and a second replacer finds the lock held (`Outcome::Contended`, nothing done). A daemon that does not stop within the wait is an error (`RefreshError::StopTimedOut`), reported once, and no second daemon is started. Other attached clients do not prevent a replacement; they reconnect through their own supervisors.

## Tests
`crates/relay-client/tests/`: `wire.rs` runs each command against an in-process engine on a Unix socket, `settle.rs` covers polling and timeout with a fake source, `endpoint.rs` and `meter.rs` the pure helpers, `refresh.rs` the replacement decision (idle stale, matching, busy, stop timeout, start failure, held lock), `revision.rs` the revision read from a connection, `daemon.rs` the launcher against a stand-in `fake-relayd` binary (start, already running, missing binary, early exit with log tail, timeout, slow start, own process group, lookup order). Each app keeps tests for its own UI, CLI and supervisor logic.
