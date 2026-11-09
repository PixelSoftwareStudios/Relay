# Relay review against all rust-skills rules

Relay has strong architectural boundaries, explicit protocol limits, testable platform abstractions, and deliberate real-time audio design. The most important changes are to encode the audio priority guard's thread affinity, handle protocol numeric boundaries explicitly, and make build revision metadata follow Git reference changes. This review applies all 265 rules in the repository's `rust-skills` catalog; it groups actionable findings and records a separate disposition for every rule below.

## Fix batch status

The findings and matrix below describe the original reviewed snapshot. The subsequent local fix batch changes these statuses:

| Finding | Implementation status |
| --- | --- |
| F01 | Thread-bound priority guard, redacting/manual Debug, and a passing compile-fail doctest |
| F02 | Fallible jitter/drift constructors, checked validation, safe helper arithmetic, sequence-exhaustion results used by the jitter receiver, nonzero replay assertion, and boundary regressions |
| F03 | Git/worktree input tracking extracted into a testable build helper; actual Cargo regression passes for commits, detached checkout, packed refs, dirty input, and source archives |
| F04 | Constant-size latest-intent mailbox, bounded acknowledgments, interruption of pending opens/sends, and source-scoped acknowledgments; all 11 socket-backed PTT tests pass |
| F05 | Input/output callback safety arguments added; Android arm64 Clippy passed at Rust 1.99.0 with NDK r28c |
| F06 | Allocation-free RAII measurement restoration, nested counts, and unwind regressions |
| F07 | Associated typed daemon errors, retained source chains, generic refresh errors and UI translation, with stop failures no longer universally called timeouts |
| F08 | Verified translation/hash/options/final-use hub copies removed; necessary shared ownership retained |
| F09 | Reported rustdoc warnings repaired, executable updater example and failure contracts added, public-doc enforcement started in relay-testkit, documentation-link CI added. The wider missing-doc/pedantic backlog remains staged work, not thousands of claimed bug fixes |
| F10 | Safe, argument-skipping spans added to link establishment/read tasks, media-event delivery, and update checks |
| F11 | Isolated Loom workspace imports production health-cell source; both Loom models passed. Pinned nightly Miri passed all five protocol-boundary and seven allocator tests; CI jobs retain these checks |
| F12 | Validated ChannelName parsing/conversion, numeric ID formatting, selected resource Debug implementations, declared Android MSRV and an Android/MSRV check job. Bounded collections retain fallible insertion semantics |

No commits, pushes, or remote CI runs have been performed. Native root-workspace checks and targeted tests are recorded in the final batch validation below; newly added CI jobs are configured, not observed passing. Performance/container suggestions marked Context remain conditional recommendations.

### Final batch validation

| Check | Result |
| --- | --- |
| Root and isolated-checker formatting, `git diff --check` | Passed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Passed |
| Workspace rustdoc, all features, no dependencies | Passed without warnings |
| Core, protocol, testkit, audio, and messaging test run | 378 passed, 2 ignored, zero failures |
| Latest protocol boundary regression rerun | 5 passed |
| Client refresh, UI daemon launch, UI startup card tests | 24 passed |
| Socket-backed client PTT tests, outside the sandbox with approval | 11 passed, including the two new cancellation/backlog regressions |
| Cargo/Git revision-invalidation fixture | 1 passed |
| Priority-guard compile-fail and updater example doctests | Both passed |
| Android MSRV/Clippy | Passed for aarch64-linux-android at Rust 1.99.0, NDK 28.2.13676358, API 28, with `-D warnings` |
| Loom | Passed: both production-source concurrency models; generated independent Cargo.lock and restored `--locked` in CI/docs |
| Miri | Passed: five protocol-boundary and seven allocator tests using nightly-2026-11-04; run from a scratch directory to avoid the macOS signing-runner conflict |
| Full workspace integration suite | Passed: 2,283 tests, 7 ignored, zero failures across 426 completed test/doc-test targets; unrestricted rerun includes network integration tests and the corrected parallel-language test |
| Added CI jobs | Matching local Loom/Miri/Android checks executed as above; no remote CI execution or publication performed |

The independent Loom checker pins its direct dependency to 0.7.2 and now has a generated lockfile. Its CI invocation uses `--locked`, as do the product workspace checks. Live Android validation also removed an unused private host accessor, scoped a platform-only priority import, and corrected cargo-ndk's API-level option to uppercase `-P`. The broader missing-documentation/pedantic backlog is explicitly not converted into a blanket pass by this batch.

The first full unrestricted workspace run exposed a pre-existing test race in `inbound_nobody`: the English-label case rendered German while the translation case changed the process-wide language. Rendering in that file now takes a shared test-only lock. Android and Miri CI commands explicitly select Rust 1.99.0 and nightly-2026-11-04 respectively, matching the toolchains validated locally.

## Scope and method

- Revision: `cf4b844d90204be3de537bf785ab84e70feffe65`, branch `main`, November 9, 2026. The pre-existing Beautiful Rust report and user-provided `.codex/` directory were untracked. Production sources were unchanged at the start of this review.
- Rule source: [.codex/skills/rust-skills/SKILL.md](../../.codex/skills/rust-skills/SKILL.md), version 1.5.1: 265 rule files, 26 categories. Individual rule IDs resolve to `.codex/skills/rust-skills/rules/<id>.md`.
- Repository-wide automated screening covered all **1,182 tracked first-party Rust files, 180,855 lines**, including root-workspace crates/apps, Android host Rust, UniFFI entry points, build scripts, benchmarks, fuzz targets, and the sound-asset generator. Manifest and workflow inspection covered feature, toolchain, lint, optimization, and validation rules.
- **163 tracked vendored Rust files** under Android's patched `third_party/winit` were inventoried separately. They are upstream dependency code, not first-party conformance findings; their internal implementation was not re-audited against these style rules. Kotlin/Gradle, shell, localization, and packaging files are context for Rust behavior, not subjects of Rust naming/API rules.
- Method: complete first-party file inventory and source-pattern scan; compiler/Clippy/rustdoc diagnostics across the root workspace; targeted reads to validate candidates; direct-source and isolated build probes. This is not a claim that every source line received manual review or that an absence of diagnostics proves a rule holds everywhere.
- Edition 2024; workspace MSRV 1.99; toolchain 1.99.1. Android's independent manifest omits `rust-version`; see its matrix entry. No production code, skill files, or existing reports were edited.

## Validation

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| Workspace Clippy, all targets/features, pedantic plus clone, lock, future-size, stack-array, and missing-doc diagnostics | Passed with warnings |
| `cargo doc --workspace --all-features --no-deps` | Passed with warnings, including broken intra-doc links |
| `cargo test -p relay-core -p relay-protocol -p relay-audio -p relay-messages` | Passed: 362 tests, one ignored, zero failures; includes doc-test targets |
| Protocol boundary probes against current source | Five reproduced panics: maximum extender reference, zero-word replay window, inverted jitter margins, horizon addition overflow, and negative drift limit |
| `PriorityGuard: Send + Sync` compile-only probe | Compiled; confirms missing thread-affinity restriction, without changing scheduler state |
| Build-revision probe | Reproduced stale metadata in an isolated `/tmp` Git/Cargo fixture using the current `embed_build_revision` function |
| Nested allocator measurement probe | Reproduced: outer scope reports 1 allocation although 3 allocations occurred |
| Full workspace network integration suite | Earlier same-revision run stopped at sandbox-denied local socket binding; unrestricted rerun was declined. Not rerun here |
| Android/Linux/Windows target execution, Miri, Loom, fuzz campaigns, performance profiles | Not run; source/workflow inspection only |

The Clippy command was:

```sh
cargo clippy --workspace --all-targets --all-features --message-format=json -- \
  -W clippy::pedantic -W clippy::redundant_clone -W clippy::clone_on_ref_ptr \
  -W clippy::await_holding_lock -W clippy::large_futures \
  -W clippy::large_stack_arrays -W missing_docs
```

Deduplicating diagnostics by lint, first-party source path, and primary line found 2,436 `missing_docs` locations, 671 `missing_errors_doc` locations, 1,002 `must_use_candidate` locations, and 323 `clone_on_ref_ptr` locations. These are diagnostics, not confirmed bugs. Generated protobuf output was excluded from those counts; repeated compilation of the same source location was counted once. Test helpers and benchmarks remain included. The large-future diagnostics inspected were in test support, not evidence of production stack exhaustion.

