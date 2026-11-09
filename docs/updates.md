# Self-update (desktop)

`crates/relay-update` is the core of the desktop self-update: check GitHub Releases, download one
signed asset, verify it, replace the installation in place, and hand the relaunch to a detached
helper. It has no UI and no daemon control. Release signing is `scripts/packaging/sign_release.py`
and the release workflow; the update dialog is a later UI round.

Status: written and tested on the build host (macOS arm64). **Windows apply is UNVERIFIED on a real
Windows machine**, the macOS apply was not run against a real signed bundle, and the Linux AppImage
apply was not run on Linux (the file operations are tested on temp files).

## Flow

```text
Updater::new(version, Platform::detect())
  check()                 GET api.github.com/repos/PixelSoftwareStudios/Relay/releases/latest
  download(&available)    GET <asset>.minisig, GET <asset>; streams to a private dir, verifies
  apply(verified)         replace on disk, start the helper -> caller quits
cleanup_leftovers(&platform)   once at start
```

The API is blocking; call it from a worker thread. `download` takes a progress callback that returns
`ControlFlow::Break(())` to cancel. A manual install (`Plan::Manual { release_url }`) only gets the
check; `download` and `apply` refuse it with `Error::ManualOnly`.

## Install kinds and assets

| Install | Detection | Asset | Replace |
|---|---|---|---|
| macOS bundle | executable is `X.app/Contents/MacOS/<exe>` | `relay-<v>-macos-<arch>-update.zip` | swap the bundle by two renames |
| Windows MSI | executable directory is `%ProgramFiles%\Relay` (or `%ProgramW6432%`) | `relay-<v>-windows-<arch>.msi` | `msiexec /i ... /passive /norestart`, elevated |
| Windows portable | any other Windows directory | `relay-<v>-windows-<arch>-portable.zip` | rename running `.exe` aside, move new ones in |
| Linux AppImage | `$APPIMAGE` names an existing absolute regular file | `relay-<v>-linux-<arch>.AppImage` | atomic `rename(2)` over the image |
| deb, rpm, tarball, `cargo` build, debug build, unknown arch | everything else | none | none: `Plan::Manual` |

The MSI is per-machine (`Scope="perMachine"` in `packaging/windows.wxs`, `ProgramFiles6432Folder\Relay`,
fixed `UpgradeCode`, `MajorUpgrade`). Detecting it by install folder is a heuristic: a portable zip that
someone unpacks into `%ProgramFiles%\Relay` is treated as an MSI install (and the MSI then upgrades
exactly that folder, which is harmless). An executable under a Cargo `target/` directory (marked by
`CACHEDIR.TAG`) and every `debug_assertions` build is `Manual`.

## What is verified, and in which order

1. Only HTTPS, only `api.github.com`, `github.com`, `release-assets.githubusercontent.com` and
   `objects.githubusercontent.com` (GitHub serves release downloads from `github.com` through a 302 to
   `release-assets.githubusercontent.com`; checked against a live release on 2026-11-09;
   `objects.githubusercontent.com` served them before 2025). The host list is checked on **every** hop,
   before a byte is sent; at most 5 redirects; no port other than 443; no userinfo; a hop from HTTPS to
   HTTP is refused.
2. The release JSON (at most 1 MiB): not a draft, tag is exactly `v<semver>`, the `prerelease` flag
   agrees with the tag, the version is strictly newer than the running one (build metadata is ignored),
   and a prerelease is only offered to a prerelease build. The asset name is built from the tag and
   the platform and must be listed exactly once with the exact `browser_download_url`
   `https://github.com/<repo>/releases/download/<tag>/<name>`; a release whose assets point elsewhere
   is refused, not followed. A tag/asset version mismatch therefore shows up as "asset missing".
   The `.minisig` must be listed too.
3. Download: the signature file (at most 4 KiB) first; its **trusted comment must equal
   `relay-update <version> <asset-file-name>`**, which stops a validly signed file being replayed as
   another version or another platform's asset (rollback, cross-platform confusion). Then the asset,
   at most 300 MiB and at most the size the release lists; received bytes must equal that size. The
   payload is hashed by minisign (Ed25519 over BLAKE2b-512, the prehashed mode the `minisign` tool
   writes; legacy non-prehashed signatures are refused) while it streams to a file in a **private
   0700 directory with a random name** beside the install (same filesystem as the final rename).
   Signature, key id and the signed trusted comment are checked **before** the file is unzipped,
   mounted, executed or even opened by anything else. Any failure deletes the directory.
