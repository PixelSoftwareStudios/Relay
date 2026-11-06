# Session messaging

Design record for call-scoped messages, inline images and file offers. Status: design accepted,
crate `relay-messages` implements the sans-IO core, `relay-net` the wire messages and the
per-transfer data link, the daemon (`relay-engine`, `docs/engine.md`) the live sessions, transfers
and permissions, and `relay-api` the local API (`docs/api.md`); client UI integration is separate
work. See the ADR "Session-scoped ephemeral messaging" in `docs/decisions.md`.

Session messaging is a companion to voice, **not a chat platform**. There are no accounts,
rooms, offline delivery, server history, read receipts, typing indicators, reactions, edits or
remote deletes. Messages exist for the lifetime of a call session, live in RAM, and the only
thing that ever persists is an explicit user export. Screen sharing and video stay out of scope.

## Scope and non-goals

In: text with limited Markdown, code blocks for IT use, inline images, file offers with a
user-approved transfer, pins, local search, local clear, JSON export.

Out: persistence of any kind other than export, sync of pins or clears, group fan-out beyond the
calls the engine already has, link previews (no fetching of URLs, ever), auto-download,
auto-open, auto-preview, executing anything.

## Crate shape

`crates/relay-messages` is sans-IO: no sockets, files, clocks or randomness. The engine injects
`Clock`, `IdSource` and (for downloads) `FileSink`. Everything is bounded; constants live in
`limits.rs`. Types never log message content: `Debug` of bodies is redacted and `Redacted`
provides a content-free `Display`.

| Module | Role |
|---|---|
| `limits` | Every numeric bound in one place |
| `env` | `Clock`, `IdSource` traits |
| `sanitize` | Text, display-name and file-name sanitization |
| `markdown` | Safe AST parser (`parse`), URL and host analysis, punycode |
| `message` | `Message`, `MessageId`, `MessageBody`, `Direction`, `Delivery`, `Wire` payloads |
| `image` | Header-only validation of PNG, JPEG, WebP, GIF |
| `rate` | Token bucket |
| `delivery` | Ack and bounded-retry outbox |
| `store` | `SessionStore`: transcript, budget, pins, search, clear, export |
| `history` | `HistoryManager`: global budget, ended-session eviction |
| `transfer` | File offer metadata, transfer state machine, `FileSink` |
| `traffic` | `TrafficClass` and bulk pacing constants |

## Messages

- Text is UTF-8, at most 4 KiB (4096 bytes) per message. Control characters other than `\n` and
  `\t` are refused (`\r\n` is normalised to `\n`, a lone `\r` is refused). Bidi controls
  (U+202A-202E, U+2066-2069, U+061C, U+200E/F), zero-width characters and other format
  characters (category Cf: U+200B-200D, U+2060-2064, U+FEFF, soft hyphen, tag characters, ...)
  are refused. One exception keeps emoji working: U+200D between two non-ASCII characters.
  Display names use the same character classes but are stripped instead of refused.
  Both daemons apply this: a receiver never trusts the sender's check.
- Each side numbers its own messages with a per-session sequence starting at 1. A `MessageId` is
  `(origin, seq)`. **Ordering never uses remote time.** The transcript is ordered by arrival: local
  messages when composed, remote messages when received. The sender's wall-clock time
  (`sent_at`) is stored and shown as a claim; the receiver's own time (`received_at`) is what the
  client displays when they disagree. Remote sequences must strictly increase. A repeated
  sequence is a duplicate (re-acked, not stored again); gaps are legal because rejected messages
  still consume a sequence number.
- Delivery states for local messages: `Sending`, `Delivered`, `Failed(reason)`. The transport
  is reliable ordered TCP, so acks are application-layer: they prove the peer's daemon stored the
  message (or tell us why it did not). Every message gets an `Ack{seq, Delivered | Rejected(reason)}`.
  The outbox resends unacked messages with a 5 s timeout, doubling, at most 3 retries, then
  `Failed(Timeout)`. A `Rejected` ack fails the message at once with that reason and never
  retries. The user may retry a failed message (same seq, fresh attempt budget).
- Reasons a receiver rejects: `Disabled` (permission off), `TooLarge`, `Invalid`, `RateLimited`,
  `HistoryFull`, `Unsupported`, `Ended`.