## Prioritized findings

Priority reflects the concrete consequence, rather than automatically inheriting a rule category's CRITICAL/HIGH label. Evidence labels mean **reproduced** (a probe or tool demonstrated it), **source-confirmed** (the implementation establishes the issue), or **opportunity** (an optional improvement without a measured product defect).

### F01 P1 Encode audio priority guard thread affinity

**Location:** `crates/relay-audio/src/priority/mod.rs:97`; restoration in platform priority modules. **Rules:** `api-newtype-safety`, `type-phantom-marker`, `mem-drop-order`. **Evidence:** reproduced compile-time property; source-confirmed lifecycle consequence.

`PriorityGuard` requires destruction on the promoted thread, but its fields allow automatic `Send + Sync`. Moving the guard to another thread or a migrating task restores that thread's priority and leaves the original promotion in place. Windows deliberately stores its handle as an integer to retain `Send`, while its restore operation also assumes the originating thread. No memory-unsafety claim is made here.

**Fix:** Add `PhantomData<Rc<()>>` to the guard, initialize it in each construction branch, and add a compile-fail test rejecting cross-thread movement. Keep the guard in each worker's synchronous thread-local scope.

```rust
pub struct PriorityGuard {
    undo: Option<Undo>,
    _thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}
```

### F02 P2 Validate numeric state at protocol boundaries

**Locations:** `crates/relay-protocol/src/seq/extend.rs:31`, `jitter/config.rs:61`, `jitter/buffer.rs:66`, `drift.rs:77`, `seq/replay.rs:31`. **Rules:** `num-overflow-explicit`, `num-saturating-clamp`, `num-nonzero`, `api-parse-dont-validate`, `type-newtype-validated`, `err-result-over-panic`, `anti-panic-expected`. **Evidence:** reproduced.

Public `commit(u64)` permits references for which `reference + half` overflows. Public jitter configuration permits overflow before widening, invalid margin ordering, and overflowing delay products. Drift configuration permits negative/NaN bounds and invalid rates/time constants; a negative `limit_ppm` panics on the second ordinary update. `ReplayWindow::<0>` constructs but panics when recording its first number. These are public library boundary defects; the shipped presets and a remote-input route to these states were not shown to be faulty.

**Fix:** Validate configuration once before construction, widen before arithmetic, use checked products, and define an explicit sequence-exhaustion result. Keep programmer-error convenience constructors only with documented panic contracts. Reject a zero replay-window size at compile time.

```rust
let wanted = u64::from(self.max_margin_frames)
    + u64::from(self.reorder_slack_frames);
const { assert!(WORDS > 0, "replay window requires at least one word") };
```

For the extender, compute the nearest candidate in `u128` and use `u64::try_from` to report exhaustion. Merely replacing every addition with saturation can change wrap-selection semantics. Add boundary properties alongside the existing normal-range simulations.

### F03 P2 Rebuild revision metadata when the Git reference changes

**Location:** `crates/relay-api/build.rs:58`. **Rules:** `proj-build-rs-minimal`, `test-mock-traits` (test external state through a controlled fixture). **Evidence:** reproduced in an isolated fixture using the actual build-revision function.

The build script reads the current commit and dirty state, then tells Cargo to watch only Git's `HEAD` file in addition to its proto inputs. On a checked-out branch, `HEAD` normally contains a symbolic reference and does not change when that branch advances. The probe built the fixture, committed its lockfile, then built again: Git HEAD advanced but the program still reported the earlier revision. This makes displayed build identity stale. Correction to the original review: `Revision::is_current` compares API and release versions, not the Git build text, so this is not evidence of a broken automatic daemon replacement decision.

**Fix:** Watch the resolved reference and packed-reference storage as well as `HEAD`, or supply build identity explicitly from the build/release system and declare the environment dependency. Treat dirty-state detection as a separate invalidation requirement: following the ref alone does not make arbitrary worktree edits rerun this script.

```rust
if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"])
    && let Some(path) = git(&["rev-parse", "--git-path", &reference])
{
    println!("cargo:rerun-if-changed={path}");
}
if let Some(path) = git(&["rev-parse", "--git-path", "packed-refs"]) {
    println!("cargo:rerun-if-changed={path}");
}
```

Retain the existing `HEAD` dependency for branch switches and detached HEADs. Verify commit, checkout, packed-ref, source-edit, and source-archive builds in a scratch repository.

### F04 P2 Bound or coalesce the push-to-talk command backlog

**Location:** `crates/relay-client/src/ptt/mod.rs:102`; sequential consumer in `ptt/worker.rs:69`, connection timeout at `:13`. **Rules:** `async-bounded-channel`, `async-mpsc-queue`. **Evidence:** source-confirmed unbounded buffering; no load benchmark.

The synchronous public methods enqueue every press/release/cancel into an unbounded queue. The consumer can await connection for up to three seconds, while producers continue enqueueing. Auto-repeat is discarded only after dequeue. This permits backlog growth and delays a release/cancel behind earlier work. Other unbounded channels, such as media events, need individual rate/overflow analysis; their existence alone is not a defect.

**Fix:** Preserve nonblocking producers through a bounded command queue plus a separate cancellation or latest-state path. Coalesce auto-repeat before enqueueing. Define overflow behavior so releases and cancels cannot be silently dropped. Replacing `send` with `try_send` and ignoring a full queue would be unsafe for this behavior. Test a stalled connection with a burst of presses followed by release and cancel.

### F05 P2 Document Android callback unsafe invariants

**Location:** `crates/relay-audio/src/device/aaudio.rs:162` and `:216`. **Rules:** `unsafe-safety-comment`, `unsafe-minimize-scope`, `lint-unsafe-doc`. **Evidence:** source-confirmed documentation gap; Android Clippy not executed.

The input raw-slice conversion has a descriptive comment but no `SAFETY:` justification, and the output conversion has none. These operations depend on the callback format, alignment, pointer validity, frame count, channel width, and exclusivity for the output slice. The workspace denies `clippy::undocumented_unsafe_blocks`; these Android-gated sites were outside the successful macOS Clippy build. This is not a demonstrated invalid pointer or runtime failure.

**Fix:** Add specific safety arguments at both calls, and run Android-target Clippy with the NDK environment. Validate conversions at Rust-controlled boundaries; rely on the AAudio callback contract only where that contract actually supplies the guarantee.

```rust
// SAFETY: the configured callback supplies a live, aligned PCM_I16
// buffer for this callback invocation; input is mono and its frame count
// describes the readable sample extent. The slice does not escape.
```

For the output site, also justify `frames * channels`, exclusive access, and the configured channel count. Do not copy the input justification unchanged.

### F06 P2 Make allocation measurement scopes nest and unwind safely

**Location:** `crates/relay-testkit/src/alloc.rs:65`. **Rules:** `test-fixture-raii`, `mem-drop-order`, `conc-thread-local`. **Evidence:** reproduced nested undercount; unwind issue confirmed by control flow.

`allocations_in` overwrites shared thread-local counters, sets `ARMED`, invokes the closure, then clears `ARMED`. Nested use resets the outer count and disables counting for the remainder of the outer scope. The probe allocated one box before, inside, and after a nested measurement; the outer count was **1**, rather than **3**. If the closure panics, the normal reset is skipped. This makes test evidence less reliable, even though current callers may not nest it.

**Fix:** Use an RAII scope that saves/restores the prior armed state and counter. Either accumulate nested counts into the parent or explicitly reject nesting before modifying state. Use destructor-free bookkeeping and an allocation-free guard. Test ordinary, nested, and panicking closures. A guard that only restores `ARMED` fixes unwinding but does not fix nested counter loss.

### F07 P2 Preserve refresh error causes as typed values

**Location:** `crates/relay-client/src/refresh.rs:72` and `:88`. **Rules:** `err-thiserror-lib`, `err-custom-type`, `err-source-chain`, `type-no-stringly`, `anti-stringly-typed`, `obs-error-chain`. **Evidence:** source-confirmed.

`Daemon` exposes string errors and `RefreshError` wraps those strings, losing source types and chains. All stop failures become `StopTimedOut`, regardless of cause. Consumers must parse text to distinguish transport, launch, or actual timeout errors.

