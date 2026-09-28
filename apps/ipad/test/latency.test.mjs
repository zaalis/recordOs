import assert from 'node:assert/strict';
import test from 'node:test';
import { FrameLatencyMeter } from '../src/lib/frame-latency.ts';
import { preferencesFrom } from '../src/lib/storage.ts';

/** A <video> stand-in whose presented frames the test triggers by hand. */
function fakeVideo() {
  const video = {
    callback: null,
    requestVideoFrameCallback(callback) {
      this.callback = callback;
      return 1;
    },
    cancelVideoFrameCallback() {
      this.callback = null;
    },
    present(metadata) {
      const callback = this.callback;
      this.callback = null;
      callback?.(metadata.expectedDisplayTime, metadata);
    },
  };
  return video;
}

test('latency is measured from capture on the PC to display on the iPad', () => {
  const meter = new FrameLatencyMeter();
  const video = fakeVideo();
  meter.observe(video);
  // Ping sent at 1000 ms, pong received at 1010 ms: the PC clock read 90 000 (1 s) at ~1005 ms.
  meter.onPong(1000, 1010, 90_000);
  // A frame captured 20 ms later on the PC (1025 ms here), shown at 1085 ms.
  video.present({ rtpTimestamp: 90_000 + 20 * 90, expectedDisplayTime: 1085 });
  assert.equal(meter.take(), 60);
  // Values are consumed.
  assert.equal(meter.take(), null);
});

test('the clock sample with the shortest round trip is used', () => {
  const meter = new FrameLatencyMeter();
  const video = fakeVideo();
  meter.observe(video);
  meter.onPong(0, 40, 0); // slow, asymmetric exchange
  meter.onPong(2000, 2002, 2000 * 90); // 2 ms round trip: PC time 2000 ms ↔ local 2001 ms
  video.present({ rtpTimestamp: 2100 * 90, expectedDisplayTime: 2131 });
  assert.equal(meter.take(), 30);
});

test('the RTP clock wrapping around does not break the measurement', () => {
  const meter = new FrameLatencyMeter();
  const video = fakeVideo();
  meter.observe(video);
  const nearWrap = 2 ** 32 - 90 * 5; // 5 ms before the 32-bit clock wraps
  meter.onPong(100, 102, nearWrap);
  video.present({ rtpTimestamp: 90 * 10, expectedDisplayTime: 101 + 15 + 25 });
  assert.equal(meter.take(), 25);
});

test('the median of a second of frames is reported', () => {
  const meter = new FrameLatencyMeter();
  const video = fakeVideo();
  meter.observe(video);
  meter.onPong(0, 0, 0);
  for (const latency of [30, 32, 500, 31, 29]) {
    video.present({ rtpTimestamp: 0, expectedDisplayTime: latency });
  }
  assert.equal(meter.take(), 31);
});

test('nothing is reported without frame timestamps or before clock sync', () => {
  const meter = new FrameLatencyMeter();
  const video = fakeVideo();
  meter.observe(video);
  video.present({ rtpTimestamp: 900, expectedDisplayTime: 50 }); // no pong yet
  meter.onPong(0, 2, 0);
  video.present({ expectedDisplayTime: 50 }); // browser without rtpTimestamp
  assert.equal(meter.take(), null);
});

test('a new stream discards the previous clock', () => {
  const meter = new FrameLatencyMeter();
  const video = fakeVideo();
  meter.observe(video);
  meter.onPong(0, 2, 0);
  meter.reset();
  video.present({ rtpTimestamp: 900, expectedDisplayTime: 50 });
  assert.equal(meter.take(), null);
});

test('observing stops when the element is released', () => {
  const meter = new FrameLatencyMeter();
  const video = fakeVideo();
  meter.observe(video);
  meter.observe(null);
  assert.equal(video.callback, null);
});

test('the former 30 FPS default moves to 60 FPS; manual choices stay', () => {
  const former = { profile: 'balanced', resolution: '1080p', fps: 30, bitrate: 'auto', codec: 'h264' };
  const migrated = preferencesFrom({ video: former, videoDefaultVersion: 3 });
  assert.equal(migrated.video.fps, 60);
  assert.equal(migrated.videoDefaultVersion, 4);

  // Already on the current version: 30 FPS is a deliberate choice.
  assert.equal(preferencesFrom({ video: former, videoDefaultVersion: 4 }).video.fps, 30);
  // 30 FPS together with another change is a deliberate choice too.
  assert.equal(preferencesFrom({ video: { ...former, resolution: '1440p' }, videoDefaultVersion: 3 }).video.fps, 30);
  // A fresh install gets the new default.
  assert.equal(preferencesFrom(undefined).video.fps, 60);
});

test('older migrations do not run again on current preferences', () => {
  const chosen = { profile: 'balanced', resolution: 'auto', fps: 'auto', bitrate: 'auto', codec: 'auto' };
  assert.deepEqual(preferencesFrom({ video: chosen, videoDefaultVersion: 4 }).video, chosen);
  // The same preset saved by a version-2 build was a former default and is replaced.
  assert.equal(preferencesFrom({ video: chosen, videoDefaultVersion: 2 }).video.resolution, '1080p');
});
