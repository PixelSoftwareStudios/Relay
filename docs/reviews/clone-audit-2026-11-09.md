# Clone audit — 2026-11-09

The audit inventoried **2,056 clone-related sites** in first-party Rust and removed **67 verified avoidable sites across 45 files**. **1,989 remain.** This is complete inventory and triage coverage, with targeted ownership review; it is not a claim that every retained clone is necessary or that every line received manual review.

The [per-site ledger](clone-audit-2026-11-09.json) records every original site, owning function, expression, disposition, assessment and evidence. Original coordinates refer to the working tree at the start of this audit, which already contained the earlier Rust review fixes. File hashes distinguish that baseline from the resulting source. A missing `current_line` means the original line changed; it does not itself mean the clone was removed.

## Scope and method

- Inventoried 1,187 first-party `.rs` files; 504 contained the invocation forms being reviewed. Included applications, libraries, tests, test support, benchmarks, fuzz targets and Loom checks. Excluded ignored build output, third-party code, `.codex` and `.agents`.
- Counted 1,817 `.clone()`, 169 explicit `Arc::clone`, 43 `.cloned()` and 23 `.clone_from()` sites, plus four qualified clone calls/callback references. Counts include platform-gated code and macro bodies. They do not count every `.to_owned()` or implicit copy.
- Parsed all inventoried source files with `syn` to attach function and receiver context. Lexical indexing also captured calls inside macro token trees.
- Used Clippy liveness/type diagnostics plus source reads of ownership, lock scopes, task lifetimes and public interfaces. The checks cover macOS workspace targets/features and the Android arm64 library. Windows-specific edits received source review, not a Windows build or runtime test.
- Applied the repository rules `anti-clone-excessive`, `own-borrow-over-clone`, `mem-clone-from` and `async-clone-before-await`, together with Beautiful Rust ownership guidance. An `Arc` clone does not deep-copy its pointee; borrowing across an await is not automatically invalid. Actual types and lifetimes govern the decision.

## Dispositions

| Assessment | Sites | Meaning |
| --- | ---: | --- |
| Removed | 67 | A local move, borrow, direct comparison or buffer append preserves the existing behavior. |
| Shared owner | 518 | Explicit `Arc::clone` or compiler-confirmed reference-counted pointer. The operation increments ownership rather than copying the pointee. |
| Fixture or expected value | 575 | Retained test inputs, expected results and shared fakes after compiler triage. This label does not certify necessity in uncompiled platform branches. |
| Owned snapshot boundary | 4 | Qualified clones retain cached secrets, immutable published gate tables, or broadcast events while supplying an independent owner. |
| Destination update | 23 | Retained `clone_from`; the source remains available and the destination may reuse allocation. |
| Benchmark input/output | 15 | Independent mutable working inputs or owned benchmark results. Required sample copying and avoidable allocation are separate questions. |
| Ownership API opportunity | 4 | Concrete candidates below that need an interface change to remove copying. |
| Other owned data or handles | 850 | Retained after source-pattern/compiler triage; no safe local removal established. These entries are explicitly not individually certified as necessary. |

## Changes made

- **Messages and networking:** append consumed Markdown list items instead of cloning them; hash an envelope with its message ID temporarily taken and restored instead of deep-cloning its body; move envelopes directly into the trusted outbox.
- **Engine:** compare borrowed session sort keys; borrow selected devices and playback health; borrow commands during execution; classify safety through a borrowed request using the same policy as `Input`; borrow the initial discovery profile.
- **UI and CLI:** dispatch owned commands while preserving the original on the unwired branch; return image bytes from the preview worker; borrow read-only settings/transmit/session values; borrow the pending link prompt; eliminate eager call-ID and RichText copies during rendering. Terminal tables construct their final cells from borrowed fields instead of first allocating temporary field copies.
- **Updater and startup:** borrow install fields while consuming the independent temporary-directory owner; move the missing asset name into its error; move Android paths and call summaries where the source is no longer needed; borrow benchmark descriptor metadata and consume the home directory on its selected branch.
- **Tests:** move final-use fixtures, including the final property-test strategy range. Replace a last-use service clone with an explicit `Clone` trait-bound assertion, preserving what the test checks.
- Enable the workspace `redundant_clone` warning so the existing Clippy warnings-as-errors check catches this class of regression.

These are ownership/allocation improvements inferred from the code. No end-to-end performance improvement was measured. A reduction in source-site count is not a runtime allocation count.

## Retained copies and further opportunities

Several expensive-looking copies preserve useful boundaries: snapshot getters release locks before returning owned values; wire messages and stored history need independent owners; task and channel handles outlive the initiating call; idempotency records retain results returned to callers. Replacing them with references can change APIs or extend lock lifetimes. `clone_from` remains appropriate when a destination is updated while retaining its source.

Four sites warrant a wider ownership refactor if reducing payload copies is the next priority:

| Function | Original sites | Evaluation |
| --- | --- | --- |
| `Actor::send_untrusted`, `crates/relay-engine/src/peers/calling.rs` | 103, 117 | Its borrowed-envelope interface preserves the caller's envelope for fallback, then clones to send. Accepting ownership and returning an unsent envelope would avoid these mutually exclusive copies. |
| `set_ringtone`, `crates/relay-engine/src/service/rpc/ops.rs` | 176 | The helper borrows the request for validation/fingerprinting while creating an actor operation that owns the data. Compute the fingerprint before consuming the request to move the payload. |
| `avatar`, same file | 239 | The same borrowed-request/owned-operation boundary copies image bytes. An ownership-taking RPC helper could move them after fingerprinting. |

Those four sites remain; therefore the answer to “have all unnecessary clones been removed?” is **not established**. The ledger gives complete site coverage and separates verified fixes from triage results, without treating a clean Clippy run as proof that every remaining copy is optimal.

## Validation

- Workspace Clippy, locked, all targets and all features, with warnings denied: passed. Also explicitly denied `redundant_clone`, `clone_on_copy`, `map_clone`, `unnecessary_to_owned`, `redundant_iter_cloned` and `cloned_instead_of_copied`.
- Android arm64 library Clippy at Rust 1.99.0, NDK r28c/API 28, with the same lint checks: passed.
- Focused `relay-net` channel-proof regression target after the final fixture edits: 8 passed.
- Root workspace `cargo fmt --all --check`, `git diff --check`, ledger identities/counts and current-source hashes: passed.
- The separate Android workspace formatter reports existing drift in `audio_session.rs`, `ffi.rs`, `host/mod.rs` and `shell.rs`. Formatting baseline and current sources in memory produced identical 14 drift hunks; this audit introduced no additional drift.
- Full locked workspace regression suite: **2,283 passed, 7 ignored, zero failures**, across 426 test/doc-test result groups.

The earlier Loom and Miri checks are recorded in the Rust skills review. Their modeled/interpreted source was not changed by this clone audit, so those results were not rerun or counted as new validation here.