**Fix:** Introduce a concrete daemon error or an associated `Error: std::error::Error + Send + Sync + 'static`, then retain it with `#[source]` in the refresh error. Distinguish timeout from other stop failures. Keep presentation strings at the UI/logging boundary, and preserve intentional protocol refusal-code compatibility.

### F08 P3 Remove verified redundant clones without rewriting ownership

**Locations:** `crates/relay-ui/src/ui/visibility.rs:46`, `apps/relay-desktop/src/main.rs:111` and `:129`, `benches/audio/neural/model.rs:31`. **Rules:** `anti-clone-excessive`, `own-borrow-over-clone`, `mem-take-replace`. **Evidence:** Clippy plus source inspection; no measured performance improvement.

The argument-bearing `tr!` invocation in `not_advertising` already returns `String`; its final `.to_owned()` copies it again on a UI rendering path. The desktop initialization has terminal-use clones of `options` and the mock branch's `hub`, and the benchmark loader clones an optional hash string from an owned manifest before its last use. Correction: `shared` is still needed later and its clone must remain. These are small opportunities, not evidence that the repository's 1,722 lexical `.clone()` occurrences are excessive. Arc snapshots, cross-thread ownership, and heterogeneous backend storage are often intentional here.

**Fix:** Return the translated string directly, move the final-use Arc bindings, and move `manifest.model.sha256` when no later use requires the manifest field. Retain the necessary earlier clones. Prefer `Arc::clone(&value)` where the explicit ref-count operation helps readers; that is a clarity change rather than allocation removal.

### F09 P3 Repair public documentation and executable examples

**Locations:** `crates/relay-engine/src/peers/handle.rs:310`, `crates/relay-ui/src/updates.rs:3`, `apps/relay-cli/src/hotkey/mod.rs:5`, `crates/relay-update/src/lib.rs:205`. **Rules:** `doc-all-public`, `doc-errors-section`, `err-doc-errors`, `doc-panics-section`, `doc-intra-links`, `doc-link-types`, `doc-examples-section`, `test-doctest-examples`, `lint-missing-docs`. **Evidence:** compiler/rustdoc diagnostics.

Rustdoc cannot resolve `RttCell::get`, three `Shell` links, and `start`. Other public docs link to private constants or contain ambiguous links. The diagnostic counts show broad missing public/failure contracts. The updater's introductory example is a `text` block, so it demonstrates intent without being compiled as a doctest.

**Fix:** Qualify links, for example ``[`super::rtt::RttCell::get`]`` and ``[`crate::shell::Shell::install_update`]``; remove or replace the stale `start` reference. Add actionable `# Errors`/`# Panics` sections and a small `no_run` updater example with imports and a hidden fallible main. Stage lint enforcement by crate rather than turning thousands of existing documentation diagnostics into an immediate build failure. Fix generated protobuf documentation in the `.proto` source.

### F10 P3 Add operation spans where concurrent logs lose context

**Location:** `crates/relay-engine/src/peers/link.rs:246`, `crates/relay-engine/src/daemon/glue.rs:95`. **Rules:** `obs-instrument-spans`, `obs-structured-fields`, `obs-no-sensitive-data`. **Evidence:** repository-wide scan found no `tracing::instrument`, `info_span!`, `debug_span!`, or `.instrument(...)` usage; source inspection confirmed structured events already exist.

Events carry useful fields, but task-level context is not attached through spans. Following a call, update, transfer, or peer-link operation requires correlating individual messages manually. This is an observability opportunity rather than a correctness defect.

**Fix:** Add narrow spans around operations, with explicit safe fields and skipped arguments. Preserve existing redacting `Debug` implementations. Do not instrument complete requests, envelopes, credentials, message bodies, or file paths by default. Confirm spans survive async suspension using `Instrument`, rather than holding an entered-span guard across `.await`.

### F11 P3 Add targeted verification for unsafe and concurrent primitives

**Location:** `.github/workflows/ci.yml`; custom primitives in `crates/relay-engine/src/media/gate.rs`, `crates/relay-audio/src/device/health.rs`, and `crates/relay-testkit/src/alloc.rs`. **Rules:** `unsafe-miri-ci`, `test-loom-concurrency`, `conc-atomic-ordering`. **Evidence:** workflow and dependency inspection; neither tool run locally.

CI runs cross-platform tests, Clippy, dependency checks, and a fuzz-target build, but contains no Miri or Loom job. Existing behavioral/property tests are useful and do not exhaust atomic interleavings. Platform FFI and actual audio hardware are not suitable for blanket Miri execution.

**Fix:** Start with small isolated models of the gate/health publication invariants and a Miri-compatible allocator/helper subset. Keep OS calls behind ordinary platform tests. Treat this as a verification gap, not proof that the present Acquire/Release/Relaxed choices are wrong. Preserve CI's distinction between compiling fuzz targets and actually fuzzing them.

### F12 P3 Make optional API and metadata polish deliberate

**Locations:** `crates/relay-core/src/channel/name.rs:18`, `crates/relay-core/src/id.rs:9`, `crates/relay-core/src/channel/membership.rs:26`, `apps/relay-android/Cargo.toml:5`. **Rules:** `conv-fromstr-parsing`, `conv-tryfrom-fallible`, `type-numeric-fmt`, `api-common-traits`, `api-impl-fromiterator`, `proj-msrv-declare`.

`ChannelName` has a fallible constructor and `as_str` but no standard parsing hook. Integer-backed ID wrappers offer `Display` but no numeric formatting traits. Some public resource types lack `Debug`. Android declares its edition but no MSRV. These are ergonomics/documentation opportunities.

**Fix:** Delegate `FromStr` to the validated constructor; add `TryFrom` only for useful input types. Add numeric formatting only where callers need numeric semantics. Declare and test the Android MSRV. Add redacting/manual `Debug` to public resource types where useful. Do **not** blindly add infallible `FromIterator`/`Extend` to bounded `Memberships`: its `insert` deliberately reports a limit, which those traits cannot propagate. A fallible collection constructor is the appropriate alternative.

## What to retain

- Strong ID newtypes, validated channel names, opaque verified update payloads, and explicit failure types already make many invalid states difficult to represent.
- The audio path uses bounded rings, fixed arrays/boxed buffers, callback allocation tests, and independent scheduling. Adding a general allocator, Rayon, or blocking channel send to this path would need specific evidence and design work.
- `watch`, `oneshot`, bounded peer outboxes, atomic snapshots, and explicit runtime ownership match their use cases. Existing `async_trait` usage often supports dyn-dispatched shells or generated Tonic service traits, so native async-trait syntax is not a mechanical replacement.
- Redacting secret types and generated-request `Debug` exclusions are worth preserving. Protocol refusal strings and wire fields are compatibility surfaces, not automatic candidates for cosmetic type/name changes.
- Release fat LTO, one codegen unit, and `panic = "unwind"` are deliberate. Do not copy the skill's generic `panic = "abort"` recommendation over the media supervisor's recovery policy. Native-CPU tuning and nightly SIMD are optional profiling decisions, not baseline conformance requirements for distributed binaries.

## Rule by rule disposition

Each row is a disposition of that rule, not a numerical compliance score. **Finding** refers to a concrete item above; **Observed** identifies supporting implementations without certifying every occurrence; **Context** records a justified alternative or conditional recommendation; **Gap** means validation or documentation is missing; **N/A** means the required feature/use case was not found. Performance suggestions require measurement and are not treated as mandates to add dependencies.


