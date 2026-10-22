/*
 * End-to-end session test against a real agent. Starts a debug `vp-agentd` with its own data
 * directory and the developer loopback endpoint, its "Connect Device" window open, then checks
 * what an iPad sees:
 *   - pairing needs the code shown on the PC; a wrong code is refused, the right one pairs at once;
 *   - a used code is replaced; a paired iPad then authenticates with its key alone;
 *   - input and pings travel inside the encrypted session; a tampered frame ends it.
 * Run: node --experimental-strip-types test/e2e-agent.ts   (after `cargo build -p vp-agent`)
 */
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { InputEncoder, SignalingClient, b64u, fingerprintOf, generateDeviceIdentity, type ClientEvent, type DeviceIdentity } from '../src/index.ts';

const root = resolve(import.meta.dirname, '../../..');
const exe = process.env.VP_AGENTD ?? join(root, 'agent/target/debug/vp-agentd.exe');
const URL = 'ws://127.0.0.1:47802/ws';

const home = mkdtempSync(join(tmpdir(), 'vp-e2e-'));
writeFileSync(join(home, 'settings.json'), JSON.stringify({ schema: 4, network: { mode: 'local-only', httpsPort: 47910, advertiseMdns: false, usbDirect: false } }));
const agent = spawn(exe, ['--dev-loopback', '--connect'], {
  env: { ...process.env, VISION_PRO_HOME: home, RUST_LOG: 'warn' },
  stdio: ['pipe', 'pipe', 'inherit'],
});
let output = '';
agent.stdout.setEncoding('utf8');
agent.stdout.on('data', (chunk: string) => {
  output += chunk;
});
const cleanup = () => {
  agent.kill();
  setTimeout(() => rmSync(home, { recursive: true, force: true }), 500);
};
process.on('exit', cleanup);

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
async function until<T>(what: string, check: () => T | undefined | false | Promise<T | undefined | false>, timeoutMs = 20_000): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const value = await check();
    if (value) return value;
    await sleep(100);
  }
  throw new Error(`timed out waiting for ${what}\n--- agent output ---\n${output}`);
}

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
  return { client, events, done };
}

const kinds = (events: ClientEvent[]) => events.map((e) => (e.type === 'error' ? `error:${e.code}` : e.type));
/** The code the PC shows right now (the agent prints it, then reprints it on every change). */
const shownCode = () => [...output.matchAll(/Code {2}(\d{6})/g)].at(-1)?.[1];

try {
  const code = await until('pairing code', shownCode);
  const identity = await generateDeviceIdentity();

  {
    const { done } = run(identity, 'pair');
    const events = await done;
    assert.deepEqual(kinds(events), ['hello', 'error:pairing_disabled']);
    const hello = events[0] as Extract<ClientEvent, { type: 'hello' }>;
    assert.deepEqual(hello.hello.pairing, { code: true });
    console.log('✓ pairing without the code is refused');
  }

  {
    const wrong = code === '000000' ? '111111' : '000000';
    const { done } = run(identity, 'pair', { proof: { kind: 'code', secret: wrong } });
    assert.deepEqual(kinds(await done), ['hello', 'error:bad_code']);
    console.log('✓ a wrong code is refused');
  }

  let fingerprint = '';
  {
    const { client, done } = run(identity, 'pair', { proof: { kind: 'code', secret: code } });
    const events = await done;
    // No confirmation on the PC: the code shown there is the PC user's consent.
    assert.deepEqual(kinds(events), ['hello', 'paired', 'ready']);
    const ready = events.at(-1) as Extract<ClientEvent, { type: 'ready' }>;
    assert.equal(ready.session.device.id, identity.deviceId);
    assert.equal(ready.session.device.permission, 'control');
    assert.ok(Array.isArray(ready.session.caps.displays));
    fingerprint = await fingerprintOf(client.agentPublicKey!);
    client.close();
    console.log('✓ the code shown on the PC pairs at once');
  }

  {
    const replaced = await until('a new code after pairing', () => {
      const next = shownCode();
      return next && next !== code ? next : undefined;
    });
    const other = await generateDeviceIdentity();
    const { done } = run(other, 'pair', { proof: { kind: 'code', secret: code } });
    assert.deepEqual(kinds(await done), ['hello', 'error:bad_code']);
    assert.notEqual(replaced, code);
    console.log('✓ a used code is replaced and cannot pair another device');
  }

  await sleep(300);

  {
    const { client, events, done } = run(identity, 'auth', { expectedFingerprint: fingerprint });
    assert.deepEqual(kinds(await done), ['hello', 'ready']);
    const input = new InputEncoder();
    // Releasing everything is harmless on the test machine and exercises both input paths.
    await client.send({ t: 'in', d: b64u(new Uint8Array(input.releaseAll())) });
    await client.send({ t: 'rt', d: b64u(new Uint8Array(input.releaseAll())) });
    await client.send({ t: 'ping', id: 7, t0: Date.now() });
    const pong = await until('pong', () => {
      const event = events.find((e) => e.type === 'message' && e.message.t === 'pong');
      return event?.type === 'message' ? event.message : undefined;
    });
    assert.equal(pong.id, 7);
    console.log('✓ input and pings travel inside the encrypted session');

    // Tampered frame: the agent must end the session.
    const closed = new Promise<ClientEvent>((resolve) => client.on((e) => e.type === 'closed' && resolve(e)));
    (client as unknown as { socket: WebSocket }).socket.send(JSON.stringify({ t: 'enc', e: 0, s: 999, d: 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA' }));
    await closed;
    console.log('✓ a tampered frame ends the session');
  }

  {
    const stranger = await generateDeviceIdentity();
    const { done } = run(stranger, 'auth');
    assert.deepEqual(kinds(await done), ['hello', 'error:unknown_device']);
    console.log('✓ an unpaired device is refused');
  }

  {
    const stranger = await generateDeviceIdentity();
    const { done } = run(stranger, 'auth', { expectedFingerprint: 'A'.repeat(43) });
    assert.deepEqual(kinds(await done), ['hello', 'error:agent_mismatch']);
    console.log('✓ a PC with another key is refused (pinned fingerprint)');
  }

  console.log('e2e session OK');
  process.exitCode = 0;
} catch (error) {
  console.error(error);
  process.exitCode = 1;
} finally {
  cleanup();
  // Let the closed WebSockets finish closing: exiting immediately trips a libuv assertion on Windows.
  setTimeout(() => process.exit(process.exitCode ?? 0), 700);
}
