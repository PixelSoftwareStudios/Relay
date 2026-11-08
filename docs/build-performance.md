# Build performance

Measurements for build time, test speed, binary size and disk use. Machine: MacBook Pro
M1 Pro (10 cores), macOS 14, Rust 1.99.1. The machine was shared with other builds, so wall
times carry noise (load averages between 20 and 120 during some runs); CPU time and sizes
are the steadier numbers. Measured on a copy of the tree at one commit, in separate
`CARGO_TARGET_DIR`s, so the tree being edited did not move underneath the runs.

## Results

| Area | Before | After |
|---|---|---|
| Test run, 1893 tests, nextest | 123-179 s (median 152 s) | 80-124 s (median 98 s) |
| Clean test build (`cargo test --workspace --no-run`) | 3m45 wall, 13m36 CPU | 3m30 wall, 15m34 CPU (hot deps are optimised) |
| Slowest tests (debug) | media_sink_adapt drift test over 60 s on first run, autostart 16-18 s, relay-ui fit tests 12-14 s | see "Tests" |
| relay (CLI) release | 12.4 MB | 8.7 MB |
| relayd release | 17.5 MB | 12.6 MB |
| relay-desktop release | 23.3 MB | 17.2 MB |
| librelay_android.so as packaged (stripped) | 29.8 MB | 25.8 MB |
| dist/android APK | 66.3 MB | 45.3 MB |
| target/debug/deps after `just clean-cache` | 108 GB | 29 GB |

Release sizes after the change are the fat-LTO build with the symbol table stripped with
`strip -x`, which is what `strip = "symbols"` does on macOS.

## Tests

`cargo test` runs test binaries one after the other (about 340 of them) and all tests of a
binary as threads of one process. `cargo nextest` runs one process per test and many
processes in parallel. Install it once with `cargo install cargo-nextest --locked`; `just test`
runs nextest and then `cargo test --doc` (nextest does not run doctests).

Optimising dependencies in `[profile.dev.package.*]` (argon2, ciphers, Opus, rubato, image
codecs, SQLite, egui text layout) cut the nextest run time from a median of 152 s to 98 s,
with no change in clean build time beyond noise. The workspace crates stay at opt-level 0.

Flake rates over five full nextest runs each (calls_fit excluded, see below):

| Config | Runs with a failure | Failures |
|---|---|---|
| default profiles | 2 of 5 | connect_trust x2 |
| optimised deps | 3 of 5 | media_capture_faults, media_rekey, daemon_cmd x2 |
| optimised deps, every engine test binary throttled to two at a time | 3 of 5 | media_rekey x3, service_forget |

Throttling did not remove the failures and made the run 40 percent slower, so only the tests
that start real `relayd` processes are grouped (`.config/nextest.toml`). The remaining
failures are not load related: media_rekey logs "prepare refused: media for this call
already exists", which looks like a real race. They need fixing in the code, not
retries (retries stay off).

Found by process isolation, not fixed here (relay-ui and engine sources belong to others):

- `relay-ui::calls_fit the_connection_details_fit_with_data_in_every_language` fails on
  every nextest run: "De 320x640 details: overflow" by 4 points. It passes under `cargo test`
  because the tests of that binary share the global active language and mask each other.
  It is probably a real German layout overflow.
- `relay-net::fuzz control_receiver_survives_arbitrary_stream_bytes` nested a runtime when
  it ran first in its process. Fixed.
- CI therefore still uses `cargo test`. Switch it to nextest (and `cargo test --doc`) once
  calls_fit and media_rekey are fixed.

## Compile time

- 340 of 985 compile units are test binaries and they are two thirds of the CPU time; an
  edit to relay-engine or relay-api followed by `cargo test --workspace --no-run` relinks
  them all (4m26). Edits to relay-ui rebuild relay-desktop in 5 s, relay-api in 11 s,
  relay-engine and relayd in 19 s. Merging test binaries per crate (`tests/it/main.rs`)
  is the largest remaining win and was not done: it touches around 250 files other people
  are editing.
- Long poles on a clean build: relay-engine tests (47 s), objc2-foundation (30 s),
  relay-ui (27 s), read-fonts (18 s), the Opus build script (14 s), SQLite's build script.