### 1. Ownership & Borrowing (CRITICAL)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `own-borrow-over-clone` | Finding | F08: verified terminal-use clones; shared snapshots intentionally own their data. |
| `own-slice-over-vec` | Context | Only typed local closures found: config `channels/mod.rs:84` uses `&Vec`; UI `mock.rs:197` uses `&String`. Broaden locally if useful; no public-function defect established. |
| `own-cow-conditional` | Context | `relay-ui/src/i18n/catalog.rs:119` receives a Cow and intentionally owns the formatted text; no forced Cow refactor. |
| `own-arc-shared` | Observed | `relay-ui/src/actions.rs` shares snapshots through Arc and synchronized mutable state; `relay-engine/src/media/system.rs` shares worker state. |
| `own-rc-single-thread` | Context | `relay-audio/src/device/cpal_backend.rs:184` uses Cell for per-callback state; no need to replace cross-thread Arc with Rc. |
| `own-refcell-interior` | Context | `relay-audio/src/device/cpal_backend.rs:184` uses Cell for per-callback state; no need to replace cross-thread Arc with Rc. |
| `own-mutex-interior` | Observed | `relay-ui/src/actions.rs` shares snapshots through Arc and synchronized mutable state; `relay-engine/src/media/system.rs` shares worker state. |
| `own-rwlock-readers` | Observed | `relay-ui/src/i18n/mod.rs` uses an RwLock around shared catalogs; read/write workload was not benchmarked. |
| `own-copy-small` | Observed | `relay-core/src/id.rs` derives Copy on small identifiers/counters; owned collections and snapshots use explicit Clone. |
| `own-clone-explicit` | Observed | `relay-core/src/id.rs` derives Copy on small identifiers/counters; owned collections and snapshots use explicit Clone. |
| `own-move-large` | Observed | `relay-engine/src/media/render_worker.rs` boxes receiver/output commands rather than repeatedly moving their large state inline. |
| `own-lifetime-elision` | Observed | `relay-protocol/src/rtp/header.rs` borrows packet data with a necessary lifetime; ordinary functions mostly use elision. |

### 2. Error Handling (CRITICAL)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `err-thiserror-lib` | Finding | F07: refresh trait stringifies causes despite otherwise widespread thiserror use. |
| `err-anyhow-app` | Observed | CLI/build entry points use application-oriented errors; updater I/O errors retain operation context. F07 identifies a specific lost chain. |
| `err-result-over-panic` | Finding | F02: protocol boundary values can panic after successful construction. |
| `err-context-chain` | Observed | CLI/build entry points use application-oriented errors; updater I/O errors retain operation context. F07 identifies a specific lost chain. |
| `err-no-unwrap-prod` | Context | Source scan included inline tests; inspected production expects guard fixed slices/invariants. F02 covers separate arithmetic/config panics. |
| `err-expect-bugs-only` | Context | Source scan included inline tests; inspected production expects guard fixed slices/invariants. F02 covers separate arithmetic/config panics. |
| `err-question-mark` | Observed | `relay-net/src/control/mod.rs` derives From conversions for I/O/session errors; receiver uses `?`. |
| `err-from-impl` | Observed | `relay-net/src/control/mod.rs` derives From conversions for I/O/session errors; receiver uses `?`. |
| `err-source-chain` | Finding | F07: refresh trait stringifies causes despite otherwise widespread thiserror use. |
| `err-lowercase-msg` | Observed | Inspected protocol/config/update error Display strings use composable lowercase messages; UI prose need not follow library error style. |
| `err-doc-errors` | Finding | F09: 671 distinct first-party primary source locations flagged by Clippy. |
| `err-custom-type` | Finding | F07: refresh trait stringifies causes despite otherwise widespread thiserror use. |

### 3. Memory Optimization (CRITICAL)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `mem-with-capacity` | Observed | `relay-core/src/queue.rs`, `relay-config/src/identity/secret.rs`, and render-worker startup preallocate known bounds. |
| `mem-smallvec` | Context | No measured allocation/layout pressure justifies new containers or narrower fields; audio already uses fixed storage. Preserve wire integer widths. |
| `mem-arrayvec` | Context | No measured allocation/layout pressure justifies new containers or narrower fields; audio already uses fixed storage. Preserve wire integer widths. |
| `mem-box-large-variant` | Observed | `relay-engine/src/media/render_worker.rs` boxes AddCall/Attach payloads; no production large-enum diagnostic requiring another change was identified. |
| `mem-boxed-slice` | Observed | `relay-protocol/src/assembly.rs:25` and `jitter/buffer.rs` store fixed-capacity buffers as boxed slices. |
| `mem-thinvec` | Context | No measured allocation/layout pressure justifies new containers or narrower fields; audio already uses fixed storage. Preserve wire integer widths. |
| `mem-clone-from` | Context | No specific repeated deep-copy destination with demonstrated benefit was established; avoid clone_from churn on immutable Arc snapshots. |
| `mem-reuse-collections` | Observed | `relay-audio/src/capture/callback.rs` reuses bounded buffers; F08 covers one redundant rendering allocation. |
| `mem-avoid-format` | Observed | Whole-source scan found no argument-free constant `format!` candidate; dynamic error/UI formatting remains appropriate. |
| `mem-write-over-format` | Context | `relay-core/src/id.rs` short IDs format individual bytes; a single write buffer is possible, with profiling needed before priority is raised. |
| `mem-arena-allocator` | Context | No measured allocation/layout pressure justifies new containers or narrower fields; audio already uses fixed storage. Preserve wire integer widths. |
| `mem-zero-copy` | Observed | `relay-protocol/src/rtp/header.rs` returns borrowed payload slices. Cryptographic decode/ownership boundaries legitimately allocate. |
| `mem-compact-string` | Context | No measured allocation/layout pressure justifies new containers or narrower fields; audio already uses fixed storage. Preserve wire integer widths. |
| `mem-smaller-integers` | Context | No measured allocation/layout pressure justifies new containers or narrower fields; audio already uses fixed storage. Preserve wire integer widths. |
| `mem-assert-type-size` | Gap | No size_of/type-size assertions found in first-party Rust; consider guards for high-volume packet/queue structures after measuring layout. |
| `mem-take-replace` | Observed | `relay-engine/src/media/system.rs:140` moves worker handles with mem::take; F08 identifies small remaining ownership opportunities. |
| `mem-drop-order` | Finding | F01 and F06: thread-bound teardown and measurement-scope restoration need explicit invariants. |

### 4. Unsafe Code (CRITICAL)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `unsafe-safety-comment` | Finding | F05: Android raw-slice blocks lack SAFETY arguments; root macOS checks do not cover those cfg branches. |
| `unsafe-minimize-scope` | Observed | Platform/allocator unsafe operations are localized; F05 requires stronger Android-site justification, not larger unsafe scopes. |
| `unsafe-miri-ci` | Gap | F11: no Miri workflow; isolate interpretable helpers rather than running platform FFI blindly. |
| `unsafe-maybeuninit` | Observed | No transmute/uninitialized/assume_init candidate found; `relay-platform/src/hotkey/global.rs:225` zeroes a Windows MSG POD inside its FFI boundary. |
| `unsafe-extern-block` | Observed | Platform declarations use unsafe extern; Android entry uses `#[unsafe(no_mangle)]` in `activity.rs:42`. Cross-target ABI execution remains unverified. |
| `unsafe-send-sync-manual` | N/A | No first-party manual unsafe Send/Sync impl found. Automatic traits can still be too permissive: F01. |
| `unsafe-no-mangle-unsafe` | Observed | Platform declarations use unsafe extern; Android entry uses `#[unsafe(no_mangle)]` in `activity.rs:42`. Cross-target ABI execution remains unverified. |

### 5. API Design (HIGH)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `api-builder-pattern` | Context | Configuration structs already group options (`MediaConfig`, StreamRequest, platform inputs); builders are conditional, not a required layer everywhere. |
| `api-builder-must-use` | Context | PriorityGuard is must_use; Clippy identified many additional candidates. Add attributes where silent discarding loses work, not every scalar getter. |
| `api-newtype-safety` | Finding | F01: encode thread affinity; `relay-core/src/id.rs` otherwise demonstrates strong ID types. |
| `api-typestate` | Observed | `relay-update/src/lib.rs` separates Available from an opaque Verified artifact; core runtime enums suit externally driven state transitions. |
| `api-sealed-trait` | Context | AudioBackend, SecretVault, Shell, and test seams intentionally permit implementations; no unwanted external-impl surface demonstrated. |
| `api-extension-trait` | Observed | `relay-io/src/sync.rs` implements locking extensions on std lock types, with deliberate poison-recovery policy. |
| `api-parse-dont-validate` | Finding | F02: configuration validation is incomplete; ChannelName and safe filename boundaries already follow the rule. |
| `api-impl-into` | Observed | `relay-io/src/sink.rs:32` accepts owned path conversion; fixed borrowed protocol buffers use slices directly. |
| `api-impl-asref` | Observed | `relay-io/src/sink.rs:32` accepts owned path conversion; fixed borrowed protocol buffers use slices directly. |
| `api-must-use` | Context | PriorityGuard is must_use; Clippy identified many additional candidates. Add attributes where silent discarding loses work, not every scalar getter. |
| `api-non-exhaustive` | Context | Updater Error/Available use non_exhaustive. Most workspace APIs evolve with one application; adding it everywhere would disrupt legitimate exhaustive matches. |
| `api-from-not-into` | Observed | Scan found From implementations and no direct first-party Into impl requiring reversal; compiler checks instantiated conversions. |
| `api-default-impl` | Observed | Memberships, Shared, and channel selection have explicit sensible defaults; I/O constructors should remain fallible rather than inventing Default. |
| `api-common-traits` | Finding | F12: inspect missing Debug on public resource types; do not derive secret-revealing or misleading Clone implementations. |
| `api-serde-optional` | Context | Serde is integral to config/application wire data; no standalone consumer requirement established to justify feature-gating every internal crate. |
| `api-impl-fromiterator` | Context | F12: bounded Memberships can fail on capacity, so infallible FromIterator/Extend is not an automatic fit. |
| `api-operator-overload` | N/A | No custom arithmetic operator implementation needing semantic review found; state changes use named methods. |