4. Right before use (`apply`) the payload is hashed again and compared with the hash taken while
   downloading (`Error::PayloadChanged`).
5. macOS only, after extraction: `Info.plist` has bundle id `dev.relay.relay-desktop`, version equal to
   the release's `major.minor.patch`, executable `relay-desktop`, which exists, is a regular file and is
   executable; `codesign --verify --deep --strict` succeeds and the code identifier is
   `dev.relay.relayd`.
6. Zip extraction (`zipx`): at most 5000 entries, 1 GiB written (counted while writing, the headers are
   not trusted), an entry of at least 1 MiB may not expand more than 100x, every entry under the single
   expected top-level directory, no absolute path, `..`, `.`, empty component, backslash, NUL, `:` or other
   Windows-reserved characters, no encrypted entry, no symlink, FIFO, socket or device, no duplicates,
   files created with `create_new` (never overwrite, never follow a link), setuid/setgid/sticky and
   group/other write bits dropped.
7. No shell string interpolation: helpers run a fixed script (`/bin/sh -c`, or PowerShell
   `-EncodedCommand`) and receive every path as a separate argument or as an environment variable of the
   child only.

Not verified or not protected:

- **The signing key itself.** `update-key.pub` is the TEST-ONLY key until the maintainer replaces it
  (`crates/relay-update/UPDATE_KEY.md`; `cargo test -p relay-update -- --ignored
  release_key_is_not_the_test_key` fails until then and belongs in the release job). Whoever has the
  signing secret can ship code to every user; there is no key rotation, revocation or expiry. Losing the
  key means shipping a build with a new compiled-in key by hand.
- **Rollback by an attacker who can serve an older signed release as "latest"** is limited by the
  strictly-newer rule and the trusted comment, but a frozen "latest" (withholding updates) is not
  detectable.
- **The packages are not Developer ID signed, notarized or Authenticode signed.** The updater's trust is
  the minisign key, not the OS. Gatekeeper never sees the downloaded files (no quarantine attribute is
  set by this process). The MSI shows an "unknown publisher" UAC prompt.
- Windows MSI: the file is hashed and compared in the elevated helper just before `msiexec`, but there
  is a small window between that check and `msiexec` reading the file in the user's temp directory
  where a process running as the same user could swap it. An attacker with the user's rights already
  controls the user's files; the elevation makes the MSI case the one worth hardening later (copy to a
  location only administrators can write, or Authenticode-sign the MSI).
- TLS roots are Mozilla's bundled `webpki-roots` (no system store, so a locally installed corporate
  root does not make an interception possible, and a machine whose clock or store is broken still
  works). The roots age with the Relay release. Proxies from the environment are honoured by `ureq`'s
  default; the proxy cannot read or alter the TLS stream.
- `Updater::apply` does not defend against a user who runs a modified copy of Relay.

No telemetry: one `GET` for the check, and for the chosen asset exactly two more (`.minisig`, asset).
`User-Agent: Relay/<version>`, no cookies (the `ureq` cookie feature is not compiled in), no auth, no
referer. URLs of redirect targets (signed queries) are never logged; error text from the HTTP stack is
stripped of anything after a `?`.

## No override

The endpoint, host list, repository and key are constants and `include_str!` data. The production
constructors `Updater::new` / `Updater::detect` cannot be given an endpoint. `Updater::with_endpoint`
exists only under `cfg(test)` or the `test-util` feature; `relay-update` enables it for its own tests
through a self dev-dependency, and no dependent crate may enable it. A test greps the sources for
`env::var` (only platform detection and `%SystemRoot%` are allowed) and `set_var`. Environment variables
read: `APPIMAGE`, `ProgramFiles`, `ProgramW6432` (detection) and `SystemRoot` (to find
`powershell.exe`).

## macOS details

