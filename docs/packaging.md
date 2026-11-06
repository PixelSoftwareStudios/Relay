# Packaging and install layout

Relay ships three programs. The engine is its own process; the other two are clients that find,
start and stop it through the local API.

| Program | Crate | Role |
|---|---|---|
| `relayd` | `apps/relay-daemon` | The per-user engine: devices, identity, peers, calls, local API. Runs with no terminal. |
| `relay` | `apps/relay-cli` | Terminal client: commands, TUI, global push-to-talk key. Never embeds the engine. |
| `relay-desktop` | `apps/relay-desktop` | Optional native window. Never needs `relay`. |

## Layout

Install the three binaries in one directory. Both clients look for `relayd` next to their own
executable first, so a portable folder works with no `PATH` entry; then `RELAY_DAEMON_BIN`, then
`PATH` (the order is: an explicit option, `RELAY_DAEMON_BIN`, the executable's directory, `PATH`).

```
<prefix>/bin/relayd
<prefix>/bin/relay
<prefix>/bin/relay-desktop        (optional)
<prefix>/share/doc/relay/THIRD_PARTY_LICENSES
```

On macOS a bundle can keep `relay-desktop` in `Contents/MacOS` and `relayd` beside it. A package must
not make `relay-desktop` depend on `relay`.

## macOS development signing (dev ergonomics only)

The keychain remembers "Always Allow" by the program's designated requirement. An unsigned or
ad-hoc Rust binary gets a requirement based on its cdhash, which changes on every build, so the
prompt for the `relay-voice` item returns after each rebuild. Signing with a fixed identifier and a
fixed self-signed certificate makes the requirement
`identifier "dev.relay.relayd" and certificate leaf = H"..."`, stable across builds.

```
scripts/dev-sign-macos.sh               # once: creates the cert, then signs target/debug binaries
cargo build && scripts/dev-sign-macos.sh   # after later builds (idempotent)
scripts/dev-sign-macos.sh --release     # target/release
scripts/dev-sign-macos.sh --no-cert     # no certificate: ad-hoc with `designated => identifier "..."`
```

The script creates a self-signed code-signing certificate "Relay Local Dev Signing" with openssl and
imports it into the login keychain (macOS may ask for the login password). It never changes trust
settings, never needs sudo, and does nothing when the binaries are already signed correctly.
`.cargo/config.toml` sets `scripts/cargo-runner-macos.sh` as the macOS runner, which re-signs
`relayd`, `relay` and `relay-desktop` on `cargo run` once the certificate exists; it is inert before
that and never fails a run. Binaries started by other means (a client autostarting
`target/debug/relayd`) need the post-build command. After signing, answer the keychain prompt once
with "Always Allow". The older per-secret items from earlier builds are not read; delete any
`relay-voice` entries other than `vault` in Keychain Access.

This is not release signing: Developer ID, hardened runtime, entitlements and notarization are
open (`docs/gates.md`, `docs/release.md`).

## Starting and stopping

Clients start the daemon when none answers: `relay` commands (unless `--no-autostart` or
`RELAY_NO_AUTOSTART=1`) and the desktop app (unless its "Start daemon automatically" setting is off or
the same variable is set), through the shared launcher with a start lock. A daemon started this way
passes `--idle-exit` (300 s, `RELAY_IDLE_EXIT_SECS`), so it exits when no client has been attached
and no call has been live for that long. Starting never opens a microphone or grants consent. A
daemon with no attached client does not advertise on the LAN and does not ring, which also holds for
a service-unit daemon: it stays silent until a client attaches. To have Relay available at login
without a window, run the daemon from a service unit (`contrib/`, no idle exit); the units remain
the way to run it at login.

- `relay daemon start` launches `relayd` detached (its own process group, no terminal) and waits for
  the endpoint; `relay daemon status`, `stop`, `restart` and `logs` do the rest. `relay daemon run`
  runs `relayd` in the foreground.
- The desktop starts the daemon at launch when none answers, restarts one that dies (exponential
  backoff, at most 3 starts in 60 s, then it shows the failure with the log tail and a manual
  "Start daemon" button) and does not restart one the user quit with "Quit daemon...".
- The desktop window offers "Start daemon" when none is reachable and "Quit daemon..." behind a
  confirmation. Closing or quitting the window never stops the daemon.
- A detached daemon appends its output to `~/Library/Logs/Relay/relayd.log` (macOS),
  `$XDG_STATE_HOME/relay/relayd.log` or `~/.local/state/relay/relayd.log` (Linux) or
  `%LOCALAPPDATA%\Relay\relayd.log` (Windows). The log is rotated once at 5 MiB, by renaming, when the
  launcher starts a daemon; there is no other rotation.
- To keep the daemon running across logins, use the supervisors below instead of the launcher.

## Run at login (written, not verified)

`contrib/launchd/org.relay.relayd.plist` (macOS LaunchAgent) and `contrib/systemd/relayd.service`
(systemd user unit) are written from the documentation of those tools. **Neither has been installed or
run on a real machine.** Check the paths, the restart policy and, for systemd, that the unit sees the
audio server and the credential store of the logged-in session. There is no Windows service or
scheduled task yet.

## Stopping through the API

`Shutdown` needs the `admin` capability, which a client must name when it registers and which is never
granted by default. `relay daemon stop` and the desktop's "Quit daemon" are the only callers; they
register a separate short-lived client for it. The daemon then ends calls, releases the microphone,
removes its endpoint and exits with status 0, so a `KeepAlive`/`Restart=on-failure` supervisor leaves
it stopped.