- `cargo tree -d`: every duplicate is forced from upstream (rust-embed pulls sha2 0.11, egui
  itertools 0.15, prost-build itertools 0.14, criterion regex and itertools 0.13, syn 2 and
  3 from different proc-macros). Nothing to align.
- `cargo machete`: relay-ui `thiserror` and relay-android `tokio-stream` look unused; not
  changed (other people's crates, no build-time effect).
- Rejected: `debug = 0` (12 GB against 14 GB) and `split-debuginfo = "off"` (13 GB) saved
  too little disk and no measurable time for losing line numbers in backtraces. macOS
  already defaults to unpacked split debuginfo and ld64 is the linker; lld and mold are not
  available for macOS arm64. Linux CI already links with lld on current Rust.
- CI: the rust-cache key is per job and only main saves it.
- macOS tip: Spotlight (`mds_stores`), XProtect and syspolicyd scan every new test binary
  and showed up at 50-60 percent CPU during runs. Add your terminal under System Settings,
  Privacy and Security, Developer Tools, and keep the target directory out of Spotlight.

## Release profile and size

| Build | relay | relayd | relay-desktop |
|---|---|---|---|
| thin LTO, symbols (before) | 12.4 MB | 17.5 MB | 23.3 MB |
| fat LTO | 10.7 | 15.8 | 20.5 |
| fat LTO + stripped (now) | 8.7 | 12.6 | 17.2 |
| opt-level s except audio and crypto crates | 13.1 | 17.6 | 24.5 (larger, rejected) |

Fat LTO links slower (about 20 percent more wall time on a loaded machine). `cargo build
--profile release-debug` keeps line tables and symbols. `panic = "abort"` is rejected: the
media supervisor (`relay-engine/src/media/supervise.rs`) catches worker panics with
`catch_unwind`, and UniFFI also relies on unwinding. Largest crates in relay-desktop's code:
std, relay_ui, egui, h2, tokio, harfrust. The `image` features (tiff, gif, bmp) cost about
250 KB; removing them would change which avatars can be loaded, so they stay.

## Android

- The Rust library uses its own profile in `apps/relay-android/Cargo.toml`. Fat LTO with one
  codegen unit: 29.8 to 25.8 MB stripped, and the cargo ndk build was not slower (2m37 to
  3m06 wall, 11m14 to 9m27 CPU). `opt-level = "s"` gives 22.8 MB, with the audio crates at
  3 it is 22.9 MB; not applied because UI speed on a phone was not measured.
- The old 66 MB debug APK held only 49 MB of entries: Gradle patches the existing APK in
  place and leaves replaced entries as dead space. `just android-apk` now deletes the old
  APK first.
- Not done: R8 for release builds (needs keep rules for JNA and UniFFI and a device to
  test them; the release APK is not what the release workflow ships). The QR
  scanner is ZXing core (pure Java), so no ML Kit native code remains.

## Disk

`target/debug` had grown to 133 GB. About 80 GB were superseded test binaries: every
dependency change relinks each of the ~340 test binaries under a new hash and the old
ones stay (nine copies of a 108 MB `ptt_engine` in a day), plus 21 GB of macOS object files
kept for split debuginfo. `just clean-cache` removes the incremental cache, superseded
test binaries (per test source file, the newest is kept) with their dep-info and object
files, and `target/android*` directories older than two weeks. It refuses to run while
cargo is running. First run: `target/debug/deps` 108 GB to 29 GB.

## Reproduce

```
cargo install cargo-nextest --locked
CARGO_TARGET_DIR=target/perf-x cargo test --workspace --no-run --timings   # clean build
CARGO_TARGET_DIR=target/perf-x cargo nextest run --workspace --no-fail-fast # "Summary [..s]"
CARGO_TARGET_DIR=target/perf-r cargo build --release -p relay-daemon -p relay-cli -p relay-desktop
cd apps/relay-android && cargo ndk -t arm64-v8a -P 28 -o out build --release
```

Use a separate target directory, delete it afterwards, and check that no other build is
running before timing.
