<p align="center">
  <img src="assets/icon/relay-mark-128.png" alt="Relay" width="96" height="96">
</p>

<h1 align="center">Relay</h1>

<p align="center">
  Peer-to-peer voice calling for your local network. No account, no server, no cloud.
</p>

Relay lets devices on the same LAN find each other and talk, with low latency and encrypted audio. Each
device runs a small per-user engine (`relayd`); a desktop app, a terminal client (`relay`) and an Android
app control it. Nothing leaves your network unless you opt in to an overlay such as Tailscale. The one
thing the desktop app can ask the internet is "is there a newer version?" (GitHub), and only when you
turn on the check at launch or press "Check for updates"; it sends nothing else and there is no
telemetry.

> **Status: pre-release.** The code, tests and release pipeline exist, but no version has been tagged and
> several qualification gates are still open (real-device latency, two-machine runs, independent security
> review). See [docs/roadmap.md](docs/roadmap.md) and [docs/gates.md](docs/gates.md) before relying on it.

## Features

- **Signed in-place updates (desktop).** "Update to <version>" appears above Settings when a newer
  release exists; it downloads, verifies a minisign signature, replaces the app and restarts it.
  Off until you opt in or press "Check for updates"; see [docs/updates.md](docs/updates.md).
- **LAN-only discovery.** Devices find each other with mDNS on the local network. You can also dial a
  `relay://ip:port#fingerprint` connection string (shown as text and QR code). Visibility is a setting:
  advertise with your name, or stay hidden.
- **Encrypted calls with pinned keys.** Peers authenticate with a Noise handshake and long-term keys.
  A device's key is pinned on first contact; a changed key is refused until you confirm it. You can
  verify a contact by comparing fingerprints.
- **Contacts you control.** Trust, verify, forget or block a device. Choose who may call you (anyone, or
  only people who share a channel). Verified contacts can be set to auto-answer.
- **Push-to-talk or open mic.** Per call, with a global hotkey on the desktop and a talk button on the
  phone. Calls never open your microphone until you allow it.
- **Channels.** Named channels, optionally hidden, password-protected or invite-only.
- **Messages and files.** Text, images and file offers inside a call, with size limits and an accept step.
- **Audio.** Opus, jitter buffering and noise suppression (on by default at moderate strength).
- **Ringtones and alerts.** Built-in or custom ringtone, notifications, and a choice of how an incoming call
  alerts you.
- **Background availability.** With the setting on, the engine stays reachable (advertises, rings,
  auto-answers) without a window open. On Android this runs as a foreground service.
