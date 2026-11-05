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

## Starting and stopping

Nothing starts the daemon implicitly.

- `relay daemon start` launches `relayd` detached (its own process group, no terminal) and waits for
  the endpoint; `relay daemon status`, `stop`, `restart` and `logs` do the rest. `relay daemon run`
  runs `relayd` in the foreground.
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
