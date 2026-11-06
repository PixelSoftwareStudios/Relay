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
| `src/commands/` | One module per command family; `views` (JSON/text), `resolve` (names to ids), `names`; `messages/` for `relay messages` (`target`, `send`, `read`, `files`, `manage`, `watch`, `view`) |
| `src/client/messaging.rs`, `session_feed.rs` | The `Messaging` trait (session RPCs over the `messages` capability) and the TUI's reopening session feed |
| `src/tui/` | `model` (pure state), `keys` (key to `TuiCommand`), `exec`, `view/` (Ratatui), `app` (event loop), `messages/` (the Messages tab: `state`, `keys`, `render`, `view`, `exec`) |
| `tests/` | Fake `Control`, in-process engine over a real Unix socket, TestBackend snapshots, binary exit codes |

Commands are generic over `Control`/`Manage`, so they run unchanged against a scripted fake. `commands::execute` is the seam tests use; `commands::run` adds connecting.

## Commands

```
relay daemon start|restart [--binary P] [--log-file P] [--wait S] [-- relayd args]
relay daemon stop | status | logs [-n N]
relay daemon run [-- relayd args]      run relayd in this terminal
relay status | peers
relay call <peer|ip:port>              never enables the microphone
relay accept|decline|end [call]
relay mute|deafen [on|off] [--call C]  global unless --call
relay gain mic|playback|receive <v> [--call C]
relay talk on|off|ptt [--call C] [--consent]
relay devices list | select [--input X] [--output Y]
relay ns set <backend>
relay profile set --name <n>
relay trust list | verify <peer> --fingerprint <hex> | forget <fingerprint-prefix|name>
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
relay avatar set <file> | clear        png, jpg, webp, gif, bmp or tiff
relay history [--limit N]              past calls, newest first (default 20, at most 200)
relay history clear                    delete the call history on this device
relay completions <shell>              bash, zsh, fish, elvish, powershell
relay man [--dir DIR]                  man page to stdout, or one page per command into DIR
relay messages send <text|->  [--call C]     text into a call session ('-' reads stdin)
relay messages watch | log [--since N] [--limit N] | search <q>
relay messages pin|unpin <msg> | clear [--yes] | export [--out FILE] [--embed-images]
relay messages copy <msg> | copy code-block <N>  [--print]
relay messages perms [--messages on|off] [--files on|off]
relay messages image send <file> | offer <path> | offers
relay messages accept <offer> [--dest DIR] [--name NAME] | reject|cancel <offer>
relay messages preview request|approve <offer> [--deny]
relay hotkey [--binding KEYS]          run the global push-to-talk key
relay hotkey status | set <keys>
relay tui [--no-hotkey | --binding KEYS]
```

Global flags (accepted anywhere): `--json`, `--socket PATH`, `--tcp-profile PATH`, `--takeover`, `--timeout-ms N`. Environment: `RELAY_SOCKET`, `RELAY_TCP_PROFILE` (flags win). Default endpoint is the per-user engine socket from `relay_api::default_socket_path`.

Targets: a peer is matched by exact id, exact name (any case), then a unique id/name prefix. An ambiguous prefix is an error listing candidates, never a guess. A call is an id prefix or a peer name; with no argument the only live call is used, and several live calls are an error.

### Consent and safety
- `talk on` and `talk ptt` fail locally (exit 2, no request sent) without `--consent`; the flag becomes `explicit_microphone_consent`. `talk off` is always allowed.
- `mic-test --accept-feedback` requires `--local-playback`.
- `trust verify` needs the 64-hex SHA-256 the user compared out of band (`:` and `-` separators are fine); it sends `out_of_band_verified = true`. `forget` uses `ForgetPeer`: the argument is a hex fingerprint prefix (at least 8 digits; `:`, `-`, spaces and case are ignored, so the short form from `trust list` or its first groups work) or an exact, case-insensitive display name, tried in that order. An unknown or ambiguous argument is exit 5; ambiguity prints the candidates with their short fingerprints and changes nothing. `trust list` and `peers` show each peer's short fingerprint (`A1B2-C3D4-E5F6-0718-293A`, a hint to tell devices apart, never a verification string).
- Call-scoped mutations first acquire the controller lease (`--takeover` sets `request_explicit_takeover`) and send its generation. Leases are not released explicitly; they expire.

