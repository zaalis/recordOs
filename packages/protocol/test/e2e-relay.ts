/*
 * End-to-end signaling through a Vision-Pro relay against a running agent in Remote Access mode:
 *   vp-relay --host 127.0.0.1 --http 127.0.0.1:18080 --turn 127.0.0.1:13478 --token <token>
 *   vp-agentd --connect   (settings: remote-access, relayUrl ws://127.0.0.1:18080)
 *   VP_AGENT_ID=… VP_CODE=… node --experimental-strip-types test/e2e-relay.ts
 */
import assert from 'node:assert/strict';
import { SignalingClient, generateDeviceIdentity, relayConnectUrl, type ClientEvent, type DeviceIdentity, type SessionReady } from '../src/index.ts';

const RELAY = process.env.VP_RELAY ?? 'ws://127.0.0.1:18080';
const AGENT_ID = process.env.VP_AGENT_ID ?? '';
const CODE = process.env.VP_CODE ?? '';
assert.ok(AGENT_ID.length === 22 && CODE.length === 4, 'VP_AGENT_ID and VP_CODE are required');

const URL = relayConnectUrl(RELAY, AGENT_ID)!;

function run(identity: DeviceIdentity, mode: 'pair' | 'auth', extra: Partial<ConstructorParameters<typeof SignalingClient>[0]> = {}, url = URL) {
  const client = new SignalingClient({ url, identity, deviceName: 'Relay iPad', deviceKind: 'ipad', mode, ...extra });
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
const pause = (ms: number) => new Promise((r) => setTimeout(r, ms));

function assertRelayedSession(session: SessionReady) {
  assert.equal(session.relay?.url, RELAY, 'the PC announces its relay inside the encrypted channel');
  assert.equal(session.link, 'relay');
}

const identity = await generateDeviceIdentity();

{
  const { done } = run(identity, 'auth', {}, relayConnectUrl(RELAY, 'Z'.repeat(22))!);
  const events = await done;
  console.log('offline agent:', kinds(events));
  assert.deepEqual(kinds(events), ['error:offline']);
}

{
  const { done } = run(identity, 'pair');
  const events = await done;
  console.log('pair without code:', kinds(events));
  const hello = events[0] as Extract<ClientEvent, { type: 'hello' }>;
  assert.deepEqual(hello.hello.pairing, { code: true });
  assert.deepEqual(kinds(events), ['hello', 'error:pairing_disabled']);
}

let fingerprint = '';
{
  const { client, done } = run(identity, 'pair', { proof: { kind: 'code', secret: CODE } });
  const events = await done;
  console.log('pair with code:', kinds(events));
  assert.deepEqual(kinds(events), ['hello', 'pairing', 'paired', 'ready']);
  const ready = events.at(-1) as Extract<ClientEvent, { type: 'ready' }>;
  assertRelayedSession(ready.session);
  const { fingerprintOf } = await import('../src/index.ts');
  fingerprint = await fingerprintOf(client.agentPublicKey!);
  client.close();
}

await pause(400);

{
  const { client, done } = run(identity, 'auth', { expectedFingerprint: fingerprint });
  const events = await done;
  console.log('auth via relay:', kinds(events));
  assert.deepEqual(kinds(events), ['hello', 'ready']);
  assertRelayedSession((events.at(-1) as Extract<ClientEvent, { type: 'ready' }>).session);
  const closed = new Promise<ClientEvent>((resolve) => client.on((e) => e.type === 'closed' && resolve(e)));
  (client as unknown as { socket: WebSocket }).socket.send(JSON.stringify({ t: 'enc', e: 0, s: 0, d: 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA' }));
  await closed;
  console.log('tampered frame: session closed by the PC');
}

{
  const { done } = run(identity, 'auth', { expectedFingerprint: 'A'.repeat(43) });
  const events = await done;
  console.log('pinned key mismatch (relay impersonation):', kinds(events));
  assert.deepEqual(kinds(events), ['hello', 'error:agent_mismatch']);
}

{
  const stranger = await generateDeviceIdentity();
  const { done } = run(stranger, 'auth');
  const events = await done;
  console.log('unpaired device:', kinds(events));
  assert.deepEqual(kinds(events), ['hello', 'error:unknown_device']);
}

console.log('e2e relay signaling OK');
// Let the closed WebSockets finish closing: exiting immediately trips a libuv assertion on Windows.
setTimeout(() => process.exit(0), 300);