### Markdown (limited, safe)

`markdown::parse(&str) -> Document` is total: it never fails and never panics. Supported:
`**bold**`, `*italic*`, `` `inline code` ``, fenced code blocks, flat bullet (`- `, `* `) and numbered
(`1. `) lists, and auto-detected URLs. Unsupported syntax stays literal text. There is no raw HTML,
no markdown images, no `[label](url)` links (a label could disguise the destination), no nested lists.

Bounds: at most 256 blocks, 4096 nodes, inline nesting depth 3. When a bound is hit the rest
degrades to plain text. The AST is derived data, rebuilt (or cached by the client) from the
stored text; it is never serialized.

URLs: only `http` and `https`, no userinfo (`https://good.com@evil.com` is not linkified),
ASCII hosts only (a raw non-ASCII host stays text), total length at most 2048, no controls. The
link carries a `HostInfo` with the lowercased ASCII host, a display form, and a `HostWarning`:
`Idn` for any `xn--` label (decoded for display), `MixedScript` when one label mixes scripts
(the strong lookalike signal), `InvalidPunycode`. A client must show the ASCII form and a warning
badge whenever a warning is present. Links are never opened automatically; opening needs an explicit
click and the client shows the full destination first. Relay never fetches a URL.

### Code blocks

A fenced block has an optional language label (`[A-Za-z0-9_+#.-]`, at most 32 characters) and the
exact content. `CodeBlock::copy_text()` returns the content bytes exactly as sent (no fences, no
trailing newline added or removed beyond the one that precedes the closing fence). Code is
displayed monospaced with a copy action and is never executed, evaluated or "run".
`SessionStore::copy_code_block` gives the same bytes for clients that only hold ids. An
unterminated fence runs to the end of the message and is flagged `closed = false`.

## History

RAM only, per daemon process.

- **Per-session budget: 64 MiB**, counted as the sum over messages of (payload bytes: text, image
  bytes, preview text, file name and metadata strings) + a conservative 1 KiB per-message
  overhead allowance. The transcript grows on demand; nothing is preallocated.
- **No message-count limit.** At the budget a new message (sent or received) is **rejected** with
  `StoreError::BudgetExceeded`; existing messages are never silently evicted. The UI tells the
  user the session history is full and offers export and clear. A rejected inbound message is
  acked `Rejected(HistoryFull)`.
- **Global budget: 256 MiB across ended sessions.** When a session ends its transcript is kept
  (read-only) so the user can still read, search, copy or export it. When the sum of ended
  transcripts exceeds the budget the **oldest ended** transcript is evicted first, repeatedly.
  Active sessions are never evicted and do not count against this budget (each is bounded by its
  own 64 MiB; with N concurrent calls the worst case is N x 64 MiB plus 256 MiB, which the engine
  caps by its call limit).
