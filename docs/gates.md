# Gate register

Every exit gate of work packages WP-00 to WP-12, plus the cross-cutting release gates, with an honest
status. Update a row in the same change that produces its evidence, and record the evidence itself in
`docs/evidence.md` (hardware, review, measurement) or name the test here.

Test names are listed as `file::test` under the owning crate's `tests/` directory. They were read from
the repository, not re-run for this register; CI (`.github/workflows/ci.yml`) is written but has not run
yet.

## Status vocabulary

| Status | Meaning |
|---|---|
| met-by-test | An automated test in the repository asserts the gate. It still passes only on the platforms where it has run. |
| simulated | Exercised only with fakes, in-process loopback or a model; real devices or networks are not involved. |
| unmet-needs-hardware | Needs real devices, a second machine or another OS. Cannot be closed from one development host. |
| unmet-needs-review | Needs a named independent reviewer, or a written record that does not exist yet. |
| blocked | Cannot start until something else exists (a corpus, a candidate, a decision, another gate). |

A hardware or review gate is never marked met by a test that uses fakes.

## Work package gates

### WP-00 Workspace, toolchain, policy

| Gate | Status | Evidence / what is needed |
|---|---|---|
| Existing-behaviour inventory and decision diff reviewed | unmet-needs-review | `docs/decisions.md` records choices; there is no review record. Needs a reviewer sign-off in `docs/evidence.md`. |
| Headless build passes on each first-tier target | unmet-needs-hardware | Built and tested on macOS (Apple silicon) by the authors. Linux and Windows are covered only by the `headless` CI job, which has not run. Record the first green run per OS. |
| Lint, test and dependency policy enforced | simulated | `rustfmt.toml`, workspace lints, `deny.toml` and CI exist. Policy is enforced only once CI runs on a pushed revision. |

### WP-01 Domain, local API, fakes

| Gate | Status | Evidence / what is needed |
|---|---|---|
| Snapshot and watch deliver without gaps | met-by-test | relay-core `watch::every_revision_is_delivered_exactly_once_in_order`, `watcher_following_events_always_equals_a_fresh_snapshot`; relay-api `engine_api::snapshot_then_watch_loses_nothing_under_concurrent_writes`, `tracker::gap_is_reported_when_strict`. |
| Stale operations are rejected | met-by-test | relay-api `guard::stale_incarnation_is_rejected`; relay-engine `service_clients::takeover_invalidates_the_previous_controller`; relay-core `routing::stale_capture_completion_is_rejected_and_loss_needs_explicit_resume`. |
| Duplicate commands are idempotent | met-by-test | relay-core `operations::duplicate_request_returns_the_cached_result_without_reexecuting`, `start_call_retry_does_not_create_a_second_call`; relay-api `engine_api::duplicate_operation_returns_previous_outcome`. |
| A fake CLI and a fake GUI observe independent sessions without media | met-by-test | relay-engine `service_clients::two_clients_keep_independent_sessions`; relay-core `watch::independent_clients_see_the_same_independent_calls`. |

### WP-02 Local audio loopback

| Gate | Status | Evidence / what is needed |
|---|---|---|
| No callback allocation after initialisation | simulated | relay-audio `allocations::capture_callback_is_allocation_free`, `playback_callback_is_allocation_free`, `full_loopback_step_is_allocation_free` on the fake-device path. Real cpal threads are not counted (`docs/audio.md`). |
| One-hour loopback and device-removal recovery on qualified hardware | unmet-needs-hardware | Not run. Needs the run recorded with device models, OS and backend. |
| Measured latency distribution and failure counts, noise suppression off baseline | unmet-needs-hardware | Only stage microbenchmarks exist (`docs/evidence.md`); no mouth-to-ear or round-trip fixture. |

### Real-time audio review (docs/reviews/realtime-audio.md)

