# Decisions

Short records of choices that changed or are easy to forget. Newest first.

## Preferences moved from TOML to SQLite

Status: accepted. Supersedes the earlier design choice of plain TOML files for preferences and trust.

The first design would have kept `config.toml` and `trust.toml`, written atomically with a `.bak` copy. That
was readable and hand-editable, but it grew into three separate persistence mechanisms (preferences,
trust, avatar files), each with its own crash-recovery code, and the planned channel feature needs
transactional multi-row state (memberships, invites, issued certificates) that does not fit
whole-file rewrites.

Now all persistent settings live in one SQLite database per OS user, in that user's data directory,
owner-only (0700/0600). Reasons:

- One crash story: WAL with `synchronous=FULL`, transactions instead of temp-file swaps, a single
  `quick_check` and snapshot-based recovery for everything.
- Versioned migrations with `PRAGMA user_version`, and a clean place to add the channel tables.
- Bounded avatar cache with eviction in one transaction instead of directory scans.
- Several users of one PC cannot touch each other's data; the file is created private.

Trade-offs accepted:

- The database is **not hand-editable**. Humans and support get `export` and `import` as TOML
  (preferences only, strict about unknown keys) plus `get`/`set` helpers for the CLI.
- A binary file is harder to inspect and diff; the redacted diagnostics bundle and `export` cover
  the support cases.
- A bundled SQLite adds build time and a C dependency (`rusqlite` with the `bundled` feature) in
  exchange for no system SQLite version surprises.
- Corruption now means "restore the last snapshot" rather than "re-read the previous file", with the
  same user-visible notices and the same safety rules (microphone cleared and receive-only after any
  recovery, no silent trust loss, newer schema refused).

Unchanged: secrets stay out of the database. The identity private key remains in the OS credential
store (or the explicit INSECURE file option), and any future channel secrets follow the same rule.
Relay had not been released when this changed, so there is no import path or compatibility code for
the TOML layout; the SQLite schema starts at version 1.

## Session-scoped ephemeral messaging is in scope

Status: accepted.

Relay may carry text messages, inline images and file offers **between the two ends of an active
call session**. This is a companion to voice, not a chat platform: no accounts, rooms, offline
delivery, server-side history, read receipts or remote deletion. Transcripts live in RAM only
(64 MiB per session, 256 MiB across ended sessions), and the only persistence is an explicit user
export. Files move only after the receiver accepts a metadata offer, over a lowest-priority
data link that must never starve call signaling.

Reasons: people on a call routinely need to hand each other a command, a log snippet, a
screenshot or a file, and without it they leave the tool for a cloud chat, which defeats a
LAN-only product. Keeping it session-scoped and ephemeral keeps the privacy and storage surface
small.

Still out of scope: screen sharing and video (the traffic class is reserved but unused),
persistent chat history, group rooms, link previews and anything that auto-downloads, auto-opens
or executes received content. Details in `docs/session-messages.md`.
