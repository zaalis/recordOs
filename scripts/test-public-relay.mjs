// Exercise a deployed HTTPS relay with an isolated real Windows agent and TS client.
import { spawn } from 'node:child_process';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { once } from 'node:events';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
const root = resolve(import.meta.dirname, '..');
const relay = process.argv[2];
if (!relay?.startsWith('wss://')) throw new Error('Pass a public wss:// relay URL');
const home = await mkdtemp(join(tmpdir(), 'vp-public-relay-'));
await writeFile(join(home, 'settings.json'), JSON.stringify({ schema: 4, network: { mode: 'remote-access', relayUrl: relay, httpsPort: 47910, advertiseMdns: false, usbDirect: false } }));
const agent = spawn(join(root, 'agent/target/debug/vp-agentd.exe'), ['--connect', '--approve-all'], { env: { ...process.env, VISION_PRO_HOME: home, RUST_LOG: 'info' }, stdio: ['pipe', 'pipe', 'pipe'] });
let output = ''; agent.stdout.on('data', chunk => { output += chunk; }); agent.stderr.on('data', chunk => { output += chunk; });
try {
  const deadline = Date.now() + 30_000;
  while (!/Code {2}\d{4}/.test(output)) {
    if (Date.now() > deadline || agent.exitCode !== null) throw new Error('Isolated test agent did not start');
    await new Promise(r => setTimeout(r, 100));
  }
  // Registration is outbound and asynchronous; allow its first TLS connection to finish.
  await new Promise(r => setTimeout(r, 4000));
  const id = /Agent +([A-Za-z0-9_-]{22})/.exec(output)?.[1];
  const code = /Code {2}(\d{4})/.exec(output)?.[1];
  const resolved = await fetch(`${relay.replace('wss://', 'https://')}/v1/pair`, {method: "POST", headers: {"content-type":"application/json"}, body:JSON.stringify({code})});
  if (!resolved.ok || (await resolved.json()).agentId !== id) throw new Error('Pairing code lookup failed');
  if (!id || !code) throw new Error('Agent test metadata missing');
  const client = spawn(process.execPath, ['--experimental-strip-types', join(root, 'packages/protocol/test/e2e-relay.ts')], { env: { ...process.env, VP_RELAY: relay, VP_AGENT_ID: id, VP_CODE: code }, stdio: 'inherit' });
  const timer = setTimeout(() => client.kill(), 45_000);
  const [exitCode] = await once(client, 'exit'); clearTimeout(timer);
  if (exitCode !== 0) throw new Error('Public relay end-to-end test failed');
} finally {
  if (agent.exitCode === null) { const stopped = once(agent, 'exit'); agent.kill(); await stopped; }
  await rm(home, { recursive: true, force: true });
}