| Gate | Status | Evidence / what is needed |
|---|---|---|
| Audio captured before the gate opened is never sent (RT-02), including after a worker stall and when the mode (which is the consent) or the default mode is applied | met-by-test | relay-engine `media_arming` (four tests, including `a_press_during_a_worker_stall_does_not_release_the_backlog` on the real capture worker). Residual: a close/re-open inside one 2 ms poll interval is not detectable without a gate epoch from relay-core. |
| A panicking or hung media worker is contained and reported (RT-01) | met-by-test | relay-engine `media_supervision` (panicking suppressor, restart limit, watchdog). Render-worker panics are covered through the shared supervisor, not with a panicking decoder. |
| No false "audio stopped" warning while idle or after the phone slept | simulated | relay-engine `media_watchdog` (idle, frozen watchdog, active hang). Needs a phone: leave Relay idle with the screen off for 10 minutes, wake it, and check no warning appears; then start a call and confirm audio. |
| Worker priority promotion (RT-03) | unmet-needs-hardware | Implemented in-tree (`relay-audio::priority`, no `audio_thread_priority`, cpal `realtime` off). Verified on macOS: policy unit tests with a fake OS, and a test that reads the time-constraint policy back with `thread_policy_get` and sees it removed on drop. Windows (MMCSS) and Linux (`SCHED_RR`, rtkit, nice) paths only type-checked and clippy-clean. No wake-jitter or glitch measurement on any OS. |
| No per-packet allocation in relay-net send/receive (RT-10) | met-by-test | relay-net `media_alloc`. |
| UDP send never blocks the capture thread (RT-04) | simulated | `try_send_audio` plus `media_capture_faults::a_full_socket_buffer_drops_the_packet_and_counts_it`; a congested real socket was not provoked. |
| Playout target follows the device callback size and a stall is shed (RT-06, RT-07) | simulated | relay-engine `media_sink_adapt` (event-ordered simulation, exact clocks). No device measured. |

### WP-03 Opus, RTP, jitter, drift

| Gate | Status | Evidence / what is needed |
|---|---|---|
| Reordering, wrap, loss, DTX-style gaps | met-by-test | relay-protocol `jitter_buffer::reordered_packets_within_margin_play_in_order`, `survives_sequence_and_timestamp_wrap`, `single_loss_is_concealed_then_resumes`, `deliberate_gap_is_silence_not_loss`; `jitter_props::invariants_hold_under_chaos`; `seq_extension.rs`; `rtp_oracle.rs` (RFC 3550 reference model). |
| Simulated drift at +-100 and +-300 ppm | met-by-test | relay-protocol `drift_sim::converges_for_each_simulated_drift`, `saturates_instead_of_chasing_excess_drift`. Simulation only; hardware clock behaviour is unmeasured. |
| Late packets cannot grow playout delay past the bound | met-by-test | relay-protocol `jitter_buffer::late_packet_is_dropped_and_does_not_grow_delay`, `stall_burst_keeps_delay_bounded`. |

### WP-04 Discovery, trust, secure transport

First contact is trust on first use: a call to or from a device that is not a contact pins its key as `UnverifiedFirstUse` when the call is answered. A LAN attacker who is present at that first contact can impersonate the device; nothing after it detects that. Comparing the fingerprint or scanning the QR code upgrades the contact to `Verified`. Tests: relay-engine `peers_call_trust`, `service_call_trust`, `peers_block`.

| Gate | Status | Evidence / what is needed |
|---|---|---|
| No microphone starts on discovery, handshake or call acceptance alone | met-by-test | relay-core `routing::receive_only_by_default_and_no_capture_before_consent`; relay-engine `media_end_to_end::a_call_carries_audio_both_ways_only_after_consent_and_stops_on_withdrawal`; relay-cli `engine_e2e::call_sends_the_peer_id_and_never_a_microphone_grant`. |
| Malformed or authentication-failed records do not change trusted state | met-by-test | relay-net `control::tampered_record_is_rejected_and_closes_the_channel`, `media::injected_garbage_and_replays_do_not_disturb_live_media`, `media::every_single_byte_flip_is_rejected`, `fuzz.rs` property tests; relay-engine `peers_trust::nothing_is_persisted_by_observation_only`. |
| Real mDNS discovery between two hosts | unmet-needs-hardware | Live test is opt-in (`cargo test -p relay-net --test discovery -- --ignored`); no multi-host run is recorded. |
| Independent security review of the Noise composition, nonces, replay and epoch handling | unmet-needs-review | `docs/net.md` states the composition is a proposal, not an audited protocol. Distributed binaries are blocked on this. |
| Fuzzing of untrusted parsers beyond property tests | unmet-needs-review | Eight cargo-fuzz targets in `fuzz/` each ran 4 minutes on nightly (6.6 M to 102 M executions, no crashes; `docs/evidence.md` Fuzzing). That is a smoke run, not a clean bill of health: corpora were still growing for the envelope, channel and avatar targets. Needs long runs (hours) and CI scheduling. |

