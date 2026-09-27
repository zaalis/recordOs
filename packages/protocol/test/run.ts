/* Self-consistency tests for the protocol package. Run: npm test -w @vision-pro/protocol */
import assert from 'node:assert/strict';
import {
  ClientHandshake,
  InputEncoder,
  SecureChannel,
  b64u,
  buildPairingFragment,
  computeCommit,
  computeTranscript,
  deriveSessionKeys,
  fromB64u,
  generateDeviceIdentity,
  idFromPublicKey,
  fingerprintOf,
  parsePairingFragment,
  randomBytes,
  sasFromBytes,
  signEcdsa,
  concat,
  utf8,
  LABEL,
  clientInitSchema,
  parse,
  BUILTIN_PROFILES,
  profileForApp,
  ALLOWED_VKS,
  VK,
  comboLabel,
  relayConnectUrl,
  cableSignalingUrl,
  cableSignalingUrls,
  ipadHotspotUrls,
  probeAgent,
  firstReachable,
} from '../src/index.ts';

const tests: [string, () => Promise<void> | void][] = [];
const test = (name: string, fn: () => Promise<void> | void) => tests.push([name, fn]);

test('base64url roundtrip', () => {
  for (let len = 0; len < 70; len++) {
    const bytes = randomBytes(len);
    assert.deepEqual(fromB64u(b64u(bytes)), bytes);
  }
  assert.throws(() => fromB64u('ab+c'));
  assert.throws(() => fromB64u('a'));
});

test('sas is six digits', () => {
  assert.equal(sasFromBytes(new Uint8Array(8)), '000000');
  assert.match(sasFromBytes(randomBytes(8)), /^\d{6}$/);
});

test('full handshake against a reference server', async () => {
  const subtle = globalThis.crypto.subtle;
  // --- reference "agent" implemented with WebCrypto ---
  const agentKeys = await subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, true, ['sign', 'verify']);
  const agentPub = new Uint8Array(await subtle.exportKey('raw', agentKeys.publicKey));
  const agentId = await idFromPublicKey(agentPub);
  const serverEph = await subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, true, ['deriveBits']);
  const serverEphRaw = new Uint8Array(await subtle.exportKey('raw', serverEph.publicKey));
  const nS = randomBytes(32);
  const commit = await computeCommit(serverEphRaw, nS);

  const device = await generateDeviceIdentity();
  const client = new ClientHandshake(device, 'Test iPad', 'ipad', 'pair', await fingerprintOf(agentPub));
  const init = await client.onHello({ agentId, agentPub: b64u(agentPub), commit: b64u(commit) });

  const initMessage = parse(clientInitSchema, {
    t: 'init', v: 1, mode: 'pair', clientEph: init.clientEph, nC: init.nC,
    deviceId: device.deviceId, devicePub: b64u(device.publicRaw), deviceName: 'Test iPad', deviceKind: 'ipad',
  });

  const transcript = await computeTranscript({
    agentId, agentPub, commit, mode: 'pair',
    clientEph: fromB64u(initMessage.clientEph), clientNonce: fromB64u(initMessage.nC),
    deviceId: device.deviceId, devicePub: device.publicRaw, deviceName: 'Test iPad', deviceKind: 'ipad',
    serverEph: serverEphRaw, serverNonce: nS,
  });
  const agentSig = await signEcdsa(agentKeys.privateKey, concat(utf8(LABEL.agentSig), transcript));
  const { sig, keys: clientKeys } = await client.onReveal({ serverEph: b64u(serverEphRaw), nS: b64u(nS), sig: b64u(agentSig) });

  const clientEphKey = await subtle.importKey('raw', fromB64u(init.clientEph), { name: 'ECDH', namedCurve: 'P-256' }, false, []);
  const shared = new Uint8Array(await subtle.deriveBits({ name: 'ECDH', public: clientEphKey }, serverEph.privateKey, 256));
  const serverKeys = await deriveSessionKeys(shared, transcript);
  assert.equal(serverKeys.sas, clientKeys.sas);
  assert.equal(serverKeys.sessionId, clientKeys.sessionId);

  const deviceKey = await subtle.importKey('raw', device.publicRaw, { name: 'ECDSA', namedCurve: 'P-256' }, false, ['verify']);
  assert.ok(await subtle.verify({ name: 'ECDSA', hash: 'SHA-256' }, deviceKey, fromB64u(sig), concat(utf8(LABEL.deviceSig), transcript)));

  // --- secure channel both ways, including tamper and replay rejection ---
  const c = new SecureChannel(clientKeys, 'client');
  const s = new SecureChannel(serverKeys, 'server');
  for (let i = 0; i < 5; i++) {
    const [frame] = await c.seal({ t: 'ping', i });
    assert.deepEqual(await s.open(frame!), { t: 'ping', i });
    const [reply] = await s.seal({ t: 'pong', i });
    assert.deepEqual(await c.open(reply!), { t: 'pong', i });
  }
  const [frame] = await c.seal({ t: 'x' });
  await assert.rejects(s.open({ ...frame!, s: frame!.s + 1 }));
});

