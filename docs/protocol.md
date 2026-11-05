# Media wire protocol (relay-protocol)

Living notes for the sans-IO media-wire crate. The crate has no clocks, sockets
or threads; callers pass arrival times and drive playout ticks.

## RTP profile

RTP v2, 12-byte fixed header, Opus at a 48 kHz clock, payload type 111, one
Opus frame per packet (10 ms = 480 ticks, 20 ms = 960). No CSRC, extension
or padding: such packets are rejected with a typed `RtpError`. Payload is
1..=1000 bytes. The marker bit flags the first packet of a talk spurt.
`RtpProfile::parse` borrows the payload from the datagram; `write` serializes
into a caller buffer. Differential tests compare against an independent RFC 3550 reference
model and fixed known-answer vectors; no third-party RTP crate is used.

## Counters

- `SeqExtender` / `TimestampExtender` map 16/32-bit values to monotonic u64.
  The first value starts one wrap above zero so slightly earlier reordered
  packets cannot underflow. `resolve` is pure; `commit` moves the reference.
  Reset the extenders (or the buffer) on a new stream generation.
- `ReplayWindow<WORDS>` is a sliding bitmap (64 numbers per word).

## Packet numbers

There is no packet-number type here. The secure-datagram nonce, its per-lane counter
and the replay window live in `relay-net` (`session` for control records,
`media::ReplayWindow` for audio), which is the only implementation of the primitive.
`seq::ReplayWindow` stays for the jitter buffer's RTP sequence numbers.

## Jitter buffer and playout planner

`JitterBuffer<T>` is generic over the stored payload (a pooled buffer, an
index, `Bytes`). Memory is fixed at construction (128 slots). The caller calls
`insert(PacketMeta, payload)` per authenticated packet and `poll()` once per
render frame. Each poll yields data:

- `Play { payload, sequence, timestamp, lost_before }`
- `Conceal { run_frames, fade, cause: Loss | Stretch }` - one PLC frame
- `Skip(Idle | Prefill | Gap | Interrupted)` - silence, no decoder

Rules:

- Timestamps drive playout; sequence numbers detect loss. A contiguous sequence
  with a timestamp jump is a deliberate `Gap`, not loss.
- Playout starts `margin` frames after the first packet. Late packets
  (`timestamp + frame <= playhead`) are dropped and never appended after newer
  audio. Admission is limited to `max margin + reorder slack` frames ahead;
  further packets are `TooFar`.
- Delay only grows through prefill and one-frame `Stretch` holds, both capped
  at the margin ceiling. It shrinks only inside `Gap` silence.
- Concealment lasts `max_plc_frames` (3 = 30 ms), then `fade_frames` of fade,
  then the stream is `Interrupted` and the next packet starts a fresh segment
  (no backlog playback). Resuming with a contiguous sequence counts as
  `intentional_silence`, not loss.
- `MarginController` grows by one frame per deadline miss (cooldown-limited) and
  shrinks one frame after a long miss-free stretch.
- Presets: `BALANCED` (2 frames start, 1..4), `AGGRESSIVE`, `STABLE`. These are
  hypotheses; margins are whole frames, so 15 ms rounds up to 2 frames.
- `stats()` follows the documented counter semantics. A late packet can count
  in both `late_drop` and `network_missing`.

## Drift estimator

`DriftEstimator::update(occupancy_samples, elapsed_samples)` returns a ratio
correction in ppm (positive = consume input faster). Slow PI loop on low-passed
occupancy: error clamped, integral clamped, transient outliers kept out of the
filter, output saturated at +-300 ppm and slew-limited to 5 ppm/s. Feed it a
sample-granular fill level (render FIFO), not a packet count, otherwise the
frame quantisation hides small errors. It is a separate loop from the margin
controller. Simulated at 0, +-50, +-100 and +-300 ppm; see `tests/drift_sim.rs`.
Tuning is from simulation only, not hardware.

## Frame assembler

`FrameAssembler<T>` re-chunks arbitrary pushes into fixed frames (8 ms DSP
output into 10 ms codec input). A frame is released as soon as enough samples
exist; added latency is below one output frame and nothing is batched to a
common multiple.

## Integration notes

- Authenticate and replay-check before `insert`; the buffer assumes trusted
  headers. Check SSRC and stream generation outside; call `reset()` on change.
- One buffer per incoming stream generation; do not share between calls.
- Each `Played`/`Conceal`/`Skip` is exactly one render frame of output.