### WP-05 First two-device call

| Gate | Status | Evidence / what is needed |
|---|---|---|
| Paired two-device call works without external services | unmet-needs-hardware | In-process end-to-end exists (relay-engine `media_end_to_end`, `peers_links`), which is simulated. A run on two real machines is needed. |
| Hangup, mute, consent withdrawal, control reconnect and daemon exit have documented outcomes | simulated | Behaviour is tested: `media_end_to_end::losing_the_microphone_pauses_sending_and_waits_for_the_user`, `service_daemon::shutdown_ends_open_streams_and_removes_the_socket`, desktop `supervisor::daemon_restart_reconnects_and_engine_resync_request_is_honoured`. A single consolidated outcome table is still to be written. |
| Full-chain latency compared to local loopback; no unsupported claim | unmet-needs-hardware | No measurement. Claims stay out of docs until `docs/evidence.md` has the run. |

### WP-06 Multi-call routing and input authority

| Gate | Status | Evidence / what is needed |
|---|---|---|
| Canary audio shows no path forwards one call into another | met-by-test | relay-engine `media_canary::each_call_hears_only_its_own_tone_on_a_hostile_network`, `a_packet_relabelled_for_another_call_is_rejected`, `wrong_generation_source_or_ssrc_is_dropped`, `concealment_and_silence_never_invent_audio_for_an_idle_call`. Uses fake devices; DSP and encoder histories on real audio are not covered. |
| Held PTT cannot survive disconnect, takeover or route-generation change | met-by-test | relay-core `ptt::held_ptt_does_not_survive_client_disconnect`, `held_ptt_does_not_survive_takeover_and_old_press_never_revives`, `held_ptt_does_not_survive_route_generation_change`, `held_ptt_does_not_survive_lock_suspend_or_device_loss`; relay-engine `service_gate::takeover_drops_the_previous_holders_press`, `a_held_press_opens_the_gate_and_closing_the_stream_closes_it`. |
| Resource admission protects existing calls | met-by-test | relay-engine `media_admission::refusing_a_second_call_leaves_the_running_call_untouched`, `an_already_overloaded_worker_admits_nothing_new`. Load numbers are modelled, not measured on target hardware. |

### WP-07 Noise-suppression selection

| Gate | Status | Evidence / what is needed |
|---|---|---|
| Choose one default from recorded quality, latency and resource evidence, or ship bypass | blocked | Superseded by "Decision: sonora" in `docs/decisions.md`: sonora noise suppression (moderate) is the default, pending a listening test on real devices. A real-speech comparison existed for nnnoiseless, sonora and the neural candidates GTCRN, DTLN and FastEnhancer t/s/b/m/l (`docs/evidence.md`, 2026-11-05 neural section): DTLN, GTCRN and FastEnhancer-s/m/l raise STOI/eSTOI at low SNR, which nnnoiseless and sonora do not. Still missing: a quiet-machine timing run, a listening result (`audio-corpus listen`, `docs/audio.md`), and a licence review of the weights. |
| No call-time model download or re-initialisation on audio callbacks | simulated | No downloader exists, and relay-audio `allocations::dsp_chain_is_allocation_free` covers the shipped chain. Candidates other than `off` are not callback-safe as measured, so sonora runs on the capture worker and never in a callback: it allocates 1-40 times per frame, the tract-based neural candidates 284-2330 times per frame (`benches/tests/neural_allocs.rs`). |
| Approved model hash, license and runtime manifest checked in | blocked | Candidate manifests with source revision, SHA-256 and runtime are checked in for the neural candidates (`benches/models/manifests/`), and the benches verify the hash on load, but all are `candidate`, `approved: false`, with weight licences and measured delay for the manifest unreviewed. No qualified manifest exists. |

### WP-08 CLI, TUI, hotkeys

