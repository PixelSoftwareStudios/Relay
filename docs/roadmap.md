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
| 07 | Noise suppression selection | evaluated on synthetic audio only; bypass is the default, selection blocked on a speech corpus |
| 08 | CLI, TUI, hotkeys | implemented, gates open (real key delivery per platform, engine input stream) |
| 09 | Preferences, identity, avatars, diagnostics | implemented, gates met by tests; non-macOS credential stores untried |
| 10 | Desktop client | implemented, gates open (resource gate unmet, never observed by a person) |
| 11 | Qualification and release | in progress: policy, CI, fuzz targets and release plan written; no gate closed |
| 12 | Speaker-mode AEC | not started (deferred past v1) |

Hardware, multi-device and external-review gates cannot be closed from a single development machine;
they are tracked as open in `docs/gates.md`, and measurements are recorded in `docs/evidence.md`.
Release procedure and the supported-target matrix are in `docs/release.md`.