### Connection strings, privacy, channels, avatar
- `relay address` prints only the engine's connection string (pipeable); `--json` adds addresses, port and fingerprint. `--copy` uses `arboard` (text only, no image codecs). If the clipboard is unavailable the string is still printed and the command exits 1 with the reason; on X11 clipboard text may vanish when `relay` exits unless a clipboard manager runs.
- `relay call relay://...` is checked locally with `relay_net::connect::ConnectionString` (scheme, 64-hex fingerprint, literal LAN address, no credentials or path) before anything is sent; failures exit 2. The local check admits overlay (100.64.0.0/10) addresses because the engine holds the opt-in and decides.
- Channel passwords and invite tokens are never argv: flags are booleans that select a hidden terminal prompt (`rpassword`) or one line of stdin. `create --password` asks twice. Secrets have a redacted `Debug`, are zeroed on drop, never appear in output, errors or logs; the only secret printed is a freshly minted invite (bare token in text mode), which is the command's purpose. Channels are matched by exact name, id or unique prefix; `join` also accepts a 32-hex id.
- `avatar set` accepts PNG, JPEG, WebP, GIF, BMP or TIFF up to 1 MiB and 2048x2048, recognised from the file's bytes (the extension is ignored), and refuses locally (exit 2) with the engine's own wording (`avatar must be 1 byte to 1 MiB (PNG, JPEG, WebP, GIF, BMP or TIFF)`, `avatar rejected: unsupported or unrecognised image format ...`, `avatar rejected: avatar dimensions WxH exceed the 2048 pixel limit`); the engine stays the authority. `avatar clear` calls `ClearAvatar` (idempotent; trusted peers are sent an empty avatar hash).

### Call history (`relay history`)
`relay history [--limit N]` lists the newest past calls (`ListCallHistory`): a table with the local time (the system zone), peer name, short fingerprint, direction, outcome and, for completed calls, the length as `m:ss` or `h:mm:ss`; `no call history` when empty. `--json` prints an array with one object per call: `id`, `call_id` (hex, names the chat session), `peer_name`, `peer_id` (hex, full fingerprint), `peer_fingerprint_short`, `direction` (`incoming`/`outgoing`), `outcome` (`completed`, `missed`, `declined`, `failed`, `cancelled`), `started_at_unix_ms`, `duration_ms`. `relay history clear` deletes everything (`ClearCallHistory`, no prompt: the list is local metadata). Recording is the preference `history.enabled` (`relay config set history.enabled false`). Source: `commands/history.rs`; tests `tests/history_cmd.rs`. The list stays on this machine (`docs/config.md`, "Call history").

### Settings (`relay config`)
With a daemon running, `set` and `import` send every key through `SetSettings` and print one line per key (`--json`: an array of `{key, outcome, reason}`): `applied live`, `saved, needs restart (reason)` (read at the next start, for example `network.*`, `hotkeys.*`, `audio.input_*`), or `rejected: <reason>` (unknown key, bad value, out of range). Valid keys are saved even when others are rejected; any rejected key makes the command exit 4 after the report. Only the security-sensitive keys (`identity.*`, `diagnostics.record_raw_audio`, `network.allow_wan`, `network.allow_overlay_manual`) need the daemon stopped: the daemon refuses them over the API, the CLI reports them as `rejected: security-sensitive: change it with the daemon stopped` and still sends the rest (a sensitive key given its current value is a no-op). `import` parses and validates the file locally, then sends only the keys that differ from the daemon's `GetSettings`. With no daemon, `set` and `import` take the writer lock themselves (a held lock is reported as "locked by another process") and `set` prints the stored value.

`get` and `export` ask a running daemon for the live values (`GetSettings`) and otherwise read the database without the settings lock. `export` is the same TOML either way and without a file prints TOML even with `--json`.

