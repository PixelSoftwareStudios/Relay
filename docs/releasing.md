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

23 assets plus `SHA256SUMS.txt`. Nothing is code-signed or notarized: macOS apps are ad-hoc signed,
Windows installers are unsigned, the APK is debug-signed. The APK is arm64-v8a only (64-bit ARM, nearly
every phone since about 2016); the name says so, and the release notes repeat it.

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
python -m pip install dmgbuild==1.6.7        # macOS only
python scripts/packaging/package_cli.py --version 0.1.0 --target aarch64-apple-darwin --bin-dir target/release --output dist
python scripts/packaging/package_release.py package --version 0.1.0 --target aarch64-apple-darwin --bin-dir target/release --output dist
python scripts/packaging/verify_macos_release.py --version 0.1.0 --target aarch64-apple-darwin --dmg dist/relay-0.1.0-macos-arm64.dmg --update dist/relay-0.1.0-macos-arm64-update.zip
python -m unittest discover -s scripts/tests -p 'test_*.py'
```

`--version` must equal the workspace version. Linux and Windows packaging need the tools listed above
and a native host.

## Verification status

Run locally on macOS (Apple silicon): the unit tests, the real `package_release.py package` /
`package_cli.py` / `verify_macos_release.py` chain with debug binaries (DMG built with `dmgbuild`,
ad-hoc signed, DMG and update ZIP verified identical), `actionlint` on the workflow and `git-cliff` on
`cliff.toml`. Unverified until the first CI run: every Windows step (WiX build, MSI, the PowerShell
check), `cargo packager` deb/AppImage output and the icon/desktop-file naming it produces,
`cargo generate-rpm` with `--set-metadata` only, the dependency lists of the `.deb` and `.rpm`,
the AppImage with a detached `relayd`, release-profile builds on all runners, the Android job on
GitHub's image, and publishing.
