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

test('explicit disconnect clears transient state and prevents a queued reconnect', async () => {
  const connection = new Connection();
  const count = ClosingSocket.instances.length;
  await connection.connect({ name: 'PC', mode: 'auth' }, {}, 'iPad', {});
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(connection.getSnapshot().phase, 'reconnecting');

  let trackStopped = false;
  const track = { onended: () => {}, stop() { trackStopped = true; } };
  connection.update({
    phase: 'streaming',
    error: 'stale error',
    sas: '123456',
    pairingExpiresAt: 1,
    session: { sessionId: 'session' },
    video: { width: 1920, height: 1080 },
    displays: [{ id: 'display' }],
    activeDisplayId: 'display',
    controlAllowed: false,
    activeApp: { exe: 'app.exe', profile: 'test' },
    stream: { getTracks: () => [track] },
    connectedAt: 1,
    reconnectAttempt: 3,
    link: 'usb',
    switching: true,
  });
  connection.startTimers();
  connection.disconnect();
  connection.resumeVisible();
  await new Promise((resolve) => setTimeout(resolve, 450));

  const state = connection.getSnapshot();
  assert.equal(ClosingSocket.instances.length, count + 1, 'disconnect must cancel the scheduled retry');
  assert.equal(state.phase, 'ended');
  assert.equal(state.error, null);
  assert.equal(state.sas, null);
  assert.equal(state.pairingExpiresAt, null);
  assert.equal(state.session, null);
  assert.equal(state.video, null);
  assert.deepEqual(state.displays, []);
  assert.equal(state.activeDisplayId, null);
  assert.equal(state.controlAllowed, true);
  assert.equal(state.activeApp, null);
  assert.equal(state.stream, null);
  assert.equal(state.connectedAt, null);
  assert.equal(state.reconnectAttempt, 0);
  assert.equal(state.link, null);
  assert.equal(state.switching, false);
  assert.equal(state.stats.pingMs, null);
  assert.equal(state.stats.agent, null);
  assert.equal(trackStopped, true, 'disconnect must release the remote media track');
  assert.equal(connection.timers.length, 0);
  assert.equal(connection.reconnectTimer, undefined);
  assert.equal(connection.target, undefined);
  connection.reset();
});

test('a PC-initiated user disconnect clears all session-only state', async () => {
  const connection = new Connection();
  connection.target = { agentId: 'pc-1', name: 'PC', mode: 'auth' };
  let trackStopped = false;
  const track = { onended: () => {}, stop() { trackStopped = true; } };
  connection.update({
    phase: 'streaming',
    session: { sessionId: 'session' },
    video: { width: 1920, height: 1080 },
    displays: [{ id: 'display' }],
    activeDisplayId: 'display',
    activeApp: { exe: 'app.exe', profile: 'test' },
    stream: { getTracks: () => [track] },
    connectedAt: 1,
    reconnectAttempt: 2,
    link: 'lan',
  });
  connection.startTimers();

  await connection.onSignalMessage({ t: 'session.end', reason: 'user' });

  const state = connection.getSnapshot();
  assert.equal(state.phase, 'ended');
  assert.equal(state.session, null);
  assert.equal(state.video, null);
  assert.deepEqual(state.displays, []);
  assert.equal(state.activeApp, null);
  assert.equal(state.stream, null);
  assert.equal(state.stats.pingMs, null);
  assert.equal(state.link, null);
  assert.equal(connection.timers.length, 0);
  assert.equal(connection.target, undefined);
  assert.equal(trackStopped, true);
  connection.reset();
});