- An ended transcript lives until: a **replacement session with the same peer fingerprint
  starts** (decision: the older transcript for that peer is dropped then, so at most one ended
  transcript per peer exists; the client shows "previous transcript will be discarded, export
  first" in the new session's start flow), or the user clears it, or the global budget evicts it,
  or the daemon exits.
- Pins are local to each side and not synced in v1; a pin is a flag on the stored message.
- Search is in-session, case-insensitive substring over text, code and file names, returning at
  most 100 hits.
- Clear is local only; it never deletes anything on the peer. Disabling messaging stops new
  messages but does not erase existing ones.
- Per-sender rate limit: token bucket, 10 messages/s, burst 20, applied to inbound (per remote
  sender) and outbound. Per-peer inbound byte limit: 4 MiB/s, burst 16 MiB. File offers: at most
  64 per session and 2/s (burst 4).

### Export

`SessionStore::export(ExportOptions) -> serde_json::Value`/string, the only persistence. Schema
(`"schema": "relay.session-export"`, `"version": 1`): session id (hex), peer `display_name` and
`fingerprint` (hex), `exported_at_ms`, `messages[]` with `seq`, `direction`, `sent_at_ms`,
`received_at_ms`, `delivery`, `pinned`, and one of `text` (with parsed `code_blocks[]` of
language + text), `image` (media type, dimensions, byte length, and `data_base64` only when
`embed_images` is set), or `file_offer` (id, name, size, media type, blake3 hex, status,
optional preview text). Export never bundles downloaded files. The client decides where it is
written (a file dialog); the crate only produces text.

## Images

Inline images are compressed bytes counted toward the session budget (their length plus the
message overhead). Allowed: PNG, JPEG, WebP, GIF. Original bytes are kept: no re-encoding, no
transcoding. Limits, checked from the **header before any decode**:

- at most 8 MiB compressed;
- decoded size at most 16 megapixels (16 x 2^20 pixels, 64 MiB as RGBA) and each side at most
  16384. This is a deliberate deviation from the suggested 64 MP: 64 MP is 256 MiB of RGBA for one
  image, which one hostile peer could multiply by several simultaneous previews, and 16 MP covers
  every phone and screen capture. Larger images are sent as file offers;
- animated (GIF, animated WebP, APNG): at most 100 frames and `frames x w x h x 4` at most
  128 MiB. The inline preview is the static first frame; playing is optional and bounded by the
  same numbers.

Decompression bombs (a tiny file that inflates enormously) are stopped by the header check plus
a requirement on the client: decode with a hard allocation limit (for the `image` crate,
`Limits::max_alloc`) and a decode deadline. Thumbnails are a separate bounded cache owned by the
client, never by this crate; evicting a thumbnail does not affect the transcript.

## File offers

A file offer is a chat message carrying **metadata only**: no bytes travel in chat.

Offer: `id`, sanitized display name, size, advisory media type (guessed from the extension,
never trusted), BLAKE3 hash computed by the sender over a snapshot (the sender hashes while
copying, or re-hashes a stable copy; if the file later differs, the transfer aborts), and an
optional text preview.

File names are sanitized by both sides: the last path component only (both separators), controls
and format characters removed, `<>:"|?*` replaced with `_`, leading dots and spaces and trailing
dots and spaces trimmed, Windows reserved names (`CON`, `NUL`, `COM1` ...) prefixed with `_`,
at most 120 bytes keeping the extension, never empty (fallback `file`). The receiver chooses
the destination directory; the name only suggests the final file name. A helper flags
executable-looking extensions so the UI can warn; nothing is ever executed or opened.

Preview: only when the receiver sends a `PreviewRequest` **and the sender's user approves** it.
At most 16 KiB and 100 lines, valid UTF-8, passing text sanitization; counts toward the session
budget. Never automatic.

### Transfer state machine

```
Offered --reject--> Rejected
Offered --accept--> Accepted --first chunk--> Transferring --all bytes--> Verifying --hash ok--> Completed
          any non-terminal state --cancel--> Cancelled     any failure --> Failed(reason)
```

- At most **2 concurrent transfers per session** (Accepted, Transferring, Verifying). A third
  `accept` is refused with `TooManyTransfers`; the offer stays `Offered`.
- Chunks are at most 64 KiB, strictly in order (`offset` must equal bytes received), inside a
  receiver-granted window (1 MiB credit, extended as the receiver consumes). No resume in v1:
  any interruption fails the transfer and the user re-offers.
- Receiver: checks free space via `FileSink::free_space` before `Accept` (size plus 10 percent
  margin or 16 MiB, whichever is larger); BLAKE3 hashes chunks as they arrive; the sink contract
  is *write to a temp name in the destination directory, fsync, verify the hash, atomic rename
  without overwriting*. An existing file is never replaced: the sink picks a unique name
  (`name (1).ext`). On mismatch, short or long data, or any sink error the temp file is removed
  (`abort`) and the transfer fails; **the file becomes visible only after verification**.
- Sender: emits `ReadChunk` effects, honours the window, and uses a `CheckSource` hook (before the
  first chunk and before finishing) where the engine stats/re-hashes the source; a changed source
  aborts with `SourceChanged`.
- A retained offer is metadata only. Once the session ends its status becomes `Expired`
  (or `Cancelled` if a transfer was running) and **Download is unavailable**; the history only
  shows that a file was offered.

### Permissions

Per session: `allow_messages` (text) and `allow_files` (images and file offers, previews and
transfers). The initiator is the host in a 1:1 call and owns the toggles. **Both daemons
enforce them**: a receiver rejects kinds that are off, regardless of the sender's UI, and a
sender will not compose them. Turning a permission off stops new items; it does not erase
existing ones. Turning `allow_files` off also cancels running transfers and refuses new accepts.

## Traffic classes

`TrafficClass`, highest priority first: `Voice` > `Video` (unused) > `Signaling` (small interactive
messages, acks, offers, transfer control) > `Preview` > `BulkFile`. Bulk bytes are paced
(default cap 16 MiB/s per transfer, configurable) and always the lowest priority. Recommendation
for relay-net: each transfer runs on its **own encrypted data link** (new TCP connection and
Noise handshake pinned to the same peer fingerprint, identified by the transfer id and
authorised by the accepted offer), so a saturated file link can never head-of-line block call
signaling on the control link. Chat messages, images (at most 8 MiB, treated as `Signaling`
only up to 64 KiB, larger as `Preview`) share the control link under the same class rules. `relay_net::transfer`
implements it; `relay-messages` only exposes the enum and pacing constants.

## Security rules

No auto-download, auto-execute, auto-open or auto-preview. File names sanitized. Hash verified
before a file is visible. Offers limited per session and per second. The crate has no I/O,
clocks or randomness. Secrets and message bodies are never logged: bodies are redacted in
`Debug`; use `Redacted` for log lines. All inputs are bounded before they are stored.

## Client rule: Send Clipboard to Call

The client must show the **destination call** (peer name and call id) before sending clipboard
contents, and require an explicit action (a button press) every time; there is no hotkey that
sends without confirmation. With more than one call it must require choosing the call (no
default). Clipboard text goes through the same sanitization and limits as typed text.

## Wire messages (peer.proto, implemented in relay-net)

`Envelope.session_message = 21` (20 is `channel`) carries `SessionMessage`; the engine handles it on the
control lane and call state ignores it. The definitions are in
`crates/relay-net/proto/relay/peer/v1/peer.proto`; validation is `relay_net::control::session_msg`.
Differences from the earlier draft:

- `ChatImage` data and `PreviewReply.text` are capped at **15 KiB**, not 8 MiB / 16 KiB: a control
  record is 16 KiB. Larger images go as file offers (the client can render a verified image file).
- `TransferControl` has `data_link` (a `DataLinkGrant`: 32-byte token, port, ttl), `result` and
  `cancel`. The draft's `credit_up_to` and `done` moved to the data link (`Credit` and `Done`
  frames), because the `Receiver` state machine enforces its credit window and the data link is the
  only place chunks flow. `result` and `cancel` exist on both links (control copy for before the
  link exists or after it closed). Enums `TransferResult` and `CancelReason` are top level.
