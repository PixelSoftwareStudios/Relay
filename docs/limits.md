# Limits and retention

Every cache size, memory budget and retention bound that is a **resource policy** (how much this device keeps) can be changed by the user. Every bound that is a **security guard or a protocol invariant** cannot; it is listed under "Fixed on purpose" with the reason. When a limit was unclear it stayed fixed.

Tweakable limits are the preferences `limits.*` in the per-user settings database (`docs/config.md`). They are read and written like every other key: `GetSettings` / `SetSettings` (per-key outcome: applied live, saved until restart, rejected with a reason), `relay config get|set|export|import`, `relay limits` (current value, default, range, and what is in use), and Settings, Storage and limits in the apps. The defaults, ranges and units live in one table, `relay_config::prefs::limits::SPECS`; the table below is checked against it by `crates/relay-config/tests/limits_docs.rs`, so a default cannot drift from the code unnoticed.

A value outside its range is **rejected, never clamped**, with a reason naming the key. One rule ties two keys: `limits.messages_retained_mib` must not be smaller than `limits.messages_session_mib`. In one request the order does not matter (a refused key is retried after the others); alone, the second change is refused with a reason.

## Tweakable

| Key | Default | Min | Max | Unit | Applies | Enforced in | What happens when reached |
|---|---|---|---|---|---|---|---|
| `limits.avatar_cache_mib` | 128 | 16 | 2048 | MiB | live | `relay_config::avatar::AvatarCache` (`avatars.db`) | The least recently used pictures are removed first; the user's own picture is never removed. Lowering the limit evicts at once; raising it just allows more. |
| `limits.messages_session_mib` | 64 | 8 | 1024 | MiB | live | `relay_messages::SessionStore` (RAM transcript of one session), set by the messaging actor | New messages and files in that chat are refused (`HISTORY_FULL`); nothing already in the chat is deleted, also when the limit is lowered below what the chat holds. Running and ended chats take the new value at once. |
| `limits.messages_retained_mib` | 256 | 8 | 1024 | MiB | live | `relay_messages::HistoryManager` | Ended chats over the budget are removed, oldest first (`HistoryEvicted`). Lowering the limit removes them at once. Running chats are never evicted. |
| `limits.history_max_entries` | 200 | 0 | 5000 | entries | live | `relay_config::history::CallHistory` (`Limits::max_entries`) | The oldest calls are deleted. Lowering the limit deletes the surplus at once; 0 keeps none. |
| `limits.history_max_age_days` | 90 | 1 | 3650 | days | live | `relay_config::history::CallHistory` (`Limits::max_age_ms`) | Calls older than this are deleted. Lowering the limit deletes them at once. |
| `limits.transfer_concurrency` | 2 | 1 | 8 | transfers | live | `relay-engine` messaging actor (offer and accept paths) | A further file offer or accept is refused (`TOO_MANY_TRANSFERS`) until a running transfer ends. Transfers already running are not stopped when it is lowered; it counts from the next one. |
| `limits.messages_retained_calls` | 3 | 0 | 20 | calls | live | `relay_messages::HistoryManager` | Chats of the last N ended calls are kept in memory until you quit; when a call ends and more than N ended chats exist, the oldest is removed first (`HistoryEvicted`). Works alongside `limits.messages_retained_mib`: whichever is stricter evicts first. 0 keeps nothing once a call ends. Lowering the limit removes the surplus at once. Chats are memory-only by design, so nothing survives a restart. |
| `limits.ringtone_max_kib` | 1024 | 64 | 8192 | KiB | live | `relay-engine` ringtone store (`SetRingtone`), checked before decoding | A custom ringtone file larger than this is refused (`RINGTONE_TOO_LARGE`) and the current ringtone stays. Lowering it never removes a ringtone already stored. The stored ring is Ogg Opus (about 8 KB per second), so even 120 s stays under 1 MiB on disk. |
| `limits.ringtone_max_seconds` | 30 | 2 | 120 | seconds | live | `relay_audio::ringtone` (while decoding) | A custom ringtone longer than this is refused (`RINGTONE_TOO_LONG`), never cut; decoding stops as soon as the length is exceeded, so a small file that expands cannot fill memory. |

"live" means the running daemon applies it with no restart; at daemon start the saved values are read (`env::load`, `MessagingLimits::from_prefs`). No `limits.*` key needs a restart. Why they are tweakable: each is a storage or memory trade-off between this device's resources and how much history the user wants, none weakens a security property (a smaller budget only refuses or removes local data), and the defaults are the values the project shipped with.

Usage next to a limit is reported only where the engine knows it cheaply: the avatar cache size, the memory of ended chats and the largest running chat (`SettingsDump.usage`, shown by `relay limits` and the page). The call history count, its oldest entry and the transfer count are not reported (a query per refresh was not worth it).

### Already tweakable under `messages.*`

These existed before the `limits.*` namespace and stay where they are (Settings, Calls / Messages, `docs/session-messages.md`); they are policy for a shared resource (disk and bandwidth), not storage.

