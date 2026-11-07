# Roadmap status

Status reflects the repository, not a release decision. "Gates open" means code and tests exist but at
least one exit gate needs hardware, review or measurements that have not happened. Per-gate detail,
test names and missing evidence are in `docs/gates.md`.

| WP | Work | Status |
|---|---|---|
| 00 | Workspace, toolchain, policy | implemented, gates open (CI and non-macOS builds not yet run) |
| 01 | Domain, local API, fake adapters | implemented, gates met by tests |
| 02 | Local audio loopback | implemented, gates open (real-device allocation, long-run and latency) |
| 03 | Opus, RTP, jitter, drift | implemented, gates met by simulation tests |
| 04 | Discovery, trust, secure transport | implemented, gates open (independent security review, multi-host discovery) |
| 05 | Two-device call | implemented in-process, gates open (no two-machine run, no latency measurement) |
| 06 | Multi-call routing, PTT leases | implemented, gates met by tests (fake devices) |
| 07 | Microphone processing (noise suppression, automatic gain) | implemented with `sonora` (`docs/decisions.md`): noise suppression on at moderate strength, AGC2 off, both settable live in Settings, Audio; gates open (listening test on real devices, no measured intelligibility gain) |
| 08 | CLI, TUI, hotkeys | implemented, gates open (real key delivery per platform, engine input stream) |
| 09 | Preferences, identity, avatars, diagnostics | implemented, gates met by tests; non-macOS credential stores untried |
| 10 | Desktop client | implemented, gates open (resource gate unmet, never observed by a person) |
| 11 | Qualification and release | in progress: policy, CI, fuzz targets and release plan written; no gate closed |
| 12 | Speaker-mode AEC | not started (deferred past v1); the sonora stage already accepts `echo_cancel` and a far-end reference through `push_render`, but nothing feeds it playback audio and no setting exposes it, so WP-12 remains: the render reference from the playback mix, its delay alignment, and a user setting |
| 13 | Named channels (password and invite) | implemented, gates open (independent review of the PAKE composition; F-12 mitigated, residual risk documented) |
| 14 | Session messaging and file offers | implemented in engine, CLI and desktop, gates open (real two-machine transfer) |
| 15 | Daemon split (`relayd`), presence, call ownership, ringtone, PTT release delay | implemented, gates met by tests; autostart verified on macOS only; background availability setting, call alert and per-contact auto-answer (verified contacts only) implemented in the engine, API revision 6 (`docs/decisions.md`); the shells' side is open |
| 16 | Overlay (Tailscale) peers | implemented behind an opt-in; unit-tested, no real overlay call yet |
| 17 | Desktop localization (en plus eight languages, Noto Sans) | implemented; translations machine-written and unreviewed, Hindi flagged experimental, not seen on screen |
| 18 | Android | app implemented (Gradle + Kotlin shell, in-process engine, Keystore-sealed secrets, foreground call service, AAudio voice path, NSD fallback), verified on an emulator only; gates open (real phone, real call, mobile resource gate, security review of the Keystore scheme); foreground service for background availability, call alert and the auto-answer indicator not built yet (`docs/android.md`) |

Hardware, multi-device and external-review gates cannot be closed from a single development machine;
they are tracked as open in `docs/gates.md`, and measurements are recorded in `docs/evidence.md`.
Release procedure and the supported-target matrix are in `docs/release.md`.
