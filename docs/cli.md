# CLI and TUI

Living notes for `apps/relay-cli` (binary `relay`). Terminal only: no WebView dependencies.

## Layout

| Path | Role |
|---|---|
| `src/main.rs` | Calls `relay_cli::main_exit()`; nothing else |
| `src/host.rs` | Process setup: plain tokio runtime, or (for `hotkey`/`tui`) main thread that starts the hotkey backend and services the platform loop while tokio runs on a worker thread |
| `src/hotkey/` | `start_with` (probe and register), `HotkeyReport` (honest state), `supervise` (keeps a `PttDriver` on the right call) |
| `src/secret.rs` | `Secret` (redacted `Debug`, zeroed on drop), hidden-prompt and stdin sources |
| `src/cli/` | clap definitions (`args`), `CliError`, `exit` codes, endpoint resolution |
| `src/client/` | `Control` (request/response), `Manage` (channels, privacy, avatar, connection info) and `Subscribe` (streams) traits; `RealClient` over `relay_api::RelayClient`; resyncing feeds |
| `src/commands/` | One module per command family; `views` (JSON/text), `resolve` (names to ids), `names` |
| `src/tui/` | `model` (pure state), `keys` (key to `TuiCommand`), `exec`, `view/` (Ratatui), `app` (event loop) |
| `tests/` | Fake `Control`, in-process engine over a real Unix socket, TestBackend snapshots, binary exit codes |

Commands are generic over `Control`/`Manage`, so they run unchanged against a scripted fake. `commands::execute` is the seam tests use; `commands::run` adds connecting.

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
relay address [--copy]                 connection string others use to call you
relay call relay://ip:port#fp          call by pasted connection string
relay privacy set [--inbound anyone|shared-channel] [--advert visible|hidden]
relay channel create <name> [--hidden] [--invite-only] [--password|--password-stdin]
relay channel join <name|id> [--password|--password-stdin|--invite|--invite-stdin]
relay channel leave|invite|revoke <channel> ...   (invite: --ttl-seconds, --uses; revoke: <fingerprint>)
relay channel list
relay config get [key] | set <key> <value> | export [file] | import <file>
relay avatar set <file.png|jpg> | clear
relay hotkey [--binding KEYS]          run the global push-to-talk key
relay hotkey status | set <keys>
relay tui [--no-hotkey | --binding KEYS]
```

Global flags (accepted anywhere): `--json`, `--socket PATH`, `--tcp-profile PATH`, `--takeover`, `--timeout-ms N`. Environment: `RELAY_SOCKET`, `RELAY_TCP_PROFILE` (flags win). Default endpoint is the per-user engine socket from `relay_api::default_socket_path`.

Targets: a peer is matched by exact id, exact name (any case), then a unique id/name prefix. An ambiguous prefix is an error listing candidates, never a guess. A call is an id prefix or a peer name; with no argument the only live call is used, and several live calls are an error.

### Consent and safety
- `talk on` and `talk ptt` fail locally (exit 2, no request sent) without `--consent`; the flag becomes `explicit_microphone_consent`. `talk off` is always allowed.
- `mic-test --accept-feedback` requires `--local-playback`.
- `trust verify` needs the 64-hex SHA-256 the user compared out of band (`:` and `-` separators are fine); it sends `out_of_band_verified = true`. `forget` sends `UNKNOWN` with no key hash.
- Call-scoped mutations first acquire the controller lease (`--takeover` sets `request_explicit_takeover`) and send its generation. Leases are not released explicitly; they expire.

### Connection strings, privacy, channels, avatar
- `relay address` prints only the engine's connection string (pipeable); `--json` adds addresses, port and fingerprint. `--copy` uses `arboard` (text only, no image codecs). If the clipboard is unavailable the string is still printed and the command exits 1 with the reason; on X11 clipboard text may vanish when `relay` exits unless a clipboard manager runs.
- `relay call relay://...` is checked locally with `relay_net::connect::ConnectionString` (scheme, 64-hex fingerprint, literal LAN address, no credentials or path) before anything is sent; failures exit 2. The local check admits overlay (100.64.0.0/10) addresses because the engine holds the opt-in and decides.
- Channel passwords and invite tokens are never argv: flags are booleans that select a hidden terminal prompt (`rpassword`) or one line of stdin. `create --password` asks twice. Secrets have a redacted `Debug`, are zeroed on drop, never appear in output, errors or logs; the only secret printed is a freshly minted invite (bare token in text mode), which is the command's purpose. Channels are matched by exact name, id or unique prefix; `join` also accepts a 32-hex id.
- `avatar set` accepts `.png`, `.jpg`, `.jpeg` up to 1 MiB (the engine decodes and validates). `avatar clear` exits 4 with code `unsupported`: the API has no clear operation.

### Settings (`relay config`)
`get` and `export` read the database without the settings lock, so they work next to a running engine. `set`: with an engine reachable, only settings it can change live are routed through it (`profile.display_name`, `audio.microphone_gain`, `audio.playback_gain`, `ptt.release_delay_ms`, `privacy.inbound_policy`, `privacy.advert_mode`); anything else exits 2 naming those keys. With no engine, `set` and `import` take the writer lock themselves; a held lock is reported as "locked by another process". `import` needs the engine stopped. `export` without a file prints TOML even with `--json`.

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

