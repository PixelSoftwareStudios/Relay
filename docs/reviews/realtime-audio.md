# Real-time audio review: callbacks, workers, UDP send, drift and failure behaviour

**This is an AI-performed review. It does not replace measurement on real hardware.** Everything
that depends on a real audio device, a real OS scheduler or a real network is labelled as such and
listed at the end under "Needs real hardware".

- Revision reviewed: `main` at `859ca50` plus the working tree present at review time. The audited
  paths (`crates/relay-audio`, `crates/relay-engine/src/media`, `crates/relay-net/src/{media,session}`,
  `crates/relay-protocol/src/{drift,jitter}`) did not change while reviewing, except that commit
  `e887519` added `relay-audio/src/tones` (read below, finding RT-18). Throwaway programs were built
  against the tree at `e887519`.
- Method: read `docs/audio.md`, `docs/engine.md` (Media and peers, Known gaps), `docs/evidence.md`
  and the `.codesight` map, then read the code listed in "Scope" end to end, then wrote scratch
  programs **outside the repository** (not committed) to confirm or refute suspicions.
- Evidence labels: **CONFIRMED** = traced in code and reproduced by a scratch program or an
  existing test I re-ran. **PLAUSIBLE** = sound from the code but not executed, or dependent on
  platform behaviour I could not exercise. **SIMULATED** = reproduced with the real code driven by a
  simulated clock (no hardware clock was measured).
- Measurement machine: Apple M1 Pro, macOS 14.7.2, rustc 1.99.1, release builds, machine not
  otherwise controlled. These are development-machine observations, not gate results, and say
  nothing about other OSes or devices.
- Nothing under `crates/` or `apps/` was modified.

## Scope

Read: `relay-audio/src/{capture,playback,ring,device,convert,mixer,codec,dsp,tones,format}`,
`relay-engine/src/media/*` (`capture_worker`, `render_worker`, `sink`, `net`, `send`, `recv`,
`packet`, `gate`, `meters`, `load`, `devices`, `live`, `system`, `system_calls`, `inbox`, `events`),
`relay-engine/src/service/gates.rs`, `relay-net/src/{media,session,record}` (send/receive cost
only), `relay-protocol/src/{drift.rs,jitter/*}`, `relay-core/src/state/{routing,flush}.rs` (how a
gate is produced), and the `cpal 0.18.2` manifest and host sources for thread priority.

Not covered: the Opus library internals, the sonora/nnnoiseless candidates (not reachable from
production), the CLI/UI, Android/iOS audio backends, and any security aspect (see the separate
security review).

## Summary

| Severity | Count |
|---|---|
| Critical | 0 |
| High | 3 |
| Medium | 5 |
| Low | 10 |
| Info | 7 |

| ID | Severity | Title | Evidence |
|---|---|---|---|
| RT-01 | High | A panic in a media worker kills that direction of every call silently; mic stays open, no event, no restart | CONFIRMED |
| RT-02 | High | Audio captured before the gate opened (PTT press, unmute) is transmitted: up to one frame, or the whole backlog after a stall | CONFIRMED |
| RT-03 | High | cpal `realtime` feature is off and no worker sets a priority: no RT/MMCSS promotion on Windows and Linux | CONFIRMED (manifest); effect needs hardware |
| RT-04 | Medium | UDP send is `Handle::block_on` on the capture thread: unbounded head-of-line blocking and a widening mute race | Mechanism CONFIRMED, stall PLAUSIBLE |
| RT-05 | Medium | Idle render worker wakes ~3,800 times/s (~1% of a core); both workers poll on a 2 ms park | CONFIRMED |
| RT-06 | Medium | Playout target is a fixed 20 ms regardless of device callback size: structural underruns for large callbacks | SIMULATED |
| RT-07 | Medium | After a render stall the added latency lasts minutes, then the loop undershoots | SIMULATED |
| RT-08 | Medium | The media receive task gives up for good after 100 consecutive errors, with no event | PLAUSIBLE (Windows) |
| RT-09 | Low | Drift thresholds are sample counts, not scaled to the device rate | SIMULATED |
| RT-10 | Low | relay-net allocates 3 times per packet sent and 3 times per packet received | CONFIRMED |
| RT-11 | Low | Capture worker drops a frame silently on a suppressor error and ignores `consumed` | Code reading |
| RT-12 | Low | AGC gain latches to NaN if an upstream stage ever emits NaN | Code reading |
| RT-13 | Low | Bounded command queues drop silently and can leave a call half-installed | Code reading |
| RT-14 | Low | Admission load meter freezes when a worker is idle and includes send stalls | Code reading |
| RT-15 | Low | A failed cpal RT promotion would be mapped to a fatal stream error | Code reading |
| RT-16 | Low | Full ingress queue drops the newest packets and keeps the oldest | Code reading |
| RT-17 | Low | `docs/engine.md` Known gaps are stale (48 kHz bypass, microphone test/gain) | Code reading |
| RT-18 | Low | Tones need a running quantum, but the render worker idles when there are no calls | Code reading |
| I-01 to I-07 | Info | See "Informational" | |

## Verdict per rule

| Rule | Verdict |
|---|---|
| No allocation in cpal callbacks | Holds. `tests/allocations.rs` re-run in release: 9 passed. I added `u16` and `i32` capture buffers: 0 allocations. Real cpal threads not tested (existing gate). |
| No blocking lock in callbacks | Holds. Callbacks touch atomics, `rtrb` and stack arrays only (`capture/callback.rs:80-114`, `playback/callback.rs:70-94`). |
| No logging / I/O / lazy init in callbacks | Holds. No `tracing`, no thread-locals, no `Once`. The cpal error callback (`cpal_backend.rs:43-52`) only does atomic stores. |
| No arbitrary destructor in callbacks | Holds. Callback state is dropped by the device thread when the stream is dropped. Nothing is freed inside `process`. |
| Panic freedom in callbacks | Holds by inspection: every slice is bounded by `CHUNK_FRAMES`, `channels >= 1` is forced, no `unwrap`/`expect`/unchecked subtraction. A panic would abort across cpal's FFI frames on CoreAudio. |
| Bounded execution on workers | Holds except the UDP send (RT-04). Capture `poll` is bounded by ring capacity (<= 4,096 x 2 + 480 samples); jitter `head()` scans <= `horizon_frames + 1` = 9 slots (BALANCED). |
| No allocation on the worker path | Holds for encode, RTP, jitter, decode, mixer, `RenderSink`: measured 0 allocations steady state (below). Does not hold for the relay-net seal/open (RT-10). |
| Never replay stale audio | Holds. Underflow writes silence (`playback/callback.rs:83-88`); overflow drops the *new* samples (`playback/writer.rs:22-28`). Late audio is delayed, not repeated (but see RT-07). |
| Mute-safe | The playback and receive side is sound. The send side has the edge-case leak RT-02. |

