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
| `keychain.lock` | config | OS advisory lock serialising read-modify-write of the credential-store item |
| `identity.insecure.key` | data | Identity key, only if the user chose `identity.key_storage = "insecure_file"` |

The directories come from `directories::ProjectDirs`, which resolve inside the current OS user's
profile. `Paths::new`/`Paths::under` override them (tests, portable installs).

**Secrets never go into SQLite.** The identity private key lives in the OS credential store
(Keychain, Credential Manager, Secret Service) or, only on explicit user choice, in the
`identity.insecure.key` file (0600, described as INSECURE by `describe()`). The database holds no
keys, tokens or passwords; only public peer keys and display names. The diagnostics bundle never
includes keys, trust entries or the database path (see below).

**One credential-store item.** Everything secret goes into a single item per OS user, service
`relay-voice`, account `vault` (`keychain::Keychain`). Its value is one versioned JSON object
`{"version":1,"identity":<hex>,"channels":{"<channel hex>/<slot>":<hex>}}` held in zeroizing,
redacted-`Debug` buffers. `KeyringStore` (identity) and `KeyringVault` (channel secrets) are views
of it, so the OS asks for permission once, not once per secret. Every change is a read-modify-write
under an in-process mutex plus an advisory `ProcessLock` on `keychain.lock`, so two processes of one
user never lose each other's updates; reads are a single atomic item read. An unchanged bundle is not
written back and an emptied one deletes the item. An unknown bundle version is refused with an
error, not guessed at. There is no per-secret item layout and nothing reads one. The INSECURE file
stores (`identity.insecure.key`, `channel-secrets/`) are unchanged and still only exist on explicit
user choice.

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
| `call_history(id, peer_name, peer_fingerprint, peer_fingerprint_short, direction, outcome, started_at, duration_ms, call_id)` | Past calls (see "Call history" below) |
| `favorites(public_key, position)` | The quick-call favorites: an ordered list of at most 6 peers. `public_key` references `trust(public_key)` with `ON DELETE CASCADE`, so forgetting a peer (or any removal of its trust) deletes its favorite; `position` is unique and 0-based. `relay_config::favorites::Favorites::{list,set}` replace the whole list in one transaction and refuse, changing nothing, a list over 6 (`You can have up to 6 favorites`), a repeated peer, or a peer that is not in `trust` |
| `blocked(fingerprint, display_name, blocked_at)` | Devices the user blocked, by the SHA-256 digest of their key (the peer id; 64 hex digits), with the name they had then. At most 256 rows (`relay_config::blocklist::MAX_BLOCKED`); blocking a full list is refused, re-blocking only renames. `Blocklist::{list,add,remove}`. A blocked key is refused by the engine; blocking a contact removes its `trust` row, and unblocking does not bring it back |

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
store by default (`KeyringVault`, one shared item, see above), or 0600 files in `channel-secrets/` only when
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

### Call history

