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

45 assets plus `SHA256SUMS.txt`: 23 packages (the APK among them) and a `.minisig` signature for each of the
22 non-APK packages (see "Update signing"). The desktop packages are not code-signed or notarized: macOS apps are ad-hoc signed,
Windows installers are unsigned. The APK is built non-debuggable and signed with the Relay release key (see
"Android signing" below); it is arm64-v8a only (64-bit ARM, nearly every phone since about 2016), and the release notes say so.

## Build and publish

1. Bump `[workspace.package] version` and `Cargo.lock`, commit, tag `vX.Y.Z` (see `docs/release.md`).
   Release notes: if `docs/release-notes/<version>.md` exists it is the body (a summary of what the
   release can do and its known limitations; 0.1.0 has one, since the first release has no previous
   version to compare against). Without that file they are generated from conventional commit subjects
   by `git-cliff` (`cliff.toml`). Either way the script adds the downloads table, the signing notes and
   the verification instructions.
2. Pushing the tag runs `.github/workflows/release.yml`:
   - `version`: `package_release.py validate` (tag equals the workspace version; also outputs the
     toolchain from `rust-toolchain.toml`), the Python unit tests, the git-cliff changelog.
   - `notices`: `cargo about generate about.hbs > THIRD_PARTY_LICENSES`, shipped in every package.
   - `build` (six native runners, matrix as PaintCraft's: `windows-2022`, `windows-11-arm`,
     `macos-15`, `macos-15-intel`, `ubuntu-22.04`, `ubuntu-22.04-arm`): builds `relay` and `relayd`
     (and checks `cargo tree` shows no GUI crate), then `relay-desktop`, then packages.
   - `android`: environment `android-signing`; NDK r28c (`28.2.13676358`) from `nttld/setup-ndk` (pinned, local
     cache) and a version check; then `just android-release` (JDK 21, `aarch64-linux-android`, cargo-ndk),
     renamed by `package_android.py`.
   - `sign`: environment `update-signing`, never on pull requests; signs every package but the APK
     (see "Update signing") and uploads `relay-<version>-signatures`.
   - `assemble`: `package_release.py finalize` requires exactly the expected asset set (packages and
     signatures), merges the per-asset `.sha256` files and the signatures' hashes into `SHA256SUMS.txt`, and `sha256sum --check` confirms it.
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
6. Look for: all nine jobs green (six `Build ...`, `Build Android APK`, `Sign update packages`, assemble); on Windows the
   "Verify MSI install, repair and uninstall" step; on Linux the package inspection step; a release
   marked pre-release with 45 assets plus `SHA256SUMS.txt` and a notes body (the hand-written file, else `git-cliff`); `.deb` and
   `.rpm` versions `0.1.0~rc.1`; `gh attestation verify <asset> -R PixelSoftwareStudios/Relay`.
7. Tag the real release from a green `main` commit: `git tag -a v0.1.0 && git push origin v0.1.0`.

Expected, not defects: the desktop installers are not code-signed (only the update signatures above). Windows SmartScreen warns; macOS blocks the first launch (Open
Anyway in Privacy & Security); attestations prove which
workflow built the files, not who the publisher is.

## Android signing

The release key lives only in the GitHub environment `android-signing` (secrets `ANDROID_KEYSTORE_BASE64`,
`ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`), restricted to `main` and `v*`
tags. Only the `android` job declares that environment, so it is skipped on pull requests. It decodes the
keystore into the runner's temp directory, runs `just android-release` (Gradle `assembleRelease`, then
`check_android_apk.py` and `check_android_signature.py`, which fails on a debug-signed APK) and deletes the file.
`versionCode` follows the version (`0.1.0` is 100, `1.2.3` is 10203), and Android only installs a higher one.

Signing certificate SHA-256 (pinned in the `justfile`; a build signed by any other key fails):

`0B:63:75:AD:94:A1:DC:96:5D:2A:0F:99:3C:06:51:61:C8:9E:8E:B2:DE:72:4C:81:C8:95:D2:5E:4A:EC:80:BB`

Check a downloaded APK with `apksigner verify --print-certs relay-<version>-android-arm64-v8a.apk`.

Keep an offline backup of the keystore, alias and both passwords. If the key is lost, nobody can update in
place and everyone has to uninstall first. Rotating it has the same cost.

## Update signing

Every package except the APK and `SHA256SUMS.txt` gets a detached minisign (Ed25519) signature, published as
`<asset-file-name>.minisig`. The self-updater (`crates/relay-update`) verifies it with the public key compiled in
from `crates/relay-update/update-key.pub`. The APK keeps its own signing (see above). Total: 23 packages + 22
signatures = 45 assets, plus `SHA256SUMS.txt`, which covers the signatures too.

**Trusted comment rule.** The signed trusted comment must be exactly `relay-update <version> <asset-file-name>`
(for example `relay-update 0.1.1 relay-0.1.1-macos-arm64-update.zip`; `<version>` has no `v`). The updater rejects
a signature whose comment names another version or file, so an old signed package cannot be replayed as a new
release. `sign_release.py` passes it with `minisign -t`, then verifies every signature with the public key and
re-checks the comment; any failure fails the job.

**Generating the key (once, by the maintainer, on a trusted machine).**

```sh
minisign -G -p update-key.pub -s relay-update.key   # asks for a password; use a long random one
```

Commit `update-key.pub` as `crates/relay-update/update-key.pub` (replacing the test key). Store
`relay-update.key` and its password in an OFFLINE backup (password manager plus an encrypted drive in another
place). If the secret key is lost, installed apps can never verify an update again: a new app version with a new
public key must then be installed by hand on every machine.

**Secrets.** Create the GitHub environment `update-signing` (deployment branches and tags: `main` and `v*`, like
`android-signing`) with secrets `UPDATE_SIGNING_KEY` (the full content of `relay-update.key`) and
`UPDATE_SIGNING_PASSWORD`. Only the `sign` job declares the environment, so it is skipped on pull requests. The
job installs minisign 0.12 from a pinned release URL checked against a SHA-256 (no third-party action sees the
key), writes the key to a 0600 temp file that is deleted in a `finally` block, and gives the password to minisign
on stdin, never on a command line.

**Test-key gate.** `update-key.pub` currently holds a TEST-ONLY key. `sign_release.py check-key` (run in the
`version` and `sign` jobs, outside pull requests) fails when that key equals a key in
`crates/relay-update/tests/fixtures/*.pub` or a key id in `scripts/packaging/test_update_key_id.txt`, and also
when neither exists. The Rust test `release_key_is_not_the_test_key` (`cargo test -p relay-update -- --ignored`)
checks the same.

**Verify by hand.**

```sh
minisign -Vm relay-0.1.1-linux-x64.deb -P <public key from update-key.pub, second line>
```

It also prints the trusted comment, which must match the rule above.

**Rotation plan.** (1) Generate the new pair. (2) Release a version whose updater trusts both the old and the new
public key, still signed with the old key. (3) Once users have it, sign releases with the new key only. (4) A
later release drops the old key. Skipping step 2 strands every installed app on the old key.

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