| Gate | Status | Evidence / what is needed |
|---|---|---|
| Terminal-only install calls without WebView or GUI dependencies | met-by-test | Structural: `relay-cli` and `relay-daemon` do not depend on `relay-ui`; the CI `headless` job fails if `cargo tree -p relay-cli -p relay-daemon` lists eframe, egui or winit (not yet run). Exercised by relay-cli `engine_e2e`. |
| PTT press, release, focus, game and lock behaviour on the supported matrix | unmet-needs-hardware | Press, renewal, release and stream-close logic is tested once, in the shared worker (relay-client `tests/ptt.rs`), plus the CLI (`apps/relay-cli/tests/hotkey.rs`) and the desktop (`apps/relay-desktop/tests/ptt_engine.rs`, real in-process engine, fake hotkey backend). Real key delivery on macOS, Windows, X11 and Wayland portals is a manual gate in `docs/platform.md`. |
| An unsupported portal reports honestly, never false success | met-by-test | relay-platform `hotkey::unavailable_backends_refuse_to_start`, `detected_backend_probe_is_never_a_false_success`. |
| TUI push-to-talk against a real engine `InputControl` handler | unmet-needs-hardware | Engine-side stream semantics listed as open in `docs/cli.md` and `docs/platform.md`; a real-engine run is missing. |
| Desktop global push-to-talk key delivered by the OS to the running app | unmet-needs-hardware | The registration logic, rebinding, hidden-window delivery path and all release paths are tested with a fake backend against a real engine (`docs/desktop.md`). Not observed: macOS Carbon hotkey events through eframe/winit (window shown, hidden to the tray, another app in front), Windows, X11, the Wayland portal (not enabled in the desktop build), the key held past 60 s, and a real microphone transmitting. |

### WP-09 Preferences, identity, avatars, supportability

| Gate | Status | Evidence / what is needed |
|---|---|---|
| A crash during a config replace never silently enables the microphone or loses trust without notice | met-by-test | relay-config `recovery::garbage_main_recovers_backup_without_enabling_mic`, `truncated_main_is_detected_and_recovered`, `corrupt_main_and_backup_resets_loudly_and_preserves_file`, `uncommitted_transaction_is_invisible_after_restart`, `garbage_wal_sidecar_never_silently_keeps_a_recovered_mic`. Power-loss injection on real filesystems is not done. |
| Decompression, size, path and malformed-avatar handling | met-by-test | relay-config `avatar::decompression_bomb_dimensions_are_rejected`, `lying_header_dimensions_are_rejected`, `oversize_encoded_is_rejected_before_decoding`, `truncated_png_is_rejected`, `path_validation`, `cache_detects_tampering`. |
| Support bundle has no keys, tokens, raw voice or sensitive paths by default | met-by-test | relay-config `diagnostics::bundle_omits_identity_paths_and_names`, `log_lines_are_redacted`, `raw_capture_needs_explicit_opt_in`; relay-cli `engine_e2e::doctor_writes_a_private_redacted_bundle_even_without_an_engine`. |
| OS credential store behaviour on Windows and Linux | unmet-needs-hardware | Only macOS has been exercised by the authors; no record exists for other stores. |
| Real macOS keychain prompt: the daemon serves while the prompt is open, a slow answer is waited for without a second daemon, a denial shows the Try again / file-storage card and Try again succeeds after allowing | simulated | Covered by tests with a fake identity store that blocks, refuses or reports a damaged item (`relay-engine` `startup_keychain`, `relay-client` `startup_wait`, `relay-cli` `startup_wait`, `relay-ui` `startup_card`, `startup_restart`). Open on-device check: the real Security framework prompt (including "Always Allow" persistence for an unsigned `relayd`) on macOS has not been run here. |
| A new identity is created only when the store says none exists | met-by-test | relay-config `identity_lifecycle` (error on read does not generate, a denied read leaves the item byte-identical, missing generates once, concurrent first starts make one, corrupt bundle refused). |

### WP-10 Desktop client

| Gate | Status | Evidence / what is needed |
|---|---|---|
| Closing the GUI has a documented effect on leases and background calls | simulated | Documented in `docs/desktop.md` (quitting never stops the daemon or ends calls). Lease behaviour is tested on the engine side (relay-engine `service_gate::a_held_press_opens_the_gate_and_closing_the_stream_closes_it`, `service_daemon::idle_clients_are_detached_and_release_their_calls`); the real window and tray were never observed by a person. |
| Headless artifacts carry no GUI dependency; GUI resources measured separately | unmet-needs-hardware | Dependency split holds structurally. Resource gate is explicitly unmet: one idle sample with no daemon only (`docs/desktop.md`). Needs idle and active CPU and RSS per platform. |
| No client path carries audio samples or inference | unmet-needs-review | By construction: the gRPC contract has no sample payloads and `docs/architecture.md` forbids it. There is no test asserting it; a reviewer should confirm against `crates/relay-api/proto`. |
| Windows named-pipe or authenticated local transport | unmet-needs-hardware | Named pipe is an unimplemented spike (`docs/api.md`); only the loopback token profile exists and its ACL hardening on Windows is open. |

