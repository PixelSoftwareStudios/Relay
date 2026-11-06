# Performance

Runtime cost only (idle power, per-call CPU, start-up, memory). Build time,
test speed and binary size are tracked elsewhere. Numbers come from real runs;
absent entries mean the run has not happened.

Machine: Apple M1 Pro, macOS 14.7.2, aarch64, release profile. Other builds were running
on the machine during the 2026-11-06 session, so timings below marked "noisy" are upper bounds.

## Idle power

### UI

`crates/relay-ui/tests/idle_short.rs` draws every page and every Settings category on short
viewports (360x400, 320x640, 720x400 desktop), with 120 frames of advancing time, and asserts
the repaint delay egui asks for: at least 60 s without a call (nothing repaints), about 1 s
with a connected call (the call clock), also under a resting pointer.

Result: all combinations pass on the current tree. The reported constant repaint on scrolling
Calls pages did not reproduce without notices; the notices work in progress moved the fade
timer (`next_fade_ms`) inside the strip block, which is the only unconditional
`request_repaint_after` that was on that path. The tests now guard it.

### relayd (private instance, temp socket, `--listen-port 0`)

Reproduce (the instance never touches the per-user socket):

```
cargo build --release -p relay-daemon
python3 idle.py target/release/relayd 30 --listen-port 0   # spawn, wait for socket, sample 30 s
```

| Measure | Value |
|---|---|
| Process start to socket present | 44-53 ms |
| Threads idle | 19-20 |
| RSS idle | 16-17 MB |
| CPU over 33 s (user + sys, including start-up) | 66-81 ms |
| Voluntary wakeups over 33 s | 31 (mDNS on) / 47 (off), about 1 per second |

Conclusion: the daemon is already quiet when idle. The 25 Hz meter and statistics streams run
only while a client holds a stream (per-client tickers in `service/rpc`), and the only
periodic work is the 2 s device rescan, the 5 s peer upkeep and the client sweeper. No timer was
removed: the whole idle budget is below 0.3 % of one core, and gating the device rescan on
attached clients would change what a plugged-in headset shows to the next client.

## Audio per 10 ms frame

Earlier quiet-machine numbers are in `docs/evidence.md` (opus encode 48.9 us, decode 8.95 us,
resample 11.8 us, mixer 4.8 us, high-pass 2.2 us, AGC 1.0 us). A busy-machine rerun of
`cargo bench -p relay-benches --bench audio_stages` gave 3-5x higher medians with wide
intervals (noisy, rejected as evidence). The whole chain is about 75 us per 10 ms frame per
call, which is 0.75 % of one core; no stage justifies an optimisation. No code was changed.

Left unchanged on purpose: AEAD choice (protocol decision: report only; ChaCha20-Poly1305 versus
AES-GCM with hardware AES on aarch64 would need a new Noise pattern name), Opus complexity 5.

## Not done, needs a quiet machine or real devices

- Start-up timing to first UI snapshot and egui font parse cost (CJK): needs a release
  relay-desktop run on a quiet machine.
- RSS of relay-desktop idle and in a call; avatar and texture cache behaviour.
- Libraries (mimalloc, smallvec, parking_lot, ahash, wide): no profile shows allocation or
  lock cost, so none is added. Re-evaluate only with a profile that points at one.