## Findings

### RT-01 (High, CONFIRMED): worker panics are unsupervised

`capture_worker.rs:107-115`, `render_worker.rs:104-116` and `devices.rs:102-117` run plain
`thread::Builder::spawn` loops with no `catch_unwind`, no heartbeat and no completion handling.
`MediaSystem::drop` joins the workers and discards the result (`system.rs:127-131`). The workspace
does not set `panic = "abort"` (`Cargo.toml:155-158`), so a panic unwinds that one thread and the
daemon carries on.

Scratch program (`workers`): a `NoiseSuppressor` that panics on its 100th frame, attached to the
real capture worker fed by a 10 ms fake callback for 2 s. Result:

- the worker thread finished (`is_finished() == true`), `join()` returned `Err`;
- no `MediaEvent` was emitted (`CaptureLost` never fires; that path only handles stream-health
  errors, `capture_worker.rs:186-198`);
- the microphone meter stayed frozen at the last value (-13.4 dBFS) because nothing clears it;
- the cpal capture callback kept running; `overrun_frames` climbed to 31,200 (every frame dropped);
- the stream handle is still held by the device thread, so the OS microphone indicator stays on.

Effect for a user: the call looks active, the remote hears nothing (fail-closed for privacy, good),
the local user is told nothing, and nothing restarts the worker. A render-worker panic is the same
for incoming audio. A device-thread panic makes every later `DeviceCmd` send fail silently
(`system.rs:243-249` ignores the `Result`), so a capture open hangs until the domain's path timeout.
Panic sites exist: `jitter/buffer.rs:265` (`expect("head slot is occupied")`), `JitterBuffer::new`
asserts, slicing in `resample.rs:156`, and any third-party suppressor.

