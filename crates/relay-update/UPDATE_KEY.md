# REPLACE THIS KEY BEFORE ANY RELEASE

`update-key.pub` is the minisign public key every Relay build trusts for updates.

**Right now it is the TEST-ONLY key** from `tests/fixtures/TEST-ONLY-update-key.pub`. Its secret
half (`tests/fixtures/TEST-ONLY-update-key.seed`) is committed to this repository, so anyone can
sign an "update" that a build with this key would install. A release built with it would let any
attacker who can serve a GitHub-looking download run code on every user's machine.

Before the first release:

1. Generate the real pair offline: `minisign -G -p update-key.pub -s <secret key file>`.
2. Keep the secret key and its password out of the repository. Put it in the release signing
   secrets the pipeline uses (see `docs/updates.md`).
3. Overwrite `crates/relay-update/update-key.pub` with the new public key file, unchanged.
4. Run `cargo test -p relay-update -- --ignored release_key_is_not_the_test_key`. It fails while
   the compiled-in key equals the test key; the release job runs it.

There is no run-time override: the key is compiled in with `include_str!`, so replacing it is a
code change that goes through review.
