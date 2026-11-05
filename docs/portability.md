# Portability: Windows and Linux

Status of building Relay for Windows and Linux, checked from a macOS machine
without running anything on those systems. **Type-checking is not running.**
Nothing here shows that Relay works on Windows or Linux; it shows only where
the compiler accepts the code for those targets.

## What was checked

Host: macOS 14 (Darwin 23.6, arm64), `rustc 1.99.1 (58cf8f9c3 2026-10-13)`.

Installed for this check:

- rustup targets `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu` (already present) and `x86_64-pc-windows-gnu`.
- Homebrew `zig` 0.17.1 (used only as a cross C compiler, `zig cc`) and `nasm` 3.02 (ring's Windows assembly path). `cmake` 4.4.4 and `ninja` 1.13.2 were already present.
- `cargo-xwin` and `cargo-zigbuild` were not installed; they are not needed for `cargo check`.

Commands, per crate, per target:

```sh
cargo check  --target <t> -p <crate> --all-targets
cargo clippy --target <t> -p <crate> --all-targets -- -D warnings
# t = x86_64-unknown-linux-gnu | x86_64-pc-windows-msvc
# extra: cargo check --target x86_64-unknown-linux-gnu -p relay-platform --features wayland
#        cargo check --target x86_64-pc-windows-gnu    -p relay-platform
```

C dependencies (ring, blake3, bundled SQLite via `libsqlite3-sys`, libopus via
`opusic-sys`/cmake) need a C compiler for the target, which a Mac does not
have. The build scripts were pointed at `zig cc` through small wrapper scripts
(`CC_<target>`, `CXX_<target>`, `AR_<target>`; the wrappers strip the
`--target=`/`-m64` flags cc-rs adds, and for the msvc target translate the
`lib.exe`-style `-out:` archiver call to `zig ar`). Consequences to keep in
mind:

- For `x86_64-pc-windows-msvc` the C code was compiled by zig's clang against mingw headers, not MSVC headers. It proves the Rust side and the build scripts run, not that MSVC compiles the C.
- Nothing was linked. `cargo check` stops before the linker, so missing system libraries at link time are not detected.
- `alsa-sys` insists on `pkg-config alsa`. A stub `pkg-config` that answers "found" let the build script pass. That checks the Rust bindings against the checked-in `alsa-sys` bindings only; it does not prove `libasound2-dev` provides what is needed (it is the documented package, see below).
- `libappindicator`/GTK crates were type-checked with the same stub; the real build needs the GTK development packages.
- No test was executed on any foreign target. All Unix-socket tests are `#![cfg(unix)]`, so a Windows test run exercises far less than a macOS or Linux run.

Source state: checks of relay-core, relay-protocol, relay-audio, relay-net,
relay-config, relay-api, relay-platform, relay-client, relay-messages, relay-ui
and the benches ran on a copy of the working tree at `8958514` plus
uncommitted edits. **relay-engine, relay-daemon and relay-cli do not compile on
any platform at that state** (relay-net now has `channel_message::Body::MemberRef`,
`ChannelProof::is_reply` and changed `gate`/`tag` signatures that relay-engine
does not yet use). Those three were therefore checked at the last consistent
commit `d1f2535`, which is a weaker statement about current code.

## Result matrix

`ok` = `cargo check --all-targets` and `cargo clippy -D warnings` pass for that target.

| Crate | linux-gnu | windows-msvc | Notes |
|---|---|---|---|
| relay-core | ok | ok | pure Rust (blake3 builds C/asm) |
| relay-protocol | ok | ok | |
| relay-net | ok | ok | ring needs nasm on msvc |
| relay-config | ok | ok | bundled SQLite compiled with zig cc; keyring v1 backends below |
| relay-audio | ok (stubbed pkg-config) | ok | libopus built with cmake+zig cc; ALSA only on Linux |
| relay-api | ok | ok | UDS code is `cfg(unix)`; Windows has loopback TCP only; all its tests are `cfg(unix)` |
| relay-platform | ok, also `--features wayland` | ok, `wayland` is a no-op; windows-gnu also ok | |
| relay-client | ok | ok | detached spawn uses `creation_flags` on Windows (compiled, not run) |
| relay-messages | ok | ok | |
| relay-engine | ok at `d1f2535`; fails at HEAD on all targets (net API drift) | ok at `d1f2535` | not a portability problem |
| apps/relay-daemon | same as relay-engine | same | |
| apps/relay-cli | ok at `d1f2535` (lib, bins, tests) | lib and bins ok; **tests fail** | see required changes |
| relay-ui | ok | lib ok; **test `link` fails** | see required changes |
| apps/relay-desktop | **fails** (tray-icon) | **fails** (tray-icon API) | ok on both once the two changes below are applied (verified on a patched copy) |
| benches | ok | ok | |
| apps/relay-android | not checked | not checked | Android build, out of scope |

Not checkable here: running anything; linking; MSVC-compiled C; the actual
`apt` package set (stubbed `pkg-config`); behavior of the keyring backends,
global hotkeys, tray, WASAPI/ALSA devices, Wayland portals, Windows detached
processes and file locks.

No change was needed in relay-protocol, relay-net, relay-config, relay-audio or
relay-platform: all five compile and pass clippy for both targets, so nothing
in those crates was edited.

## Required changes in crates owned elsewhere

1. **apps/relay-desktop, `src/tray.rs`**: `TrayIcon::set_icon_templated` and `TrayIconBuilder::with_icon_templated` exist only on macOS. On Windows and Linux use `set_icon` / `with_icon`, e.g. behind `#[cfg(target_os = "macos")]` / `#[cfg(not(target_os = "macos"))]` (lines around 61 and 81). Fails to compile on both targets otherwise.
2. **Workspace `Cargo.toml` (tray-icon)**: `tray-icon = { default-features = false }` fails on Linux with "either the `libappindicator` or `ksni` feature must be enabled on Linux and BSD". Add one for Linux, for example in apps/relay-desktop: `[target.'cfg(target_os = "linux")'.dependencies] tray-icon = { workspace = true, features = ["ksni"] }`. `ksni` is pure Rust over D-Bus and needs no GTK; `libappindicator` also type-checks but pulls GTK 3 (`libgtk-3-dev`) and, at run time, an appindicator library. `tray::build` already returns `None` on every OS except macOS and Windows, so the Linux tray is currently unused either way.
3. **apps/relay-cli, `tests/common/mod.rs`**: `pub mod engine;` uses `UdsListener`, `serve_unix` and `Endpoint::Unix`, all `cfg(unix)`. Gate it with `#[cfg(unix)]` (and gate its users). Every test binary that includes `common` currently fails to compile on Windows, which fails `cargo test` in the `headless` CI job on `windows-latest`.
4. **crates/relay-ui, `tests/link.rs`**: uses `Endpoint::Unix`; add `#![cfg(unix)]` or use `LoopbackTcp` on Windows.
5. **relay-engine** needs to catch up with the relay-net changes listed above before any further cross-platform check of relay-engine, relay-daemon or relay-cli at HEAD is meaningful.

## Linux system packages

| Need | Package (Debian/Ubuntu) | Why |
|---|---|---|
| ALSA headers | `libasound2-dev` | `alsa-sys` via pkg-config (cpal). Only ALSA is built; no PulseAudio, PipeWire or JACK crate is in the dependency graph. |
| pkg-config | `pkg-config` | `alsa-sys` |
| cmake, ninja, C/C++ compiler | `cmake ninja-build build-essential` | libopus (`opusic-sys`). GitHub Ubuntu and Windows runners ship cmake and ninja; install them on bare images. |
| GTK 3 | `libgtk-3-dev` | only if tray-icon uses the `libappindicator` feature |
| Runtime appindicator library | `libayatana-appindicator3-0` | only at run time with `libappindicator`; it is loaded with dlopen, not needed to build |

Wayland, X11, xkbcommon and EGL are loaded with `dlopen` (`wayland-sys` has the
`dlopen` feature, `xkbcommon-dl`, `x11-dl`, glutin), and the portal and Secret
Service clients are pure Rust (`zbus`), so no development package for them
appears in the dependency graph. This is inferred from the graph and the
stubbed build, not from an `apt` build on Linux; a real Linux CI run is the
confirmation. `libxdo-dev` was in the CI list but no `libxdo` crate is in the
graph, so it was removed from `.github/workflows/ci.yml`.

Runtime on Linux (not verifiable here): a running Secret Service provider
(gnome-keyring or KWallet) for the keyring, an X11 `DISPLAY` for global
hotkeys (Wayland sessions report the X11 backend as unsupported; the portal
backend is behind the `wayland` feature of relay-platform, which no app
enables yet), and ALSA devices.

## Per-OS behavior and known Windows gaps

- **Keyring** (relay-config, keyring 4.2 default `v1`): macOS Keychain; Windows Credential Manager (`windows-native-keyring-store`); Linux Secret Service over `zbus` (no libdbus). Compiles for all; behavior untested off macOS.
- **File permissions**: owner-only directories and files (0700/0600), the profile-file permission refusal, and parent-directory creation modes are `cfg(unix)` and live in one place, `relay-io` (`fs::private`), used by relay-config, relay-api `tcp.rs`, relay-client `daemon.rs` and relay-cli `doctor.rs`. On Windows these paths fall back to ordinary `create_dir_all`/`fs::write`: **no ACL hardening**. The code comments rely on per-user profile ACLs, but the loopback TCP profile (holding the auth token) defaults to the temp directory (`%TEMP%\relay`) and `tcp.rs` itself says Windows ACL hardening is an unmet gate. Treat the token file as readable by whoever can read that directory until explicit ACLs are set. **Windows ACL gap (unmet gate):** `relay-io` does not apply an owner-only DACL to files or directories; no small maintained crate makes that simple (it needs `SetNamedSecurityInfoW` with a protected DACL for the current user SID), so the limitation is documented rather than half-implemented. Closing it means adding the DACL in `relay_io::fs::private` (`ensure_private_dir`, `ensure_owned_private_dir`, `restrict_file`, `create_private_*`) and a Windows test that reads the ACL back.
- **IPC**: Unix domain sockets on Unix; on Windows only loopback TCP with a token profile. There is no named-pipe transport. `--socket` and `RELAY_SOCKET` are rejected on Windows by the CLI. An authenticated loopback port is reachable by other local users, so the token is the only barrier.
- **Daemon launch**: detached spawn is `process_group(0)` on Unix and `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP` on Windows (compiled, never run). The start lock uses `File::try_lock` (works on Windows via `LockFileEx`).
- **Shutdown**: SIGTERM is handled on Unix; on Windows only Ctrl-C. A Windows service stop or console close is not handled.
- **Global hotkey**: macOS needs Input Monitoring; Windows reports Available without a probe; Linux X11 only.
- **Atomic file replace and received files** (`relay-io`): `fs::rename` over an existing file, then a directory fsync that is a no-op off Unix. Durability after power loss is weaker on Windows. Received files commit through `rename_noreplace`: `renameat2(RENAME_NOREPLACE)` on Linux, hard link plus unlink elsewhere (macOS, Windows/NTFS), and an exclusive placeholder where hard links are unsupported (FAT/exFAT, small race documented in the module).
- **Tests**: Unix-socket suites in relay-api, relay-client, relay-engine and relay-cli are `cfg(unix)`; there is no automated Windows coverage of the daemon, the API server or the CLI end to end.

## Recommended CI additions

Current CI already runs `headless` and `desktop` on ubuntu, macos and windows.

1. Fix the failing Windows test targets (required changes 3 and 4) so the `headless` job passes on `windows-latest`, and the Linux tray feature (change 2) so the `desktop` job builds on `ubuntu-latest`. Until then those cells are expected to fail.
2. Add `cargo check --workspace --all-targets` for the UI crate on all three OSes (the `desktop` job covers relay-desktop only; relay-ui is outside `default-members`).
3. Add a Linux cell with `--features wayland` for relay-platform: `cargo check -p relay-platform --features wayland --all-targets`, so the portal backend is not left uncompiled.
4. Add Windows named-pipe and ACL tests only when those exist; until then add a Windows smoke test that starts `relayd --tcp-profile` and checks the profile's access rights.
5. Run one headless Linux job with real audio-less devices (`ALSA` null device or none) so the cpal code path at least initializes with no hardware.
6. Pin `cmake` and `ninja` explicitly (`apt-get install cmake ninja-build`) in the Linux step instead of relying on the runner image.