test('commit mismatch is rejected', async () => {
  const device = await generateDeviceIdentity();
  const subtle = globalThis.crypto.subtle;
  const agentKeys = await subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, true, ['sign']);
  const agentPub = new Uint8Array(await subtle.exportKey('raw', agentKeys.publicKey));
  const client = new ClientHandshake(device, 'x', 'ipad', 'auth');
  await client.onHello({ agentId: await idFromPublicKey(agentPub), agentPub: b64u(agentPub), commit: b64u(randomBytes(32)) });
  const eph = await subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, true, ['deriveBits']);
  const raw = new Uint8Array(await subtle.exportKey('raw', eph.publicKey));
  await assert.rejects(client.onReveal({ serverEph: b64u(raw), nS: b64u(randomBytes(32)), sig: b64u(randomBytes(64)) }), /commit_mismatch/);
});

test('input encoder layouts', () => {
  const enc = new InputEncoder();
  assert.equal(enc.moveAbs(0.5, 0.5).byteLength, 12);
  assert.equal(enc.button(0, true, null, null).byteLength, 16);
  assert.equal(enc.wheel(0, 120, 0.1, 0.2).byteLength, 20);
  assert.equal(enc.key(0x41, true).byteLength, 8);
  assert.equal(enc.pen({ phase: 1, x: 0.2, y: 0.3, pressure: 0.5, tiltX: 10, tiltY: -10, twist: 0, buttons: 0 }).byteLength, 24);
  assert.equal(enc.touch([{ id: 1, phase: 1, x: 0, y: 0, pressure: 1 }]).byteLength, 24);
  assert.equal(enc.combo([0x11, 0x5a]).byteLength, 10);
  assert.equal(enc.magnify(1, 0.05, 0.5, 0.5).byteLength, 20);
  assert.equal(enc.text('héllo')[0]!.byteLength, 16);
  const view = new DataView(enc.moveAbs(2, -1));
  assert.equal(view.getFloat32(4, true), 1);
  assert.equal(view.getFloat32(8, true), 0);
});

test('pairing links', () => {
  const link = { agentId: 'A'.repeat(22), fingerprint: 'B'.repeat(43), token: 'C'.repeat(22), name: "Bryan's MacBook Pro" };
  assert.deepEqual(parsePairingFragment(buildPairingFragment(link)), link);
  assert.equal(parsePairingFragment('#/pair?a=short'), null);
});

test('creative application profiles resolve from macOS bundle identifiers', () => {
  assert.equal(profileForApp('org.blenderfoundation.blender', BUILTIN_PROFILES)?.id, 'blender');
  assert.equal(profileForApp('com.autodesk.Maya.2025', BUILTIN_PROFILES)?.id, 'maya');
  assert.equal(profileForApp('com.maxon.zbrush', BUILTIN_PROFILES)?.id, 'zbrush');
  assert.equal(profileForApp('com.sidefx.houdini20.5', BUILTIN_PROFILES)?.id, 'houdini');
  assert.equal(profileForApp('com.adobe.Photoshop', BUILTIN_PROFILES)?.id, 'photoshop');
  assert.equal(profileForApp('com.apple.TextEdit', BUILTIN_PROFILES), undefined);
});

test('every built-in shortcut uses allowlisted Mac keys', () => {
  for (const profile of BUILTIN_PROFILES) {
    for (const section of profile.sections) {
      for (const control of section.controls) {
        const keys = control.type === 'key' ? control.keys : [...control.decrease, ...control.increase];
        for (const key of keys) assert.ok(ALLOWED_VKS.has(VK[key]), `${profile.id}/${control.id}: ${key}`);
      }
    }
  }
  assert.equal(comboLabel(['Cmd', 'Shift', 'Z']), '⇧⌘Z');
  assert.equal(comboLabel(['Control', 'Up']), '⌃↑');
});