### WP-11 Qualification, packaging, release

| Gate | Status | Evidence / what is needed |
|---|---|---|
| Every mandatory gate has evidence, a named reviewer and a passing artifact revision | blocked | Depends on every open row in this file. |
| No unreviewed plaintext or insecure credential fallback in a distributed build | unmet-needs-review | There is no plaintext transport mode (`docs/net.md`). The explicit INSECURE identity file option exists (`docs/decisions.md`); the release build must be reviewed to confirm it is not enabled by default and is labelled. |
| Published claims match measured configurations and known limits | blocked | Needs the measurements above; the support matrix in `docs/release.md` stays empty of claims until then. |
| Dependency licenses and provenance | simulated | `deny.toml` written, `LICENSE` and manifests agree (Apache-2.0). Mechanical checks pass only once recorded as run; third-party notices and SBOM are generated at release time (`docs/release.md`). |
| Reproducible, hashed, signed artifacts | unmet-needs-review | Plan in `docs/release.md`; no signing identity, notarization or Windows signing exists. |
| Signed-bundle keychain behaviour (Developer ID, hardened runtime, entitlements) | unmet-needs-hardware | Not tested: whether the single `relay-voice`/`vault` item prompts once, and keeps "Always Allow" across updates, for a Developer ID signed, hardened-runtime, entitled bundle needs a signed build on a real Mac. `scripts/dev-sign-macos.sh` (`docs/packaging.md`) is a local self-signed dev convenience only and is no evidence for this gate; its effect on the keychain prompt was not observed either. |
| Clean-machine install, run and uninstall on each supported OS | unmet-needs-hardware | Not performed. |

### WP-12 Speaker-mode AEC

| Gate | Status | Evidence / what is needed |
|---|---|---|
| Speaker-mode qualification including double-talk and changing output gain | blocked | Deferred past v1 (headset-first). Needs render-reference alignment work and hardware. Only an AEC3 timing sample on a silent render exists. |
| No unexplained double suppression or second AEC in use | blocked | Depends on the above; no AEC is wired into calls. |

## Cross-cutting release gates

