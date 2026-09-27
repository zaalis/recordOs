/* End-to-end handshake tests against a running agent: vp-agentd --dev-loopback --approve-all */
import assert from 'node:assert/strict';
import { SignalingClient, generateDeviceIdentity, type ClientEvent, type DeviceIdentity } from '../src/index.ts';

const URL = process.env.VP_URL ?? 'ws://127.0.0.1:47802/ws';

function run(identity: DeviceIdentity, mode: 'pair' | 'auth', extra: Partial<ConstructorParameters<typeof SignalingClient>[0]> = {}) {
  const client = new SignalingClient({ url: URL, identity, deviceName: 'E2E iPad', deviceKind: 'ipad', mode, ...extra });
  const events: ClientEvent[] = [];
  const done = new Promise<ClientEvent[]>((resolve) => {
    client.on((event) => {
      events.push(event);
      if (event.type === 'ready' || event.type === 'error' || event.type === 'closed' || (event.type === 'paired' && !event.approved)) resolve(events);
    });
  });
  client.connect();
  return { client, done };
}

const kinds = (events: ClientEvent[]) => events.map((e) => (e.type === 'error' ? `error:${e.code}` : e.type));

const identity = await generateDeviceIdentity();

{
  const { client, done } = run(identity, 'pair');
  const events = await done;
  console.log('pair:', kinds(events));
  assert.deepEqual(kinds(events), ['hello', 'pairing', 'paired', 'ready']);
  const ready = events.at(-1) as Extract<ClientEvent, { type: 'ready' }>;
  assert.equal(ready.session.device.id, identity.deviceId);
  client.close();
}

await new Promise((r) => setTimeout(r, 300));

{
  const { client, done } = run(identity, 'auth');
  const events = await done;
  console.log('auth:', kinds(events));
  assert.deepEqual(kinds(events), ['hello', 'ready']);
  // Tampered frame: the agent must end the session.
  const closed = new Promise<ClientEvent>((resolve) => client.on((e) => e.type === 'closed' && resolve(e)));
  (client as unknown as { socket: WebSocket }).socket.send(JSON.stringify({ t: 'enc', e: 0, s: 0, d: 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA' }));
  await closed;
  console.log('tamper: session closed');
}

{
  const stranger = await generateDeviceIdentity();
  const { done } = run(stranger, 'auth');
  const events = await done;
  console.log('unknown:', kinds(events));
  assert.deepEqual(kinds(events), ['hello', 'error:unknown_device']);
}

{
  const stranger = await generateDeviceIdentity();
  const { done } = run(stranger, 'auth', { expectedFingerprint: 'A'.repeat(43) });
  const events = await done;
  console.log('pinned mismatch:', kinds(events));
  assert.deepEqual(kinds(events), ['hello', 'error:agent_mismatch']);
}

console.log('e2e handshake OK');
// Let the closed WebSockets finish closing: exiting immediately trips a libuv assertion on Windows.
setTimeout(() => process.exit(0), 300);