test('relay signaling URLs are restricted to wss (ws only on loopback)', () => {
  const id = 'AbCdEfGhIjKlMnOpQrStUv';
  assert.equal(relayConnectUrl('wss://relay.example.com', id), `wss://relay.example.com/v1/connect/${id}`);
  assert.equal(relayConnectUrl('wss://relay.example.com:8443/', id), `wss://relay.example.com:8443/v1/connect/${id}`);
  assert.equal(relayConnectUrl('ws://127.0.0.1:8080', id), `ws://127.0.0.1:8080/v1/connect/${id}`);
  assert.equal(relayConnectUrl('ws://relay.example.com', id), null);
  assert.equal(relayConnectUrl('https://relay.example.com', id), null);
  assert.equal(relayConnectUrl('wss://user:pass@relay.example.com', id), null);
  assert.equal(relayConnectUrl('wss://relay.example.com', '../../x'), null);
  assert.equal(relayConnectUrl('not a url', id), null);
});

test('cable signaling URLs only accept private IPv4 endpoints', () => {
  assert.equal(cableSignalingUrl('wss://172.20.10.2:47800/ws'), 'wss://172.20.10.2:47800/ws');
  assert.equal(cableSignalingUrl('wss://192.168.42.20:47800/ws'), 'wss://192.168.42.20:47800/ws');
  assert.equal(cableSignalingUrl('ws://127.0.0.2:47802/ws'), 'ws://127.0.0.2:47802/ws');
  assert.equal(cableSignalingUrl('ws://172.20.10.2:47800/ws'), null);
  assert.equal(cableSignalingUrl('wss://8.8.8.8:47800/ws'), null);
  assert.equal(cableSignalingUrl('wss://evil.example.com:47800/ws'), null);
  assert.equal(cableSignalingUrl('wss://172.20.10.2/ws'), null);
  assert.equal(cableSignalingUrl('wss://172.20.10.2:47800/other'), null);
  assert.equal(cableSignalingUrl(42), null);
  assert.deepEqual(cableSignalingUrls(['wss://172.20.10.2:47800/ws', 'wss://172.20.10.2:47800/ws', 'bad']), ['wss://172.20.10.2:47800/ws']);
  assert.deepEqual(cableSignalingUrls('nope'), []);
  assert.deepEqual(ipadHotspotUrls(47800), ['wss://172.20.10.2:47800/ws', 'wss://172.20.10.3:47800/ws', 'wss://172.20.10.4:47800/ws', 'wss://172.20.10.5:47800/ws']);
});

test('probing a signaling endpoint checks the Mac identity', async () => {
  class FakeSocket {
    static answers = new Map<string, string | null>();
    onmessage: ((e: { data: string }) => void) | null = null;
    onerror: (() => void) | null = null;
    onclose: (() => void) | null = null;
    closed = false;
    constructor(url: string) {
      const answer = FakeSocket.answers.get(url);
      setTimeout(() => {
        if (answer === undefined) return; // silent host: the probe must time out
        if (answer === null) this.onerror?.();
        else this.onmessage?.({ data: answer });
      }, 5);
    }
    close() {
      this.closed = true;
    }
  }
  const id = 'AbCdEfGhIjKlMnOpQrStUv';
  FakeSocket.answers.set('wss://a/ws', JSON.stringify({ t: 'hello', agentId: id }));
  FakeSocket.answers.set('wss://b/ws', JSON.stringify({ t: 'hello', agentId: 'SomeoneElse0000000000x' }));
  FakeSocket.answers.set('wss://c/ws', null);
  const Impl = FakeSocket as unknown as typeof WebSocket;
  assert.equal(await probeAgent('wss://a/ws', id, 200, Impl), true);
  assert.equal(await probeAgent('wss://b/ws', id, 200, Impl), false);
  assert.equal(await probeAgent('wss://c/ws', id, 200, Impl), false);
  assert.equal(await probeAgent('wss://silent/ws', id, 50, Impl), false);
  assert.equal(await firstReachable(['wss://silent/ws', 'wss://b/ws', 'wss://a/ws'], id, 100, Impl), 'wss://a/ws');
  assert.equal(await firstReachable(['wss://silent/ws', 'wss://c/ws'], id, 50, Impl), null);
  assert.equal(await firstReachable([], id, 50, Impl), null);
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    await fn();
    console.log(`  ✓ ${name}`);
  } catch (error) {
    failed++;
    console.error(`  ✗ ${name}\n`, error);
  }
}
if (failed) {
  console.error(`${failed} test(s) failed`);
  process.exit(1);
}
console.log(`${tests.length} tests passed`);
