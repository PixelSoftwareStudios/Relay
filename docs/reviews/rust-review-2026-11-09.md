# Rust review: Relay workspace

Follow-up implementation and validation status is tracked in the [rust-skills fix batch](rust-skills-review-2026-11-09.md#fix-batch-status). Findings below describe the original snapshot.

Relay is a LAN voice and messaging application with reusable Rust libraries, desktop and terminal applications, and a separate Android workspace. The code has clear crate boundaries, typed errors in most subsystems, and substantial behavioral testing. The highest-priority correction is to encode the audio priority guard's thread affinity in its type. Formatting and workspace Clippy completed successfully; test results are recorded below. This is a review with proposed fixes, not an implementation change.

## Scope and validation

- Reviewed revision: `cf4b844d90204be3de537bf785ab84e70feffe65` on `main`, with a clean starting working tree, on November 9, 2026.
- Edition: Rust 2024. Declared MSRV: 1.99. Toolchain: 1.99.1.
- Applied Beautiful Rust's `rust-review` and companion `rust-best-practices` guidance. Library API rules apply to cross-crate interfaces; application code is assessed for simplicity and behavior. The workspace is application-oriented, so speculative API evolution changes are not treated as release blockers.
- Repository-wide source-pattern scan, followed by closer reading of protocol arithmetic and configuration, audio priority and worker lifecycles, updater verification and installation, I/O, storage locking, client refresh and push-to-talk, and platform hotkeys. This is a sampled review, not a line-by-line audit of all 1,133 first-party Rust files under `crates/` and `apps/`.
- Vendored Android `third_party/winit` code was excluded. Android, Windows, and Linux target-specific behavior was not executed on this macOS host. The separate Android and fuzz workspaces were not built.

| Check | Result |
| --- | --- |
| `cargo fmt --check` | Passed |
| `cargo clippy --workspace --all-targets --all-features -- -W clippy::pedantic` | Passed, with warnings |
| `cargo test --workspace` | Stopped at `relay-api --test channels`: seven failures from sandbox-denied socket binding (`PermissionDenied`, OS code 1); not evidence of seven product defects |
| Unrestricted workspace test rerun | Requested after the sandbox failure; permission was declined, so it was not run |
| `cargo test -p relay-core -p relay-protocol` | Passed: 139 tests; no failures or ignored tests; both doc-test targets also passed (zero examples) |
| Direct-source protocol probes compiled with `rustc --edition 2024` | Five boundary cases reproduced panics, detailed below |
| Compile-only `PriorityGuard: Send + Sync` assertion against workspace build metadata | Compiled successfully; no scheduling changes were made |

Clippy emitted 2,975 warning diagnostic occurrences, including repeated diagnostics across targets. The largest groups were 1,008 `must_use` suggestions and 671 missing `# Errors` sections. These counts are diagnostic occurrences, not distinct defects. Cast warnings were assessed in context rather than automatically classified as bugs. Logs were captured in `/tmp/relay-review-{fmt,clippy,test,test-pure}.log` for this session. The workspace test command stopped at its first failing integration target, so subsequent suites were not verified by that run.

## Must fix

### 1. Encode priority guard thread affinity (`crates/relay-audio/src/priority/mod.rs:97`)

**Rule:** Rust API Guidelines **C-VALIDATE**: enforce invariants through types where possible; Beautiful Rust, “Concurrency and async” and “Unsafe and FFI”: auto traits must preserve the abstraction's invariants.

**Problem:** `PriorityGuard` documents that it must be dropped on the promoted thread, but it is automatically `Send + Sync`. A compile-only assertion confirmed both traits on macOS. A caller can move the guard into another thread or a migrating async task; `Drop` then restores the receiving thread's priority and leaves the originating thread promoted. The Windows undo representation even explicitly preserves `Send` by storing its handle as an integer (`priority/windows.rs:34`), while restoration assumes the originating thread. This is a scheduling/lifecycle bug; this review does not claim memory unsafety or an observed production failure.

**Fix:** Make the guard thread-bound and initialize its marker in both construction branches. Existing worker-local use remains valid. Add a compile-fail check that rejects moving the guard to another thread.

```rust
use std::{marker::PhantomData, rc::Rc};

pub struct PriorityGuard {
    undo: Option<Undo>,
    _thread_bound: PhantomData<Rc<()>>,
}

// In each PriorityGuard initializer:
_thread_bound: PhantomData,
```

### 2. Guard extended-number arithmetic at the public boundary (`crates/relay-protocol/src/seq/extend.rs:31`)

**Rule:** Rust API Guidelines **C-VALIDATE**; Beautiful Rust review, “Correctness and soundness”: input-reachable arithmetic must not overflow.

**Problem:** Public `commit(u64)` accepts any reference, but `resolve` adds `half` to that reference and the candidate, and can add `SPAN` to the candidate. `SeqExtender::new(); commit(u64::MAX); resolve(0)` reproduced an overflow panic. Both extender types share the macro. Release overflow checks are explicitly enabled for `relay-protocol`, so that configuration also traps instead of silently wrapping. No packet-driven path to such a reference was established; the confirmed trigger is the public API, not normal stream longevity.

**Fix:** Either constrain and validate committed references, or define an explicit exhaustion policy using wider intermediate arithmetic. For example, retain the nearest-number calculation in `u128`, then report values that cannot fit in `u64`:

```rust
let reference = u128::from(reference);
let span = u128::from(Self::SPAN);
let candidate = (reference & !(span - 1)) | u128::from(value);
let half = span / 2;
let extended = if candidate > reference + half {
    candidate.saturating_sub(span)
} else if candidate + half <= reference {
    candidate + span
} else {
    candidate
};
u64::try_from(extended).map_err(|_| ExtendError::Exhausted)
```

This proposal introduces a typed `ExtendError` and a fallible resolver; update `extend` and callers consistently, or introduce a fallible sibling and document the existing convenience method's panic contract. Preserve behavior for ordinary wrapping and reordering inputs.

## Should fix

### 3. Validate protocol configuration before processing (`crates/relay-protocol/src/jitter/buffer.rs:66`)

**Rule:** Rust API Guidelines **C-VALIDATE**, **C-FAILURE**; Beautiful Rust, “Model the domain with types.”

**Problem:** Configuration structs expose unrestricted fields, while constructors check only some assumptions or none. Direct-source probes reproduced these failures:

- `MarginController::new` panics when `min_margin_frames > max_margin_frames` (`jitter/margin.rs:25`), including through `JitterBuffer::new`, whose assertions only check slots and frame length.
- `JitterConfig { max_margin_frames: u32::MAX, ..BALANCED }.horizon_frames()` overflows before conversion to `u64` (`jitter/config.rs:61`). `max_delay_samples` also multiplies unrestricted `u32` fields (`:67`, inspected but not separately executed).
- A `DriftEstimator` with `limit_ppm = -1.0` constructs successfully and panics on its second ordinary update (`drift.rs:77`, `:128`). Other floating-point parameters likewise lack finite/range validation.
- `ReplayWindow::<0>::new().accept(1)` indexes an empty bitmap (`seq/replay.rs:62`).

These are caller-controlled library edge cases. The shipped presets were not shown to be invalid, and this finding is not evidence of a remote crash in the application.

**Fix:** Validate configurations once in fallible constructors; check finite values, positive rates and time constants, nonnegative limits, margin ordering, and representable delay products. Keep public configuration helpers safe even before construction. Enforce the const-generic replay-window size at construction.

```rust
// JitterConfig::horizon_frames: widen before addition.
let wanted = u64::from(self.max_margin_frames)
    + u64::from(self.reorder_slack_frames);
wanted.min((self.slots as u64).saturating_sub(1))

// In a proposed DriftConfig::validate() -> Result<(), ConfigError>:
if !self.limit_ppm.is_finite() || self.limit_ppm < 0.0 {
    return Err(ConfigError::InvalidLimit);
}

// ReplayWindow::new: reject the invalid type parameter immediately.
const { assert!(WORDS > 0, "replay window requires at least one word") };
```

`InvalidLimit` and the fallible configuration API above are proposed additions. Validate all related fields, not only the example shown. Add boundary tests alongside the existing protocol simulations and property tests.

### 4. Move blocking joins into explicit shutdown (`crates/relay-engine/src/media/devices.rs:155`)

**Rule:** Rust API Guidelines **C-DTOR-BLOCK**: destructors do not block; provide explicit teardown.

**Problem:** `DeviceThread::drop` sends shutdown then joins a thread that can be inside backend enumeration, stream creation, or a device operation. Those calls have no bound enforced by this abstraction. `Inner::drop` similarly joins media workers (`media/system.rs:138`). Releasing the last `MediaSystem` handle can therefore block its calling thread despite the type's “all methods are non-blocking” description. This is confirmed by control flow; no deliberately wedged hardware backend was run.

**Fix:** Provide explicit shutdown that signals and joins workers on a controlled blocking thread, exposes failures, and defines how a stalled backend is handled. Keep `Drop` limited to stop signals and wakeups. Merely dropping a join handle detaches a thread, so design ownership and resource lifetime before adopting that behavior.

```rust
impl DeviceThread {
    pub fn shutdown(mut self) -> std::thread::Result<()> {
        let _ = self.commands.send(DeviceCmd::Shutdown);
        self.join.take().map_or(Ok(()), |join| join.join())
    }
}

impl Drop for DeviceThread {
    fn drop(&mut self) {
        let _ = self.commands.send(DeviceCmd::Shutdown);
    }
}
```

Apply the same explicit lifecycle at the `MediaSystem` boundary; the snippet is the device-thread portion, not a complete shutdown redesign.

### 5. Preserve typed failures through the refresh API (`crates/relay-client/src/refresh.rs:88`)

**Rule:** Rust API Guidelines **C-GOOD-ERR**: public error types implement `std::error::Error`; Beautiful Rust, “Errors and panics”: preserve wrapped causes with `source()`.

**Problem:** Public `Daemon` trait methods return `String`, and `RefreshError` embeds those strings. Implementations flatten errors before callers can inspect them. Consumers cannot reliably distinguish a transport failure from launch or storage failure, or inspect an error chain; they must parse human-readable text. `RefreshError::StopTimedOut(String)` also names every stop error as a timeout regardless of its cause. This matters at a cross-crate library boundary even though user-facing UI text can remain a string.

**Fix:** Give the trait an associated error type and preserve it in a generic refresh error, or define a concrete typed daemon error shared by its implementations. For the associated-type option:

```rust
pub trait Daemon {
    type Error: std::error::Error + Send + Sync + 'static;
    async fn probe(&mut self) -> Result<Option<Probe>, Self::Error>;
    async fn stop(&mut self) -> Result<(), Self::Error>;
    async fn start(&mut self) -> Result<(), Self::Error>;
}

#[derive(Debug, thiserror::Error)]
pub enum RefreshError<E: std::error::Error + 'static> {
    #[error("cannot inspect the running daemon: {0}")]
    Probe(#[source] E),
    #[error("cannot stop the running daemon: {0}")]
    Stop(#[source] E),
    #[error("cannot start the new daemon: {0}")]
    Start(#[source] E),
}
```

Update `refresh` to return `RefreshError<D::Error>` and map stop failures to `Stop`. If `Clone` and `Eq` are required by consumers, make those bounds conditional rather than reducing all errors to text.

## Consider

### 6. Document and contain lock recovery and callback reentrancy (`crates/relay-io/src/sync.rs:17`)

**Rule:** Beautiful Rust, “Errors and panics” and “Concurrency and async”: avoid continuing with poisoned state and avoid calling user closures under locks; Rust API Guidelines **C-FAILURE**.

**Problem:** Poison recovery is an explicit daemon availability policy, documented in `relay-io`; it should not be mechanically replaced with panics. However, its generic helpers recover arbitrary state without checking invariants. Separately, `ChannelStorage::update` deliberately executes the caller's closure while holding the shared database connection (`relay-config/src/channels/store.rs:79`, through `db/mod.rs:73`). A callback that re-enters the same database can deadlock. Its atomicity rationale is documented, so this is a trade-off to clarify rather than an observed application failure.

**Fix:** Document the non-reentrancy restriction and the state invariants required after poison recovery. Prefer a constrained mutation API if external callers do not need arbitrary callbacks. Do not move the closure outside the lock without replacing the lost atomicity, for example with revision-checked writes.

```rust
/// The callback runs under the database connection lock. It must not call
/// methods that access this database, including through cloned handles.
///
/// # Errors
/// Returns an error if reading, validating, or persisting the channel fails.
pub fn update(/* existing signature */) { /* existing implementation */ }
```

The signature in this documentation sketch is abbreviated; preserve the existing API when adding the contract.

### 7. Add failure contracts where users need them (`crates/relay-update/src/lib.rs:205`)

**Rule:** Rust API Guidelines **C-FAILURE**; Clippy **missing_errors_doc** and **missing_panics_doc**.

**Problem:** The code often explains normal behavior well, but many fallible public methods omit `# Errors`, and public convenience APIs with caller preconditions omit `# Panics`. Clippy reported 671 missing-error-section diagnostic occurrences. For example, updater `check` documents `None` but not request, release-validation, and asset-selection failures. `FrameAssembler::pop_frame` describes output capacity in prose but does not provide a `# Panics` section (`relay-protocol/src/assembly.rs:65`). These omissions make integration and recovery behavior harder to discover.

**Fix:** Document actionable failure categories, caller obligations, and side effects on failure, starting with exported entry points. Avoid boilerplate on every private helper.

```rust
/// # Errors
/// Returns an error if the request fails, the release document is invalid,
/// or a newer in-place release lacks the required installer or signature.
```

For `pop_frame`, document that a short output buffer panics when a frame is ready, or add a fallible variant. Existing programmer-error preconditions are not automatically correctness defects.

## What's already good

- Crate boundaries separate deterministic domain/protocol logic from I/O, platform integration, and UI. Explicit clocks and fake backends support behavioral tests without hardware.
- RTP decoding obtains a complete fixed-size header before indexing, and rejected protocol conditions have typed errors. This is a useful pattern to extend to configuration boundaries.
- The updater separates `Available` from an opaque `Verified` payload, checks signatures and trusted comments, rehashes before application, and tests rejection of signatures made with the test key. The active `tests/key.rs` provides meaningful behavior checks rather than only checking key constants.
- Workspace lints already deny undocumented unsafe blocks and unsafe operations inside unsafe functions. Unsafe platform calls inspected in this review carry concrete safety reasoning; the pedantic Clippy run completed without compilation errors.

The workspace already has `[workspace.lints]`; no replacement lint configuration is needed. Triage useful pedantic diagnostics in focused changes rather than adopting all suggestions or raising the entire warning backlog to errors at once.