- Seq must be >= 1; `ChatAck.status` and enums outside the declared range are refused.
- A refused preview (`approved = false`) must not carry text. Offer names are bounded at 255 raw
  bytes on the wire (honest senders send at most 120 sanitized bytes), media types at 127.
- Per-link flood guard: session messages beyond 4x the per-sender limit close the link (below that,
  `relay-messages` answers `RateLimited` acks).

## Data link for file bytes

Implemented in `relay_net::transfer` (details and tests in `docs/net.md`). Offer and accept travel
on the control link as before. After `Accept`, the receiver (listener) calls `TransferHub::expect`
and sends `TransferControl.data_link`; the sender dials with the token, and chunks, credit, `Done`
and the verification `Result` flow on that link as `TransferMsg` values. The token is single use,
expires in 30 s, and is bound to offer id, session id and the peer's fingerprint. Rate: 16 MiB/s
per link by default, adjustable at runtime, plus an optional hub-wide cap; at most 8 links.

### API the engine needs

```rust
// relay_net::transfer
TransferHub::bind(addr, Arc<Identity>, HubConfig) -> Result<TransferHub, DataError>
hub.local_addr(); hub.open_links(); hub.pending_tokens(); hub.set_global_rate(Option<u64>)
hub.expect(Expectation { offer_id: [u8;16], session_id: [u8;16], peer: Fingerprint,
                         role: TransferRole, size: u64 }, ttl: Option<Duration>)
    -> Result<(TransferToken, PendingLink), DataError>   // token.as_bytes() -> DataLinkGrant.token
PendingLink::wait(self) -> Result<DataLink<TcpStream>, DataError>   // Expired on timeout/revoke; drop revokes
hub.dial(SocketAddr, &Fingerprint, &Ticket { session_id, offer_id, token }, TransferRole, size)
    -> Result<DataLink<TcpStream>, DataError>
DataLink { tx: DataTx<_>, rx: DataRx<_> }   // fields are public; split by moving them into tasks
dataLink.close_handle() -> CloseHandle      // close() ends both halves, even mid-send
dataLink.peer() -> &Fingerprint
DataTx::send(&TransferMsg), DataTx::pacer().set_rate(Option<u64>), DataTx::shutdown()
DataRx::recv() -> Result<TransferMsg, DataError>   // Closed on EOF; every error is terminal
// control: new_envelope(Body::SessionMessage(msg)), control::session_msg::validate(&msg),
// ControlReceiver::set_session_limit(SessionRateLimit)
```

