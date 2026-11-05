# Shared client crate

Living notes for `crates/relay-client`. It holds what `apps/relay-cli` and `apps/relay-desktop` genuinely share over `relay-api`, and nothing toolkit-specific (no clap, ratatui, egui or tray). It depends only on `relay-api`, so it builds and tests without the engine.

| Module | Role |
|---|---|
| `command` | `Command` (every engine mutation), `CallTarget`, `CallAction`. Call-bound commands carry the controller generation; microphone consent is a field of `SetTransmit`, i.e. in the same request |
| `wire` | The one place commands become RPCs: `send`, `mutation` (fresh idempotent operation id), `acquire_generation` (lease, optional explicit takeover), `open_telemetry`, `connect_registered` |
| `operation` | `settle` polls until an operation leaves `ACCEPTED`, over the `OperationSource` trait (a client, a client behind a lock taken per poll, or a test fake) |
| `endpoint` | `RELAY_TCP_PROFILE`, `RELAY_SOCKET`, per-user default, `describe`. Flags stay in the CLI |
| `meter` | dBFS to 0..1 bar fill (60 dB floor) |

## What stays in the apps
- Validation differs by design (CLI: finite gain 0 or more, non-empty name; desktop: gain 0..4, 1 to 64 printable characters, avatar size and type, known ids), so it is not forced into one rule set.
- Lease policy: the CLI acquires before every call mutation (optional `--takeover`); the desktop reuses the generation from state and never takes over.
- Error mapping and exit codes (CLI `CliError`) versus `EngineError` (desktop).
- Supervisors: the CLI feed forwards `Update`s with a fixed retry and the `Control`/`Subscribe` traits for fakes; the desktop mirrors state into a view model with exponential backoff and `Operations`/`Engine` ports. Their consumers differ, so only the wire and settle code below them is shared.
- Name and id resolution (`commands/resolve.rs`, `names.rs`), view models, transmit text, and hotkey/PTT status (the desktop has no hotkey surface) are single-client.

## Tests
`crates/relay-client/tests/`: `wire.rs` runs each command against an in-process engine on a Unix socket, `settle.rs` covers polling and timeout with a fake source, `endpoint.rs` and `meter.rs` the pure helpers. Each app keeps tests for its own UI, CLI and supervisor logic.