### 6. Async/Await (HIGH)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `async-tokio-runtime` | Observed | Desktop owns its runtime in `apps/relay-desktop/src/main.rs`; portal owns a current-thread runtime; UI retains a Handle rather than constructing one per action. |
| `async-no-lock-await` | Observed | Expanded Clippy emitted no await_holding_lock finding. Source review checked worker and snapshot lock scopes; this is not a cross-target proof. |
| `async-spawn-blocking` | Observed | `relay-engine/src/service/rpc/messages.rs:397` moves export filesystem work into spawn_blocking; replacing its inner std::fs with tokio::fs is unnecessary. |
| `async-tokio-fs` | Observed | `relay-engine/src/service/rpc/messages.rs:397` moves export filesystem work into spawn_blocking; replacing its inner std::fs with tokio::fs is unnecessary. |
| `async-cancellation-token` | Context | Oneshot shutdown, atomic stop flags, and abort handles already express lifecycle cancellation. Dropped handles must still be audited individually; no blanket new dependency needed. |
| `async-join-parallel` | Context | Peer handshake, identity and ordered writes depend on prior results; no concrete independent sequential wait with measured latency cost was established. |
| `async-try-join` | Context | Peer handshake, identity and ordered writes depend on prior results; no concrete independent sequential wait with measured latency cost was established. |
| `async-select-racing` | Observed | PTT and transfer link loops race work with ticks/close state; inspected branches have explicit stop behavior. |
| `async-bounded-channel` | Finding | F04: public PTT producer can outpace a sequential blocked consumer; define release-safe overflow handling. |
| `async-mpsc-queue` | Observed | `relay-engine/src/peers/link.rs` uses bounded outboxes with receiver tasks; F04 covers the unbounded PTT exception. |
| `async-broadcast-pubsub` | Context | No requirement for broadcast-every-event substitution established; reliable domain streams and watch-latest feeds have different semantics. |
| `async-watch-latest` | Observed | `relay-client/src/ptt/mod.rs:103` publishes latest PttState via watch rather than retaining obsolete states. |
| `async-oneshot-response` | Observed | `relay-engine/src/media/devices.rs` returns an OpenPlayback result through a oneshot sender. |
| `async-joinset-structured` | Context | No JoinSet use found; `peers/link.rs` retains abort handles in Established for fixed reader/writer tasks. Dynamic task lifetime deserves targeted testing rather than mechanical replacement. |
| `async-clone-before-await` | Observed | Worker/supervisor task setup clones shared handles before move; cloning every borrow across await is unnecessary when lifetimes and Send already hold. |
| `async-fn-in-trait` | Context | Tonic service traits and dyn-dispatched Shell require their boxing/object-safe contract. `relay-client/src/refresh.rs` uses native async trait methods where appropriate. |
| `async-async-fn-bounds` | Context | Two legacy FnOnce-to-Future bounds found in channel RPC/UI helpers; a possible ergonomic update, with Send and capture requirements to preserve. |
| `async-cancel-safety` | Observed | `peers/link.rs:246` discards the link after receive timeout; transfer close/timeout marks failure terminal. No demonstrated reused-partial-read bug. Public receivers could document cancellation obligations. |

### 7. Concurrency (HIGH)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `conc-rayon-par-iter` | Context | No measured CPU-parallel batch opportunity established; real-time audio threads must not gain unpredictable pool scheduling. |
| `conc-scoped-threads` | Context | Workers outlive constructor stack frames; scoped threads would force premature joining. No short borrowed parallel batch requiring them found. |
| `conc-atomic-ordering` | Gap | F11: inspected Relaxed counters and Acquire/Release publication are intentional; model testing was not performed. Do not weaken remaining SeqCst by style alone. |
| `conc-thread-local` | Observed | `relay-testkit/src/alloc.rs:18` uses destructor-free thread-local Cells instead of static mut; F06 addresses nested scope semantics. |

### 8. Compiler Optimization (HIGH)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `opt-inline-small` | Context | Audio/protocol loops are bounded and buffer-oriented. Inlining, cold splitting, branch layout and cache changes require profiles/assembly evidence; no speedup claimed. |
| `opt-inline-always-rare` | Observed | No first-party inline(always) attribute found; no forced-inlining removal needed. |
| `opt-inline-never-cold` | Context | Audio/protocol loops are bounded and buffer-oriented. Inlining, cold splitting, branch layout and cache changes require profiles/assembly evidence; no speedup claimed. |
| `opt-cold-unlikely` | Context | Audio/protocol loops are bounded and buffer-oriented. Inlining, cold splitting, branch layout and cache changes require profiles/assembly evidence; no speedup claimed. |
| `opt-likely-hint` | Context | Audio/protocol loops are bounded and buffer-oriented. Inlining, cold splitting, branch layout and cache changes require profiles/assembly evidence; no speedup claimed. |
| `opt-lto-release` | Observed | Root and Android release profiles use fat LTO and one codegen unit. Unwind remains intentional for supervised workers. |
| `opt-codegen-units` | Observed | Root and Android release profiles use fat LTO and one codegen unit. Unwind remains intentional for supervised workers. |
| `opt-pgo-profile` | Context | No PGO pipeline found; release-training workloads and measured benefit are needed before adding one. |
| `opt-target-cpu` | Context | Portable release binaries should retain a baseline CPU target; native tuning is suitable for local controlled benchmarks only. |
| `opt-bounds-check` | Context | Audio/protocol loops are bounded and buffer-oriented. Inlining, cold splitting, branch layout and cache changes require profiles/assembly evidence; no speedup claimed. |
| `opt-simd-portable` | Context | No first-party nightly portable SIMD usage; measure DSP before introducing nightly or architecture-specific requirements. |
| `opt-cache-friendly` | Context | Audio/protocol loops are bounded and buffer-oriented. Inlining, cold splitting, branch layout and cache changes require profiles/assembly evidence; no speedup claimed. |

### 9. Numeric & Arithmetic Safety (HIGH)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `num-overflow-explicit` | Finding | F02: unchecked extender/config arithmetic and zero replay capacity. Existing core generation counters correctly use checked_add. |
| `num-cast-try-from` | Context | 135 distinct first-party truncation-lint locations require domain reasoning, including tests. F02 covers confirmed arithmetic defects; fixed-width packet indexing is not automatically unsafe. |
| `num-float-compare` | Observed | Simple lexical equality-against-float-literal matches were confined to test/bench/fuzz paths. This scan is not a complete floating-point equivalence analysis. |
| `num-saturating-clamp` | Finding | F02: unchecked extender/config arithmetic and zero replay capacity. Existing core generation counters correctly use checked_add. |
| `num-nonzero` | Finding | F02: unchecked extender/config arithmetic and zero replay capacity. Existing core generation counters correctly use checked_add. |