Preflight (also done at `download`, so the failure comes before 100 MB are fetched) returns distinct
errors the UI can translate: `AppTranslocated` (path contains `/AppTranslocation/`: the app was run
straight from the quarantine without being moved), `ReadOnlyVolume` (mounted read-only, e.g. the DMG),
`ParentNotWritable` (for example `/Applications` owned by another user). The zip is extracted into a
`.relay-update-*` directory next to the bundle (same volume), checked, and swapped:
`Relay.app -> .Relay.app.old-<random>`, then new `-> Relay.app`; if the second rename fails the first is
undone. The sequence is two renames, not one atomic exchange: between them no `Relay.app` exists for a
few microseconds. The helper (`/bin/sh`, own process group, waits up to 5 minutes for this pid) runs
`open -n <bundle>` and, only if that succeeded, removes the old bundle; `cleanup_leftovers` removes
`.Relay.app.old-*` and stale staging directories (older than one day) at the next start.

### Keychain behaviour after an update (measured)

The release `.app` is ad-hoc signed with `--identifier dev.relay.relayd` and no explicit requirement
(`scripts/packaging/package_macos.py`). Measured on macOS 14 with a copy of `/bin/echo` and one of
`/bin/cat`, both `codesign --force --sign - --identifier dev.relay.relayd`:

```
# designated => cdhash H"f656f63e..." or cdhash H"e0452b8c..."     (echo)
# designated => cdhash H"e4828c3e..." or cdhash H"3519a080..."     (cat)
```

The identifier is **not** part of the designated requirement of an ad-hoc signature; the requirement
is the cdhash, which changes with every build. `docs/packaging.md` used to say the opposite (corrected).
The identity and channel secrets live in the legacy login-keychain item `relay-voice` / `vault`
(`keyring` default store), whose access control trusts the creating program by its code requirement.
So after a self-update the new `relay-desktop`/`relayd` has another cdhash than the one the user
approved with "Always Allow", and macOS shows the keychain prompt again once; "Always Allow" then
binds to the new cdhash and the prompt returns after the next update. The data is not lost and the
update does not touch the item. The prompt itself was **not** observed on a real keychain (no keychain
was touched); the cdhash measurement is solid, the prompt follows from documented legacy-keychain ACL
semantics and is the same gap as the open gate in `docs/gates.md` ("Signed-bundle keychain
behaviour"). Fixing it needs a stable requirement: Developer ID signing (`identifier ... and anchor
apple generic and certificate leaf[subject.OU] = TEAM`), or at least signing with `-r='designated =>
identifier "dev.relay.relayd"'`, which any ad-hoc binary with that identifier would also satisfy (weaker;
this is what `scripts/dev-sign-macos.sh --no-cert` does). Changing the packager is not part of this
crate.

## Windows details (UNVERIFIED on Windows)

Compiled and type-checked for `x86_64-pc-windows-msvc` (with the HTTP client stubbed: `ring`
cannot be built for MSVC from a Mac); the planning, archive and rename logic is unit-tested on the host.
Never run: PowerShell helper, `msiexec`, UAC.

- MSI: after exit the helper (hidden PowerShell started with `-EncodedCommand`, which the default
  "Restricted" execution policy does not block; inputs are `RELAY_UPDATE_*` variables of that child only)
  checks the SHA-256, runs `Start-Process msiexec /i "<msi>" /passive /norestart -Verb RunAs -Wait`, then
  starts `relay-desktop.exe` and deletes the staging directory. A cancelled UAC prompt leaves the old
  version installed and it is simply started again.
- Portable: `relay-desktop.exe`, `relay.exe`, `relayd.exe` from `<top>/bin/` replace the running ones
  after each is renamed to `<name>.old-<random>` (Windows allows renaming a running image); any other
  file in `bin/` is refused; on failure all renames are undone. The helper deletes the `.old-*` files after
  the app exited; `cleanup_leftovers` removes any that were locked.
- **Open problem: the old daemon.** `relay-desktop.exe --run-daemon` runs from the same file. For the
  portable swap the rename works; for the MSI, Windows Installer finds `relay-desktop.exe` in use
  and may show a "files in use" dialog or schedule the replacement for the next restart. The UI should
  stop the daemon (with the existing confirmation, not while a call is live) before `apply` on
  Windows. This crate never kills processes.

## Linux AppImage