- **Desktop, CLI and Android.** Windows, macOS and Linux (x64 and arm64), and Android (arm64).
- **Nine languages** in the app: English, German, Spanish, Hindi, Japanese, Korean, Brazilian Portuguese,
  Russian and Simplified Chinese. Only English is reviewed; see [Security and limits](#security-and-limits).
  The CLI is English only.

## Screenshots

No screenshots are committed to the repository yet. The UI test suite can render them
(`crates/relay-ui/tests/screenshots.rs`), but the output is not tracked.

## Download

Releases will appear on the [Releases page](../../releases/latest) once the first version is tagged.
Names follow `relay-<version>-<platform>.<ext>`; `<platform>` is `windows-x64`, `windows-arm64`,
`macos-arm64`, `macos-x64`, `linux-x64` or `linux-arm64`. Every release includes `SHA256SUMS.txt`.

| Platform | Desktop app | CLI and daemon only (`relay-cli-<version>-<platform>`) |
|---|---|---|
| Windows | `.msi` installer, or `-portable.zip` | `.zip` |
| macOS | `.dmg` (also `-update.zip`, used by the self-updater) | `.tar.gz` |
| Linux | `.AppImage`, `.deb`, `.rpm` or `.tar.gz` | `.tar.gz` |
| Android | `relay-<version>-android-arm64-v8a.apk` | |

Verify a download with `sha256sum -c SHA256SUMS.txt` (macOS: `shasum -a 256 -c SHA256SUMS.txt`). Every package except the APK also has a minisign signature: `minisign -Vm <file> -P <public key>` with the key in [`crates/relay-update/update-key.pub`](crates/relay-update/update-key.pub) (see [docs/releasing.md](docs/releasing.md#update-signing)).

Nothing is code-signed by a trusted authority, so each system warns on first run:

- **macOS:** apps are ad-hoc signed, not notarized. On first launch, if macOS blocks it, open **System Settings → Privacy & Security** and choose **Open Anyway** (on macOS 14 and earlier, right-click `Relay.app` and choose **Open**).
  (Or allow it under System Settings, Privacy & Security.)
- **Windows:** installers are unsigned, so SmartScreen shows "Windows protected your PC". Choose
  **More info**, then **Run anyway**.
- **Android:** the APK is signed with the Relay release key and installed by sideloading: allow "install unknown apps" for your
  browser or file manager, then open the file. Only 64-bit ARM (arm64-v8a) is built.
- **Linux:** `chmod +x` the AppImage, or install the `.deb` / `.rpm` with your package manager.

## Quick start

**Desktop.** Install and open Relay. It starts its own daemon if none is running, asks for microphone
access on first launch, and lists devices it finds on your network. Pick one to call; the other side
accepts. Open the same app on a second device on the same network to try it. On Windows and macOS closing
the window hides it to the tray.

**CLI.** The `relay` command starts the daemon on demand (it never opens the microphone by itself).

```sh
relay peers                    # devices found on the LAN
relay call <peer>              # call by name or id (or ip:port, or a relay:// string)
relay accept | decline | end   # answer or finish a call
relay talk ptt                 # push-to-talk for the call (also: muted, unmuted)
relay tui                      # full-screen terminal interface
relay trust verify <peer> --fingerprint <hex>
relay doctor                   # check devices and setup
```

All commands, flags, `--json` output and exit codes are in [docs/cli.md](docs/cli.md).

**Android.** Install the APK, grant microphone (and notification) permission, and open the app. Both
phones must be on the same Wi-Fi. See [docs/android.md](docs/android.md) for what has and has not been
tested on real hardware. The app has no telemetry, no analytics and no Google services; it asks for
`INTERNET` only because Android requires it for any network socket, including the local-network ones.
The permissions and the build-time check for this are listed in [docs/android.md](docs/android.md).

## How it works

One engine process per user, `relayd`, owns audio devices, your identity key, discovered peers and call
sessions. Clients control it over a local gRPC API and never carry audio. Devices find each other through
mDNS, open an authenticated signaling channel (Noise XX with pinned long-term keys), and exchange Opus
audio as encrypted UDP media. The audio path avoids allocation and locking in its callbacks. Details are
in [docs/architecture.md](docs/architecture.md), with the wire formats in
[docs/protocol.md](docs/protocol.md) and [docs/net.md](docs/net.md).

## Security and limits

- Calls are authenticated and encrypted between the two devices; a first-contact key is trusted but marked
  unverified until you compare fingerprints. First use is not man-in-the-middle proof by itself.
- Channel passwords use a PAKE; the composition of these primitives has **not had an independent security
  review**, and that review is a release gate ([docs/channels.md](docs/channels.md),
  [docs/reviews/security-net-channels.md](docs/reviews/security-net-channels.md)).
- Relay is LAN-only by design. Internet calling is not supported; an opt-in overlay (Tailscale) mode is
  unit-tested but has no real-world call yet.
- The eight non-English translations were machine-written and have not been reviewed by native speakers;
  Hindi is flagged experimental ([docs/i18n.md](docs/i18n.md)).
- The Android app has only been verified on an emulator so far; there is no echo cancellation yet, so use
  headphones or a handset ([docs/roadmap.md](docs/roadmap.md)).
- Release builds are not code-signed or notarized.

## Build from source

Requirements: the Rust toolchain pinned in `rust-toolchain.toml` (installed automatically by `rustup`),
and on Linux the usual audio and windowing development packages (CI installs `libasound2-dev pkg-config libgtk-3-dev libxkbcommon-dev libwayland-dev libayatana-appindicator3-dev`).

```sh
cargo run                                  # desktop app (starts its own daemon)
cargo run -p relay-desktop -- --mock       # desktop UI without an engine
cargo run -p relay-cli -- tui              # terminal client
cargo build -p relay-cli -p relay-daemon   # headless: relay + relayd only

cargo install cargo-nextest --locked       # once
just test                                  # all tests (nextest) plus doctests
```

Android: `just android-setup` once (NDK, Rust targets, `cargo-ndk`; needs the Android SDK and a JDK), then
`just android-apk` to build `dist/android/relay-<version>-android-arm64-v8a.apk`, or `just android-install`
to install it on a connected phone. Packaging and release steps are in
[docs/releasing.md](docs/releasing.md) and [docs/packaging.md](docs/packaging.md).

## Project layout

| Path | Contents |
|---|---|
| `apps/relay-daemon` | `relayd`, the per-user engine process |
| `apps/relay-cli` | `relay`, the CLI and TUI |
| `apps/relay-desktop` | Native desktop shell (egui) with tray |
| `apps/relay-android` | Android app (Kotlin shell around the Rust engine and UI) |
| `crates/relay-core`, `relay-protocol` | Call state machine, routing; RTP, jitter and wire formats |
| `crates/relay-audio` | Devices, Opus, mixer, noise suppression |
| `crates/relay-net` | Discovery, trust, Noise transport, signaling |
| `crates/relay-engine`, `relay-api` | Engine assembly; local gRPC API |
| `crates/relay-client`, `relay-ui`, `relay-platform` | Shared client logic, shared UI, hotkeys |
| `crates/relay-config`, `relay-io`, `relay-messages` | Preferences and identity, OS I/O, session chat and files |
| `localizations/` | Fluent translations, one folder per language |
| `assets/`, `packaging/`, `scripts/packaging/`, `contrib/` | Icons, fonts, sounds; installer config; release scripts; launchd and systemd examples |
| `docs/` | Design notes |

## Documentation

| Topic | Files |
|---|---|
| Design | [architecture](docs/architecture.md), [engine](docs/engine.md), [net](docs/net.md), [protocol](docs/protocol.md), [audio](docs/audio.md), [channels](docs/channels.md), [session messages](docs/session-messages.md), [decisions](docs/decisions.md) |
| Clients | [CLI](docs/cli.md), [desktop](docs/desktop.md), [UI](docs/ui.md), [Android](docs/android.md), [API](docs/api.md), [config](docs/config.md), [i18n](docs/i18n.md) |
| Project | [roadmap](docs/roadmap.md), [gates](docs/gates.md), [evidence](docs/evidence.md), [limits](docs/limits.md), [release policy](docs/release.md), [releasing](docs/releasing.md), [packaging](docs/packaging.md) |

## Contributing

- Format with `cargo fmt`; the repository indents with tabs (`rustfmt.toml`). CI also runs clippy.
- Use [Conventional Commits](https://www.conventionalcommits.org/) (`feat(net): ...`, `fix(ui): ...`,
  `docs: ...`); release notes are generated from them.
- Add or update tests with every change and run `just test`.
- User-visible text goes through Fluent (`tr!`), and **all nine languages are updated together** in
  `localizations/<lang>/`. Sections are discovered automatically ([docs/i18n.md](docs/i18n.md)).
- Relay is unreleased: there are no compatibility shims or migrations for older formats.

## Licence

Apache License 2.0, see [LICENSE](LICENSE). Licences of bundled dependencies and fonts are listed in
[THIRD_PARTY_LICENSES](THIRD_PARTY_LICENSES) (regenerated at release time and shipped with every package).