### 10. Type Safety (MEDIUM)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `type-newtype-ids` | Observed | `relay-core/src/id.rs` distinguishes CallId, OperationId, client/session IDs and generation counters. |
| `type-newtype-validated` | Finding | F02: protocol configs accept invalid values; `relay-core/src/channel/name.rs` already validates names at construction. |
| `type-enum-states` | Observed | Core call state and jitter State enums encode exclusive lifecycle modes; no blanket typestate rewrite warranted. |
| `type-option-nullable` | Observed | Optional session/output/identity fields use Option throughout inspected code; fixed protocol sentinel values remain compatibility decisions. |
| `type-result-fallible` | Finding | F02 and F07: tighten specific configuration and typed-error boundaries; ordinary protocol I/O already returns Result. |
| `type-phantom-marker` | Finding | F01: a zero-sized marker can encode the priority guard auto-trait restriction. |
| `type-never-diverge` | N/A | No incorrectly returning diverging Rust function was established; no reason to introduce nightly never-type generic arguments. |
| `type-generic-bounds` | Observed | Protocol buffer impl blocks carry operation-specific bounds; no generic-bound regression found by compilation/review. |
| `type-no-stringly` | Finding | F07: public refresh errors flatten causes. Stable wire/refusal strings are intentional and should preserve their bytes. |
| `type-repr-transparent` | Context | ID wrappers cross boundaries as explicit byte/protobuf mappings rather than being passed directly with a C ABI. Transparent repr is not required for every newtype. |
| `type-deref-coercion` | Observed | No first-party inheritance-style Deref implementation found; validated strings use explicit as_str/expose accessors. |
| `type-display-vs-debug` | Observed | Core IDs have Display; Secret32/SecretBytes redact Debug. F09 addresses public documentation, not a request to dump internal structures. |
| `type-numeric-fmt` | Context | F12: integer-backed identifiers lack alternate numeric format traits; useful only if numeric formatting is part of their intended API. |

### 11. Trait & Generics Design (MEDIUM)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `trait-associated-type-vs-generic` | Observed | `relay-audio/src/priority/policy.rs:57` associates one Undo type with each OS implementation. |
| `trait-blanket-impl` | Observed | `relay-io/src/sync.rs` generically extends Mutex/RwLock over T; the impl is local and intentional. |
| `trait-coherence-newtype` | Observed | Local ID/secret wrappers implement their own formatting/equality; compiled root workspace found no coherence violation. |
| `trait-default-methods` | Context | Backend and platform contracts need their required operations; no redundant implementation burden demanding a default was established. |
| `trait-dyn-vs-generic` | Observed | AudioBackend/Shell support heterogeneous platform implementations; protocol stream processing remains generic. The successful root build validates instantiated dyn uses. |
| `trait-object-safety` | Observed | AudioBackend/Shell support heterogeneous platform implementations; protocol stream processing remains generic. The successful root build validates instantiated dyn uses. |

### 12. Conversions (MEDIUM)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `conv-tryfrom-fallible` | Context | F12: ChannelName could expose standard parsing hooks; deliberate multi-output parsers need not all become TryFrom impls. |
| `conv-fromstr-parsing` | Context | F12: ChannelName could expose standard parsing hooks; deliberate multi-output parsers need not all become TryFrom impls. |
| `conv-asmut-mutable` | Context | Writable fixed buffers already take `&mut [T]`; generic AsMut is optional and would not improve their basic borrow contract. |

### 13. Const & Compile-Time (MEDIUM)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `const-block` | Finding | F02: reject ReplayWindow<0> at compile time; const thread-local initializers already exist. |
| `const-fn` | Observed | `relay-core/src/id.rs` and time.rs expose const accessors/constructors where their contracts permit it. |
| `const-generics` | Observed | ReplayWindow<const WORDS: usize> models compile-time capacity; F02 supplies its missing nonzero check. |
| `const-vs-static` | Observed | Protocol limits use const; allocator total counter and i18n shared state use static for single addressed instances. |

### 14. Serde (MEDIUM)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `serde-rename-all` | Observed | Configuration/model serde attributes select external conventions; no wire-renaming change proposed. |
| `serde-default-compat` | Observed | `relay-config/src/keychain/bundle.rs` defaults optional identity/channels while validating its version explicitly. |
| `serde-skip-empty` | Context | CLI JSON and persisted formats can require explicit fields; no payload-size profile justifies stripping empty fields across existing contracts. |
| `serde-flatten` | Context | No need to flatten intentionally structured preference/wire schemas; introducing it can weaken unknown-field checks. |
| `serde-enum-representation` | Observed | Configuration enums use explicit serde representation decisions; protobuf transport has its separate generated contract. |
| `serde-deny-unknown-fields` | Observed | Strict preference structs use deny_unknown_fields and portable preference import checks keys; GitHub release JSON deliberately tolerates additional API fields. |
| `serde-custom-with` | Observed | `relay-config/src/keychain/bundle.rs:42` provides custom SecretBytes deserialization and redacting serialization ownership. |
| `serde-try-from-validate` | Context | Bundle::decode validates its version after private deserialization; absence of the exact try_from attribute is not a validation bug. |

### 15. Pattern Matching (MEDIUM)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `pat-let-else` | Observed | PTT worker and jitter buffer use let-else and matches for control-flow extraction and boolean pattern checks. |
| `pat-matches-macro` | Observed | PTT worker and jitter buffer use let-else and matches for control-flow extraction and boolean pattern checks. |
| `pat-if-let-chains` | Observed | `relay-net/src/control/receiver.rs:130` combines a retention-bound check with a let binding; no mandatory nested-if rewrite. |
| `pat-exhaustive-enum` | Observed | Core/worker command matches enumerate actions; cross-platform updater apply has a deliberate unsupported-platform fallback. No hidden new variant bug established. |
| `pat-at-bindings` | Context | No concrete duplicated destructure requiring @ was identified; use only where it clarifies a bound-and-tested value. |

### 16. Macros (MEDIUM)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `macro-prefer-functions` | Observed | ID/extender macros generate type/impl families; tr! enforces literal message keys. These uses are not ordinary functions in disguise. |
| `macro-rules-hygiene` | Observed | `relay-ui/src/i18n/mod.rs:66` exports tr! and references its helpers via `$crate`, allowing external call sites. |
| `macro-fragment-specifiers` | Observed | ID/extender/tr! macros use ident, ty, literal, meta and expr captures rather than indiscriminate token trees. |
| `macro-export-crate-path` | Observed | `relay-ui/src/i18n/mod.rs:66` exports tr! and references its helpers via `$crate`, allowing external call sites. |
| `macro-private-helpers` | Context | tr! targets intentionally public i18n functions; no generated helper namespace pollution requiring __private found. |
| `macro-proc-two-crate` | N/A | No first-party proc-macro crate found; dependencies and generated prost/UniFFI code are not custom macro implementations to rewrite. |
| `macro-proc-syn-quote` | N/A | No first-party proc-macro crate found; dependencies and generated prost/UniFFI code are not custom macro implementations to rewrite. |
| `macro-proc-error-spans` | N/A | No first-party proc-macro crate found; dependencies and generated prost/UniFFI code are not custom macro implementations to rewrite. |

### 17. Closures (MEDIUM)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `closure-fn-trait-bounds` | Context | Database::with and allocator scope accept FnOnce; ChannelStorage::update could narrow FnMut to FnOnce, but its lock/reentrancy contract matters more. |
| `closure-impl-fn-return` | Observed | Updater Error::io returns impl FnOnce, avoiding a boxed short-lived mapping closure. |
| `closure-move-capture` | Observed | Peer-link/desktop task setup explicitly moves shared handles into escaping closures; no whole-object clone forced by capture was demonstrated. |
| `closure-static-vs-dyn` | Observed | EventSink/BackendFactory store erased callbacks intentionally; local transformations use static closure dispatch. |
| `closure-disjoint-capture` | Observed | Peer-link/desktop task setup explicitly moves shared handles into escaping closures; no whole-object clone forced by capture was demonstrated. |

### 18. Collections (MEDIUM)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `coll-binaryheap` | N/A | No repeated sort-for-extract-max queue requiring a heap was established; sequence-number jitter slots are not a priority queue. |
| `coll-map-choice` | Observed | Memberships uses BTreeMap for deterministic order; duplicate-message cache uses HashMap with VecDeque eviction order. |
| `coll-seq-choice` | Observed | BoundedQueue uses VecDeque; dense buffers use Vec/arrays/boxed slices; no LinkedList use found. |
| `coll-set-membership` | Context | Sets are used for notification/trust membership. Tiny fixed arrays and bounded short lists can legitimately use linear contains without a hasher. |