`history::CallHistory` (`crates/relay-config/src/history/`) keeps one row per call the user saw end, in the `call_history` table of the same per-user database (schema version 1, no migration: the product is unreleased). A row holds exactly: the peer's display name as seen at the time, the peer's full fingerprint (64 hex digits, the peer id) and the short hint (`Fingerprint::short()`, as `Peer.fingerprint_short`), the direction (`incoming`/`outgoing`), the outcome (`completed`, `missed`, `declined`, `failed` for an outgoing call that did not reach the peer, `cancelled`), `started_at` (unix ms), `duration_ms` (connected time, 0 when it never connected) and the call id (16 bytes, which names the call's chat session). Nothing else is stored: no audio, no message content, no addresses.

Bounds, enforced inside the insert transaction ("pruned on write"): by default the 200 newest rows and nothing older than 90 days (against the caller's clock). The defaults (`MAX_ENTRIES`, `MAX_AGE_MS`, the page sizes `MAX_LIST`/`DEFAULT_LIST`, the name and hint lengths) are named constants in `history/limits.rs`; the daemon opens the history with `CallHistory::with_limits(Limits)` from the preferences `limits.history_max_entries` (0 to 5000) and `limits.history_max_age_days` (1 to 3650), and `CallHistory::set_limits(limits, now_ms)` changes them for every clone and prunes at once, so lowering a limit deletes the surplus entries immediately. `list(limit, before)` returns newest first and pages by row id (`before` = the last id of the previous page); `clear()` deletes everything. The preference `history.enabled` (default `true`) decides whether the engine records; turning it off keeps the entries until they are cleared. The store itself does not look at the preference (the engine does), so `clear` and `list` always work. The history is never part of a diagnostics bundle, an export of the settings or anything sent to a peer.

#### Privacy

Call history is local only. It is written by the daemon on this device, read over the local API by this user's clients, and never sent to the other person or anywhere else. To stop it recording, turn off "Keep call history" in Settings, Privacy and trust (or `relay config set history.enabled false`); existing entries then stay until cleared. To delete it, use "Clear history" on the Calls page or `relay history clear`. Entries also leave by themselves after 90 days or when more than 200 are stored (the defaults; both are settings, see `docs/limits.md`). The peer's name is stored as it was shown then, so it keeps an old name after a rename and stays readable after the device is forgotten.

## Rules

- Writes are transactions; there are no more temp-file swaps for settings.
- Damage is never silent. Opening returns `Notice`s the UI must show (`Storage::notices`).
- A database that is not a database, fails `PRAGMA quick_check`, is truncated, or is missing/empty
  while a `.bak` exists is moved to `.corrupt`, then the `.bak` is restored. Recovered or reset
  preferences have **no microphone selected and start muted** (`calls.default_transmit_mode = muted`)
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
| `calls.default_transmit_mode` | The mode a call starts in when it connects, incoming accepted or outgoing: `muted`, `unmuted` (open mic) or `push_to_talk` (default). Choosing `unmuted` or `push_to_talk` is advance consent to send the microphone in every call; `muted` makes calls start silent. A mode chosen in the call wins. Replaces `privacy.new_calls_receive_only`. Applies live |
| `ptt.enabled` | Push-to-talk may be used (default true). Off: the global key is not registered at the next start and is ignored while running, the talk button is not offered, `push_to_talk` cannot be chosen (`PTT_DISABLED`), a default of `push_to_talk` becomes `muted` (reported as `effective_default_transmit_mode`) and calls in push-to-talk are muted at once. Applies live |
| `ptt.release_delay_ms` | Push-to-talk release delay: after an ordinary key release the transmit gate stays open this long (0 to 2000 ms, default 200; 0 closes at once) so a sentence end is not clipped. Applies live; every safety close (disconnect, takeover, mute, lock, device loss, call end) ignores it |
| `audio.ringtone_enabled` / `audio.ringtone_volume` | Play the ring for an incoming call while a client is attached (default true) and its volume (0.0 to 1.0, default 0.7). Apply live; disabling while ringing stops the ring |
| `audio.ringback_enabled` | Play the ringback while an outgoing call rings (default true). Applies live |
| `audio.call_sounds_enabled` | Short connect and hang-up blips (default true). Applies live |
| `messages.enabled` | Master switch for session messaging (default true). Off: nothing is sent, accepted or transferred (a peer's message is answered `Disabled`); existing transcripts stay readable. Applies live (`SetMessagingSettings`) |
| `messages.max_offer_size_mib` | Largest file this device offers or accepts, 1 to 4096 MiB (default 2048). A larger inbound offer is refused with `TooLarge` before anything is stored. Applies live |
| `messages.transfer_rate_limit_mbps` | Pacing cap of one file transfer in MiB/s, 0 to 1024 (default 16, 0 removes the cap). Applies live, to running transfers too |
| `network.transfer_port` | TCP port of the file-transfer data-link listener (default 0 = a free port each run). It is told to a peer only inside an authenticated control-link grant, never advertised. Applies at daemon start; the effective port is `MessagingSettings.transfer_port` |
| `limits.*` | Storage and retention limits, seven numeric keys under one namespace (`limits.avatar_cache_mib`, `limits.messages_session_mib`, `limits.messages_retained_mib`, `limits.history_max_entries`, `limits.history_max_age_days`, `limits.transfer_concurrency`, `limits.messages_retained_calls`). Defaults, ranges and units are one table in code, `prefs::limits::SPECS`, and one in `docs/limits.md` (a test keeps them equal). A value outside its range is rejected with a reason naming the key, never clamped; `limits.messages_retained_mib` must not be below `limits.messages_session_mib`. `flat::set_many` applies several keys and retries a key refused only because of another key in the same request. All apply live |
| `hotkeys.push_to_talk` | Global push-to-talk accelerator (default `Ctrl+Alt+Space`; empty disables); read by `relay hotkey` and `relay tui` |
| `PreferenceStore::read_only(&Paths)` | Read without the lock, recovery or writes (for `doctor`, `config get` next to a running daemon) |
| `Storage::open(&Paths)` | Lock + open + recover + import; returns `database`, `lock`, `notices` |

Writing while the daemon runs must go through the daemon (it holds the writer lock): `SetSettings` accepts any key of this table (`docs/api.md`, "Settings by key"), says per key whether it applied live or needs a restart, and refuses identity, raw-audio-capture and WAN/overlay keys. `prefs::flat::all(&Preferences)` is the sorted `key -> text` map `GetSettings` returns.

## Identity

Identity uses the OS credential store by default. If it fails, the error surfaces. The plaintext
file store is built only when the setting is `insecure_file`, and `describe()` says so.

The identity key and the channel secrets share **one** credential-store item (`relay-voice` /
`vault`, a versioned bundle). Only the daemon opens it. `Keychain::os(lock_file)` returns one
cached handle per process: the item is read once per start, held in memory, writes go through
and update the copy, concurrent first reads queue behind the one in flight (a prompt that lasts
minutes is not asked twice), and a failed read is not cached. `Keychain::new` (tests, other
backends) reads every time.

`IdentityStore::load` answers found, `Ok(None)` (definitively no item) or an error (denied,
locked, interrupted, timeout, corrupt or newer bundle: `ConfigError::BundleUnreadable`,
`IdentityMalformed`, `CredentialStore`). `load_or_create` generates only on `Ok(None)` and stores
through `IdentityStore::create`, which is create-if-absent and never overwrites (keychain: one
read-modify-write under the cross-process file lock, which is also what makes concurrent first
starts create one identity; file: stage then hard-link; an error reading the item writes
nothing and leaves it byte-identical). A different process holding the lock is waited for (30 s),
never overridden. `delete` (only the confirmed reset) removes a bundle that cannot be read at all.
When the identity changes: `relay identity reset --yes`, the user deleting the keychain item,
choosing the file store when it holds none, or (Android) wiped app data. Contacts then see a
changed key and have to verify this device again. Tests: `tests/keychain_cache.rs`,
`tests/identity_lifecycle.rs`.

## Avatar pipeline

Upload: encoded <= 1 MiB (`ENCODED_MAX_BYTES`, also the cap for what is stored, served or
accepted from a peer). Accepted inputs are PNG, JPEG, WebP, GIF, BMP and TIFF (sniffed from the
bytes, not the extension); animated GIF/WebP contribute their first frame. Input <= 2048x2048,
decoder allocation capped (2048x2048 RGBA, so header lies and decompression bombs fail before
pixels are allocated). Then center-crop, Lanczos resize to 256x256 and encode as a **lossless
WebP** with the pure-Rust `image-webp` encoder (via the `image` crate; version pinned by
`Cargo.lock`, currently 0.2.4). The encoder has no options, timestamps or metadata, so the same
pixels give the same bytes and the same blake3 hash. Encoding runs at upload only. Typical sizes
for 256x256 RGBA: flat colour ~0.2 KB, smooth gradient ~29 KB, vector-style art ~19 KB, photo-like
~67 KB, incompressible noise ~197 KB (about 0.2 to 0.5 ms each); worst case stays far below the
1 MiB cap.

Receive: `validate_canonical` checks the canonical form from the header, without re-encoding and
before any pixel allocation: size <= 1 MiB; `RIFF` length equal to the file size minus 8;
`WEBP` form type; exactly one chunk, `VP8L` (so no lossy `VP8`, `VP8X`, `ALPH`, `ANIM`, `ANMF`,
`EXIF`, `XMP`, `ICCP` or unknown chunks), zero padding and no trailing bytes; VP8L signature,
version 0 and dimensions exactly 256x256. A bounded full decode (same limits) must then succeed.
PNG, JPEG and every other format are not canonical. The hash is blake3 of the received bytes.

### Avatar cache

`AvatarCache` keeps bytes in `avatars.db` (own SQLite file next to `relay.db`, created
`0600` in a `0700` directory, `synchronous=OFF`, no WAL, never backed up) so the frequently
backed-up settings database does not carry them. It is bounded to 128 MiB per user by default
(`CACHE_MAX_BYTES`; the preference `limits.avatar_cache_mib` sets it, 16 to 2048 MiB) with least-recently-used eviction: room is made before the insert, so the
bound is never exceeded, even briefly. `AvatarCache::set_limit(bytes)` changes the bound for the cache and every clone and evicts at once down to it (never the pinned local avatar), so shrinking is immediate and growing needs nothing. Entries larger than the bound are skipped; reads
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

## Profile display name default

`profile.display_name` is stored only when the user chooses one; the factory value and an empty string both mean "not chosen". Reading it for the advertisement (`FileSettings`, relay-engine) applies, in order: the stored name, `RunOptions::default_display_name` (the host's device name; nothing is stored for it), the hostname, each sanitised. See `docs/engine.md` ("Profile").
