import assert from 'node:assert/strict';
import test from 'node:test';
import { Connection } from '../src/lib/connection.ts';
import { detectCodecs, measureRefreshRate } from '../src/lib/platform.ts';

globalThis.window = globalThis;
globalThis.document = { hidden: true };
globalThis.location = { protocol: 'https:', host: 'pc.local' };
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { userAgent: 'iPad', maxTouchPoints: 5 },
});

class ClosingSocket {
  static instances = [];
  readyState = 1;
  onclose = null;
  constructor() {
    ClosingSocket.instances.push(this);
    queueMicrotask(() => this.close());
  }
  send() {}
  close() {
    if (this.readyState === 3) return;
    this.readyState = 3;
    queueMicrotask(() => this.onclose?.({ code: 1006 }));
  }
}
globalThis.WebSocket = ClosingSocket;

test('refresh-rate detection is bounded when Safari suspends animation frames', async () => {
  assert.equal(await measureRefreshRate(), 60);
  document.hidden = false;
  globalThis.requestAnimationFrame = () => 1;
  globalThis.cancelAnimationFrame = () => {};
  assert.equal(await measureRefreshRate(), 60);
  document.hidden = true;
});

test('codec probing cannot hold a connection open indefinitely', async () => {
  globalThis.RTCRtpReceiver = { getCapabilities: () => ({ codecs: [{ mimeType: 'video/H264' }] }) };
  navigator.mediaCapabilities = { decodingInfo: () => new Promise(() => {}) };
  const started = Date.now();
  assert.deepEqual(await detectCodecs(), [{ codec: 'h264', powerEfficient: true }]);
  assert.ok(Date.now() - started < 1_200);
  delete globalThis.RTCRtpReceiver;
  delete navigator.mediaCapabilities;
});

test('an authenticated socket close retries and a restored tab retries immediately', async () => {
  const connection = new Connection();
  const count = ClosingSocket.instances.length;
  await connection.connect({ name: 'PC', mode: 'auth' }, {}, 'iPad', {});
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(ClosingSocket.instances.length, count + 1);
  assert.equal(connection.getSnapshot().phase, 'reconnecting');
  assert.equal(connection.getSnapshot().reconnectAttempt, 1);

  connection.suspendForPagehide();
  connection.resumeVisible();
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(ClosingSocket.instances.length, count + 2);
  connection.reset();
});
