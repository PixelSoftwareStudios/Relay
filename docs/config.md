# relay-config

Local persistence: preferences, trust store, avatar cache, identity key storage, diagnostics.

## Where data lives

All persistent settings are in **one SQLite database per OS user**. Secrets are not.

| File | Dir | Contents |
|---|---|---|
| `relay.db` | data | Preferences, trust entries, channel metadata, local avatar hash (+ `-wal`, `-shm`) |
| `relay.db.bak` | data | Consistent snapshot used for recovery (`VACUUM INTO`, atomic rename) |
| `relay.db.corrupt` | data | A damaged database, kept for inspection after recovery |
| `avatars.db` | data | Disposable avatar cache (bytes only); no backup, wiped and recreated if damaged |
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
| `state(key, value)` | Small internal flags: local avatar hash (the pinned entry of the avatar cache) |
| `channels`, `channel_*` | Channel metadata (see below) |

`Preferences::config_version` (currently 1) is the data-model version stored in `prefs`; it is
separate from the SQL schema version.

### Channels

Channel state (`docs/channels.md`) is stored by `channels::ChannelStorage`, which is generic and
knows nothing about the channel protocol: certificates, delegations and revocations are opaque
blobs, policy flags and roles are small integers the caller defines.

| Table | Purpose |
|---|---|
| `channels` | id, name, flags, role, owner public key, optional delegation and own certificate (public blobs) |
| `channel_issued`, `channel_revocations`, `channel_invites` | Issued-certificate expiries, revocation blobs, invite id/expiry/uses left (children of `channels`, `ON DELETE CASCADE`) |
| `channel_secrets` | Manifest of vault entries: `(channel, slot, generation, state)` with state `pending`, `committed` or `dead` |

**Secrets are never in SQLite.** The owner signing key, the password-derived credential and the
invite credentials go to a `vault::SecretVault` keyed by `(channel id, slot)`: the OS credential
store by default (`KeyringVault`), or 0600 files in `channel-secrets/` only when
`identity.key_storage = "insecure_file"` (`FileVault`, same explicit opt-in as the identity key);
`MemoryVault` is for tests. `vault::open_vault(&prefs, &paths)` picks the right one.

Writes keep the two stores consistent without orphans:

1. The new secret versions are recorded as `pending` generations and committed.
2. They are written to the vault under `slot.generation` names (old versions stay untouched).
3. One transaction writes the metadata, marks the new generations `committed` and the
   superseded or removed ones `dead`.
4. Dead generations are wiped from the vault and their rows deleted.

If step 2 or 3 fails, the staged vault entries are removed again. If the process dies at any
point, `ChannelStorage::open` wipes remaining `pending` and `dead` entries, so a crash never
leaves a secret without a record and never loses a committed one. `update` runs the caller's
closure once with the connection held, as one read-modify-write, so single-use invites cannot be
spent twice. The engine adapter is `daemon::SqliteChannelStore`, implementing
`relay_net::channel::ChannelStore` (this crate does not depend on `relay-net`).

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
| `hotkeys.push_to_talk` | Global push-to-talk accelerator (default `Ctrl+Alt+Space`; empty disables); read by `relay hotkey` and `relay tui` |
| `PreferenceStore::read_only(&Paths)` | Read without the lock, recovery or writes (for `doctor`, `config get` next to a running daemon) |
| `Storage::open(&Paths)` | Lock + open + recover + import; returns `database`, `lock`, `notices` |

Writing while the daemon runs must go through the daemon (it holds the writer lock).

## Identity

Identity uses the OS credential store by default. If it fails, the error surfaces. The plaintext
file store is built only when the setting is `insecure_file`, and `describe()` says so.

## Avatar pipeline

Upload: encoded <= 1 MiB (`ENCODED_MAX_BYTES`, also the cap for what is stored, served or
accepted from a peer), PNG/JPEG only, input <= 2048x2048, decoder allocation capped
(2048x2048 RGBA), then center-crop, resize to 256x256 and encode as RGBA8 PNG. The PNG is
recompressed losslessly with `oxipng` (libdeflate level 12, preset-2 filter set, metadata
stripped, colour/bit-depth/palette reduction off so the output is always RGBA8, no timeout, no
threads): same input, same bytes, same blake3 hash. This runs at upload only. Typical results:
a flat avatar ~0.7 KB, a smooth gradient ~0.8 KB, a vector-style picture ~19 KB, incompressible
noise ~218 KB (worst case about 262 KB, far under the cap); ~30 to 150 ms in release builds.

Receive: `validate_canonical` checks the canonical form without re-encoding: size <= 1 MiB,
PNG signature, chunks exactly `IHDR`, `IDAT`+ and `IEND` with valid CRCs (any other chunk,
ancillary or not, or trailing bytes is refused), 256x256, 8-bit RGBA, non-interlaced; the
image must also decode under the same limits. JPEG is not canonical. The hash is blake3 of the
received bytes.

### Avatar cache

`AvatarCache` keeps bytes in `avatars.db` (own SQLite file next to `relay.db`, created
`0600` in a `0700` directory, `synchronous=OFF`, no WAL, never backed up) so the frequently
backed-up settings database does not carry them. It is bounded to 128 MiB per user
(`CACHE_MAX_BYTES`) with least-recently-used eviction: room is made before the insert, so the
bound is never exceeded, even briefly. Entries larger than the bound are skipped; reads
re-verify the hash and drop tampered rows. A file that is not a database or reports
corruption is deleted and recreated empty (logged at debug, no notice): it is only a cache.
The local user's avatar hash lives in the settings `state` table (`set_local`/`local`); that
entry is pinned and never evicted, and `local()` reports `None` if its bytes are gone from a
wiped cache, so the avatar must be uploaded again. `AvatarCache::open(dir)` makes a standalone
cache plus a small settings database in `dir` (tools, tests); the daemon uses
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
