# Releasing

How a tag becomes a GitHub release. The policy, gates and signing plan are in `docs/release.md`; the
install layout is in `docs/packaging.md`. The pipeline is modelled on the owner's PaintCraft release
workflow (without its codec helper, file associations or MIME data).

## Downloads

Names are `relay-<version>-<platform>.<ext>` for the desktop and `relay-cli-<version>-<platform>.<ext>`
for the headless pair (`relay`, `relayd`). Platforms: `windows-x64`, `windows-arm64`, `macos-arm64`,
`macos-x64`, `linux-x64`, `linux-arm64`.

| Platform | Desktop | Headless |
|---|---|---|
| Windows | `.msi`, `-portable.zip` | `.zip` |
| macOS | `.dmg`, `-update.zip` (the raw `Relay.app`, for self-updating) | `.tar.gz` |
| Linux | `.AppImage`, `.deb`, `.rpm`, `.tar.gz` | `.tar.gz` |
| Android | `relay-<version>-android-arm64-v8a.apk` | |

23 assets plus `SHA256SUMS.txt`. The desktop packages are not code-signed or notarized: macOS apps are ad-hoc signed,
Windows installers are unsigned. The APK is built non-debuggable and signed with the Relay release key (see
"Android signing" below); it is arm64-v8a only (64-bit ARM, nearly every phone since about 2016), and the release notes say so.

## Build and publish

1. Bump `[workspace.package] version` and `Cargo.lock`, commit, tag `vX.Y.Z` (see `docs/release.md`).
   Release notes are generated from conventional commit subjects by `git-cliff` (`cliff.toml`); there
   is no hand-written notes file.
2. Pushing the tag runs `.github/workflows/release.yml`:
   - `version`: `package_release.py validate` (tag equals the workspace version; also outputs the
     toolchain from `rust-toolchain.toml`), the Python unit tests, the git-cliff changelog.
   - `notices`: `cargo about generate about.hbs > THIRD_PARTY_LICENSES`, shipped in every package.
   - `build` (six native runners, matrix as PaintCraft's: `windows-2022`, `windows-11-arm`,
     `macos-15`, `macos-15-intel`, `ubuntu-22.04`, `ubuntu-22.04-arm`): builds `relay` and `relayd`
     (and checks `cargo tree` shows no GUI crate), then `relay-desktop`, then packages.
   - `android`: `just android-apk` (JDK 21, NDK `28.2.13676358`, `aarch64-linux-android`, cargo-ndk),
     renamed by `package_android.py`.
   - `assemble`: `package_release.py finalize` requires exactly the expected asset set, merges the
     per-asset `.sha256` files into `SHA256SUMS.txt`, and `sha256sum --check` confirms it.
   - `publish`: creates a draft release, uploads, diffs the uploaded names against `dist/`, and only
     then un-drafts it (prerelease when the version has a `-` suffix). A published release is never
     modified: use a new tag.
3. Pull requests that touch packaging files, and manual runs, execute everything except `publish`.
   A manual run with `artifacts-run` and `publish-tag` publishes an existing tag from a finished run
   after `verify_release_run.py` checks the run built that exact commit.

## Platform details

- **Windows**: `crt-static` (`RUSTFLAGS`) so no VC runtime installer is needed. The MSI
  (`packaging/windows.wxs`, WiX 5) installs the three programs to `%ProgramFiles%\Relay`, adds the
  folder to the machine `PATH`, adds a Start menu shortcut and the licences; the icon is generated from
  `assets/icon/relay-mark-*.png` at package time.
- **macOS**: deployment target 13.0 (a chosen floor, not tested on 13). `Relay.app` is built from
  `apps/relay-desktop/resources/Info.plist` (microphone text) plus bundle keys; programs are signed
  ad hoc with fixed identifiers; `dmgbuild` writes the DMG (drag to Applications). `verify_macos_release.py`
  checks that the DMG and the update ZIP contain the identical app (digest of every file), the
  signature, architecture and executable bits.
- **Linux**: `cargo-packager` 0.11.8 (deb, AppImage) and `cargo-generate-rpm` 0.21.0, pinned in
  `.github/actions/release-tools`. Runtime libraries are the host's (`ldd` must show none missing);
  the AppImage bundles only the windowing libraries listed in `packaging/cargo-packager.json`. The
  AppImage launches `relay-desktop`, which starts `relayd` from the same directory inside the mount.

## Local packaging

```
cargo about generate about.hbs > THIRD_PARTY_LICENSES
cargo build --release -p relay-daemon -p relay-cli && cargo build --release -p relay-desktop
python -m pip install --require-hashes -r scripts/packaging/requirements.txt   # macOS only (dmgbuild)
python scripts/packaging/package_cli.py --version 0.1.0 --target aarch64-apple-darwin --bin-dir target/release --output dist
python scripts/packaging/package_release.py package --version 0.1.0 --target aarch64-apple-darwin --bin-dir target/release --output dist
python scripts/packaging/verify_macos_release.py --version 0.1.0 --target aarch64-apple-darwin --dmg dist/relay-0.1.0-macos-arm64.dmg --update dist/relay-0.1.0-macos-arm64-update.zip
python -m unittest discover -s scripts/tests -p 'test_*.py'
```

`--version` must equal the workspace version. Linux and Windows packaging need the tools listed above
and a native host.

## Pinned inputs

