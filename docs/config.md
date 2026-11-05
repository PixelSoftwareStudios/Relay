# relay-config

Local persistence: preferences, identity key storage, trust store, avatars, diagnostics.

## Files

| File | Dir | Contents |
|---|---|---|
| `config.toml` | config | Preferences (no secrets, no tokens). `.bak` = previous good copy |
| `trust.toml` | data | Pinned peer keys (TOFU / verified), mode 0600 |
| `identity.insecure.key` | data | Identity key, only if the user chose `identity.key_storage = "insecure_file"` |
| `avatars/<blake3>.png` | data | Content-addressed avatar cache, max 32 MiB, oldest evicted first |
| `writer.lock` | config | OS advisory lock; one writer process at a time |

Directories come from `directories::ProjectDirs`; `Paths::new`/`Paths::under` override them.

## Rules

- Writes are atomic: temp file, fsync, copy old to `.bak`, rename, fsync dir.
- Damage is never silent. Loading returns `Notice`s the UI must show.
- Recovered or reset preferences have no microphone selected and start receive-only.
- A missing explicit device resolves to `DeviceChoice::Unavailable`; there is no fallback.
- A lost trust store yields an empty store plus `Notice::TrustStoreLost`; the bad file is kept as `.corrupt`.
- Identity uses the OS credential store by default. If it fails, the error surfaces. The plaintext
  file store is built only when the setting is `insecure_file`, and `describe()` says so.
- Raw-audio capture needs `diagnostics.record_raw_audio = true`; it is never in a support bundle.

## Schema versions

`config_version` is the schema version (current 1). Add a step in `prefs/migrate.rs` per bump.
A file newer than the build is refused, not downgraded.

## Avatar pipeline

Encoded <= 256 KiB, PNG/JPEG only, input <= 2048x2048, decoder allocation capped, then
center-crop, resize to 128x128, re-encode PNG, blake3 hash. Peer-supplied bytes use
`validate_canonical` (<= 128x128) before caching.

## Integration

Open one `WriterLock` per process and share the `Arc` with `PreferenceStore` and `TrustStore`.
Key types are generic 32-byte values (`Secret32`, `StoredIdentity`); convert at the engine boundary.
