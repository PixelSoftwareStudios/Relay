# Decisions

Short records of choices that changed or are easy to forget. Newest first.

## Decision: background availability and auto-answer

Status: accepted (engine and settings built; the Android service, the desktop tray and the UI are the
shells' work, see `docs/android.md` and `docs/desktop.md`).

Two features share one aim: a device can stay reachable and answer without anyone touching it.

- **Background availability** (`background.available`, default on). The daemon already advertises and listens
  while a client is attached; the setting asks the OS shells to keep a client attached (Android foreground
  service, desktop tray, a CLI-started daemon) while no window is open and the device is on the LAN. The engine
  only stores and publishes it; it starts no service itself.
- **Call alert** (`calls.alert`: `system` default, `ring_and_vibrate`, `vibrate`, `silent`). `system` follows the OS
  or phone profile (ringer mode, Do Not Disturb); the others override it. The engine ringtone player rings for
  `system` and `ring_and_vibrate` only; vibration belongs to the Android shell. On a desktop `vibrate` is silent.
- **Auto-answer**, per contact (`SetPeerAutoAnswer`), plus a global switch `calls.auto_answer_enabled` (default on).

Threat model of auto-answer. Answering opens a call, which can open the local microphone, for someone other than
the person at the device. What it must not allow: an attacker, a stranger, a device that merely has the same name,
or a once-trusted contact whose key changed getting an open line. What it gives an attacker who does hold a
verified contact's key (a stolen unlocked device): a call that connects without a tap, in the user's own default
call mic state, announced and audible.

Controls, all enforced in the engine (`service/actor_auto.rs`), none left to a UI:

1. Opt-in per contact, off by default, and only for a contact stored as `verified` (the safety code was compared out
   of band). A TOFU contact cannot be switched on (the store refuses) and a TOFU label is ignored at call time.
2. The flag is a row that references the contact's trust row, so forgetting, blocking or replacing the pinned key
   deletes it in the same statement. A key-changed device is a different identity and never matches.
3. Checked twice, when the call starts ringing and again when the delay is over: the global switch is on, the
   caller is flagged and labelled verified, the link is the authenticated one for that key, no other call is live,
   and a client is attached to own the call. Anything else keeps ringing as today.
4. Not a bypass of privacy: the inbound policy, hidden advert mode and blocking act before a call can ring, so
   a device that is private or not accepting calls refuses as it always did.
5. A fixed 1.5 s delay with the ring and notification running, so the local user can decline first (a decline in
   that window wins).
6. The call connects like an accepted one: the mic state is `calls.default_transmit_mode` (push-to-talk by
   default), never forced open and not a separate mode.
7. Audible and visible: the connect chime plays even when call sounds are off, `Call.auto_answered` stays true for
   the life of the call and an `AutoAnswered` state event is sent once, so shells show a persistent indicator and
   a notification.
8. Kill switch: `calls.auto_answer_enabled = false` ignores every flag at once (flags are kept), live.

Addendum (engine-held presence). The first cut said the engine only stores `background.available` and a long-lived
client keeps the daemon reachable. That left a CLI-started `relayd` with no client doing nothing, against the
requirement that on desktop it works "if the app is in the tray or if the daemon was started from the CLI". Now the
engine holds the presence itself: with `background.available` on and no watch, a reserved client `ClientId::HOUSE`
counts as attached. Consequences: the daemon advertises, listens, rings and auto-answers headless; it does not
idle-exit while the setting is on (an autostarting CLI therefore leaves a daemon running, and its stderr line says so;
`background.available false` restores the 300 s exit and the old client-held rules). `HOUSE` owns a headless
auto-answered call, and any client may take its lease without an explicit takeover, so a TUI or app that attaches can
hang up, mute or push to talk. A headless `relayd` shows no notification (no shell): it rings and the call is in the
history. Hidden mode, the block list and the inbound policy act before a call can ring and are unchanged. Also added:
`SetClientQuiet` (API revision 7), so a shell that alerts calls itself (Android, where the engine cannot see the
silent or vibrate profile) can keep the engine ringtone silent while it is the only attached client.

Not done: the call history does not mark auto-answered calls (it would need a column; the live `Call` carries the
flag). The first ring of an auto-answered call is also what wakes a phone; that part is the shell's.

## Decision: sonora

Status: accepted. Replaces the earlier plan to pick a noise-suppression backend from a speech corpus (the
`nnnoiseless` and `eval-sonora` / `eval-nnnoiseless` scaffolding is removed; the measurements stay in
`docs/evidence.md` as history).

`sonora` (pure-Rust port of the WebRTC audio processing module, BSD-3-Clause) is the single processing
stage of the capture path. Reasons:

- One crate gives noise suppression, AGC2 and AEC3 behind one 10 ms frame interface, so the stages
  cannot double-process and speaker mode (WP-12) needs no second library.
- Pure Rust: no C build, no model file to vendor, hash or license-review, nothing downloaded at call time.
- Cheap: well under a tenth of a core for a 10 ms frame, and about 7 ms (334 samples) of added delay
  with noise suppression on, none for gain control alone (measured in `relay-audio` `tests/sonora.rs`).

Shipped behaviour: noise suppression on at moderate strength, automatic gain (AGC2) off, echo
cancellation off and not user-selectable. With everything off the stage is the plain copy, so a
disabled stage costs nothing and is bit-exact. Settings are `audio.noise_suppression`,
`audio.noise_suppression_level` (`low`, `moderate`, `high`) and `audio.auto_gain`, live.

Caveats, stated so they are not forgotten:

- No measured intelligibility gain. On the corpus sonora did not raise STOI/eSTOI at low SNR the way
  the neural candidates did (`docs/evidence.md`); it removes steady background noise, which is what
  most calls need, and the neural candidates remain a possible later upgrade behind the same contract.
- sonora allocates on every frame (1 to 40 allocations measured). It therefore never runs in a device
  callback: it runs on the capture worker thread (`relay-capture`, promoted through the `priority`
  module), which reads the callback's bounded ring. An overrun drops audio in the callback and is
  counted, a late poll drops the stale batch; neither blocks the callback.
- Default-on is provisional until a listening test on real devices (`audio-corpus listen`, built-in
  and USB microphones, a noisy room) says moderate noise suppression does not make voices thin.

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

Decision: the desktop UI is translated with Project Fluent (`fluent-bundle` 0.16, `unic-langid`), catalogs in the repository-root `localizations/<lang>/<section>.ftl`, embedded by `rust-embed` (every `*.ftl` is discovered at build time, no list of sections in code), system language from `sys-locale`.

Why Fluent: the nine languages need real plural rules (Russian has three forms, Japanese and Chinese one) and grammatical agreement, which only a message format with selectors handles; code must not assemble sentences. `rust-i18n` (macro lookup over YAML, JSON or TOML) has no plural or gender support, keys-as-text by default and build-time macros that read files from the crate directory, so it was rejected. `fluent-templates` wraps `fluent-bundle` with a build-time static loader that expects one directory tree through its own macro and `walkdir`, bringing nothing we need and fighting the section layout and `include_str!` embedding; `fluent-bundle` directly is the same engine, maintained by Mozilla, with a small API. The concurrent bundle is `Sync`, so one static serves the UI thread, the tray and the notification thread.

Per-frame cost: strings are read every frame, so an argument-free message is formatted once per language, leaked into a `&'static str` and served from a map (bounded by the catalog size, no allocation after the first use). Messages with arguments are formatted per call and returned as `String`; they are the minority and were already `format!`ed before.

Layout: one folder per language so translators work in parallel without merge conflicts, split into ten section files so no file passes 500 lines. English is the reference: tests require every other folder to have the same files, no extra ids, matching `$variables` and each id in exactly one section; missing messages fall back to English per message. Ids are semantic keys, never English text, so rewording English does not invalidate translations.

Settings and fonts: the choice is stored in `desktop.toml` like the other desktop-only preferences and applied live; `fonts::order` puts the language's own CJK face first (shared Han glyphs differ per region) and the fonts are reinstalled on change. Hindi is flagged experimental until its on-screen rendering and translation are reviewed. Daemon and engine text is shown as received. Translations are machine-written and gated on native-speaker review (`docs/gates.md`).

Loader and macro, compared with Flare (Signal client for GTK/Rust, gettext through `gettext-rs`): Flare keeps the language list in `po/LINGUAS`, the translatable files in `po/POTFILES` and lets Meson compile `.po` files; adding a language is a data file, not a code edit. We take that property: sections are discovered by `rust-embed` (maintained, embeds a folder in release and reads it from disk in debug, which is the live reload for free; `include_dir` was the alternative but has no debug reload and a `build.rs` generator would be hand-rolled code), so a new section file is a file drop. Languages are not scanned from the folders: the picker order (English first, then by English name) and native names are data that folder names cannot carry, so one `Language` entry stays in `lang.rs`, and a test fails if the folder list and that list differ. Flare relies on gettext call-site functions (we found no custom macro); our `tr!` is the only lookup everywhere, and `i18n_keys` is the compile-time-equivalent check that every literal id exists in English and none is unused (a proc-macro check was judged not worth a new crate).

## Android QR scanning is CameraX plus ML Kit's bundled model

Need: a Scan button under the QR code (People page) that reads the other device's `relay://` connection string. The scanned text takes the same check as a pasted string (`relay_client::connection::check`), only fills the address field, and never trusts or dials by itself.

Options compared (versions as pinned or documented at the time of writing):
- Google Code Scanner (`play-services-code-scanner`): smallest code (no camera screen of our own) and no `CAMERA` permission, because Google's scanner UI runs in Play services. It needs Google Play services on the device and downloads its module on first use, so it fails on devices without Play services and offline on first use. Rejected as the primary path for a LAN-only app that should work with no internet and no Google account.
- zxing-android-embedded: no Google dependency, but the Android wrapper is lightly maintained (ZXing core is in maintenance mode), the decoder is less robust on small or low-contrast codes and it ships its own activity and theme to override. Rejected.
- CameraX 1.4.2 plus ML Kit `barcode-scanning` 17.3.0 (bundled model): works offline and without Play services (the bundled variant carries the model in the APK; the unbundled variant is the one that needs Play services), both libraries are actively maintained by Google, and the decoder is the same one Code Scanner uses. Cost: about 6 MB uncompressed per ABI in the APK (`libbarhopper_v3.so` 4.9 MB plus about 0.9 MB of models, measured in the arm64 debug APK), the `CAMERA` permission (asked on the first tap of Scan, never at startup) and about 160 lines of our own scanner screen (`ScanActivity.kt`).

Decision: CameraX plus bundled ML Kit. The cost is modest, so no Play-services fallback path is kept. The camera permission has its own `PermissionGate` (separate from the microphone's); a refusal that the system will repeat is "denied" (tap Scan again), a permanent one is "blocked" and the UI offers Open system settings. The shared UI sees only the platform-neutral `Shell::can_scan_qr` / `scan_qr` / `open_camera_settings`.

## Icons are the Phosphor font, embedded and subset, with our own codepoint table

Need: the UI had plain-text buttons for universal actions (copy, send, close, more, star). Icons must render on every platform the same, scale with the text, take the theme colours, work with egui 0.37 and cost almost nothing in size.

Options compared:
- `egui-phosphor` 0.14.0 (MIT or Apache-2.0, Phosphor Icons MIT): the best-kept egui wrapper and the glyph set we want, but it depends on egui 0.36, so it would pull a second egui into the tree (its releases trail egui by a few weeks), it ships five full 450-530 KB fonts, and registers them into the Proportional family where glyphs fall through text fonts.
- `egui_material_icons` (Material Symbols, Apache-2.0): same wrapper idea, a variable font of about 3 MB without subsetting, and its glyphs are rounder and heavier than the rest of the UI.
- Lucide font (ISC): fine licence, but it lacks filled variants, which a favourite star needs.
- Phosphor font bytes embedded by us (chosen): MIT, 1,500 icons in six weights with matching codepoints, a regular and a fill weight with the same codepoints. `assets/font/subset-icons.py` keeps only the glyphs listed in `assets/font/icon-glyphs.txt`: 16.7 KB regular plus 1.3 KB fill. Codepoints are a `match` in `crates/relay-ui/src/icons.rs`, checked against the font by `tests/icons.rs`. Each face has its own egui family (`icons`, `icons-fill`), so an icon never falls back to a text font and a text glyph never lands in the icon font.

The glyph source for the subset is the `res/` folder of the egui-phosphor 0.14.0 crate (the unmodified upstream Phosphor TTFs); nothing from that crate is linked. Licence: MIT, text shipped as `assets/font/LICENSE-phosphor.txt` next to the OFL files (`docs/release.md`); it is not a crate, so `cargo deny` does not see it and `deny.toml` needs no change.

Rules: an icon stands alone only where its meaning is universal (mute, send, close, back, search, copy, more, star); every icon-only control has a tooltip that is also its accessible name; actions whose meaning is not universal (Call, Hang up, Trust this device) keep a visible label beside the icon. Icon buttons are 44 points with touch style and 32 with a pointer.