Every action in `.github/` is pinned to a full commit SHA with the tag in a trailing comment
(`uses: owner/repo@<40 hex> # v4`); `dtolnay/rust-toolchain` is pinned to the commit of its `1.99.1`
branch and always receives `toolchain:` explicitly (the repo toolchain from `rust-toolchain.toml`; the
fuzz job uses `nightly-2026-11-04`). `.github/dependabot.yml` opens weekly PRs that bump the SHA and the
comment together; review the upstream diff, then merge. The toolchain version and the nightly date are
bumped by hand. To add or bump an action by hand, resolve the tag with
`gh api repos/<owner>/<repo>/git/ref/tags/<tag>` and, when the object type is `tag`, follow it with
`gh api repos/<owner>/<repo>/git/tags/<sha>` to the commit.

Tool versions are fixed too: `cargo-about@0.9.3`, `just@1.43.0` and `cargo-ndk@4.1.2` through
`taiki-e/install-action`; `cargo-packager` 0.11.8, `cargo-generate-rpm` 0.21.0 and `cargo-fuzz` 0.13.2
through `cargo install --version ... --locked`. macOS packaging installs Python tools only from
`scripts/packaging/requirements.txt` with `--require-hashes` (all transitive dependencies hashed).
Regenerate it from `requirements.in` with `pip-compile --generate-hashes --strip-extras`.
`cargo deny` runs for the root, `apps/relay-android` and `fuzz` lock files.

## First release checklist

One-time, by the maintainer (the workflows use only the built-in `GITHUB_TOKEN`; no secrets exist or are needed):

1. Repository must be public: build provenance attestations (`actions/attest-build-provenance`) are free
   only there; private repositories need GitHub Enterprise Cloud. The free `windows-11-arm` and
   `ubuntu-22.04-arm` runners are also public-repository only.
2. Settings > Actions > General: allow Actions (GitHub-authored and the third-party actions used:
   `dtolnay/rust-toolchain`, `Swatinem/rust-cache`, `taiki-e/install-action`, `orhun/git-cliff-action`,
   `gradle/actions`, `EmbarkStudios/cargo-deny-action`). Workflow permissions may stay "read"; the
   `publish` job requests `contents`, `id-token` and `attestations` write itself, unless an organisation
   policy caps the token at read.
3. Protect `main` (CI green required) and add a tag ruleset for `v*` (creation limited to maintainers, no
   deletion or force-update). Optionally enable release immutability once the first release is checked.
4. Run the workflow by hand first (Actions > release > Run workflow on `main`): it builds and verifies
   all packages and publishes nothing. This is the cheapest way to find a broken runner.
5. Dress rehearsal with a pre-release: on a throwaway branch set `[workspace.package] version` to
   `0.1.0-rc.1`, run `cargo update --workspace` so `Cargo.lock` agrees (the builds use `--locked`),
   commit, tag `v0.1.0-rc.1` and push the tag. The tag must equal the version, so the real `v0.1.0`
   tag needs the version `0.1.0` back. Afterwards delete the release and the tag.
6. Look for: all eight jobs green (six `Build ...`, `Build Android APK`, assemble); on Windows the
   "Verify MSI install, repair and uninstall" step; on Linux the package inspection step; a release
   marked pre-release with 23 assets plus `SHA256SUMS.txt` and a notes body from `git-cliff`; `.deb` and
   `.rpm` versions `0.1.0~rc.1`; `gh attestation verify <asset> -R PixelSoftwareStudios/Relay`.
7. Tag the real release from a green `main` commit: `git tag -a v0.1.0 && git push origin v0.1.0`.

Expected, not defects: nothing is signed. Windows SmartScreen warns; macOS blocks the first launch (Open
Anyway in Privacy & Security); attestations prove which
workflow built the files, not who the publisher is.

## Android signing

The release key lives only in the GitHub environment `android-signing` (secrets `ANDROID_KEYSTORE_BASE64`,
`ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`), restricted to `main` and `v*`
tags. Only the `android` job declares that environment, so it is skipped on pull requests. It decodes the
keystore into the runner's temp directory, runs `just android-release` (Gradle `assembleRelease`, then
`check_android_apk.py` and `check_android_signature.py`, which fails on a debug-signed APK) and deletes the file.
`versionCode` follows the version (`0.1.0` is 100, `1.2.3` is 10203), and Android only installs a higher one.

Signing certificate SHA-256: _to be recorded here from `keytool -list -v` before the first release._

Keep an offline backup of the keystore, alias and both passwords. If the key is lost, nobody can update in
place and everyone has to uninstall first. Rotating it has the same cost.

## Verification status

Run locally on macOS (Apple silicon): the unit tests, the real `package_release.py package` /
`package_cli.py` / `verify_macos_release.py` chain with debug binaries (DMG built with `dmgbuild`,
ad-hoc signed, DMG and update ZIP verified identical), `actionlint` on the workflow and `git-cliff` on
`cliff.toml`. Linux and Windows packaging were reviewed line by line against the `cargo-packager` 0.11.8 and
`cargo-generate-rpm` 0.21.0 sources, not run. Unverified until the first CI run: every Windows step
(WiX build, ICE validation, MSI, the PowerShell check), `cargo packager` deb/AppImage output and the icon/desktop-file naming it produces,
`cargo generate-rpm` with `--set-metadata` only, the dependency lists of the `.deb` and `.rpm`,
the AppImage with a detached `relayd`, release-profile builds on all runners, the Android job on
GitHub's image, and publishing.
