# The update key

`update-key.pub` is the minisign public key every Relay build trusts for updates. It is the REAL
release key (key id `92AC5C640F5EEC91`); its secret half is held by the maintainer and is not in this
repository.

The TEST-ONLY key (`tests/fixtures/TEST-ONLY-update-key.pub` and `.seed`, whose secret half IS public)
exists only for tests. It is reachable only through `Updater::with_endpoint`, which is compiled only
under `cfg(test)` or the `test-util` feature; `Updater::new` / `detect` always trust `update-key.pub`.
Tests prove that a signature by the test key is rejected by a release-key updater.

`cargo test -p relay-update -- --ignored release_key_is_not_the_test_key` (release job) fails if the
compiled-in key ever equals a test fixture key.

## Rotating the key

There is no run-time key list, revocation or expiry: the key is `include_str!`ed.

1. Generate a new pair offline (`minisign -G`), keep the secret out of the repository.
2. Overwrite `update-key.pub` with the new public key file, unchanged, and update the key id above.
3. Update the signing secrets of the release workflow.
4. Builds already installed trust only the OLD key, so they can update only to a release signed with
   the old key. To rotate, first ship a release signed with the old key that contains the new
   `update-key.pub`; only later releases may be signed with the new key. If the old key is lost or
   leaked, users must reinstall by hand from a trusted source.