### Limits (`relay limits`)
`relay limits` prints every storage and retention limit (`docs/limits.md`): key, current value, default, range and, when a daemon runs and knows it, what is in use. `--json` prints an array of `{key, current, default, min, max, unit, used_bytes?}`. Values come from the running daemon (`GetSettings`) and otherwise from the settings database without a lock. Change one with `relay config set limits.avatar_cache_mib 256`; `config get`, `export` and `import` include the `limits.*` keys like any other, and an out-of-range value is refused with the key named. Source `commands/limits.rs`; tests `tests/limits_cmd.rs`.

### Completions and man page
`relay completions <shell>` prints a completion script (bash, zsh, fish, elvish, powershell; `clap_complete`). `relay man` prints the `relay(1)` page to stdout; `relay man --dir DIR` writes `relay.1` and one page per subcommand (`relay-trust.1`, ...) into `DIR` (`clap_mangen`). Neither connects to the daemon.

### Session messaging (`relay messages`)

Text, inline images and file offers inside a call session (`docs/api.md`, `docs/session-messages.md`). The daemon owns the transcripts, so the desktop app and the CLI see the same session. Messages exist only for the life of a call and in the daemon's RAM; the only thing that ever persists is an explicit `export`.

- **Least privilege.** These commands (and the TUI's messages connection) register with the `messages` capability and nothing else; every other command never asks for it, so a daemon refuses messaging to them. Auto-start works as for other commands (`--no-autostart`, `RELAY_NO_AUTOSTART`).
- **Targeting.** `--call <id|name>` takes a call id prefix, session id prefix or peer name (exact before prefix, any case). Without it the only *active* session is used; none, or several, is an error listing the candidates (exit 5), never a guess. A retained (ended) transcript is readable (`log`, `search`, `copy`, `export`, `clear`) but must be named with `--call`.
- **`send <text|->`** sends exactly the text (`-` reads stdin, one trailing newline removed). Empty text and text over 4096 bytes are refused locally (exit 2).
- **Message numbers.** `log` prints each entry as `#<n>` (the session ordinal, never reused, surviving a clear), time (local, `HH:MM:SS`), direction, delivery state (`sending`, `delivered`, `failed (reason)`) and `[pinned]`. `pin`, `unpin` and `copy` take that number. Times use the system time zone (`TZ`, `/etc/localtime`; daylight saving applied per message) through `relay_io::localtime::LocalZone`, which `Out::with_zone` and `MessagesState::zone` let tests replace. `--json` is unchanged: it carries `time_unix_ms` (UTC milliseconds) and no local field. The header says `(local times)`.
- **Code blocks and links.** Fenced blocks in received text are listed under their message as `[code <N>: <language>, <lines> lines]`; `N` counts blocks across the whole session in transcript order, so it is the same in `log`, `watch` and `copy code-block N`. Links are parsed with the daemon's own safe parser and **never opened**: each is printed with its ASCII host, and a lookalike host (internationalised, mixed-script or invalid punycode) adds a `WARNING`. Control characters in received text are replaced before printing.
- **`copy`** uses the clipboard helper of `relay address --copy` (`arboard`); `--print` also prints the text (and with a clipboard failure the text is still printed and the command exits 1).
- **`watch`** prints new entries, delivery changes, offer progress, permission changes, pins and session start/end as lines (`--json`: one object per line with an `event` tag). History is `log`'s job. When the daemon answers `SessionResync`, or a gap, overflow or new incarnation is detected, it prints `-- resync: <reason>`, opens a fresh watch and reads each transcript from the last ordinal printed, so nothing repeats and nothing is lost; the engine closing the stream is an error (exit 1). Ctrl-C stops.
- **`log --limit N`** is the first `N` entries from `--since`; when more remain it says how to continue.
- **`clear`** asks first (`[y/N]` on stderr, stdin) unless `--yes`; an active session is emptied, an ended one removed; local only.
- **`export`** prints the export JSON (always JSON); `--out FILE` makes the path absolute against the current directory (`.`/`..` folded) and has the daemon create it, owner-only. An existing file is refused locally and by the daemon. Over about 960 KiB the daemon requires `--out`.
- **`image send`** accepts a regular file of at most 15 KiB (`PNG`, `JPEG`, `WebP`, `GIF`; the daemon validates the header); larger images are refused with a pointer to `offer`.
- **File offers.** `offer <path>` makes the path absolute locally; **the daemon opens the file** (same user, same machine) and nothing moves until the peer accepts. `offers` lists each offer with id prefix, message number, direction, name, size, state, progress (`50% of 10.0 MiB`) and notes (executable-looking name, preview pending, saved name, failure). `accept <offer>` saves into `--dest DIR` (an existing directory, default the current one) with an optional suggested `--name`; the daemon sanitizes the name and never overwrites (`name (1).ext`). `reject`, `cancel` and `preview request|approve [--deny]` take the same id prefix; an ambiguous prefix lists candidates.
- **Permissions and budget.** `perms` shows the session's `messages`/`files` switches, message count and transcript budget; with flags it changes them (the host's choice is pushed to the guest, the guest's stays local). A full budget is stated with the way out (`export`, then `clear`).
- **Problem codes** (`MESSAGES_DISABLED`, `SESSION_PERMISSION_OFF`, `HISTORY_FULL`, `FILE_REFUSED`, ...) become `error.code` with exit 4; some carry a hint (what to run next). Message text, file names and paths are never logged at any level and never appear in an error.

## Output

Text by default. With `--json`, stdout is exactly one JSON value per command (arrays for `peers`, `devices list`, `trust list`); errors go to stderr as `{"error":{"code","message"}}`. `error.code` is the engine's `Problem.code` for rejections, otherwise one of `engine_unreachable`, `timeout`, `api_error`, `target`, `usage`, `io`, `json`, `failure`. Mutations print `{operation_id, state, revision, call_id}`.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | other failure (I/O, a failed `doctor` check, hosted engine error) |
| 2 | bad usage or a request refused locally (clap errors, missing `--consent`, bad gain or fingerprint) |
| 3 | engine unreachable: auto-start is disabled (`--no-autostart`, `RELAY_NO_AUTOSTART`) or failed (the reason and the log tail are printed), or the command never starts one (below), or `daemon stop` ran earlier in this process |
| 4 | engine rejected the request (failed operation, gRPC error other than below) |
| 5 | target not found or ambiguous (peer, call, device, gRPC `NOT_FOUND`) |
| 6 | timed out waiting for an operation |

## Auto-start, presence and ownership

Every command that needs the engine starts `relayd` when none answers (`relay_client::daemon::ensure_running`, which holds a cross-process start lock so concurrent commands start one daemon), prints one line on stderr (never stdout, so `--json` stays clean) and goes on. The line says that the daemon opens no microphone and that the device advertises on the LAN while a client is attached, by default with its name (privacy default `visible`; `relay privacy` hides it). `--no-autostart` or `RELAY_NO_AUTOSTART=1` turns it off, as does the stored `clients.autostart_daemon` preference (changed in the desktop Settings; the CLI has no setter); then an unreachable daemon is exit 3. Never started: `daemon stop|status|logs`, `hotkey status`, `config get|export` (they use a daemon only if one already answers), `doctor`, `completions`, `man`. After `daemon stop` the same process never starts one again. A daemon started this way exits after 300 s with no attached client and no call (`--idle-exit`, `RELAY_IDLE_EXIT_SECS`); `relay daemon start` has no idle exit.

A daemon with no attached client (no open `WatchState` stream) neither advertises nor rings: a short-lived command such as `relay peers` attaches, so it counts for the second or so it lives (the engine debounces presence by 2 s).

A call belongs to the client that holds its controller lease. When that client detaches, the call ends after a 3 s grace, unless it re-attaches or another attached client takes over. So `relay call` and `relay accept` stay in the foreground holding a watch; Ctrl-C or `q` hangs up. `relay end` implies takeover. `mute`, `deafen`, `gain` and `talk` on a call another client owns still need `--takeover`; a taken-over call then belongs to the CLI process, so when that command exits the call ends after the grace. Use the desktop app, or `relay call`, to keep a call going.

## `relay doctor`

Checks, in order: endpoint, engine reachability and version, input and output device availability, engine warnings, preferences readability. It reads the preferences database read-only (`PreferenceStore::read_only`) without taking the writer lock, so it works while the engine runs. `--bundle FILE` writes `relay_config::diagnostics::build_bundle` output (redacted, size capped) with mode `0600`, even if the engine is down. Any failed check exits 1.

## TUI

`relay tui` opens registered connections: one for commands, one for the state watch, one for telemetry (plus two `messages`-only ones, below). The state feed (`client::run_state_feed`) reopens the watch, which begins with a snapshot, on revision gaps, stale revisions, incarnation changes, subscription overflow, an engine `Resync`, or a closed stream; the header shows `live`, `resyncing (...)` or `offline (...)` meanwhile. Telemetry is best-effort and reopens after errors.

Screens: Peers, Calls (cards), Devices, Settings, Messages. Header: link state, revision, microphone meter, mute/deafen, noise suppression. Call cards show peer, state, speaking indicator, route, consent, the engine's `effective_transmitting` ("TRANSMITTING" or "mic closed"), `blocked_reason`, receive meter and transport stats.

Keys: `tab`/`1-5` screens, `j/k` move, `enter` call or select, `c` call, `a/x/e` accept/decline/end, `t` cycle route (receive-only, push-to-talk, open mic; transmitting modes ask `y` first), `space` push-to-talk, `m`/`d` mute/deafen, `+`/`-` gain on Settings (and the PTT release delay row, in 50 ms steps from 0 to 2000), `q` quit.

### Messages tab

`relay tui` opens two more registered connections for messaging, with the `messages` capability only: one for commands and one for the session watch. If it cannot be opened (or the daemon refuses the capability) the rest of the UI works and the tab says `Messages are unavailable: <reason>`. The session feed (`client::run_session_feed`) opens `WatchSession`, loads each session's transcript (at most 8 pages), and on `SessionResync`, a gap, an overflow, a new incarnation or a closed stream reopens it and reloads, with the header showing `live`, `resyncing (...)` or `offline (...)`. The pane keeps at most 1000 rows per session and 16 sessions; bursts of events are applied between two redraws.

One pane per call, `[` and `]` switch (ended calls stay as read-only transcripts). Rows show time (local, `HH:MM:SS`), who, delivery state (`sending`, `delivered`, `FAILED (reason)`), `[pinned]`, text with bold/italic/inline code, numbered code blocks (`+- code N: lang`), links (underlined, never opened, lookalike hosts flagged in red with the real host), images as a one-line summary, and file offers with a progress bar and warnings. Header lines state `messages on/off  files on/off`, counts and the transcript budget; honest states: permissions off ("turned off for this call"), budget full (`e` export, `X` clear), call ended (read-only), no call, link lost.

Browse keys (the compose box and prompts take every key while open, so tab switching and `q` only apply while browsing): `i`/`enter` write (Enter sends, Ctrl-J new line, Esc leaves; the draft is dropped only after the daemon accepted it), `j/k` move, `g/G` first/newest, `/` search then `n/N` jump between matches, `p` pin, `b` choose a code block in the selected message, `Ctrl-Y` or `y` copy it, `o` offer a file (path prompt), `a` accept the selected incoming offer (directory prompt, default the current directory), `r` reject, `c` cancel, `v` request a preview (or approve the peer's request), `e` export to a new file (path prompt), `X` empty the transcript (`y` confirms). These keys shadow the global `a/c/e/m/d/...` only on this tab.

The Settings tab lists the microphone and playback gain, mute, deafen and the **PTT release delay** (how long the gate stays open after the key is released, 0 to 2000 ms; `relay config set ptt.release_delay_ms 300` does the same). While the delay runs the call shows the engine's `releasing` state; nothing is assumed locally.

Push-to-talk in the TUI has two paths:
- **Global hotkey** (`hotkeys.push_to_talk`, default `Ctrl+Alt+Space`): `relay tui` starts the platform backend on the main thread and drives a `PttDriver` over its own `InputControl` stream, exactly like `relay hotkey`. The header's third line says what is true: `active via <backend>` plus `ptt: idle|transmitting|held, engine not transmitting|blocked...|rejected|lease expired|failed` and the controlled call, or `needs Input Monitoring: ...`, `unsupported: <reason>`, `failed: <reason>`, `off (...)`. Nothing is claimed when the backend refused.
- **Terminal key**: terminals do not report key release, so `space` latches (press, renew every 100 ms tick, release on the next `space`). It needs the call already in push-to-talk mode with consent. The engine's effective state is what is displayed.

## Global hotkey (`relay hotkey`)

The platform backend (`relay-platform`) must be created on the process main thread on macOS, and the run loop must be serviced there, so `main` starts the backend, then runs tokio on a worker thread and calls `hotkey::run_main_loop` until the command ends (`host.rs`). The daemon is unaffected. `relay hotkey` prints one line per change (`--json`: one object per line with an `event` tag) until Ctrl-C, which releases transmission.

- `relay hotkey status` probes without starting: backend state, saved binding, lock-detection coverage, and on macOS Accessibility and Input Monitoring status. JSON has `state` = `active|disabled|needs_permission|unsupported|failed`.
- `relay hotkey set <keys>` validates with the platform parser and saves `hotkeys.push_to_talk` (empty disables). With a daemon running it goes through `SetSettings` and prints `saved, needs restart` (the key is read when `relay hotkey` or the desktop app starts; `--binding KEYS` is for a single run). With none running it takes the settings lock itself; a lock held by another process is reported.
- The key controls the one `Connected` call: once a second the supervisor takes that call's controller lease (`--takeover` to take it from another client) and rebinds the driver when the call or its route generation changes. No connected call: the key is blocked. Several connected calls: it will not guess (`several calls`). Muting, missing consent and gating still decide whether audio flows; the driver shows `held, engine not transmitting`.
- Permissions on macOS attach to the terminal app that runs `relay`, not the binary.

## Testing

`cargo test -p relay-cli`. `FakeControl` (scripted, records calls; implements `Control`, `Subscribe`, `Manage`) covers command logic, secrets and the TUI drive loop; `EngineHarness` serves a fake `Engine` over a Unix socket for wire-level behavior (lease generation, consent bits, secrets on the wire, rejection mapping, live `config set`); `tests/real_engine.rs` runs against the real service layer with recording adapters; `tests/messages_*.rs` cover the `relay messages` commands against the scripted fake (`tests/common/chat.rs` holds the messaging semantics shared with the socket engine), `tests/messages_wire.rs` the capability and a TRACE-level log capture proving text and file names never reach a log, `tests/tui_messages.rs` the Messages tab on a `TestBackend`, `tests/messages_feed.rs` the feed, and `tests/real_chat.rs` two real daemons in a call (text both ways, a file offer accepted and verified byte for byte; it includes the engine's own harness by path); `tests/daemon_cmd.rs` builds `relayd` and runs start, status, restart and stop against it; `tests/hotkey.rs` drives `supervise` with `relay_platform::testing` fakes; TUI snapshots use ratatui `TestBackend`; binary tests check exit codes and `hotkey status`.

## The daemon is a separate program

`relay` never embeds the engine. `relay daemon start` launches the `relayd` binary detached (see
`docs/packaging.md` for where it is looked for, `RELAY_DAEMON_BIN`) and waits until the endpoint
accepts connections; it prints `already running` instead of starting a second one. `stop` and
`restart` register a short-lived client with the `admin` capability and call `Shutdown`; no other
command requests it. `status` exits 3 when nothing serves the endpoint, like every other command
(`daemon not running at ...; try relay daemon start`). `--socket` and
`--tcp-profile` choose the endpoint and are passed on to `relayd`. `relay-engine` is only a
dev-dependency (for `tests/real_engine.rs`).

## Gaps

- Real key delivery is a manual gate (see `docs/platform.md`): `relay hotkey status` was only exercised on macOS here, and press/release through the OS was not.
- `hotkeys.push_to_talk` can be set while the engine runs, but it is read when `relay hotkey` or the desktop app starts (`saved, needs restart`). Channel and privacy RPCs depend on the engine; an `UNIMPLEMENTED` answer is reported as a rejection (exit 4).
- `relay-client` is being extended with channel and privacy commands by another change; this app calls the generated RPCs for them in `client/manage.rs` and should move to the shared vocabulary once that lands.
- `trust verify` requires the user to supply the full fingerprint: `Peer` carries only the short form, which is a hint and not enough to verify.
- No Windows named-pipe endpoint (API gate); on Windows only `--tcp-profile` works.
- `doctor` cannot report preference-recovery notices (needs the writer lock) and passes no log lines or counters beyond peer/call counts.
