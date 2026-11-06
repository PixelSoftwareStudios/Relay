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

## Local message times use `jiff`

Status: accepted.

Message times were rendered in UTC because the app had no time-zone data. They are now shown in the
viewer's local zone with `jiff` (`relay_io::localtime::LocalZone`), a small, maintained, pure-Rust
crate. The data stays UTC Unix milliseconds on the wire, in `--json` and in the hover text.

Why `jiff` and not the alternatives: `time` can only read the local offset when the process is
single-threaded on Unix (`local-offset` refuses otherwise, and it applies one offset to every
instant, so daylight saving is wrong for old messages); both Relay apps are multi-threaded.
`chrono` needs `chrono-tz` or `iana-time-zone` pieces to match. `jiff` reads the system zone
(`TZ`, `/etc/localtime`, the Windows registry), applies the rules per instant, bundles the zone
database only on platforms that have none, and takes an explicit zone value, so tests inject a
fixed offset or a named zone instead of depending on the host.

## Noto Sans is the UI font, with subset fallbacks

Status: accepted.

The UI bundles Noto Sans plus Noto Sans JP, KR, SC and Devanagari (`assets/font`, loaded by
`relay_ui::fonts::install`). egui's default fonts cover Latin and little else, and user-supplied
display names and messages arrive in any script, so missing glyphs would show as boxes. Noto is the
one family that covers all nine planned UI languages (English, Spanish, German, Russian, Brazilian
Portuguese, Korean, Japanese, Simplified Chinese, Hindi) with one look, under the SIL OFL 1.1, which
is compatible with shipping inside an Apache-2.0 application when the licence text travels with the
fonts.

Subset policy: full Latin, Latin Extended, Greek, Cyrillic and Devanagari; for CJK, the common set
only (JIS X 0208 level 1 plus Joyo for Japanese, GB2312 for Simplified Chinese, KS X 1001 Hangul
syllables and Jamo for Korean) plus every non-ASCII character in UI strings. Full CJK fonts are
4 to 16 MB each; the subsets total about 3.1 MB. Regular weight only: egui synthesises no bold or
italic, so extra weights would only be dead weight. Rarer characters fall back to egui's built-in
fonts or show as missing. Devanagari shaping needs no extra stack (see the next decision). Font order is per language (`fonts::order`). Build recipe and licences:
`assets/font/README.md`.

## Devanagari shaping comes from egui 0.37's harfrust, not from an extra stack

Status: accepted.

Finding: epaint 0.37 depends on `harfrust` 0.12 and shapes every text run in `text_layout.rs` (`shape_text`), with GPOS kerning, per-run font fallback, cluster-aware cursors and zero-width continuation glyphs. Earlier docs saying egui does no shaping described egui 0.27 and older. So conjuncts, matra reordering and reph already apply to labels, buttons, wrapped paragraphs, peer names, messages and editable fields; the bundled Devanagari subset keeps all its layout features. Tests (`crates/relay-ui/tests/shaping_*.rs`) shape क्षत्रिय, हिन्दी, कि and कर्म with harfrust on the bundled face and check egui galley widths and the Hindi catalog for `.notdef`.

Candidates (all MIT or Apache-2.0, so allowed by `deny.toml`): harfrust (MIT, HarfBuzz port on read-fonts, already in the tree through epaint); rustybuzz (MIT, ttf-parser based, superseded by harfrust); swash (Apache-2.0 or MIT, shaper plus rasteriser, used by older Linebender code); parley 0.12 (Apache-2.0 or MIT, MSRV 1.88, layout over fontique, harfrust and ICU4X: bidi, line breaking, alignment); cosmic-text 0.19 (MIT or Apache-2.0, same role for COSMIC). Parley was evaluated first as requested. It is a layout engine that yields glyph runs, and egui has no hook to take them: `Galley` rows and glyph atlas are built inside epaint, no parley or egui-parley integration exists for 0.37 (web search found none), so using it would mean painting our own glyph meshes with a second rasteriser and atlas, re-implementing text editing, accessibility and wrapping for every widget, and its default `system` feature (fontique scanning installed fonts) would have to be disabled to stay bundled-only. That adds a large dependency tree and a parallel text path for zero shaping gain, since both end up in harfrust.

Decision: no new shaping crate and no wrapper module; `harfrust` is a dev-dependency only (same version as epaint) for the tests. Revisit parley only if egui gains a pluggable text backend or Relay needs bidi (Arabic, Hebrew), which egui 0.37 does not do.

## Desktop translations use Project Fluent, one folder per language

Decision: the desktop UI is translated with Project Fluent (`fluent-bundle` 0.16, `unic-langid`), catalogs in the repository-root `localizations/<lang>/<section>.ftl`, embedded with `include_str!`, system language from `sys-locale`.

Why Fluent: the nine languages need real plural rules (Russian has three forms, Japanese and Chinese one) and grammatical agreement, which only a message format with selectors handles; code must not assemble sentences. `rust-i18n` (macro lookup over YAML, JSON or TOML) has no plural or gender support, keys-as-text by default and build-time macros that read files from the crate directory, so it was rejected. `fluent-templates` wraps `fluent-bundle` with a build-time static loader that expects one directory tree through its own macro and `walkdir`, bringing nothing we need and fighting the section layout and `include_str!` embedding; `fluent-bundle` directly is the same engine, maintained by Mozilla, with a small API. The concurrent bundle is `Sync`, so one static serves the UI thread, the tray and the notification thread.

Per-frame cost: strings are read every frame, so an argument-free message is formatted once per language, leaked into a `&'static str` and served from a map (bounded by the catalog size, no allocation after the first use). Messages with arguments are formatted per call and returned as `String`; they are the minority and were already `format!`ed before.

Layout: one folder per language so translators work in parallel without merge conflicts, split into ten section files so no file passes 500 lines. English is the reference: tests require every other folder to have the same files, no extra ids, matching `$variables` and each id in exactly one section; missing messages fall back to English per message. Ids are semantic keys, never English text, so rewording English does not invalidate translations.

Settings and fonts: the choice is stored in `desktop.toml` like the other desktop-only preferences and applied live; `fonts::order` puts the language's own CJK face first (shared Han glyphs differ per region) and the fonts are reinstalled on change. Hindi is flagged experimental because egui does not shape Devanagari. Daemon and engine text is shown as received. Translations are machine-written and gated on native-speaker review (`docs/gates.md`).