| Key | Default | Min | Max | Unit | Applies | Enforced in | Why |
|---|---|---|---|---|---|---|---|
| `messages.max_offer_size_mib` | 2048 | 1 | 4096 | MiB | live | messaging actor, before anything is stored | A user decides how big a file this device takes; 4096 is the protocol ceiling. |
| `messages.transfer_rate_limit_mbps` | 16 | 0 | 1024 | MiB/s | live | data-link pacing, running transfers included | Bandwidth politeness; 0 removes the cap. |

## Fixed on purpose

None of these is exposed. A user who could change them could weaken the app against a hostile peer, break compatibility with other versions, or make it unusable.

| Bound | Value | Where | Why fixed |
|---|---|---|---|
| Avatar upload and canonical size | 1 MiB encoded, 2048x2048 input, 256x256 lossless WebP | `relay_config::avatar::limits` | Wire invariant: peers validate exactly this canonical form, and the input cap stops decompression bombs. |
| Handshake limits | message 4096 B, timeout 5 s, 8 pending, 2 per source, preface 1 s | `relay-net::limits` | Defend the listener against unauthenticated flooding. |
| Untrusted links | 16 total, 2 per source | `relay-net::limits` | Same, before a peer is known. |
| Record sizes | UDP 1200 B, control 16 KiB | `relay-net::limits` | Protocol framing; the MTU assumption is part of the design. |
| Media replay window | 1024 packets | `relay-net::limits` | Replay protection; a smaller window accepts replays, a larger one costs memory. |
| Rekey bounds | 24 h or 2^32 records | `relay-net::limits` | AEAD nonce and key-lifetime safety. |
| Epoch overlap | 2 epochs, 2 s | `relay-net::limits` | Key rotation protocol constant. |
| Discovery table | 256 entries, 8 per source | `relay-net::limits` | Bounds memory against forged adverts on the LAN. |
| Trust records in memory | 1024 | `relay-net::identity` | Bound against a flood of unknown peers; not a storage preference. |
| Active calls | 8 | `relay-net::limits` | Mixing and link capacity of the call engine, not a user resource. |
| Pairing and channel-join attempts | 5 failures per (peer, channel), lockout 60 s up to 1 h, 4 joins at once, 4 dials per 10 s | `relay-net::channel::limits`, `relay-engine::channels` | Brute-force defence for channel passwords and invites. |
| Channel structure | password 8 to 128 B, message 8192 B, 64 members, 4 admins, 16 invites, 128 revocations, invite and certificate lifetimes | `relay-net::channel::limits` | Part of the signed channel protocol. |
| Session message rates | 10 messages/s (burst 20), 4 MiB/s inbound (burst 16 MiB), 2 offers/s (burst 4), 64 offers per session | `relay_messages::limits` | Abuse defence: a peer cannot flood a chat. |
| Message content | text 4096 B, file name 120 B, image 8 MiB / 16 Mpx / 16384 px, 100 animation frames, 128 MiB decoded animation, inline image 15 KiB, preview 16 KiB / 100 lines, Markdown 256 blocks / 4096 nodes / depth 3 | `relay_messages::limits` | Wire format and decoder-bomb guards, checked before decoding. |
| Transfer framing | 64 KiB chunk, 1 MiB window, 4 GiB file, 3 retries, 5 s ack timeout, 30 s grant | `relay_messages::limits`, `relay-engine::messaging` | Flow-control and authentication protocol constants. |
| Free-space margin | 10 % or 16 MiB | `relay_messages::limits` | Keeps a transfer from filling the disk; protective, not a preference. |
| Search | 100 results, 256 B query | `relay_messages::limits` | Response size bound of an RPC. |
| Call history paging and text | 200 per page (50 default), 64-char name, 32-char hint | `relay_config::history::limits` | API paging and column widths. |
| API envelope | 64 keys / 128 B key / 1024 B value per settings request, 1024 idempotency entries, 2048 replay events, telemetry 20 Hz | `relay-engine::service` | Request bounds of the local API; replay window of the watch stream. |
| Engine queues | command/output depth 64 to 256, per-link outbox 64, sessions 256 (32 per peer), early media 10 s / 4 per peer / 16 | `relay-engine` | Backpressure; sizing is tuned together with the code. |
| Jitter buffer | adaptive margin, slots, concealment and fade lengths (`JitterConfig`) | `relay-protocol::jitter` | Audio latency tuning, not storage. A wrong value breaks calls and the buffer already adapts to the network. Unsure, so fixed. |
| Diagnostics bundle | 256 KiB | `relay_config::diagnostics` | Bound of a redacted support bundle. No log files are written by the daemon, so there is no log retention to set. |

## Not exposed yet (UI memory)

These are display budgets of the desktop UI, not engine limits. They were left fixed, not because they must be, but because the Messages screens were being rewired while this was built; each is a candidate for a later `limits.*` key.

| Bound | Value | Where |
|---|---|---|
| Local transcript entries kept per chat in the UI | 5000 | `relay-ui::messaging::state` (`MAX_LOCAL_ENTRIES`) |
| Avatar texture cache | 64 textures | `relay-ui::avatar` (`CACHE_CAPACITY`) |
| Message image texture cache | 48 textures | `relay-ui::ui::messages` |
| Call history rows per page in the UI | 20 | `relay-ui::history_view` (`PAGE`) |
