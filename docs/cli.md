# CLI and TUI

Living notes for `apps/relay-cli` (binary `relay`). Terminal only: no WebView dependencies.

## Layout

| Path | Role |
|---|---|
| `src/main.rs` | Calls `relay_cli::main_exit()`; nothing else |
| `src/cli/` | clap definitions (`args`), `CliError`, `exit` codes, endpoint resolution |
| `src/client/` | `Control` (request/response) and `Subscribe` (streams) traits; `RealClient` over `relay_api::RelayClient`; resyncing feeds |
| `src/commands/` | One module per command family; `views` (JSON/text), `resolve` (names to ids), `names` |
| `src/tui/` | `model` (pure state), `keys` (key to `TuiCommand`), `exec`, `view/` (Ratatui), `app` (event loop) |
| `tests/` | Fake `Control`, in-process engine over a real Unix socket, TestBackend snapshots, binary exit codes |

Commands are generic over `Control`, so they run unchanged against a scripted fake. `commands::execute` is the seam tests use; `commands::run` adds connecting.

## Commands

```
relay daemon [--tcp]                   host the engine (see Gaps)
relay status | peers
relay call <peer|ip:port>              never enables the microphone
relay accept|decline|end [call]
relay mute|deafen [on|off] [--call C]  global unless --call
relay gain mic|playback|receive <v> [--call C]
relay talk on|off|ptt [--call C] [--consent]
relay devices list | select [--input X] [--output Y]
relay ns set <backend>
relay profile set --name <n>
relay trust list | verify <peer> --fingerprint <hex> | forget <peer>
relay mic-test [--seconds N] [--local-playback --accept-feedback]
relay doctor [--bundle FILE]
relay tui
```

Global flags (accepted anywhere): `--json`, `--socket PATH`, `--tcp-profile PATH`, `--takeover`, `--timeout-ms N`. Environment: `RELAY_SOCKET`, `RELAY_TCP_PROFILE` (flags win). Default endpoint is the per-user engine socket from `relay_api::default_socket_path`.

Targets: a peer is matched by exact id, exact name (any case), then a unique id/name prefix. An ambiguous prefix is an error listing candidates, never a guess. A call is an id prefix or a peer name; with no argument the only live call is used, and several live calls are an error.

### Consent and safety
- `talk on` and `talk ptt` fail locally (exit 2, no request sent) without `--consent`; the flag becomes `explicit_microphone_consent`. `talk off` is always allowed.
- `mic-test --accept-feedback` requires `--local-playback`.
- `trust verify` needs the 64-hex SHA-256 the user compared out of band (`:` and `-` separators are fine); it sends `out_of_band_verified = true`. `forget` sends `UNKNOWN` with no key hash.
- Call-scoped mutations first acquire the controller lease (`--takeover` sets `request_explicit_takeover`) and send its generation. Leases are not released explicitly; they expire.

## Output

Text by default. With `--json`, stdout is exactly one JSON value per command (arrays for `peers`, `devices list`, `trust list`); errors go to stderr as `{"error":{"code","message"}}`. `error.code` is the engine's `Problem.code` for rejections, otherwise one of `engine_unreachable`, `timeout`, `api_error`, `target`, `usage`, `io`, `json`, `failure`. Mutations print `{operation_id, state, revision, call_id}`.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | other failure (I/O, a failed `doctor` check, hosted engine error) |
| 2 | bad usage or a request refused locally (clap errors, missing `--consent`, bad gain or fingerprint) |
| 3 | engine unreachable (not running, wrong socket, gRPC `UNAVAILABLE`) |
| 4 | engine rejected the request (failed operation, gRPC error other than below) |
| 5 | target not found or ambiguous (peer, call, device, gRPC `NOT_FOUND`) |
| 6 | timed out waiting for an operation |

## `relay doctor`

Checks, in order: endpoint, engine reachability and version, input and output device availability, engine warnings, preferences readability. It reads the preferences database read-only (`PreferenceStore::read_only`) without taking the writer lock, so it works while the engine runs. `--bundle FILE` writes `relay_config::diagnostics::build_bundle` output (redacted, size capped) with mode `0600`, even if the engine is down. Any failed check exits 1.

## TUI

`relay tui` opens three registered connections: one for commands, one for the state watch, one for telemetry. The state feed (`client::run_state_feed`) reopens the watch, which begins with a snapshot, on revision gaps, stale revisions, incarnation changes, subscription overflow, an engine `Resync`, or a closed stream; the header shows `live`, `resyncing (...)` or `offline (...)` meanwhile. Telemetry is best-effort and reopens after errors.

Screens: Peers, Calls (cards), Devices, Settings. Header: link state, revision, hotkey backend, microphone meter, mute/deafen, noise suppression. Call cards show peer, state, speaking indicator, route, consent, the engine's `effective_transmitting` ("TRANSMITTING" or "mic closed"), `blocked_reason`, receive meter and transport stats.

Keys: `tab`/`1-4` screens, `j/k` move, `enter` call or select, `c` call, `a/x/e` accept/decline/end, `t` cycle route (receive-only, push-to-talk, open mic; transmitting modes ask `y` first), `space` push-to-talk, `m`/`d` mute/deafen, `+`/`-` gain on Settings, `q` quit.

Push-to-talk: terminals do not report key release, so `space` latches (press, then renewal on every 100 ms tick, release on the next `space`). It needs the call already in push-to-talk mode with consent. The effective state always comes from the engine, not from the latch.

Hotkey backend: shown as `global` when the engine advertises capability `global-hotkeys`, else `terminal only`.

## Testing

`cargo test -p relay-cli`. `FakeControl` (scripted, records calls) covers command logic and the TUI drive loop; `EngineHarness` serves a real `Engine` over a Unix socket for wire-level behavior (lease generation, consent bits, rejection mapping, mic-test); TUI snapshots use ratatui `TestBackend`; binary tests check exit codes. When `relay_engine::daemon::run` lands, add an end-to-end suite that starts the real engine.

## Gaps

- `relay daemon` is a stub: `commands::run_engine_host(DaemonOptions)` returns an error until the engine exposes `daemon::run(RunOptions)`; it is the single place to wire.
- Requested client capabilities (`observe`, `call-control`) are a guess at the engine's names; the engine grants what it supports.
- `trust verify` requires the user to supply the fingerprint because the API's `Peer` carries no key hash.
- TUI push-to-talk uses the `InputControl` stream, which the API doc lists as engine-side open work; untested against a real engine.
- No Windows named-pipe endpoint (API gate); on Windows only `--tcp-profile` works.
- `doctor` cannot report preference-recovery notices (needs the writer lock) and passes no log lines or counters beyond peer/call counts.
