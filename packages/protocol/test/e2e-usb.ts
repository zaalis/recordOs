/*
 * USB Direct end-to-end test. Starts a debug `vp-agentd` whose simulated cable is 127.0.0.2
 * (`--simulate-usb`), with its own data directory, then checks what an iPad sees:
 *   - a session over Wi-Fi learns the cable endpoint, a session on the cable is a USB session;
 *   - the cable session carries input (its whole traffic stays on the cable connection);
 *   - plugging the cable in tells Wi-Fi sessions to move; unplugging ends cable sessions at once.
 * Run: node --experimental-strip-types test/e2e-usb.ts   (after `cargo build -p vp-agent`)
 */
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { InputEncoder, SignalingClient, b64u, generateDeviceIdentity, probeAgent, type ClientEvent, type DeviceIdentity, type SessionReady } from '../src/index.ts';

const root = resolve(import.meta.dirname, '../../..');
const exe = process.env.VP_AGENTD ?? join(root, 'agent/target/debug/vp-agentd.exe');
const LAN = 'ws://127.0.0.1:47802/ws';
const CABLE = 'ws://127.0.0.2:47802/ws';

const home = mkdtempSync(join(tmpdir(), 'vp-usb-'));
writeFileSync(join(home, 'settings.json'), JSON.stringify({ schema: 4, network: { mode: 'local-only', httpsPort: 47900, advertiseMdns: false } }));
const agent = spawn(exe, ['--dev-loopback', '--simulate-usb', '--connect'], {
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
const info = async () => (await fetch('http://127.0.0.1:47802/api/info')).json() as Promise<{ agentId: string; usb: { urls: string[] } }>;
const toggleCable = () => agent.stdin.write('u\n');

function open(url: string, identity: DeviceIdentity, mode: 'pair' | 'auth', code?: string) {
  const client = new SignalingClient({ url, identity, deviceName: 'USB iPad', deviceKind: 'ipad', mode, ...(code ? { proof: { kind: 'code' as const, secret: code } } : {}) });
  const events: ClientEvent[] = [];
  client.on((event) => events.push(event));
  client.connect();
  const ready = until(`session.ready on ${url}`, () => {
    const failure = events.find((e) => e.type === 'error' || e.type === 'closed');
    if (failure) throw new Error(`${url}: ${JSON.stringify(failure)}`);
    const event = events.find((e) => e.type === 'ready');
    return event?.type === 'ready' ? event.session : undefined;
  });
  const message = (t: string) => until(`${t} on ${url}`, () => {
    const event = events.find((e) => e.type === 'message' && e.message.t === t);
    return event?.type === 'message' ? event.message : undefined;
  });
  return { client, events, ready, message };
}

try {
  await until('agent start', () => /simulated cable/.test(output));
  const { agentId } = await until('cable announced by /api/info', async () => {
    const current = await info().catch(() => undefined);
    return current?.usb.urls.includes(CABLE) ? current : undefined;
  });

  // Pair over "Wi-Fi" with the code the PC shows: a LAN session that learns the cable endpoint.
  const code = await until('pairing code', () => /Code {2}(\d{6})/.exec(output)?.[1]);
  const identity = await generateDeviceIdentity();
  const lan = open(LAN, identity, 'pair', code);
  const lanReady: SessionReady = await lan.ready;
  assert.equal(lanReady.link, 'lan');
  assert.deepEqual(lanReady.usb?.urls, [CABLE]);
  console.log('✓ Wi-Fi session is "lan" and learns the cable endpoint');

  // Only the PC we paired with answers the probe.
  assert.equal(await probeAgent(CABLE, agentId, 2000), true);
  assert.equal(await probeAgent(CABLE, 'x'.repeat(22), 2000), false);
  console.log('✓ cable probe checks the PC identity');

  // A session on the cable address is a USB session.
  const usb = open(CABLE, identity, 'auth');
  const usbReady = await usb.ready;
  assert.equal(usbReady.link, 'usb');
  // Opening the cable session replaced the Wi-Fi one (same iPad).
  assert.equal((await lan.message('session.end')).reason, 'replaced');
  const input = new InputEncoder();
  await usb.client.send({ t: 'in', d: b64u(new Uint8Array(input.releaseAll())) });
  await usb.client.send({ t: 'ping', id: 1, t0: Date.now() });
  assert.equal((await usb.message('pong')).id, 1);
  console.log('✓ the cable session carries input and pings');

  // Unplugging the cable ends the cable session immediately with reason "link".
  toggleCable();
  assert.equal((await usb.message('session.end')).reason, 'link');
  await until('cable removed from /api/info', async () => (await info()).usb.urls.length === 0);
  console.log('✓ unplugging ends the cable session at once (reason "link")');

  // A Wi-Fi session is told to move when the cable is plugged in.
  const wifi = open(LAN, identity, 'auth');
  const wifiReady = await wifi.ready;
  assert.equal(wifiReady.link, 'lan');
  assert.equal(wifiReady.usb, undefined);
  toggleCable();
  const moved = await wifi.message('link.usb');
  assert.deepEqual(moved.urls, [CABLE]);
  console.log('✓ plugging the cable in tells Wi-Fi sessions to move (link.usb)');
  wifi.client.close();

  console.log('e2e USB OK');
  process.exitCode = 0;
} catch (error) {
  console.error(error);
  process.exitCode = 1;
} finally {
  cleanup();
  setTimeout(() => process.exit(process.exitCode ?? 0), 700);
}
