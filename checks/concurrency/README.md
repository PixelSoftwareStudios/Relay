# Concurrency checks

Run the production stream-health source against Loom's instrumented atomics:

```sh
RUSTFLAGS="--cfg loom" cargo test --locked --manifest-path checks/concurrency/Cargo.toml
```

This independent test workspace keeps Loom out of product dependency graphs. It imports
the actual health-cell source and explores concurrent failure publication, observation,
and diagnostic counters. It does not model OS callbacks or certify every media gate race.

CI also runs allocator and protocol boundary tests under Miri. Platform FFI remains under
the ordinary native/Android checks.

On macOS, run Miri from a temporary directory and pass the repository's absolute
`Cargo.toml` through `--manifest-path`. This keeps Cargo from loading the local signing
runner in `.cargo/config.toml`, which conflicts with Miri's interpreter runner.