Fix: wrap each worker step in `catch_unwind(AssertUnwindSafe(..))`; on a panic close every gate
(emit `CaptureLost` or a new `WorkerFailed` event), drop the stream, and respawn the worker from a
supervisor with a restart limit. Independently, add a per-worker heartbeat counter that the control
side checks so a hang (not just a panic) is also detected. Decide explicitly whether the daemon is
`panic = "abort"` (then the client's `ensure_running` restarts it and the OS releases the mic) or
unwinding with supervision; the current middle state is the worst of both.

### RT-02 (High, CONFIRMED): pre-press audio is transmitted at the closed-to-open edge

Mute is enforced only by the gate at the very end of the send path; the capture ring, resampler,
suppressor and `FrameAssembler` run continuously (`capture_worker.rs:144-173`), and
`CaptureShared::set_muted` is never called by the engine (I-02). The frame stamp compares only
`route` and `capture` generations (`send.rs:160-172`). A push-to-talk press changes `ptt_held` and
flips `open`, but does **not** advance `route_generation` (`advance_route_generation` is called
only from route changes and `reset_route`, `relay-core/src/state/routing.rs:40,55`). So nothing
marks frames captured before the press as stale.

Consequences, in increasing size:

1. The first frame after the gate opens is assembled from the previous 0-10 ms of microphone audio
   (the assembler holds a partial frame), plus the resampler/suppressor group delay.
2. Frames read in the same `poll` as the press are sent even if captured before it (`stamps` are
   taken once per poll, `capture_worker.rs:144-145`).
3. After a capture-worker stall the ring holds 40 ms (small callbacks) to about 180 ms
   (`ring_samples_for` with the assumed 4,096-frame callback, `device/backend.rs:82-86`); a press
   during the stall sends all of it, in a burst, labelled as new speech.

Scratch program (`gate`): a closed `CallSender` returns `Closed`; after `GateCell::publish` of an
open snapshot with identical generations, `emit` of a frame stamped *before* the open returns `Sent`.

Why it matters: the product promise is that audio captured while muted or not pushing is never
sent. Case 1 is small (<= ~12 ms) but systematic: every PTT press clips in a fragment of the user's
pre-press speech or noise.

Fix (cheap): require the gate to have been open at the *previous* check as well. In `CallSender`
keep `armed: bool`; on `Closed`/`Stale` set it false; on an open gate with `armed == false`, set it
true and drop this frame (costs one 10 ms frame of onset latency, or trim to the sample where the
open was observed). Better: stamp the gate with a monotone `open_epoch` that increments on every
closed-to-open transition and give frames a capture-sample index (already available as
`timeline.next_sample`); publish the capture index at open time from `CaptureShared::frames_seen`
and drop frames whose index precedes it. Also clamp the backlog the worker will send: discard queued
audio older than N frames at the start of a `poll` after a stall instead of sending it late.

### RT-03 (High, CONFIRMED manifest, effect needs hardware): no real-time promotion on Windows and Linux

`cpal = "0.18.2"` is declared without features (`Cargo.toml:56`; cpal's `default = []`). cpal's
thread promotion (`audio_thread_priority`: MMCSS and `THREAD_PRIORITY_TIME_CRITICAL` on WASAPI,
rtkit/`SCHED_FIFO` on ALSA/PipeWire) is behind the `realtime` feature
(`cpal-0.18.2/src/host/wasapi/stream.rs:716-727`, `host/alsa/mod.rs:919-925`). CoreAudio's IO thread
is real-time by the OS, so macOS is unaffected. No code in `relay-engine` sets a priority or QoS on
`relay-capture` or `relay-render` (grep for `qos`, `priority`, `sched`, `mmcss`: none); they run at
the default priority and wake from `park_timeout` (RT-05).

`docs/evidence.md` already shows that a *normal-priority* worker misses 10 ms several times per
3,000 frames even for a no-op stage, and records that real-time priority "was not tried". The cpal
callback thread missing its deadline is worse: it is a glitch with no buffer behind it.

Fix: enable cpal's `realtime` feature (and `realtime-dbus` on Linux for rtkit), but see RT-15
first. For the workers, see "Thread priority and QoS options" below.

### RT-04 (Medium): UDP send is `Handle::block_on` on the capture thread

`EndpointSink::send` (`net.rs:139-144`) calls `runtime.block_on(endpoint.send_audio(..))` from
`relay-capture`, once per sender per frame, inside the poll loop. `send_audio` takes the endpoint's
`std::sync::Mutex` (`endpoint.rs:164-175`), which the tokio receive task also holds while it
authenticates and decrypts every inbound datagram (`endpoint.rs:184-196`), then seals (3 allocations,
RT-10) and awaits `UdpSocket::send_to`.

What I measured (CONFIRMED, dev Mac, loopback, 92-byte payload, real `MediaEndpoint` pair):

| Measurement | Result |
|---|---|
| Allocations in `EndpointSink::send` on the calling thread | 3 (all from seal; `block_on` itself added none) |
| `send` latency at 10 ms cadence, 3,000 sends, three runs | p50 46-57 us, p99 0.33-2.6 ms, p99.9 2.2-21 ms, max 3.6-30 ms |
| Same shape with a plain thread and std `UdpSocket::send_to` (one run) | p50 51 us, p99 1.2 ms, p99.9 4.9 ms, max 21 ms |
| Burst, 20,000 sends | p50 4-6 us, p99 11-105 us |

So on a quiet loopback the `block_on` adds nothing measurable over a plain send; the millisecond
tails are scheduler noise that a normal-priority thread has regardless. The risk is what happens
when the send does **not** complete immediately, which I could not provoke on macOS loopback:

- If the kernel send buffer is full (Linux with a congested Wi-Fi qdisc, any OS under
  `SO_SNDBUF` pressure), `send_to` parks the capture thread until the socket is writable. There is
  no timeout. During that time the microphone ring fills and overruns, **every other call's** frames
  are delayed behind it (one worker, one loop over senders), and the gate is not re-read. Shutdown
  joins this thread (`system.rs:127-131`) and would wait for it.
- Mute race: the gate is checked twice, the second time immediately before sealing
  (`send.rs:132`). Everything after that (seal, mutex, `block_on`, possible parking) is outside the
  check. With an unbounded send the "an acknowledged mute is already closed for the media thread"
  guarantee has an unbounded exception: a packet sealed at T can leave at T + stall.
- `block_on` also requires a live runtime I/O driver on other threads; the daemon uses a
  multi-thread runtime (`apps/relay-daemon/src/main.rs:19`), so this works, but under a current-thread
  runtime the I/O driver would not be driven while the capture thread waits.

Alternatives, best first:

1. **Non-blocking send on the worker, drop on `WouldBlock`.** `tokio::net::UdpSocket::try_send_to`
   is synchronous and callable from any thread. Scratch program (`trysend`) from a plain thread
   with no runtime context: 0 allocations, p50 35 us at 10 ms cadence, and a 2 s flood of 1,200-byte
   datagrams produced 715,998 successes, 0 `WouldBlock`, never blocked the caller. Add
   `MediaEndpoint::try_send_audio` that takes the lock, seals into a caller buffer (RT-10) and calls
   `try_send_to`; count `WouldBlock` as a dropped frame. Real-time audio should drop, not wait.
2. A dedicated send thread fed by a bounded `rtrb` queue of sealed packets (adds one thread and a
   hop, isolates stalls from capture, still needs the mute check at the send thread).
3. Keep `block_on` but wrap in a deadline and re-check the gate after it returns. Worst option: it
   cannot cancel the syscall.

### RT-05 (Medium, CONFIRMED): idle render worker spins; both workers poll on 2 ms

`render_worker.rs:126-129` sets `next_tick = Instant::now()` and returns when there is no output or
no call. The thread loop then computes `next_tick.saturating_duration_since(now)` = 0 and parks for
`max(0, 200 us)` (`render_worker.rs:109-113`). Scratch program (`workers`, `getrusage`, 3 s each):

| Thread state | Wakeups/s | CPU |
|---|---|---|
| Process baseline | ~0 | 0.01 ms/s |
| Capture worker, no input attached | ~417 | 2.4 ms/s |
| Render worker, no output and no calls | ~3,815 | 10.6 ms/s (~1% of a core) |

(Counted from `ru_nivcsw`; macOS reported the timer wakeups there.) The daemon is meant to run for
hours at login with no call, so this is a constant battery/CPU tax and defeats low-power idle.
While active the worker still wakes every 2 ms (`IDLE_WAIT`) to serve a 10 ms tick, so ticks fire up
to 2 ms late by construction; the tick lateness is added to every output frame's jitter against a
20 ms FIFO target.

Fix: when idle, `park()` (no timeout) and have `Attach`/`AddCall`/`AddSource` (control side) `unpark`
the worker; when active, `park_timeout(next_tick - now)` exactly, and use `unpark` from the
control side for commands. For capture, unpark from the callback is not allowed (it may take a lock
inside the OS), so keep a timer but derive it from the sample count: sleep until
`(frames_needed - queued) / rate`, not a fixed 2 ms.

Platform risk of polling with `park_timeout` (PLAUSIBLE, needs hardware): on Windows the default
timer resolution is 15.6 ms unless raised, so a 2 ms park can last 15 ms (bursty emission, capture
ring has only 40 ms); on macOS a background daemon without an activity assertion is subject to
timer coalescing/App Nap.

### RT-06 (Medium, SIMULATED): fixed 20 ms playout target vs device callback size

`MediaConfig::playout_target_ms = 20` (`system.rs:73`) is the FIFO fill the drift loop steers
toward, whatever the device's callback size. The playback ring is sized from the *assumed* maximum
callback (`ASSUMED_MAX_CALLBACK = 4096` when cpal reports no range, `cpal_backend.rs:16-18,140-144`),
and the callback pops whole buffers. If one callback needs more than the minimum fill, the pop is
short and the remainder is zeros.

Simulation (`drift`, real `RenderSink` + `playback_pair`, simulated clocks, 180 s, last 120 s
counted, cb = frames per device callback, `under` = silence inserted):

| Device rate | cb (ms) | clock offset | min fill before a pull | under (120 s) | applied drift |
|---|---|---|---|---|---|
| 48 kHz | 10 | 0 ppm | 22.2 ms | 0 | +13 ppm |
| 48 kHz | 21 | 0 ppm | 23.8 ms | 0 | -36 ppm |
| 48 kHz | 42.7 | 0 ppm | 42.2 ms | **26 ms** | pinned -300 ppm |
| 16 kHz | 42.7 | 0 ppm | 42.1 ms | **30 ms** | pinned -300 ppm |
| 44.1 kHz | 42.7 | 0 ppm | 42.2 ms | **34 ms** | pinned -300 ppm |

With a 42.7 ms callback the loop sees a mean fill of ~47 ms against a 20 ms target and keeps asking
for faster drain, which is exactly what makes every pull short: continuous micro-underruns for as
long as the stream runs, with the controller saturated. Which real devices use such buffers
(Bluetooth, some ALSA/PulseAudio defaults, Android) needs hardware.

Fix: derive the target from the opened stream: `target = max(20 ms, 1.5 x observed max callback
frames + one render frame)`, publish the max callback size from the callback with a `Relaxed`
`fetch_max` (allowed in a callback), and size the ring from the actual negotiated buffer size, not
4,096. Report the negotiated buffer size in `ActualConfig` for the UI.

### RT-07 (Medium, SIMULATED): a stall adds latency for minutes, then the loop undershoots

The render worker writes one frame per wall-clock tick and catches up at most 4 ticks after a stall
(`render_worker.rs:132-140`). Late audio is queued, not skipped, so a stall of S ms leaves the FIFO
about S ms deeper than before (bounded by the ring). The only thing that removes it is the drift
loop, limited to 300 ppm (0.3 ms of removal per second of audio) and slewed at 5 ppm/s.

Simulation (48 kHz, cb 480, 0 ppm, 30 ms and 100 ms stalls at t = 60 s, mean fill before pull):

| t | 30 ms stall | 100 ms stall |
|---|---|---|
| 59 s | 23.6 ms | 23.6 ms |
| 61 s | 38.8 ms | 36.0 ms |
| 100 s | 32.8 ms | 32.4 ms |
| 140 s | 20.8 ms | 20.5 ms |
| 200 s | 12.9 ms | 13.1 ms |

So ~16 ms extra latency decays over ~2 minutes and then overshoots to ~13 ms against the 20 ms target
(integral wind-up; `drift.rs` clamps the integrator at 1.25 x limit, `drift.rs:12`). A second stall
during the undershoot underruns at a smaller threshold. Repeated stalls ratchet latency up to the
ring size (RT-06).

Fix: add a fast path to `RenderSink`: if `occupancy > target + 2 frames` for N consecutive ticks,
drop one frame of FIFO content (or skip one write) and log the event; reset the integrator when a
large step is observed (anti-windup).

### RT-08 (Medium, PLAUSIBLE on Windows): receive task exits after 100 consecutive errors

`net.rs:83-119`: any `recv()` error sleeps 10 ms and counts; after 100 consecutive errors the task
returns and nothing is reported. On Windows an unconnected UDP socket gets `WSAECONNRESET` from
`recv_from` after a previous `send_to` provoked an ICMP port-unreachable (tokio does not disable
this by default). A peer that is away for ~1 s (100 x 10 ms) therefore permanently deafens the
call: the sender keeps sending, `PeerSocket` still looks alive, no event reaches the domain. I could
not test Windows. The `recv` error type also mixes real socket death with this benign case.

Fix: treat `ConnectionReset`/`ConnectionRefused` as non-errors (and on Windows set
`SIO_UDP_CONNRESET` false), count them, and on a genuinely dead socket emit a media-failed event.

### RT-09 (Low, SIMULATED): drift thresholds are sample counts, not scaled to the rate

`RenderSink::new` passes `DriftConfig { sample_rate: device_rate, ..DEFAULT }` (`sink.rs:26-32`)
but `error_clamp = 192`, `transient_samples = 960` and `kp`/`ki` per *sample* are 48 kHz numbers
(`drift.rs:28-45`). The FIFO target is scaled (`device_rate / 1000 * target_ms`, which also
truncates 44.1 to 44), the thresholds are not. Simulated at 192 kHz, 21 ms callbacks (4,089 frames),
+300 ppm device offset: the loop settles at 195 ppm instead of 300 and inserts 14.7 ms of silence in
120 s, because the sawtooth exceeds 960 samples (5 ms at that rate) and is discarded as a transient.
At 10 ms callbacks the same rate works (min fill 10.7 ms at +300 ppm, no underrun). Most devices run
at 44.1/48 kHz, so low impact.

Fix: express `error_clamp`, `transient_samples` and gains in milliseconds and convert with the
device rate; compute the target in samples with rounding.

### RT-10 (Low, CONFIRMED): per-packet allocations in relay-net

Counted with a thread-local counting allocator around the real functions:

| Operation | Allocations | Bytes (92-byte RTP, 193-byte datagram) | Where |
|---|---|---|---|
| `seal_audio` (send) | **3** | 450 | `media/datagram.rs:32` (body), `session/mod.rs:134` (plaintext), `session/mod.rs:139` (record) |
| `MediaReceiver::receive` (recv) | **3** | 365 | `session/mod.rs:165` (plaintext), `session/mod.rs:184` (`rest[1..].to_vec()`), `media/receiver.rs:160` (`payload.to_vec()`) |
| `EndpointSink::send` end to end | 3 | 450 | the seal; `block_on` adds none |
| `CallSender::emit` with a null sink, gate open | **0** | 0 | encode + RTP into arrays |
| `CallReceiver::ingest` + `render` (jitter, Opus decode, meters), 350 packets | **0** | 0 | |
| `RenderSink::write` incl. drift update, 380 ticks | **0** | 0 | |

`docs/engine.md` says relay-net allocates "the sealed datagram (send) and the payload Vec
(receive)": the real count is three each way. At 100 packets/s/direction/call and 8 calls that is
4,800 small allocations/s on two worker threads; each is ~50-100 ns, so the cost is the allocator's
tail (a shared arena lock with tokio threads), not CPU. Seal itself costs 2.4 us.

Zero-allocation design (one change in `relay-net`, no wire change):

- `SecureSession::seal_into(kind, parts: &[&[u8]], out: &mut [u8]) -> Result<usize>`: write the
  32-byte header into `out`, build the inner plaintext (`header copy || kind || call || generation
  || RTP`) in a caller-owned scratch array on the worker (`[u8; UDP_RECORD_MAX_BYTES]`, already
  bounded to 1,200), and call `StatelessTransportState::write_message(nonce, scratch, &mut
  out[32..])`. No `Vec`, no `truncate`.
- `SecureSession::open_into(header, record, scratch: &mut [u8]) -> Result<OpenedRef<'_>>` returning
  borrowed `kind` and `body: &[u8]` into `scratch`; `MediaReceiver::receive_into` returns
  `Received::Audio { payload: &[u8] }` borrowing it.
- `MediaEndpoint::recv_into(&self, slot: &mut Ingress)` copies the payload straight into the
  preallocated `Ingress` array (the only copy), replacing `AudioPacket { payload: Vec<u8> }`.
- Keep the existing allocating APIs for the control lane and tests.

### RT-11 (Low): capture worker loses frames silently on suppressor errors

`capture_worker.rs:153-155`: `let Ok(progress) = suppressor.process_into(..) else { return; }` drops
the 10 ms frame with no counter or log, and does not advance `timeline.next_sample`, so RTP
timestamps close the hole (the receiver sees a 10 ms excision, not a loss). Also `progress.consumed`
is ignored (`:156`), although the trait allows a backend to consume less than offered; the rest of
`raw` is lost. Both are latent with `OffSuppressor`. Fix: count errors in `Meters`, advance the
timeline for dropped frames, and loop on `consumed`.

### RT-12 (Low): AGC can latch to NaN

`dsp/filters.rs:107-134`: if `frame` contains NaN, `level_db` is NaN, the `<` gate test is false,
`wanted` is NaN, `clamp` of NaN stays NaN, and `gain_db` becomes NaN permanently; every later
sample is multiplied by NaN until `reset()`. Device samples are sanitised on entry (`sample.rs`) and
the high-pass drops non-finite input, so only a faulty suppressor/AEC upstream can trigger it.
Fix: `sanitize` after the NS stage, and guard `if !level_db.is_finite() { return; }` in the AGC.

### RT-13 (Low): bounded command queues drop silently

`Inbox` capacity is 64 (`system.rs:36`). `retire` ignores failures of `RemoveSender`, `RemoveCall`
and `RemoveSource` (`system_calls.rs:211-217`); a lost `RemoveSender` keeps the call's encoder and sender slot
forever (its gate is retired, so nothing is sent). `try_activate` pushes `AddCall`
then `AddSender` (`:149-157`); if the second fails the receiver stays installed and the call is not
activated. The render worker drops `AddCall` silently when the mixer is full
(`render_worker.rs:212-219`), `RenderCmd::AddSource` can grow `sources` past its reserved capacity
(`:227`, a reallocation on the render thread), and `AddSender` is dropped when 8 senders exist
(`capture_worker.rs:203-207`). Admission caps calls at 8 so these are defensive paths, but they
fail without an event. Fix: return the pushed command on failure and retry or surface a
`MediaEvent`; reserve capacity for sources.

### RT-14 (Low): admission load meters

`LoadMeter` is updated only when a worker does work (`capture_worker.rs:177-182`,
`render_worker.rs:176-177`), so after the last call ends it keeps the last EWMA; a transient (for
example a long send stall, RT-04, folded into "time per frame") can make `admit` refuse the next
call (`load.rs:84-99`: budget 5 ms) until some work is recorded again. Fix: decay the meters when
the worker is idle, and exclude time blocked in the sink.

### RT-15 (Low): enabling cpal `realtime` could pause the microphone

With the feature on, cpal reports a failed promotion through the stream *error callback*
(`alsa/mod.rs:919-925`). `health_callback` maps any kind other than device-lost/invalidated/xrun to
`StreamState::BackendError` (`cpal_backend.rs:43-52`) and `recovery_for` turns every non-running
state into `PauseAndPrompt` (`health.rs:79-84`). A machine without rtkit/permission would then drop
the microphone at start. Fix before enabling RT-03: ignore a promotion failure kind (log once off
the audio thread) and keep running.

### RT-16 (Low): ingress overflow policy

`net.rs:102-107` drops the packet being pushed when the 256-slot queue is full, so after a render
stall the queue keeps the *oldest* 2.5 s of packets and refuses the newest. The jitter buffer
rejects the old ones as late, but until it has caught up the call hears nothing new. Cheap fix:
drain the queue from the producer side by calling a `force_push` (drop oldest) or size the queue
to 10-20 packets and count overflow as stale.

### RT-17 (Low): stale documentation

`docs/engine.md` Known gaps still say `StreamResampler` is bypassed at 48 kHz and ignores drift, and
that the microphone test, microphone gain and non-`off` suppression return `Unsupported`.
Commit `727d09a` made `StreamResampler::new` always adjustable (`resample.rs:41-45`), `live.rs`
implements the microphone test, and `SetGain` exists. The "no steady-state allocation guarantee"
bullet understates relay-net (RT-10). `docs/audio.md` correctly lists real-callback allocation and
hardware as unmet gates.

### RT-18 (Low, new in `e887519`): tones vs an idle render worker

`TonePlayer::render_into` is allocation- and lock-free (`tones/mod.rs:172-206`; its allocation test
passes). But the render worker returns early whenever `output.is_none()` or there are no calls and
no monitor (`render_worker.rs:126-129`), so `Mixer::add_tones` would never run for an incoming ring
(there is no call yet). `docs/audio.md` notes the quantum must run while a tone plays; the worker
needs a "tone active" condition. The player also decodes the sound assets in `TonePlayer::new`, so
it must be built on the control side, not on the worker.

## Informational

- **I-01 Denormals.** No FTZ/DAZ is set. The `f64` high-pass IIR decays to subnormal after ~1.4 s of
  digital silence (about 70,000 samples at its pole radius); on x86 that costs a few tens of
  microseconds per 480-sample frame at worst. Not measured; not worth a fix now.
- **I-02 Production mute is gate-only.** `CaptureShared::set_muted`, `PlaybackShared::set_muted`
  and `request_flush` are never called outside tests, so the reader-side discard and the callback
  mute are not exercised in production; the tests prove them in isolation. See RT-02 for the
  consequence.
- **I-03 Two drift controllers.** `PlaybackSink::update_drift` (`playback/writer.rs:72-76`) is a
  proportional-only, explicitly "not validated" controller used by the loopback pipeline; the engine
  uses `RenderSink` with the PI `DriftEstimator`. Keep only one.
- **I-04 Limiter.** Instant attack with per-sample gain steps (`limiter.rs:29-46`) is safe for
  overload but not transparent; any audible-quality review needs a listening test.
- **I-05 Destructors on worker threads.** `SetSuppressor`, `Detach`/`Attach` (drop of the
  `CaptureChain`), `RemoveSender` (drop of an Opus encoder) and an `arc_swap::Guard` drop that
  releases the last reference to a replaced `GateTable` run `free` on the worker. Bounded and
  small; ignore unless the allocator shows tails.
- **I-06 Ring capacity with unknown buffer sizes.** With `BufferSize::Default` and no reported
  range, rings are `2 x 4096 + rate/100` = 8,672 samples (180 ms at 48 kHz) in each direction.
  That is the worst-case latency ceiling of RT-02 and RT-07.
- **I-07 `fan_out`** (`convert/channels.rs:57-61`) uses `chunks_exact_mut`, so a callback buffer
  whose length is not a multiple of the channel count would leave the trailing partial frame
  unwritten (stale). cpal always delivers whole frames; harmless.

## Atomics and ordering

| Item | Ordering | Verdict |
|---|---|---|
| Gate read before sealing: `GateCell::load` (`gate.rs:93-105`) = `retired.load(Acquire)` then `ArcSwap::load` of an immutable `Arc<GateTable>` / `Versioned`; publisher stores a whole new table (`gates.rs:58-63`) | Acquire / ArcSwap (stronger) | **Correct.** `open`, `route` and `capture` live in one `Copy` snapshot behind one pointer, so a reader can never see a torn combination; the actor publishes before replying, so an acknowledged mute happens-before any later read. Two reads per frame (`stamp`, `refuse`) are each internally consistent. The remaining problems are not ordering: RT-02 (semantics) and RT-04 (time between check and send). |
| `GateCell::retire` / `retired` | Release store, Acquire load | Correct (flag only gates a read of immutable data). |
| `GatePublisher` read-clone-store | none needed | Single writer (the actor); correct. |
| Meters: `Level`, `capture_overruns`, `render_underruns`, `RecvStats`, `SendStats`, `LoadMeter` | Relaxed | Correct: independent monotonic counters or last-value cells, no cross-variable invariant; `LoadMeter` has a single writer. Readers may lag by a poll. |
| `meters.monitor` (Relaxed) | Relaxed | Correct: the data it guards travels through an `rtrb` ring (Acquire/Release). |
| `CaptureShared::muted`, `PlaybackShared::muted` | Release store, Acquire load | Stronger than needed (a Relaxed flag suffices, nothing is published with it); harmless. |
| `PlaybackShared::flush_generation` | `fetch_add(AcqRel)` / `load(Acquire)` | Correct; over-discards (never under-discards) audio pushed between request and observation. |
| `StreamHealth::report` | `fetch_max(AcqRel)` | Correct and monotone (a lost device is never downgraded). |
| `stop` flag | Release store, Acquire load | Correct; workers also need an `unpark` (done in `Drop`). |
| `rtrb` SPSC rings | Acquire/Release internal | Sound as used; each ring has exactly one producer and one consumer (`Inbox` serialises producers with a mutex on the control side only). |

## Queue sizing

| Queue | Capacity | Time at 48 kHz | Assessment |
|---|---|---|---|
| Capture ring (`ring_samples_for`) | max(40 ms, 2 x max callback + 10 ms) | 40 ms (small callbacks) to 180 ms (unknown buffer) | OK for a 2 ms poll; a worker stall > ring loses audio (counted, timeline exact). Large rings raise RT-02/RT-07 ceilings. |
| Gap events | 64 | n/a | A full queue only extends the pending gap; correct. |
| Playback ring | same formula | 40-180 ms | Target 20 ms is below one callback for big buffers (RT-06). |
| Playout target | 20 ms | 2 frames | Reasonable for 10 ms callbacks; no adaptation. |
| Jitter buffer (BALANCED) | start 2, max 4 frames, slack 4, 128 slots | 20-40 ms margin, 80 ms horizon | Sound. Remote-clock skew is absorbed by margin and loss/stretch, there is no resampling toward the remote clock; at +100 ppm that is one frame per 100 s (inferred from `buffer.rs`, not run). |
| Ingress (per peer) | 256 packets of ~1.2 KB (~300 KB) | 2.56 s of one stream | Too deep for real time (RT-16). |
| Commands (`Inbox`) | 64 per worker | n/a | Enough for 8 calls; failures silent (RT-13). |
| Monitor | 8 frames | 80 ms | Drops the newest; the mixer keeps only the latest frame. Fine. |

## Drift correction at all rates

Verified directly (`rates`): the capture chain over 10 s of tone at 8, 11.025, 16, 22.05, 32, 44.1,
48, 88.2, 96, 176.4 and 192 kHz with odd callback sizes produced 999-1,000 frames, no timeline
gaps, peak 0.500 preserved, no overruns; reported delays 384 samples (8 kHz) down to 16 (192 kHz),
0 at 48 kHz (fixed bypass). The adjustable 48k-to-device resampler used by playback produced
80,000-1,920,000 samples per 10 s within 8 samples of ideal, and `set_drift_ppm(+300)` added 143 of
144 expected samples at 48 kHz and within 1 sample at every other rate. Sign handling is correct
(`sink.rs:69`: a too-full FIFO needs fewer output samples, so the estimator's "consume faster" is
inverted). The loop is a PI with a 2 s filter, 5 ppm/s slew and a 300 ppm clamp
(`DriftConfig::DEFAULT`); at 48 kHz with 10 ms callbacks it held the pre-pull fill within 10.7-39.2 ms for device
offsets of -300 to +300 ppm with no underrun, and a +450 ppm device (beyond the clamp) lost ~20 ms
per 120 s as expected. Limits found: RT-06, RT-07, RT-09. The capture side uses `new_fixed`
(no drift correction) by design: the capture clock is the sender's clock.

## Thread priority and QoS options

| OS | Callback thread | Worker threads (`relay-capture`, `relay-render`) | Notes |
|---|---|---|---|
| macOS | CoreAudio IO thread, already real time | `pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE)` or `thread_policy_set(THREAD_TIME_CONSTRAINT_POLICY)` (period 10 ms, computation ~1-2 ms, constraint ~5 ms) | `evidence.md` found no improvement from QoS in single runs and never tried time-constraint. A headless daemon should also hold `ProcessInfo.beginActivity(.userInitiated + .latencyCritical)` during a call to avoid App Nap and timer coalescing. |
| Windows | WASAPI via cpal: needs the `realtime` feature (MMCSS "Pro Audio", fallback `THREAD_PRIORITY_TIME_CRITICAL`) | MMCSS `AvSetMmThreadCharacteristicsW("Pro Audio")` per worker; `CreateWaitableTimerExW(HIGH_RESOLUTION)` or `timeBeginPeriod(1)` instead of `park_timeout` | Default timer tick 15.6 ms makes a 2 ms park unreliable (RT-05). |
| Linux | ALSA/PipeWire via cpal: needs `realtime` (+ `realtime-dbus` for rtkit) | `audio_thread_priority::promote_current_thread_to_real_time(frames, rate)` (rtkit, or `SCHED_FIFO` with `RLIMIT_RTPRIO`) | The crate is already in cpal's dependency tree. Needs a documented fallback when rtkit is absent (RT-15). |
| Android | AAudio low-latency callback (high priority by the platform) | `Process.setThreadPriority(THREAD_PRIORITY_URGENT_AUDIO)` from JNI; avoid `SCHED_FIFO` | Foreground-service state decides throttling. |
| iOS | Audio unit IO thread | QoS user-interactive; keep `AVAudioSession` active | Out of current scope. |

None of these was exercised; every one needs a measurement (p99/max wake lateness, glitches per
hour) before it is adopted. Because the repo has no priority code at all, start with one
`media::thread::promote(kind)` shim called at the top of each worker loop, behind a config flag, so
the effect can be A/B tested on hardware.

## Resource budget claims

- `docs/engine.md` states the admission defaults (50% of 10 ms per worker, 400 us assumed first-call
  cost) are "design defaults, not measurements". Agreed. For scale, on the measurement machine
  `CallSender::emit` (Opus VoIP 32 kbit/s complexity 5 + RTP, null sink) averaged 54-65 us per
  frame per call in quiet runs and 90-330 us in two noisy runs; seal cost 2.4 us; send 4-6 us p50.
  Extrapolating (not measured) 8 senders is about 0.5 ms of a 10 ms frame, so 400 us per call is
  roughly 6x conservative on this CPU, and the budget would be reached at the call cap before the
  CPU is.
- `docs/evidence.md` quotes p99 <= 97 us per frame and <= 0.7% of a core for the noise-suppression
  candidates; those are compute figures on this class of machine, and the same document records that
  scheduling, not compute, causes the misses. Idle cost of the daemon is dominated by RT-05
  (~1.3% of a core with no call).
- Memory: `Ingress` ~1.2 KB x 256 per peer, jitter 128 slots x ~1 KB per call, capture and playback
  rings 8-40 KB each. No unbounded buffers were found on the audio paths.
- The "no steady-state allocation" claim holds for the audio crate and the engine media path except
  relay-net (RT-10); "zero allocations on real cpal threads" is unproven (existing gate).

## What is sound

- Callback discipline and the allocation tests (re-run, passing); wait-free rings with exact index
  accounting across overrun, mute and gap events (`reader.rs`, `callback.rs`); a single gap queue
  that cannot grow.
- Underflow = silence, overflow = drop, never replay; muted playback still drains the ring; flush
  generation is conservative.
- Device loss is fail-closed with explicit user action and no automatic fallback.
- Sample-format normalisation (non-finite to silence, saturating output), channel selection, the
  resampler's accounting and drift accuracy at every tested rate, the mixer and limiter ceilings,
  the fixed-order DSP chain.
- The gate read itself: one pointer, one immutable snapshot, double-checked around encode, per-call
  stale detection by route and capture generation.
- Per-call state is preallocated; `CallSender`, `CallReceiver`, `RenderSink` allocate nothing in
  steady state (measured).
- Poison-tolerant mutexes (`unwrap_or_else(into_inner)`) so one panic does not cascade through the
  control side.

## Needs real hardware (not claimed anywhere above)

1. Allocation and priority behaviour of the real cpal threads on CoreAudio, WASAPI, ALSA/PipeWire,
   AAudio.
2. Effective wake jitter and glitch rate of the workers per OS, with and without the options in the
   priority table, under CPU load and with the daemon in the background.
3. Whether RT-06 occurs: the callback sizes real built-in, USB, Bluetooth and HDMI devices negotiate
   with `BufferSize::Default`.
4. Real device clock offsets (the simulation used constants up to 450 ppm), drift on USB and
   Bluetooth, and sleep/resume and unplug/replug behaviour of the streams.
5. Whether `send_to` can block or fail under Wi-Fi/queue pressure on each OS (RT-04), and the Windows
   `WSAECONNRESET` behaviour (RT-08).
6. Mouth-to-ear latency, long-run (hours) stability, and CPU with 8 simultaneous calls.

## Reproduction notes

Scratch programs (kept outside the repository, not committed) were a small cargo project that
path-depends on a copy of the tree at `e887519`: `allocs` (thread-local counting allocator around
`seal_audio`, `MediaReceiver::receive`, `CallSender::emit`, `EndpointSink::send` over a real
loopback `MediaEndpoint` pair after a real Noise handshake, `CallReceiver`, `RenderSink`; latency
histograms), `workers` (idle wakeups via `getrusage`; panicking suppressor), `drift` (event-ordered
simulation of `RenderSink` + `playback_pair`), `rates` (capture chain and resampler accounting),
`gate` (closed-to-open stamp check) and `trysend` (`try_send_to` from a plain thread). Existing
test re-run: `cargo test --release -p relay-audio --test allocations` (9 passed). One of my own
baseline measurements (std `UdpSocket`) stalled in several later runs of the scratch program; it is
reported from the single run that completed and was not investigated.

## Resolutions

Commits are on `main`; tests named are in `crates/relay-engine/tests` unless a crate is given. All
simulation and fake-device evidence; nothing was measured on real hardware.

- **RT-01 Resolution** (`a7704ff`, `53c6a76`): both workers run under `media/supervise.rs`: a
  panicking step is caught, the worker is reset (capture drops the stream, swaps the suspect
  suppressor for pass-through and reports `CaptureLost`; render removes the call being rendered),
  `MediaEvent::WorkerFailed` is emitted and the daemon shows `MEDIA_WORKER_FAILED`; a fourth
  panic within 60 s is final; a watchdog reports a worker with no heartbeat for 3 s; join results
  are logged at shutdown; a failed device-thread send now reports `CaptureFailed`. Tests:
  `media_supervision`. Not done: the device thread itself is not wrapped in `catch_unwind`, and
  `panic = "abort"` was not chosen (supervision is).
- **RT-02 Resolution** (`a7704ff`): a frame is sent only if the gate was open at the start of its
  poll and of the previous poll, the worker was not more than 20 ms late, and its audio is newer than
  the last unclean poll plus conversion and suppressor delay (`Emit::Unarmed`). Tests:
  `media_arming` (fail without the fix, including the stalled-worker scenario on the real capture
  worker). Residual: a close and re-open inside one 2 ms poll interval is undetectable without an
  open epoch in `GateSnapshot` (relay-core, not changed).
- **RT-03 Resolution** (`c7aba91`, `a7704ff`): cpal `realtime` enabled (no `realtime-dbus`, so no
  rtkit); `relay_audio::priority` promotes the workers (macOS QoS user-interactive; Windows and
  Linux through `audio_thread_priority`), result logged, `MediaConfig::promote_workers` switches it
  off. Unmet: effect unmeasured on every OS; the Windows/Linux paths were not compiled or run here;
  Windows timer resolution and macOS App Nap are not addressed.
- **RT-04 Resolution** (`2b03481`, `a7704ff`): `MediaEndpoint::try_send_audio` (stack buffer,
  `try_send_to`); `EndpointSink` drops on `WouldBlock` and counts it (`SendStats::send_dropped`,
  `Emit::Dropped`, RTP sequence still advances). Test: `media_capture_faults`. A congested real
  socket was not provoked.
- **RT-05 Resolution** (`a7704ff`): idle workers sleep 500 ms and are unparked by their command
  queue; the render worker parks exactly to its next tick while active. The capture poll stays a
  fixed 2 ms while a microphone is attached (not derived from the sample count).
- **RT-06 Resolution** (`a7704ff`): `PlaybackShared::max_callback_frames` and an adaptive target in
  `RenderSink`. Test: `media_sink_adapt::a_large_device_callback_raises_the_target_and_ends_the_underruns`
  (the fixed target underruns in the same simulation). The ring is still sized from the assumed
  maximum callback and `ActualConfig` does not report the negotiated buffer size.
- **RT-07 Resolution** (`a7704ff`): `RenderSink` sheds a frame at a time while the fill stays above
  target; test `media_sink_adapt::a_stalls_extra_latency_is_shed_in_seconds_not_minutes`. The
  integrator is not reset (no wind-up from a stall); the drift loop's own slow start-up transient is
  unchanged.
- **RT-08 Resolution** (`2b03481`, `a7704ff`): `ConnectionReset`/`ConnectionRefused` receive errors
  are counted and skipped inside `MediaEndpoint`; a genuinely failing socket emits a final
  `WorkerFailed` (network) after 100 consecutive errors. Not done: `SIO_UDP_CONNRESET` on Windows,
  and no test (needs Windows).
- **RT-09 Resolution** (`a7704ff`): drift thresholds and gains are scaled to the device rate.
  `media_sink_adapt::drift_thresholds_hold_at_a_high_device_rate` passes but does not fail without the
  scaling (the adaptive target already avoids the underrun), so it does not prove that part.
- **RT-10 Resolution** (`2b03481`): `SecureSession::seal_into`/`open_into`,
  `MediaReceiver::receive_ref`, `MediaEndpoint::recv_into`/`try_send_audio`; wire format unchanged.
  Test: relay-net `media_alloc` (0 allocations each way, old receiver accepts the new sealer).
- **RT-11 Resolution** (`a7704ff`): suppressor errors are counted (`Meters::suppressor_errors`) and
  keep the RTP timeline exact; `consumed` is honoured. Test: `media_capture_faults`.
- **RT-12 Resolution** (`c7aba91`): the AGC scrubs non-finite samples. Test: relay-audio
  `chain::agc_does_not_latch_to_nan_after_a_poisoned_frame`.
- **RT-13 Resolution** (`a7704ff`, partial): full-queue and full-mixer drops now log, `sources` has
  reserved capacity and is capped. Not done: commands are still not retried or surfaced as events.
- **RT-14 Resolution** (`a7704ff`): the load meters reset when a worker has nothing to serve; time
  blocked in the sink no longer exists (non-blocking send). Test: `media_capture_faults`.
- **RT-15 Resolution** (`c7aba91`): `ErrorKind::RealtimeDenied` sets `StreamHealth::realtime_denied`
  and does not change the stream state. Test: relay-audio `device`.
- **RT-16 Resolution** (`a7704ff`, partial): ingress depth 256 to 32 (about 0.3 s per stream);
  the queue still drops the newest packet on overflow, counted in `NetStats::overflow`.
- **RT-17 Resolution** (`03a380d`): `docs/engine.md` Known gaps, `docs/audio.md` and `docs/gates.md`
  updated.
- **RT-18 Resolution** (`fe749fc`): `RenderCmd::SetTones` and `MediaSystem::set_tone_player`/
  `wake_render`; the render worker keeps ticking while a tone plays. Test: `media_tones`. Nothing in
  the daemon starts a tone yet.
- **I-01 to I-07**: unchanged by design (I-03 and I-05 remain as described; I-02 now matters less
  because RT-02 no longer relies on the gate alone being the only barrier).