### 19. Naming Conventions (MEDIUM)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `name-types-camel` | Observed | Root compiler/Clippy checks and inspected public types follow Rust naming; generated protobuf naming is upstream-defined. |
| `name-variants-camel` | Observed | Root compiler/Clippy checks and inspected public types follow Rust naming; generated protobuf naming is upstream-defined. |
| `name-funcs-snake` | Observed | Root compiler/Clippy checks and inspected public types follow Rust naming; generated protobuf naming is upstream-defined. |
| `name-consts-screaming` | Observed | Root compiler/Clippy checks and inspected public types follow Rust naming; generated protobuf naming is upstream-defined. |
| `name-lifetime-short` | Observed | Inspected stream/packet APIs use short meaningful lifetime/type parameters; no public naming defect established. |
| `name-type-param-single` | Observed | Inspected stream/packet APIs use short meaningful lifetime/type parameters; no public naming defect established. |
| `name-as-free` | Observed | ChannelName::as_str and PublicKey::as_bytes borrow existing data; no allocating as_ getter was established. |
| `name-to-expensive` | Observed | ID to_bytes creates a fixed-size representation; to_owned/to_string mark owned output. This convention does not imply every to_ method must allocate. |
| `name-into-ownership` | Observed | Scanned explicit into_* conversions consume owned values or use standard trait/container semantics; no contrary API found. |
| `name-no-get-prefix` | Observed | No zero-argument public get_ getter matched. gRPC get_* methods perform requests and preserve schema names. |
| `name-is-has-bool` | Observed | Examples include has_frame, is_empty, is_promoted, and can_self_update; domain queries may use established verbs. |
| `name-iter-convention` | Observed | Memberships::iter borrows entries; owned/mutable standard containers use their normal iterator conventions. |
| `name-iter-method` | Observed | Memberships::iter borrows entries; owned/mutable standard containers use their normal iterator conventions. |
| `name-iter-type-match` | N/A | No named first-party iterator wrapper type requiring method/type naming alignment found in the reviewed surfaces; impl Iterator returns are intentional. |
| `name-acronym-word` | Observed | Root compiler/Clippy checks and inspected public types follow Rust naming; generated protobuf naming is upstream-defined. |
| `name-crate-no-rs` | Observed | First-party package names use relay-* or explicit tool names, without -rs/-rust suffixes. |

### 20. Testing (MEDIUM)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `test-cfg-test-module` | Observed | Inline tests are cfg-gated (for example protocol channel/device helpers); integration tests deliberately import public APIs instead of super. |
| `test-use-super` | Observed | Inline tests are cfg-gated (for example protocol channel/device helpers); integration tests deliberately import public APIs instead of super. |
| `test-integration-dir` | Observed | Workspace crates keep extensive behavior-named integration tests under tests/, including updater key rejection and protocol properties. |
| `test-descriptive-names` | Observed | Workspace crates keep extensive behavior-named integration tests under tests/, including updater key rejection and protocol properties. |
| `test-arrange-act-assert` | Observed | Inspected protocol/update tests arrange fixtures, exercise behavior, and assert results; literal AAA comments are not necessary. |
| `test-proptest-properties` | Observed | `relay-protocol/tests/jitter_props.rs` and core property tests exercise invariants; F02 identifies uncovered constructor edges. |
| `test-mockall-mocking` | Context | Handwritten fake backends/traits already isolate external behavior. A mockall dependency is not required to satisfy the testing objective. |
| `test-mock-traits` | Observed | AudioBackend, Clock, Daemon, SecretVault and Shell provide test seams; F03 uses a controlled Git fixture. |
| `test-fixture-raii` | Finding | F06: allocation measurement lacks an unwind-safe restoration guard; tempfile ownership already handles filesystem fixtures. |
| `test-tokio-async` | Observed | Async integration tests use Tokio test runtimes; network suites remain blocked by previously declined unrestricted execution. |
| `test-should-panic` | Context | Use should_panic for intended programmer-error contracts; F02 should become validation tests rather than merely blessing accidental panics. |
| `test-criterion-bench` | Observed | `benches/audio/stages.rs` and `benches/protocol/jitter.rs` use Criterion and std::hint::black_box. |
| `test-doctest-examples` | Finding | F09: updater introductory text example is not compiled; targeted crate doc-tests do not exercise it. |
| `test-loom-concurrency` | Gap | F11: no first-party Loom model or CI invocation found; current concurrent stress tests are not schedule enumeration. |
| `test-snapshot-testing` | Context | UI screenshot/catalog assertions and explicit output tests already exist; insta is optional where text snapshots improve reviewability. |

### 21. Documentation (MEDIUM)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `doc-all-public` | Finding | F09: tool-identified missing public/failure docs and non-executable introductory example. |
| `doc-module-inner` | Context | Many crate/module roots have //! docs; leaf modules without public module-level contracts need proportionate documentation, not boilerplate. |
| `doc-examples-section` | Finding | F09: tool-identified missing public/failure docs and non-executable introductory example. |
| `doc-errors-section` | Finding | F09: tool-identified missing public/failure docs and non-executable introductory example. |
| `doc-panics-section` | Finding | F09: tool-identified missing public/failure docs and non-executable introductory example. |
| `doc-safety-section` | Observed | Inspected public APIs mostly wrap unsafe platform operations safely; allocator unsafe impl methods inherit GlobalAlloc contracts. F05 covers call-site justification. |
| `doc-question-mark` | Context | New examples should use a hidden fallible main and ?; F09 proposes this for updater docs. No broad source rewrite needed. |
| `doc-hidden-setup` | Context | New examples should use a hidden fallible main and ?; F09 proposes this for updater docs. No broad source rewrite needed. |
| `doc-intra-links` | Finding | F09: rustdoc reproduced unresolved and ambiguous links. |
| `doc-link-types` | Finding | F09: rustdoc reproduced unresolved and ambiguous links. |
| `doc-cargo-metadata` | Context | Workspace inherits version/edition/license/repository. Description/readme/category metadata matters if individual internal crates are to be published; publication intent was not assumed. |
| `doc-crate-readme` | Context | Repository README and crate-specific //! contracts serve different audiences; no duplicated per-crate README/rustdoc content requiring include_str unification established. |

### 22. Observability (MEDIUM)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `obs-tracing-over-log` | Observed | Library components emit tracing events; subscriber setup was found in application entry/logging code, not shared library initialization. |
| `obs-library-facade` | Observed | Library components emit tracing events; subscriber setup was found in application entry/logging code, not shared library initialization. |
| `obs-structured-fields` | Observed | Peer link logs attach link/error fields; F10 proposes inherited task context rather than replacing all messages. |
| `obs-instrument-spans` | Finding | F10: source scan found no supported span/instrumentation patterns; useful operation context is an opportunity. |
| `obs-levels-filter` | Observed | Application logging initializes EnvFilter-style configuration and uses distinct levels; no workload log-volume audit was run. |
| `obs-error-chain` | Finding | F07: refresh strings prevent retaining typed source chains; format changes alone cannot recover lost causes. |
| `obs-no-sensitive-data` | Observed | Secret32/SecretBytes and generated sensitive requests have redacting Debug; diagnostics redaction exists. No live trace audit, so this is not a no-leak guarantee. |

### 23. Performance Patterns (MEDIUM)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `perf-iter-over-index` | Context | DSP and wire decoding sometimes need fixed indexing; no unsafe-bounds removal justified. Iterator rewrites require equivalent semantics and evidence. |
| `perf-iter-lazy` | Context | Some collects establish owned snapshots or release locks before later work (`Android host detach`); no blanket fusion of those boundaries. |
| `perf-collect-once` | Context | Some collects establish owned snapshots or release locks before later work (`Android host detach`); no blanket fusion of those boundaries. |
| `perf-entry-api` | Context | Memberships checks existence before enforcing a bound; Entry could avoid repeated lookup, but the small bounded map is not a demonstrated bottleneck. |
| `perf-drain-reuse` | Context | Android detach drains task handles into an owned Vec to release the lock before aborting them; that intermediate allocation has a synchronization purpose. |
| `perf-extend-batch` | Observed | Identity encoding extends preallocated buffers by slices; batching is already used where output size is known. |
| `perf-chain-avoid` | Context | No profiled chain bottleneck identified; removing an adapter alone does not establish a speedup. |
| `perf-collect-into` | Context | Stable toolchain: prefer clear + extend if reuse is needed; do not introduce the skill rule's nightly-only collect_into example. |
| `perf-black-box-bench` | Observed | `benches/audio/stages.rs` and `benches/protocol/jitter.rs` use Criterion and std::hint::black_box. |
| `perf-release-profile` | Observed | Root and Android release profiles use fat LTO and one codegen unit. Unwind remains intentional for supervised workers. |
| `perf-profile-first` | Observed | Repository contains real-time reviews, allocation tests and audio benchmarks. New performance proposals here are explicitly conditional, not measured gains. |
| `perf-ahash` | Context | Peer/message inputs are untrusted; retain resistant default hashing unless trusted-key constraints and measured benefit justify a faster hasher. |
| `perf-io-buffering` | Context | HTTP/download/file transfer already processes chunks; no byte-at-a-time syscall hotspot established. Additional buffering must preserve memory caps. |

