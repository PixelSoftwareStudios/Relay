# Android feasibility spike

Status: **spike, not a commitment.** This records what was tried on one Mac on 2026-11-05, with evidence. Nothing here promises an app. Anything not listed under "Observed" is unverified.

The spike lives in `apps/relay-android/` (experimental, not a workspace member: it has its own `[workspace]` table and `Cargo.lock`). No shared crate was edited; required source changes are listed under "Required changes in shared crates".

## Environment (inventory)

| Item | Found |
|---|---|
| Host | macOS 14 (Darwin 23.6), Apple Silicon |
| Rust | 1.99.1 (`rust-toolchain.toml`); targets were only apple/linux/windows/wasm |
| Installed by the spike | `rustup target add aarch64-linux-android x86_64-linux-android`; `cargo install cargo-ndk --locked` (4.1.2); NDK r28c (`ndk;28.2.13676358`) into a scratch SDK root via `sdkmanager --sdk_root=<scratch>` (the user's SDK was not modified) |
| Already present | Android SDK at `~/Library/Android/sdk` (platform android-36.1, build-tools 35.0.0, emulator, platform-tools), AVD `Medium_Phone_API_35` (Android 15, arm64-v8a, Google Play image), Temurin JDK 21 |
| Absent | Gradle on PATH (only stale `~/.gradle` caches), Kotlin toolchain, `cargo-apk`, `xbuild` |

No Gradle/Kotlin build was attempted, so **no Kotlin shell exists in the spike**. The APK was assembled without Gradle (`apps/relay-android/build-apk.sh`: cargo-ndk, `aapt2 link`, `zip`, `zipalign`, `apksigner` with a throwaway debug key). A real shell needs Gradle, which was not available (not tried).

## Cross-compile results

Command: `ANDROID_NDK_HOME=<ndk> cargo ndk -t arm64-v8a -P 26 check -p <crate>` from the repo root (API level 26 = minSdk). Build dir was a scratch `CARGO_TARGET_DIR`. Other agents were editing shared crates concurrently; a snapshot that was mid-edit failed twice (see note) and passed when rerun.

| Crate | aarch64 `check` | Notes |
|---|---|---|
| relay-core | compiled | blake3 builds its C/asm via NDK clang fine |
| relay-protocol | compiled | |
| relay-audio | compiled | cpal 0.18.2 on Android pulls `ndk` + `jni` (AAudio host, no oboe, no extra C++). `opus` builds libopus from source via `opusic-sys` with NDK clang: no system lib needed |
| relay-net | compiled | `mdns-sd`, `snow`, `argon2`, `spake2` all pure Rust |
| relay-config | compiled, **runtime unverified** | `rusqlite` bundled compiles with NDK clang. `keyring` 4.2 compiles but has no default store on Android unless one is registered (see below) |
| relay-api | compiled | tonic over UDS is fine; protoc is vendored |
| relay-client | compiled | |
| relay-ui | compiled | egui without eframe |
| relay-engine (lib) | compiled | `cargo ndk build -p relay-engine -p relay-ui` for arm64-v8a ok |
| relay-platform | compiled | global-hotkey is a no-op on Android; not needed there |

Also: `cargo ndk -t x86_64 ... build --release --features engine` for the spike crate (links the engine into a cdylib) succeeded, so the x86_64 emulator ABI builds too (an earlier x86_64 engine attempt failed only on in-flight edits by other agents: missing `PttUpdate::immediate`, `ptt_release_delay`, `Reject::InvalidReleaseDelay`; not an Android issue).

`cargo ndk check` only proves type-checking plus build scripts. The stronger check: the spike cdylib with feature `engine` calls `DaemonHandle::start`, so opus, sqlite, cpal, mdns, snow and tonic cannot be dead-stripped; it **linked** (arm64 release `librelay_android.so`, 26 MB unstripped; 12 MB without the engine). That engine path was **never run** on a device.

## Hello APK (relay-ui compact layout, mock mode)

`apps/relay-android`: `cdylib` with `android_main`, eframe 0.37 (`glow`, `android-native-activity`), `android-activity` 0.6, relay-ui in mock mode with a stub `Shell`. Build:

```
export ANDROID_NDK_HOME=<scratch sdk>/ndk/28.2.13676358 ANDROID_HOME=~/Library/Android/sdk
OUT=<scratch dir> apps/relay-android/build-apk.sh        # ABIS="arm64-v8a x86_64" for both
```

Result: 4.0 MB APK, signature verified by `apksigner`, `aapt2 dump badging` shows minSdk 26 / targetSdk 35.

### Observed (emulator `Medium_Phone_API_35`, arm64, headless, swiftshader GPU, 1080x2400 @ 420 dpi)

- Installs and launches; `librelay_android.so` loads; renders with GLES (EGL_emulation); no crash in logcat.
- The compact layout selected itself: one column, bottom nav (Calls, People, Channels, Settings), mock data and banner, call card with Accept/Decline.
- Touch: an adb tap on the Settings tab switched the page.
- Soft keyboard: tapping the profile name field showed the Gboard IME (`mInputShown=true`); `adb input text abc` appended to the field. This is key-event injection, not real IME composition.
- Home then relaunch: same pid, same screen afterwards (the activity was backgrounded and resumed; no crash). Not an explicit surface-loss stress test.

### Defects seen in the screenshots

1. No safe-area/inset handling: the header sat under the status bar and the bottom nav overlapped the gesture bar (target 35 is edge-to-edge). Fixed in relay-ui: `Shell::safe_area_insets`; the Android shell must supply the values.
2. Cards and long labels ran off the right edge. Cause: non-wrapping `horizontal` rows, 280 point text fields, and labels beside sliders wider than the row, so the frame grew past the viewport. Fixed in relay-ui for 320 to 430 points at 2.6 pixels per point and longer German and Russian text (tests `phone_overflow`).
3. The "Channels" tab label wrapped to "Channel/s". Fixed: the tab font shrinks, then an ellipsis (test `phone_tabs`).
4. The soft keyboard did not push content up. Fixed in relay-ui: `Shell::keyboard_height` (the shell supplies it); the tabs hide, the bottom is padded and the focused field is scrolled into view.
5. Settings copy said "no tray", "no on-screen button" (desktop wording). Fixed: capability hooks `has_global_hotkey`, `has_talk_button` and `runs_in_background` select the wording.

### Not verified

Real device, touch-screen gestures and scrolling feel, IME composition/predictive text/IME actions, rotation, process death and restore, explicit `Surface` destroy/recreate and GL context loss, 120 Hz or low-end GPUs, idle zero-repaint on device, CPU/battery/RSS (the mobile resource gate in `docs/ui.md` stays unmeasured), accessibility (eframe's `accesskit` is not available with NativeActivity: it is a `compile_error!`), any audio, any networking, the engine running on Android, real-device Wi-Fi/multicast.

## Android-only shell needs

Boundary proposal: the Rust cdylib owns engine + UI; Kotlin owns anything that needs the Android framework. Prefer UniFFI (generated Kotlin, async-friendly) for the engine-control surface and plain `jni` calls (already a transitive dependency via cpal) for tiny leaf calls. Sizes are rough Kotlin line counts for a working version, unmeasured.

NativeActivity cannot host a foreground service, runtime-permission dialogs, SAF intents or a proper IME bridge without Java. **Recommendation: use `GameActivity` (android-game-activity, needs Gradle + the Jetpack games-activity AAR) or a small Kotlin `Activity` that embeds the Rust surface.** The spike used NativeActivity only because Gradle was missing.

| Need | Mechanism | Boundary | Kotlin size |
|---|---|---|---|
| Foreground service, type `microphone`, with notification (call, mute, hang up actions) | `Service` + `startForeground(id, n, FOREGROUND_SERVICE_TYPE_MICROPHONE)`; manifest `FOREGROUND_SERVICE` and `FOREGROUND_SERVICE_MICROPHONE`; start only while a call is up and mic permission is held (Android 14+ throws otherwise). The engine runs in this service's process | Service calls `Engine.start(paths)` / `stop()`; notification actions call engine commands | ~150 |
| `RECORD_AUDIO`, `POST_NOTIFICATIONS` (13+) | `ActivityResultContracts.RequestPermission`; deny states shown by UI | Kotlin passes `granted` to Rust; the engine's per-call transmit consent stays in the engine | ~60 |
| Multicast + Wi-Fi locks | `WifiManager.MulticastLock` (needed to receive mDNS), `WifiLock` (`WIFI_MODE_FULL_LOW_LATENCY`, API 29+) held for the service lifetime; permissions `CHANGE_WIFI_MULTICAST_STATE`, `ACCESS_WIFI_STATE`, `WAKE_LOCK` | Service acquires/releases; no Rust call | ~40 |
| NsdManager alternative | `NsdManager.discoverServices("_praxis-relay._tcp")`/`registerService`; use if `mdns-sd` multicast is unreliable on some OEMs. Results go to Rust as peer hints | `engine.push_discovered(ip, port, txt)`; needs a new discovery adapter on the Rust side | ~120 + Rust adapter |
| Identity key in Keystore | The Noise static key must be exportable to Rust, so Keystore cannot hold it directly: wrap it (AES-GCM key in Keystore, ciphertext in app files). Implements `IdentityStore` | `RunOptions::identity_store` and `channel_vault` already accept injected stores. Kotlin exposes `wrap(bytes)`/`unwrap(bytes)` over JNI; a Rust `AndroidKeystoreStore` calls it | ~100 + ~80 Rust |
| SAF/content-URI picker | `ActivityResultContracts.OpenDocument("image/*")`; read size from `OpenableColumns.SIZE` before reading (1 MiB limit), then bytes | Fits `FilePicker::pick_image`: Rust passes a request id, Kotlin completes it with `PickedFile{name,bytes}` or cancel (JNI callback into a oneshot) | ~60 |
| Clipboard | `ClipboardManager.setPrimaryClip`; Android 13+ shows its own toast | `Shell::copy_text` -> JNI static call | ~15 |
| Push-to-talk | The on-screen hold button (relay-ui, `Shell::push_to_talk`) drives the engine's push-to-talk lease. There is no hardware key binding: the volume keys keep changing the system volume. See "Push-to-talk" below | `RelayEngine.cancelTalk`, `AndroidShell::push_to_talk` -> `Host` -> `InputControl` | ~60 + ~290 Rust |
| Lifecycle | `onStop`/`onStart` -> `Shell::window_visible`; surface loss handled by android-activity/winit; call survives via the service | small JNI setter | ~30 |
| Battery / doze | Foreground service exempts the app during a call; idle (no call) should keep nothing running. Do **not** request battery-optimization exemption by default. Incoming calls while idle are not possible on a LAN-only design without a running discovery; this is a product limit to state, not engineer around | n/a | n/a |
| Wi-Fi power save | `WIFI_MODE_FULL_LOW_LATENCY` lock is the lever (API 29+); effect on jitter is **unmeasured** | | in locks above |
| AEC | Open capture with `VOICE_COMMUNICATION` source so the platform AEC/NS/AGC apply. cpal's AAudio host selects the input preset only via its own API: **unverified whether cpal 0.18 exposes it**; otherwise relay-audio needs its own AAudio/Oboe stream or JNI `AudioRecord`. Also route output with `USAGE_VOICE_COMMUNICATION` and set `AudioManager.MODE_IN_COMMUNICATION` | Kotlin sets mode/`AudioManager` routing; Rust opens stream | ~50 + Rust change |
| Audio focus / headset / Bluetooth routing | `AudioManager` focus and `setCommunicationDevice` (31+) | JNI | ~100 |

Total Kotlin: roughly 700-900 lines for the above, excluding Gradle setup and tests (estimate).

## Push-to-talk

Hold-to-talk uses the engine's real lease path, the same one `relay-cli` and the desktop hotkey use, not a mode switch. `AndroidShell::push_to_talk` (the large on-screen button) calls `Host`, which sends press and release to one worker (`apps/relay-android/src/host/ptt.rs`). The worker holds an `InputControl` stream and sends `PRESS` with a 400 ms lease, renews it every 150 ms while the button is down and sends `RELEASE` on button up. The engine opens the transmit gate only while a lease is alive. Consent and the push-to-talk route are still set by the UI (`set_transmit`), and the press is refused unless the UI client controls a non-terminal push-to-talk call.

Shell capabilities: `has_global_hotkey() = false`, `has_talk_button() = true`, `runs_in_background() = true` (the microphone foreground service keeps a call alive without the activity), `hides_to_tray() = false`.

**Client identity.** The engine accepts a press only from the client that holds the call's controller lease (the UI's client), and `relay-ui` keeps that connection private. The worker opens a second connection to the same in-process engine and adopts the UI client's registered context (`RelayClient::adopt_context`; the client id is an identifier, not a credential, and both ends are in this process). The lease generation comes from the UI state. Taking over the lease with a separate client is not an option: a takeover resets the call's transmit route. A `relay-ui` operation that sends presses on its own connection would remove this adoption; it is a recorded follow-up, not done here because `relay-ui` is owned elsewhere.

**Every way the button can vanish ends in a release.**

| Event | Path |
|---|---|
| Button up | `RELEASE` on the stream (the engine holds the gate for the release delay) |
| Button no longer drawn | relay-ui reports `push_to_talk(.., false)` |
| `onPause`, window focus lost, `surfaceDestroyed`, `onStop` | Kotlin `MainActivity` -> `RelayEngine.cancelTalk` -> the stream is closed (immediate release) |
| Process sent to the background | `Host::set_foreground(false)` cancels, even when a call keeps the client attached |
| Client detach (`Host::detach`), call end, engine stop | `Ptt::reset`: the stream is closed, which the engine treats as an immediate release of everything the source held (no release delay) |
| Call no longer push-to-talk, not controlled, or terminal | checked every renewal; the worker releases |
| Engine refuses a renewal | the worker stops renewing |
| This process dies or stalls | the lease lapses within 400 ms |

Tests: Rust `host::ptt_tests` (in-process engine on a Unix socket, UI client attached the way the app attaches it, fake media and peers; the engine's published gate is the assertion): button hold across several lease periods and release, no press without a push-to-talk route, cancel (stream closed), background, detach, closing the stream while attached releases without the release delay, a normal key-up keeps the tail for the release delay, and call end. Engine-side guarantees they build on: relay-engine `service_gate` (`a_held_press_opens_the_gate_and_closing_the_stream_closes_it`, `takeover_drops_the_previous_holders_press`).

## Required changes in shared crates (recorded, not made)

Ranked by effort, smallest first.

1. **keyring on Android (small, runtime-blocking).** `relay-config` uses `keyring::Entry::new`. keyring 4.x delegates to `keyring-core` stores; without a registered default store it returns `NoDefaultStore`. On Android either enable the `android-native-keyring-store` feature and initialise it from Java, or (preferred) use `cfg(target_os = "android")` to skip `KeyringVault` and require an injected `SecretVault`. `RunOptions::channel_vault` and `identity_store` already allow injection, so the engine needs no change; `relay-config` needs a cfg gate or a feature `os-keyring` (default on) that Android builds turn off. Unverified at runtime.
2. **Paths.** `Paths::platform()` uses `directories::ProjectDirs`, which has no Android home. Use `Paths::new` with the app's `filesDir` (already supported by `RunOptions::new(paths)`); no change, but do not call `platform()`.
3. **Transport.** `Transport::Unix(Some(path))` in `filesDir` is expected to work on Android; the default path must not be used. An in-process option is cleaner: expose the tonic service through an in-memory duplex (`tonic` `Server::serve_with_incoming` over a channel) so the UI links `relay-client` to the engine without a socket. This is a new `Transport::InProcess`-style entry plus an `Endpoint` variant in `relay-client` (medium; touches relay-engine, relay-api, relay-client). Not needed for a first spike because UDS in app storage should work (unverified).
4. **In-process entry point / FFI surface.** A single `start(paths, vault, identity, transport) -> handle` and `stop()` for UniFFI. `DaemonHandle::start`/`shutdown` already have this shape (medium, mostly wrapper code in a new crate, no edits to existing crates required).
5. **cpal capture options.** Verify or add the VOICE_COMMUNICATION input preset for AAudio (relay-audio, medium, may need a custom backend behind the existing `AudioBackend` trait).
6. **Discovery adapter for NsdManager** (relay-net or engine, medium, optional).
7. **relay-ui layout defects** above (insets, overflow, tab label), and a `Shell` method for safe-area insets (small-medium).
8. **Debug/dev features.** `global-hotkey`, `tray-icon`, `rfd`, `arboard` are not pulled in by the Android build path today: the spike's lockfile contains none of them (relay-ui is clean). `relay-platform` compiles but should not be a dependency of the Android app.

## Recommended architecture

- One Android app. The engine runs **in-process** inside a foreground microphone service; the UI (egui, relay-ui) runs in the activity in the same process. First step: loopback over a UDS in `filesDir`, later an in-memory transport.
- Kotlin: Activity (GameActivity or Kotlin Activity hosting a Surface), `RelayService`, permission/SAF/clipboard/Keystore/lock helpers. Rust: one cdylib crate (`relay-android`) depending on `relay-engine`, `relay-ui`, `relay-client`.
- Keep the engine as the owner of consent and calls: Kotlin never starts capture on its own.
- Build: cargo-ndk to `jniLibs`, Gradle for the app; aarch64 for devices, x86_64 for emulators.

## Effort estimate (single experienced developer, rough, low confidence)

| Piece | Estimate |
|---|---|
| Gradle project, GameActivity/Surface, insets, IME, lifecycle, signing | 1-2 weeks |
| Foreground service, permissions, notification, locks | 1 week |
| Engine in-process on device: paths, vault/Keystore, UDS or in-memory transport, first call on two phones | 2-3 weeks (largest uncertainty: audio, see below) |
| Audio: AAudio via cpal, latency, AEC, routing, Bluetooth | 2-4 weeks, may need custom backend |
| Picker, clipboard, PTT, UI polish for touch | 1 week |
| Device testing across vendors, doze, Wi-Fi power save, mDNS quirks, resource gate | 2-4 weeks |

About 9-15 weeks to a trustworthy beta; confidence is low because audio latency/AEC behaviour and Wi-Fi multicast behaviour on real devices are unmeasured. Treat as plus or minus 50%.

## Microphone and notification permission

`MainActivity.onCreate` reads both permissions and, while one was never asked, shows the system dialog at once (`RECORD_AUDIO`, plus `POST_NOTIFICATIONS` on Android 13+). Observed on the `Medium_Phone_API_35` emulator after `pm revoke`/`pm clear`: the dialog appears on a fresh install, each refusal moves `PermissionGate` to `DENIED` and then `BLOCKED`, and the Calls page shows the shared microphone card (`docs/ui.md`): "Relay needs your microphone" with **Allow microphone** while the system will still ask, "Microphone access is turned off" with **Open system settings** (the app's details page) once it will not. The earlier hard-coded red error ("Microphone permission is denied...") is gone: it fired while the dialog was still open and never cleared after a grant.

A grant that arrives later is noticed without a restart: the gate is re-read on every `onResume` (back from Settings), on the dialog result, and every 3 s while the microphone is missing and the app is visible (a permission changed by `adb shell pm grant`, split-screen settings). Observed: `pm grant` while the app was in the foreground and while it was in Settings; the same process kept running and the card disappeared. Granting through `pm grant` does not kill the process; revoking does (Android's rule). `RelayEngine.set_microphone(state)` tells the engine: on `READY` the in-process daemon is told `input_access_changed`, so a capture that failed to open for lack of permission is retried in the running call, and the next call retries it anyway (`relay-core` `capture_retry`). `MICROPHONE_UNAVAILABLE` is withdrawn (a `Removed` event of kind `warning`) when capture opens.

Not observed on the emulator: the capture stream opening after the grant (no peer to call; the AAudio open itself is covered by the fake-backend test `media_permission`).

## QR scanner and device name

**Scan.** The Scan card at the top of Connect (shared UI, shown because `AndroidShell::can_scan_qr` is true once Kotlin reports a camera) opens `ScanActivity`: CameraX preview plus ML Kit's bundled QR model (no Play services, no network; choice and alternatives in `docs/decisions.md`). It returns the raw text only; Rust checks it like a pasted connection string, then dials the device and trusts it when its key matches the code (`Controller::connect_and_trust`; `docs/ui.md`). `CAMERA` is requested on the first tap, never at startup, through its own `PermissionGate` (`RelayRuntime.camera`), driven by the pure `ScanFlow` (JVM tests: `ScanFlowTest`): first tap asks, a refusal the system will repeat is "denied" (tap again), a permanent one is "blocked" and the UI offers Open system settings (the app's details page; a grant made there is read on the next tap). A cancelled scan changes nothing. The request lives in `ScanFlow`, not in the activity, and the launchers are registered in `onCreate`, so a recreated `MainActivity` still receives the answer; `ScanActivity` handles configuration changes itself and the camera is bound to its lifecycle. The torch button appears only when the camera has a flash unit. The scanner screen has three strings (`res/values*/strings.xml`, nine languages).

**Observed** on the `Medium_Phone_API_35` emulator (arm64, fresh `pm clear`): the Scan button appears under the QR code; the first tap shows the system camera dialog; Don't allow shows the "needs the camera" message and the button works again; a second refusal shows "Camera access is turned off" with Open system settings, which opens the app's system settings page; after `pm grant` the next tap opens the scanner and the emulator's virtual camera scene shows live in the preview; Cancel returns to People with no message. **Not observed:** decoding a real QR (the virtual scene has none to point at), the torch (no flash unit on the emulator), a real camera, a device without Play services, recreating the activity during a scan. Rust tests cover the hand-over (`host::tests`) and the shared UI (`tests/qr_scan.rs`).

**Device name.** An Android hostname is `localhost`, so `EngineConfig.default_display_name` carries the name Kotlin chose (`DeviceName.choose`: `Settings.Global` `device_name`, else `Build.MODEL`, blank ignored; JVM test `DeviceNameTest`) into `RunOptions::default_display_name`. It applies only while the user has not chosen a name; a name set in Profile always wins (`docs/engine.md`). Observed on the emulator after `pm clear`: Profile shows the device name (`sdk_gphone64_arm64` there). Rust test: `host::tests::engine_starts_attaches_detaches_and_stops`.

## Unmet gates

- Mobile resource gate (CPU, RSS, battery): not measured.
- Real-device run, real audio, real mDNS, real call between two devices: none done.
- Security review of the Keystore wrapping scheme: not designed in detail.
- The engine has never been run on Android; only linked.
- Gradle/Kotlin toolchain: not available here, so the shell design is paper only.
