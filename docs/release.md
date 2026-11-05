# Release process

Relay has not been released. This document is the plan and the checklist; nothing here describes a
shipped artifact. Gate status lives in `docs/gates.md`; measurements in `docs/evidence.md`.

## Licensing

- Project license: Apache-2.0. The root `LICENSE` is the unmodified Apache License 2.0 text (it differs
  from the canonical text only by one leading blank line) and `[workspace.package] license = "Apache-2.0"`
  is inherited by every crate, so the two agree.
- The appendix boilerplate in `LICENSE` still has the `[yyyy] [name of copyright owner]` placeholder.
  That is normal for the license file itself; add a copyright line to `README.md` or a `NOTICE` file
  when the owner decides the wording.
- Dependency policy is `deny.toml`. Permissive licenses only; three reviewed exceptions (MPL-2.0 for
  `option-ext`, OFL-1.1 and Ubuntu-font-1.0 for the desktop's bundled fonts). Check with
  `cargo deny check`.
- `cargo deny check` was run locally on 2026-11-05 and reported advisories, bans, licenses and sources
  ok (three unmaintained-crate advisories for the zbus family are ignored with reasons in `deny.toml`).
  CI runs it too, once CI has run.

## Tooling

| Purpose | Command | Notes |
|---|---|---|
| Dependency policy | `cargo deny check` | `cargo install cargo-deny --locked` |
| Third-party notices | `cargo about generate about.hbs > THIRD_PARTY_LICENSES` | `cargo install cargo-about --locked --features cli`. Config in `about.toml` (accepted list mirrors `deny.toml`) and `about.hbs`. Generated at release time from the tagged revision and shipped with the binaries (attribution is required for distributed binaries, not for the source tree); the output is git-ignored, not committed. |
| SBOM | `cargo cyclonedx --manifest-path apps/relay-daemon/Cargo.toml --format json --describe binaries` (repeat for `apps/relay-cli` and `apps/relay-desktop`) | `cargo install cargo-cyclonedx --locked`. Run once on 2026-11-05 (CycloneDX 1.3, 387 components for `relay`); output was not committed because it is tied to one lockfile. It writes `*_bin.cdx.json` next to each binary crate's manifest, including `relay-desktop` and the benches; ship only the files for artifacts that are shipped. |
| Fuzz build | `cd fuzz && cargo +nightly fuzz build` | Needs nightly and `cargo install cargo-fuzz --locked`. |
| Fuzz run | `cd fuzz && cargo +nightly fuzz run <target> -- -max_total_time=600` | Targets: `rtp_parse`, `record_header`, `control_envelope`, `media_datagram`, `channel_wire`, `channel_text`, `manual_endpoint`, `avatar_decode`. |

SBOMs are release outputs attached to the release, not committed on `main`. The notices file is committed
as a snapshot but is only authoritative when regenerated at the release revision.

## Steps

1. **Freeze.** Pick the revision. All crates share `[workspace.package] version`; bump it, update
   `Cargo.lock` in the same commit, tag `vX.Y.Z` (annotated, signed when a signing key exists).
2. **Gates.** Walk `docs/gates.md`. Every gate that applies to the claimed targets must be
   met-by-test or have recorded hardware or review evidence at this revision. If not, the release
   either narrows its claims or does not happen. A tag cannot be cut with an open blocker row.
3. **Local checks on the tag.** `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`,
   `cargo test`, `cargo deny check`, plus the desktop crate built separately.
4. **CI green** on the same commit for ubuntu, macos and windows (see `.github/workflows/ci.yml`).
5. **Build artifacts** from a clean checkout with the pinned toolchain (`rust-toolchain.toml`), using
   `cargo build --release --locked -p relay-daemon -p relay-cli` (and `-p relay-desktop` for the GUI). Artifacts are `relayd`, `relay` and `relay-desktop`, installed side by side (`docs/packaging.md`). Each target is
   built on its native runner; there is no cross-compiled artifact unless it is separately qualified.
6. **Notices and SBOM.** Generate `THIRD_PARTY_LICENSES` and the CycloneDX files from the tagged revision. Translations are embedded from `localizations/` (nothing to ship beside the binary); a language is released only after its native-speaker review (`docs/i18n.md`, `docs/gates.md`). Also ship `assets/font/OFL-*.txt` (the bundled Noto fonts are OFL 1.1 and are not crates, so `cargo about` does not list them).
7. **Hash and sign** (below), publish, then record the release in `docs/evidence.md` with the commit,
   toolchain, host OS and artifact hashes.

## Hashing and signing plan

Planned, not in place:

- A `SHA256SUMS` file lists every artifact, SBOM and notices file. It is itself signed.
- Release signing with a project-held key (minisign or `gpg`); the public key is published in the
  repository and the release notes. Signing happens on a maintainer machine, never in CI with a
  stored private key.
- Build provenance: GitHub artifact attestation from the release workflow when that workflow exists.
  Until then, the recorded commit, toolchain and `Cargo.lock` hash are the provenance claim, and
  reproducibility is not claimed.
- macOS: Developer ID signing and notarization of the app bundle and the `relay` binary. Needed for the
  microphone permission flow to be tested at all. No identity exists yet.
- Windows: Authenticode signing. No certificate exists yet. Until then Windows artifacts, if built, are
  labelled unsigned developer builds.
- Linux: signed tarball plus checksums; no distro packaging is planned for the first release.

## Supported-target matrix

A cell is filled only when its evidence exists. Everything below is currently unverified.

| Target | Headless CLI/engine build | Test suite | Real audio call | Global PTT | Desktop GUI | Status |
|---|---|---|---|---|---|---|
| macOS 14, Apple silicon | built and tested locally by authors | run locally by authors | not run | not run | built locally, not observed | developer builds only |
| macOS, Intel | not built | not run | not run | not run | not run | unsupported |
| Windows 10/11 x64 | CI job written, not run | not run | not run | not run | not run | unsupported |
| Linux x86_64 (X11) | CI job written, not run | not run | not run | not run | not run | unsupported |
| Linux (Wayland, portal PTT) | not built | not run | not run | not run | not run | unsupported |
| Linux aarch64 | not built | not run | not run | not run | not run | unsupported |

Promotion rule: a target becomes "supported" for a capability only when a dated entry in
`docs/evidence.md` names the OS version, hardware, command and result, and the matching rows in
`docs/gates.md` are updated. Latency or resource numbers are quoted only with the conditions listed
there; the current microbenchmarks are not call latency.

## Known blockers for any public release

- Independent security review of the peer transport and channels (`docs/net.md`, `docs/channels.md`).
- Windows local transport (named pipe) and its access control.
- Hardware validation of capture/playback, global PTT and a two-machine call.
- Signing identities for macOS and Windows.
- Desktop client resource measurement.

## Rollback

- Before publishing: delete the tag and draft release; nothing has reached users.
- After publishing: mark the release as withdrawn in its notes, remove the download links, publish
  a `SHA256SUMS` revocation note, and ship a fixed patch version. Tags are never moved or reused.
- There is no auto-update channel, so a withdrawn build stays installed until the user replaces it;
  the release notes state the replacement and any manual step.
- Data: the per-user SQLite database carries a schema version and refuses a newer schema without
  touching the file, so downgrading the binary cannot corrupt a newer database. A downgrade to an
  older schema is not supported; a user restores the snapshot taken at open time or starts fresh.
  No compatibility shims exist because there is no earlier release.
- Security-relevant rollbacks (key handling, transport) bump the wire major so mismatched peers fail at
  the preface instead of falling back.