### 24. Project Structure (LOW)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `proj-lib-main-split` | Observed | CLI/daemon/desktop packages expose libraries with thin orchestration entry points; some desktop setup remains appropriately app-specific. |
| `proj-mod-by-feature` | Observed | Crates organize service, peers, messaging, media, channel, transfer and UI features with consistent multifile modules. |
| `proj-flat-small` | Context | This 1,182-file application needs hierarchy; flattening to satisfy a small-project recommendation would hurt navigation. |
| `proj-mod-rs-dir` | Observed | Crates organize service, peers, messaging, media, channel, transfer and UI features with consistent multifile modules. |
| `proj-pub-crate-internal` | Observed | Engine/platform internals use pub(crate)/pub(super); root reexports are deliberate cross-crate interfaces. |
| `proj-pub-super-parent` | Observed | Engine/platform internals use pub(crate)/pub(super); root reexports are deliberate cross-crate interfaces. |
| `proj-pub-use-reexport` | Observed | Protocol, I/O, audio and update crate roots reexport principal types without requiring consumers to follow internal file layout. |
| `proj-prelude-module` | Context | No requirement for a new prelude established; explicit imports suit these application libraries and avoid an additional API promise. |
| `proj-bin-dir` | Context | Separate CLI/daemon/desktop packages have different dependencies; combining them into src/bin merely for convention is inappropriate. |
| `proj-workspace-large` | Observed | Root Cargo.toml centralizes members/dependencies. Separate Android/fuzz workspaces have target/toolchain reasons; their duplicate manifests need intentional maintenance. |
| `proj-workspace-deps` | Observed | Root Cargo.toml centralizes members/dependencies. Separate Android/fuzz workspaces have target/toolchain reasons; their duplicate manifests need intentional maintenance. |
| `proj-feature-additive` | Observed | rtkit/neural/test-util features add behavior/APIs; no first-party feature that removes a public item was established. Only root-host all-feature compilation was executed. |
| `proj-msrv-declare` | Context | Root MSRV 1.99 and pinned 1.99.1 CI exist; F12: standalone Android manifest has no rust-version. Exact 1.99.0 was not tested. |
| `proj-build-rs-minimal` | Finding | F03: build script omits branch-ref/worktree dependencies for the data it embeds. |

### 25. Clippy & Linting (LOW)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `lint-deny-correctness` | Observed | CI runs Clippy with -D warnings; default Clippy groups are active. No need to repeat default groups as crate attributes. |
| `lint-warn-suspicious` | Observed | CI runs Clippy with -D warnings; default Clippy groups are active. No need to repeat default groups as crate attributes. |
| `lint-warn-style` | Observed | CI runs Clippy with -D warnings; default Clippy groups are active. No need to repeat default groups as crate attributes. |
| `lint-warn-complexity` | Observed | CI runs Clippy with -D warnings; default Clippy groups are active. No need to repeat default groups as crate attributes. |
| `lint-warn-perf` | Observed | CI runs Clippy with -D warnings; default Clippy groups are active. No need to repeat default groups as crate attributes. |
| `lint-pedantic-selective` | Context | Extra review lints reveal a backlog; adopt useful individual checks after triage, not entire groups as new error gates. |
| `lint-missing-docs` | Gap | F09: workspace has no enforced missing_docs policy; staged crate-by-crate coverage would avoid a mass unrelated change. |
| `lint-unsafe-doc` | Finding | F05: Android raw-slice blocks lack SAFETY arguments; root macOS checks do not cover those cfg branches. |
| `lint-cargo-metadata` | Context | Workspace inherits version/edition/license/repository. Description/readme/category metadata matters if individual internal crates are to be published; publication intent was not assumed. |
| `lint-rustfmt-check` | Observed | CI runs cargo fmt --all -- --check; the same command passed locally. |
| `lint-workspace-lints` | Observed | Root workspace denies unsafe_op_in_unsafe_fn and undocumented_unsafe_blocks; root members inherit lints. Separate Android host crate has no inherited lint table. |
| `lint-cfg-check` | Observed | Cargo/compiler cfg checking is active in the successful root build; no custom cfg requiring check-cfg declaration was identified in inspected code. |
| `lint-clippy-nursery-selected` | Context | Extra review lints reveal a backlog; adopt useful individual checks after triage, not entire groups as new error gates. |

### 26. Anti-patterns (REFERENCE)

| Rule | Disposition | Evidence and judgment |
| --- | --- | --- |
| `anti-unwrap-abuse` | Context | Lexical unwrap counts include extensive inline tests. Production fixed-slice/invariant expects were checked in context; F02 records actual panic findings. |
| `anti-expect-lazy` | Context | Lexical unwrap counts include extensive inline tests. Production fixed-slice/invariant expects were checked in context; F02 records actual panic findings. |
| `anti-clone-excessive` | Finding | F08: remove the proven redundant copies; ordinary Arc snapshot clones retain ownership intentionally. |
| `anti-lock-across-await` | Observed | Expanded Clippy emitted no await_holding_lock finding. Source review checked worker and snapshot lock scopes; this is not a cross-target proof. |
| `anti-string-for-str` | Context | Local closures have one typed &String and one &Vec candidate; no first-party public function parameter requiring those owned containers was found. |
| `anti-vec-for-slice` | Context | Local closures have one typed &String and one &Vec candidate; no first-party public function parameter requiring those owned containers was found. |
| `anti-index-over-iter` | Context | DSP and wire decoding sometimes need fixed indexing; no unsafe-bounds removal justified. Iterator rewrites require equivalent semantics and evidence. |
| `anti-panic-expected` | Finding | F02: protocol configuration boundaries should validate or document intentional convenience panics. |
| `anti-empty-catch` | Context | Best-effort drop cleanup and sends to stopped receivers can ignore failures intentionally. Storage commit/abort outcomes require individual handling; no mass logging rewrite. |
| `anti-over-abstraction` | Observed | Backend/Shell trait objects have multiple platform/fake implementations; local deterministic protocol generics serve real stream/payload variation. |
| `anti-premature-optimize` | Observed | Repository contains real-time reviews, allocation tests and audio benchmarks. New performance proposals here are explicitly conditional, not measured gains. |
| `anti-type-erasure` | Observed | Backend/Shell trait objects have multiple platform/fake implementations; local deterministic protocol generics serve real stream/payload variation. |
| `anti-format-hot-path` | Context | F08 removes a verified extra UI copy. Dynamic short-ID/diagnostic format calls are opportunities only where profiles show meaningful cost. |
| `anti-collect-intermediate` | Context | Some collects establish owned snapshots or release locks before later work (`Android host detach`); no blanket fusion of those boundaries. |
| `anti-stringly-typed` | Finding | F07: public refresh errors flatten causes. Stable wire/refusal strings are intentional and should preserve their bytes. |

## Coverage accounting

All **265** installed rule IDs appear exactly once across **26** category tables. Dispositions: **85 Context**, **35 Finding**, **5 Gap**, **8 N/A**, **132 Observed**. These counts overlap conceptually through rule aliases, so findings are grouped instead of counted repeatedly.

These matrix dispositions describe the original review snapshot; current fixes and executed checks are recorded at the top. Validation beyond this batch includes real-device execution, native Linux/Windows runs, sustained fuzz campaigns, and workload profiling. Passing source checks and targeted models does not establish a speedup or certify all concurrency and memory-safety behavior.