| Gate | Status | Evidence / what is needed |
|---|---|---|
| Independent security review of peer security and channels | unmet-needs-review | `docs/net.md`, `docs/channels.md`: composition is a proposal; `spake2` unaudited. Needs a named reviewer, scope, findings and a closing revision. |
| Default noise suppressor chosen, with licensed and hashed model | blocked | See WP-07. Not required while bypass ships. |
| Real-time deadlines: callback and worker p99, p99.9 and maximum under load, xruns | unmet-needs-hardware | Allocation freedom is tested on fakes only; no timing under load on real devices. |
| Multi-call privacy on real audio | simulated | Canary tests use fake devices; native sample non-interference through real DSP is not shown. |
| Global PTT on advertised platforms | unmet-needs-hardware | See WP-08. |
| Windows local IPC isolation | unmet-needs-hardware | See WP-10. Blocks any Windows claim. |
| macOS identity and microphone permission flow (signed bundle, deny, revoke, restart) | unmet-needs-hardware | Verified here: the TCC status read (`relay-platform` test reads "not determined" on this Mac without prompting), the launch prompt, card, denied state and focus refresh logic against a fake OS (`apps/relay-desktop/tests/microphone.rs`), and `Info.plist` embedded in the binary. Not verified: the real prompt and its deny, revoke and restart behaviour, because that needs a signed `.app` whose own `Info.plist` carries `NSMicrophoneUsageDescription` (a `cargo run` binary inherits Terminal's permission, so the prompt and the Privacy entry name Terminal). No signed bundle exists. Blocks any macOS release claim. |
| Platform backend and latency claims | unmet-needs-hardware | All latency and resource figures in the repository are microbenchmarks or targets. |
| Licenses and source provenance | simulated | See WP-11. Apache-2.0 chosen; `LICENSE` and manifests agree. Needs a recorded `cargo deny` run and the generated notices. |
| Schema and API compatibility | simulated | Contracts compile and are exercised by tests. The cross-version matrix is an open gate in `docs/api.md`; the product is unreleased, so no old-client compatibility is promised. |
| Cross-platform persistence and secrets | unmet-needs-hardware | SQLite recovery is tested on the host filesystem (see WP-09); Windows and Linux credential stores and filesystem permissions are untested. |
| Adversarial networking and resource limits | simulated | Property tests (`relay-net` `fuzz.rs`, `channel_fuzz.rs`), bounded queues and limiter tests exist. Coverage-guided fuzzing ran only as 4-minute per-parser smoke runs (no findings); longer runs, identity churn, interface changes and rekey under load are not run. |
| Desktop translations reviewed by native speakers | unmet-needs-review | `docs/i18n.md`: the catalogs in `localizations/` are machine-written (all eight languages are written; none reviewed). Each language needs a named native-speaker review of its section files, and `cargo test -p relay-ui --test i18n_catalogs` green, before it may appear in a release picker. Hindi stays flagged experimental (no text shaping in egui). |
| Overlay (Tailscale) call carries audio | unmet-needs-hardware | Media address comes from the authenticated link when the opt-in is on (`docs/net.md`); unit-tested only, because tests cannot create a 100.64.0.0/10 interface. Needs a real call over a Tailscale pair. |
| Desktop rendered on screen in every language | unmet-needs-hardware | Never seen by a person. Check the Calls page (favorites grid of 1 to 6 tiles and its empty states, History collapsed and open, the call card in each mode with the Hold to talk button, the volume popup and the connection details) and Settings, Push-to-talk and Calls, on a phone and a desktop. Check Hindi shaping (क्षत्रिय, हिन्दी, कर्म; names, messages and text fields, cursor across conjuncts), German and Russian string overflow in buttons and tabs, and CJK fallback; the redesigned screens: sidebar and Settings category list at 720x800 and at the minimum window size, the compact drill-down and Back, Settings filling the whole window height with no strip above it (a ringing call shows only as the dot on the Calls entry or tab, plus the ringtone), an incoming call over an open chat, Esc and Ctrl/Cmd+1 to 4 shortcuts, the Contacts page (online and offline cards, local address, the More menu with the Forget this device confirmation, the empty state) and the Connect page (Nearby cards with their local address and the Trust confirmation, no trust notice anywhere, the Connect a device card, a phone scan that adds the device to Contacts), the five phone tabs at 360x800 in en/de/ru, the notices strip (one, three and five notices; Dismiss on each and Dismiss all; +N more; an info note fading after 10 s and staying while hovered; a key-change warning naming the device with the digests behind Details; no jargon in en/de/ru; 44 pt close buttons at 360x800 and 320x640), the icons (sidebar and tab icons, the star, kebab, copy, send, attach and back buttons with their tooltips, the right-hand chevrons on Connect and Channels, no missing-glyph boxes in any language), and the Startup category with and without a tray; the Calls page History section (empty text, a row per outcome with the marker word, the missed emphasis, times in the local zone (`1:54 AM`, `Yesterday at 6:51 PM`, `11/04/26, 3:33 PM`, and the per-language forms), Call back and its disabled reason, Show more after 20 rows, the Clear history confirmation, nothing overflowing at 720x800 and 360x800 in en/de/ru) the call chat (bubbles right and left with grouping, date separators, the three-dot message menu with Copy text and Pin, the Chat options menu, the composer with + and Send above the keyboard on a phone, Sending…/Delivered/Retry, the New messages pill when scrolled up, the empty state; the call card's Messages button toggling a panel below the card on desktop and a full-screen chat with Back to the call on a phone, the Messages (N) unread badge, Esc closing it, and a history row's Messages button enabled or disabled with "Messages from this call are no longer kept" and opening the read-only transcript; nothing overflowing at 720x800 and 360x800 in en/de/ru, 44 point touch targets on the phone) and the Keep call history switch in Settings, Privacy and trust with its outcome line; then drop "(experimental)" from the Hindi picker label. |
| Key-changed contact card on screen | met-by-test (logic, layout, engine), unmet-needs-hardware (screen, phone) | relay-ui `tests/key_changed.rs` (card only for `key_changed`, Trust enabled only for the exact fingerprint, forget sends the pinned key, no overflow and 44 pt targets at 360x800, 320x640 and 720x800 in en/de/ru, idle repaint), relay-engine `peers_key_change.rs` (forget by either key, replace moves trust and favorite, mismatch and unrelated refused, second change replaces the first), `peers_key_change_scan.rs`. Needs a person: regenerate the laptop's identity, open Contacts on the phone and the desktop, check the card wording, the compare step with the laptop's fingerprint, the QR repair with Scan their QR code, Remove contact, and that the contact keeps its favorite star. |
| GUI packaging and footprint | unmet-needs-hardware | Clean-machine build, signing, install and footprint measurement not done. |

### WP-18 Android app

| Gate | Status | Evidence / what is needed |
|---|---|---|
| Engine starts, attaches, detaches and stops in-process over a Unix socket in app storage | met-by-test | `apps/relay-android` `host::tests::engine_starts_attaches_detaches_and_stops` (host build, fake audio and Keystore); started and stopped for real on the `Medium_Phone_API_35` emulator (`docs/android.md`, verification log). |
| Identity and channel secrets are sealed by a non-exportable Keystore key and bound to their slot | simulated | Rust `sealed::tests` and JVM `SealerTest` use a software key; `KeystoreSealerTest` ran against the emulator's Keystore. Not run on StrongBox or on a phone. No independent review of the scheme. |
| Foreground microphone service lifecycle and permission state machines | simulated | JVM tests of `ServicePolicy` and `PermissionGate`; service start and notification observed on the emulator only. The system permission dialogs, the denied and blocked cards, "Open system settings" and a grant while running (`pm grant` in the foreground and from Settings) were observed on the `Medium_Phone_API_35` emulator; a capture stream opening after the grant was not. |
| Real call between the phone and the laptop (audio both ways, echo, latency) | unmet-needs-hardware | Needs a physical phone on a LAN with a desktop. The AAudio voice-communication path opened on the emulator but carried no real microphone or speaker. |
| mDNS and NSD discovery on a physical Wi-Fi network, multicast lock effect | unmet-needs-hardware | Locks were acquired on the emulator; the emulator's NAT carries no multicast. |
| Mobile resource gate (CPU, RSS, battery per hour of call and idle) | unmet-needs-hardware | Not measured. |
| Hold to talk through the engine's push-to-talk lease | met-by-test (engine side), unmet-needs-hardware (phone) | `apps/relay-android` `host::ptt_tests` (in-process engine, fake media; gate follows press, renewal, release, cancel, background, detach and stream close). On a phone, unverified: touch hold and release on the large button, thumb leaving the button, multi-touch, the held button surviving `onPause`/`surfaceDestroyed` events as simulated, rotation during a hold, 400 ms lease renewals under Doze or Wi-Fi power save, audible latency from press to first audio, and hold behaviour with a Bluetooth headset. See `docs/android.md`. |
| Android adopts the UI client's context for the lease stream | unmet-needs-review | The push-to-talk worker reuses the UI's registered client id on a second in-process connection (`docs/android.md`, "Push-to-talk"). Needs review, or a `relay-ui` operation that owns the stream, before release. |
| Text entry with the soft keyboard (focus survives the keyboard, typing, Back) | met-by-test (logic), simulated (emulator), unmet-needs-hardware (phone) | `relay-ui` `tests/keyboard_focus.rs` (a focused field keeps focus when the keyboard height goes 0, 300, 0, with system insets), `tests/back.rs`, JVM `BackRouterTest`, Rust `back::tests`. Emulator (Gboard, `Medium_Phone_API_35`): the keyboard stays up after a tap, text typed on the Gboard keys, space, backspace, `input text` and `input keyevent` reach the field, a tap elsewhere hides it, Back hides the keyboard before navigating, the Back button and the edge-swipe gesture walk the stack (`docs/android.md`, "Keyboard and Back"). On the Galaxy S25 Ultra, unverified: Samsung Keyboard composition, predictive text and autocorrect, the IME action key, the three-button navigation mode, predictive back animation, and the keyboard on rotation or split screen. |
| Security review of the Keystore wrapping and of the app's exported surface | unmet-needs-review | Needs a named reviewer. |

