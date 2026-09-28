/**
 * Glass-to-glass video latency: from the moment the PC captured a desktop image to the moment
 * this iPad shows it. Every displayed frame carries its RTP timestamp, which the PC derives
 * from the capture time; ping/pong exchanges map the PC's RTP clock onto this iPad's clock.
 *
 * Unlike the per-stage estimate, this includes every queue on the way (PC send queue, network,
 * jitter buffer, decoder, compositor). It needs `requestVideoFrameCallback` with `rtpTimestamp`;
 * where the browser does not provide them, `take()` returns null and the estimate is used.
 */

/** RTP video clock ticks per millisecond (90 kHz). */
const TICKS_PER_MS = 90;
/** Clock samples kept; the one with the shortest round trip is used (about 16 s of pings). */
const CLOCK_SAMPLES = 16;
/** Values outside this range can only come from a clock that does not belong to the stream. */
const PLAUSIBLE_MS = { min: -30, max: 5_000 };

interface ClockSample {
  /** PC RTP clock when it answered the ping. */
  rtp: number;
  /** Estimated local time (performance.now()) at that instant: the middle of the round trip. */
  localMs: number;
  roundTripMs: number;
}

export class FrameLatencyMeter {
  private clock: ClockSample[] = [];
  private shown: number[] = [];
  private video: HTMLVideoElement | null = null;
  private handle = 0;

  /**
   * A pong from the PC. `sentAt` and `receivedAt` are `performance.now()` values of the ping and
   * of the pong; `rtp` is the PC's RTP clock when it answered.
   */
  onPong(sentAt: number, receivedAt: number, rtp: number): void {
    const roundTripMs = receivedAt - sentAt;
    if (!Number.isFinite(rtp) || !(roundTripMs >= 0)) return;
    this.clock.push({ rtp: rtp >>> 0, localMs: sentAt + roundTripMs / 2, roundTripMs });
    if (this.clock.length > CLOCK_SAMPLES) this.clock.shift();
  }

  /** Follows the frames presented by this element (null stops). */
  observe(video: HTMLVideoElement | null): void {
    if (video === this.video) return;
    if (this.video && this.handle) this.video.cancelVideoFrameCallback(this.handle);
    this.video = video;
    this.handle = 0;
    if (!video || typeof video.requestVideoFrameCallback !== 'function') return;
    const onFrame = (_now: DOMHighResTimeStamp, metadata: VideoFrameCallbackMetadata) => {
      if (this.video !== video) return;
      this.record(metadata);
      this.handle = video.requestVideoFrameCallback(onFrame);
    };
    this.handle = video.requestVideoFrameCallback(onFrame);
  }

  /** Median latency of the frames shown since the previous call; null when unmeasurable. */
  take(): number | null {
    const values = this.shown;
    this.shown = [];
    if (!values.length) return null;
    values.sort((a, b) => a - b);
    const middle = values.length >> 1;
    const median = values.length % 2 ? values[middle]! : (values[middle - 1]! + values[middle]!) / 2;
    return Math.max(0, median);
  }

  /** A new stream starts: its RTP clock has a new origin, so earlier samples no longer apply. */
  reset(): void {
    this.clock = [];
    this.shown = [];
  }

  private record(metadata: VideoFrameCallbackMetadata): void {
    if (typeof metadata.rtpTimestamp !== 'number') return;
    let reference: ClockSample | undefined;
    for (const sample of this.clock) {
      if (!reference || sample.roundTripMs < reference.roundTripMs) reference = sample;
    }
    if (!reference) return;
    // Signed 32-bit difference: correct across the RTP clock wrapping around.
    const ticks = ((metadata.rtpTimestamp >>> 0) - reference.rtp) | 0;
    const capturedAt = reference.localMs + ticks / TICKS_PER_MS;
    const latency = metadata.expectedDisplayTime - capturedAt;
    if (latency < PLAUSIBLE_MS.min || latency > PLAUSIBLE_MS.max) return;
    this.shown.push(latency);
    // A frame callback that is never drained (stats paused while hidden) must not grow forever.
    if (this.shown.length > 1_000) this.shown.splice(0, this.shown.length - 1_000);
  }
}