`download` stages the file beside `$APPIMAGE`, `apply` gives it the old file's permission bits plus owner
execute (setuid/setgid/sticky never carried over) and renames it over the old image: anything starting the
file sees the old or the new image, never half of one. The helper waits for this pid and `exec`s the
image with `APPIMAGE`, `APPDIR`, `ARGV0` and `OWD` removed from its environment (the runtime sets them
for the new image). The old process keeps running from its already-mounted image. FUSE-less setups that
run the image with `--appimage-extract-and-run` have no `$APPIMAGE` and are `Manual`.

## The old daemon after an update

This crate never stops the daemon. On macOS and Linux the old `relayd` keeps running from the replaced
(now unlinked or moved-aside) file. Read in `crates/relay-ui/src/model.rs` and
`crates/relay-api/src/revision.rs`: the "background service is out of date" card
(`crates/relay-ui/src/ui/outdated.rs`) appears when `daemon_api_revision != API_REVISION`. **That only
covers a release that bumps `API_REVISION`.** An update between two versions with the same API revision
(the common case) leaves an old daemon running and the card never shows. `BUILD_REVISION` is the git
commit (and `-dirty`) and is deliberately not compared. What is missing for self-update: the daemon must
report its crate version (or the UI must compare it) and `daemon_out_of_date` must also be true when
the daemon's version differs from the app's in a release build, or every release must bump
`API_REVISION`. Not changed here (relay-ui is outside this task). The existing restart button is
already disabled during a call, which is the behaviour wanted after an update too.

## Dependencies

| Crate | Why | Evidence |
|---|---|---|
| `ureq` 3.5.0 (`rustls` only) | smallest maintained blocking HTTP client; per-request redirect control (`max_redirects(0)`), `https_only`, timeouts. No HTTP client with TLS was in the tree (tonic's hyper is plaintext over a local socket). Features: ring + webpki-roots, no gzip, no cookies, no proxy extras. | v3.5.1 published 2026-11-02, ~260M downloads, repo algesten/ureq |
| `minisign-verify` 0.3.0 | verification-only, zero dependencies, maintained by the author of minisign (jedisct1) | 0.3.0 published 2026-09-25, 21M downloads, MIT |
| `semver` 1.0.28 | already in the tree; precedence rules | dtolnay |
| `zip` 9.1.3 (`deflate-flate2`, no default features) | reading only deflate/stored; flate2 with `rust_backend` (miniz_oxide, already in tree). 9.5.0 (published today) deliberately not taken | zip-rs/zip2 |
| `sha2`, `uuid`, `tempfile`, `serde`, `serde_json`, `thiserror`, `rustix` | already workspace dependencies | |
| dev: `blake2`, `ed25519-dalek`, `base64`, `hex` | the test-only minisign signer | |

`cargo deny check`: advisories, bans and sources pass; **licenses fail on one new crate:
`webpki-roots` 1.0.11 is `CDLA-Permissive-2.0`** (a permissive data licence for the Mozilla root list), not
in `deny.toml` `allow`. Decision for the maintainer: allow `CDLA-Permissive-2.0` for `webpki-roots` as an
exception (and list it in `THIRD_PARTY_LICENSES`), or switch `ureq` to the system verifier
(`rustls-platform-verifier`, no webpki-roots, but it trusts locally installed roots). No exception was
added.

## Tests

`cargo nextest run -p relay-update`: version and prerelease rules, per-platform asset selection and
tag/asset/URL consistency, install-kind detection, host allowlist (including a redirect to a forbidden
host that is never requested, HTTPS to HTTP, userinfo and port tricks), redirect limit, size caps and
mismatches, exact User-Agent, no cookies, wrong key, wrong key id, replayed trusted comment (other
version, other asset, other formats), tampered payload, missing `.minisig`, cancel, zip-slip, symlink and
device entries, zip bombs, bundle checks, macOS swap and rollback on fake bundles with an injected
`codesign`, AppImage swap and relaunch helper (waits for the pid, scrubs the image variables, a path
full of shell metacharacters stays a path), Windows portable swap, rollback and PowerShell encoding, the
production-endpoint constant and the no-`env::var` guard. All local: a private temp directory per test and
a loopback server; no network, no real app, data directory, keychain, socket or process is touched.
`release_key_is_not_the_test_key` is `#[ignore]`d and fails while the test key is compiled in.