Screens: Peers, Calls (cards), Devices, Settings. Header: link state, revision, microphone meter, mute/deafen, noise suppression. Call cards show peer, state, speaking indicator, route, consent, the engine's `effective_transmitting` ("TRANSMITTING" or "mic closed"), `blocked_reason`, receive meter and transport stats.

Keys: `tab`/`1-4` screens, `j/k` move, `enter` call or select, `c` call, `a/x/e` accept/decline/end, `t` cycle route (receive-only, push-to-talk, open mic; transmitting modes ask `y` first), `space` push-to-talk, `m`/`d` mute/deafen, `+`/`-` gain on Settings (and the PTT release delay row, in 50 ms steps from 0 to 2000), `q` quit.

The Settings tab lists the microphone and playback gain, mute, deafen and the **PTT release delay** (how long the gate stays open after the key is released, 0 to 2000 ms; `relay config set ptt.release_delay_ms 300` does the same). While the delay runs the call shows the engine's `releasing` state; nothing is assumed locally.

Push-to-talk in the TUI has two paths:
- **Global hotkey** (`hotkeys.push_to_talk`, default `Ctrl+Alt+Space`): `relay tui` starts the platform backend on the main thread and drives a `PttDriver` over its own `InputControl` stream, exactly like `relay hotkey`. The header's third line says what is true: `active via <backend>` plus `ptt: idle|transmitting|held, engine not transmitting|blocked...|rejected|lease expired|failed` and the controlled call, or `needs Input Monitoring: ...`, `unsupported: <reason>`, `failed: <reason>`, `off (...)`. Nothing is claimed when the backend refused.
- **Terminal key**: terminals do not report key release, so `space` latches (press, renew every 100 ms tick, release on the next `space`). It needs the call already in push-to-talk mode with consent. The engine's effective state is what is displayed.

## Global hotkey (`relay hotkey`)

The platform backend (`relay-platform`) must be created on the process main thread on macOS, and the run loop must be serviced there, so `main` starts the backend, then runs tokio on a worker thread and calls `hotkey::run_main_loop` until the command ends (`host.rs`). The daemon is unaffected. `relay hotkey` prints one line per change (`--json`: one object per line with an `event` tag) until Ctrl-C, which releases transmission.

- `relay hotkey status` probes without starting: backend state, saved binding, lock-detection coverage, and on macOS Accessibility and Input Monitoring status. JSON has `state` = `active|disabled|needs_permission|unsupported|failed`.
- `relay hotkey set <keys>` validates with the platform parser and saves `hotkeys.push_to_talk` (empty disables). It needs the settings lock, which a running engine holds: it then fails telling you to stop the engine, or use `--binding KEYS` for a single run. There is no engine route for this setting.
- The key controls the one `Connected` call: once a second the supervisor takes that call's controller lease (`--takeover` to take it from another client) and rebinds the driver when the call or its route generation changes. No connected call: the key is blocked. Several connected calls: it will not guess (`several calls`). Muting, missing consent and gating still decide whether audio flows; the driver shows `held, engine not transmitting`.
- Permissions on macOS attach to the terminal app that runs `relay`, not the binary.

## Testing

`cargo test -p relay-cli`. `FakeControl` (scripted, records calls; implements `Control`, `Subscribe`, `Manage`) covers command logic, secrets and the TUI drive loop; `EngineHarness` serves a fake `Engine` over a Unix socket for wire-level behavior (lease generation, consent bits, secrets on the wire, rejection mapping, live `config set`); `tests/real_engine.rs` runs against the real daemon with recording adapters; `tests/hotkey.rs` drives `supervise` with `relay_platform::testing` fakes; TUI snapshots use ratatui `TestBackend`; binary tests check exit codes and `hotkey status`.

## Gaps

- Real key delivery is a manual gate (see `docs/platform.md`): `relay hotkey status` was only exercised on macOS here, and press/release through the OS was not.
- `hotkeys.push_to_talk` cannot be saved while the engine runs (no API route for arbitrary settings); same for `config import` and for any setting outside the live list.
- `avatar clear` is unsupported by the API. Channel and privacy RPCs depend on the engine; at the time of writing the engine answered some of them `UNIMPLEMENTED`, and the CLI reports that as a rejection (exit 4).
- `relay-client` is being extended with channel and privacy commands by another change; this app calls the generated RPCs for them in `client/manage.rs` and should move to the shared vocabulary once that lands.
- `trust verify` requires the user to supply the fingerprint because the API's `Peer` carries no key hash; the engine does not accept `UNKNOWN` from clients, so `trust forget` is rejected by it.
- No Windows named-pipe endpoint (API gate); on Windows only `--tcp-profile` works.
- `doctor` cannot report preference-recovery notices (needs the writer lock) and passes no log lines or counters beyond peer/call counts.