Engine wiring: map `Effect::Send(TransferMsg::Chunk | Done)` (sender) and `Credit | Result`
(receiver) to `DataTx::send`; feed `DataRx::recv()` results to `Sender::on_message` /
`Receiver::on_message`; `Accept`, `Reject`, `Cancel` also go over the control link
(`OfferResponse`, `TransferControl.cancel`). Run each link in its own task; on cancel or session
end call `CloseHandle::close()`. Use `TrafficClass::BulkFile` pacing: lower `set_rate` while a call is
active. Known gaps: the engine must generate offer/session ids, map `ChatAck` and rate limits, and
decide the listener address (the control link's remote IP plus the granted port); no resume; no
Windows QoS marking; the hub's own listener port must be reachable (firewall).

## Local API (relay-api, implemented)

The contract is in `docs/api.md` ("Session messaging") and `crates/relay-api/proto`. Differences
from the earlier draft, all decided during integration:

- One new capability, `messages`, for every method and for the event stream; it is separate from
  `observe` and `call-control` and not implied by `admin`. Mutations need only `messages`.
- Methods address a **session** (`session_id`, 16 bytes) not a call id: a session starts with a
  call and its retained transcript outlives it. `ListSessions` maps sessions to calls and peers.
- `SearchMessages`/`ClearHistory`/`ExportHistory` became `SearchSession`/`ClearSession`/
  `ExportSession`; `CopyCodeBlock` is client-side (the engine returns the exact stored text).
  `SetMessagingSettings` changes the live preferences. `GetTranscript` pages by arrival ordinal
  and is the resync path for `WatchSession`.
- `OfferFile` takes an absolute path that the **daemon** opens (same machine, same user); see
  `docs/api.md` for the rules. `RespondOffer` takes the destination directory the client chose;
  the daemon sanitizes the name and never overwrites.
- `ExportSession` returns the JSON and writes nothing; only an explicit `destination_path` makes
  the daemon create a (new, owner-only) file.
- Inline images are limited to 15 KiB by the lane; the engine keeps the bytes and does not decode.
- The error vocabulary is the `Problem.code` list in `docs/api.md`, mapped from `StoreError`,
  `TransferError` and the file checks.

The daemon, not the client, owns transcripts, so a CLI and the desktop UI see the same session.

In the desktop UI a session is shown as part of its call, not as a page: the call card's Messages button opens the chat of the live call, and a call-history row's Messages button opens that call's transcript read only. The UI maps a history entry to its session through `call_id` (`SessionSummary.call_id`, hex in the UI model). Because messages are memory-only, after a daemon restart or an eviction the session is gone and the row says so; see `docs/ui.md`, "Session messaging UI".

## Engine decisions

- Sessions are keyed by peer: at most one per peer; a message is routed by the authenticated
  link's peer and can only reach that peer's session. The data-link session id is the call id.
- The guest may change its own permission pair; the host's `PermissionUpdate` overwrites it.
- Transcripts are cleared on daemon exit and idle exit (RAM only); there is no other persistence.

## Open questions

- Whether a guest's own permission change should survive a later host push (today the host's push overwrites it).
- Video class is reserved; no behavior until a video decision exists.
