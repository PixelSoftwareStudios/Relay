# relay-config

Local persistence: preferences, trust store, avatar cache, identity key storage, diagnostics.

## Where data lives

All persistent settings are in **one SQLite database per OS user**. Secrets are not.

| File | Dir | Contents |
|---|---|---|
| `relay.db` | data | Preferences, trust entries, avatar cache, local avatar hash (+ `-wal`, `-shm`) |
| `relay.db.bak` | data | Consistent snapshot used for recovery (`VACUUM INTO`, atomic rename) |
| `relay.db.corrupt` | data | A damaged database, kept for inspection after recovery |
| `writer.lock` | config | OS advisory lock; one writer process at a time |
| `identity.insecure.key` | data | Identity key, only if the user chose `identity.key_storage = "insecure_file"` |

The directories come from `directories::ProjectDirs`, which resolve inside the current OS user's
profile. `Paths::new`/`Paths::under` override them (tests, portable installs).

**Secrets never go into SQLite.** The identity private key lives in the OS credential store
(Keychain, Credential Manager, Secret Service) or, only on explicit user choice, in the
`identity.insecure.key` file (0600, described as INSECURE by `describe()`). The database holds no
keys, tokens or passwords; only public peer keys and display names. The diagnostics bundle never
includes keys, trust entries or the database path (see below).

## Per-user isolation

Each OS user of a shared PC has their own database in their own profile directory, and the daemon
socket/runtime directory is per user as well. The crate enforces this on top of the OS:

- Unix: directories are created `0700`, the database and its `-wal`/`-shm`/`.bak` files `0600`.
  Looser permissions found at open time are tightened; if that is refused (the path belongs to
  another user) opening fails instead of continuing.
- Windows: the per-user profile ACLs isolate the directories; no extra ACL is applied.
- Tests simulate two users with two separate roots and assert neither sees the other's rows.

## Database settings and schema

WAL journal, `synchronous=FULL` (a committed setting or trust change survives power loss),
`foreign_keys=ON`, `trusted_schema=OFF`, 5 s busy timeout, one connection behind a mutex plus the
single-writer `WriterLock` (a second process gets `ConfigError::Locked`).

`PRAGMA user_version` is the schema version (`db::SCHEMA_VERSION`, currently 1). Migrations are
the entries of a list in `db/migrate.rs`, each run in its own transaction; append, never edit.
A database with a higher version than the build is refused (`ConfigError::UnsupportedVersion`) and
left untouched.

| Table | Purpose |
|---|---|
| `prefs(key, value)` | One row per setting, dotted key (`audio.microphone_gain`), TOML-encoded typed value |
| `trust(public_key, display_name, level, first_seen)` | TOFU / Verified peers; `CHECK` on key length and level |
| `avatars(hash, data, size, last_used)` | Content-addressed avatar blobs, LRU evicted |
| `state(key, value)` | Small internal flags: local avatar hash |

`Preferences::config_version` (currently 1) is the data-model version stored in `prefs`; it is
separate from the SQL schema version.

### Reserved: channels

The channel feature (`docs/channels.md`) will add its tables as the next migration entry. That
work is intentionally not done yet: the `ChannelStore` trait lives in `relay-net`, and its records
hold PAKE-equivalent secrets and signing keys, which by the rule above belong in the OS credential
store, not in this database. The expected split is non-secret metadata rows here and secret
material in the credential store (or one blob encrypted with a key held there). Needs a decision
before implementation.

## Rules

- Writes are transactions; there are no more temp-file swaps for settings.
- Damage is never silent. Opening returns `Notice`s the UI must show (`Storage::notices`).
- A database that is not a database, fails `PRAGMA quick_check`, is truncated, or is missing/empty
  while a `.bak` exists is moved to `.corrupt`, then the `.bak` is restored. Recovered or reset
  preferences have **no microphone selected and start receive-only**
  (`Notice::PreferencesRecoveredFromBackup`, `TrustRecoveredFromBackup`). With no usable backup
  the result is an empty database plus `PreferencesReset` and `TrustStoreLost`.
- Backups are refreshed at every open, after each trust change (forced) and after preference
  saves (at most every 5 minutes). Trust entries added after the last snapshot are the only thing a
  recovery can lose, and `TrustRecoveredFromBackup` says so.
- Rows that fail validation (manual edits) load as defaults with `PreferencesReset`.
- A missing explicit device resolves to `DeviceChoice::Unavailable`; there is no fallback.
- Raw-audio capture needs `diagnostics.record_raw_audio = true`; it is never in a support bundle.

## Export and import as TOML

Settings are not hand-editable in place any more. For humans and support:

- `prefs::export_toml(&Preferences)` / `PreferenceStore::export_toml()` produce TOML with a comment
  header. Preferences only: no keys, tokens or trust data.
- `prefs::import_toml(text)` / `PreferenceStore::import_toml(text)` parse,
**reject unknown keys** (typos) and out-of-range values, then save atomically. Importing is an
  explicit user action and may select a microphone.

CLI-facing helpers (for `relay config get|set|export|import`):

| Function | Use |
|---|---|
| `prefs::flat::keys()` | Settable keys, sorted |
| `PreferenceStore::get(key)` / `set(key, text)` | One setting; string settings are taken literally, other types parse as TOML (`2.5`, `false`, `rnnoise`); validated before saving |
| `PreferenceStore::read_only(&Paths)` | Read without the lock, recovery or writes (for `doctor`, `config get` next to a running daemon) |
| `Storage::open(&Paths)` | Lock + open + recover + import; returns `database`, `lock`, `notices` |

Writing while the daemon runs must go through the daemon (it holds the writer lock).

## Identity

Identity uses the OS credential store by default. If it fails, the error surfaces. The plaintext
file store is built only when the setting is `insecure_file`, and `describe()` says so.

## Avatar pipeline

Encoded <= 256 KiB, PNG/JPEG only, input <= 2048x2048, decoder allocation capped, then
center-crop, resize to 128x128, re-encode PNG, blake3 hash. Peer-supplied bytes use
`validate_canonical` (<= 128x128) before caching. The cache table is bounded to 32 MiB with
least-recently-used eviction; entries larger than the bound are skipped, reads re-verify the hash
and drop tampered rows. The local user's avatar hash (`AvatarCache::set_local`/`local`) is
persisted, restored on start if its bytes are still cached, and never evicted.
`AvatarCache::open(dir)` makes a standalone cache database in `dir` (tools, tests); the daemon uses
`AvatarCache::new(&storage.database)`.

## Diagnostics

The support bundle is redacted and bounded (256 KiB). It has no field for keys, trust entries or
avatars. Device ids are fingerprinted, the display name is omitted, and log lines are scrubbed.
`diagnostics::database_location(path, reveal)` hides the database path unless `reveal` is set,
because it contains the OS user name.

## Integration

Open `Storage` once per process, then share the handles:
`PreferenceStore::open(&storage.database)`, `TrustStore::open(&storage.database)`,
`AvatarCache::new(&storage.database)`. Key types are generic 32-byte values (`Secret32`,
`StoredIdentity`); convert at the engine boundary.
