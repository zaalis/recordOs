/* Generates deterministic interop vectors consumed by the Rust agent tests. */
import { writeFileSync } from 'node:fs';
import { b64u, computeCommit, computeTranscript, deriveSessionKeys, nextEpochKey, pairProofMac, SecureChannel } from '../src/index.ts';

const fill = (n: number, v: number) => new Uint8Array(n).fill(v);
const agentPub = new Uint8Array(65).fill(4); agentPub[0] = 4;
const serverEph = fill(65, 5);
const serverNonce = fill(32, 6);
const commit = await computeCommit(serverEph, serverNonce);
const transcript = await computeTranscript({
  agentId: 'AAAAAAAAAAAAAAAAAAAAAA', agentPub, commit, mode: 'pair',
  clientEph: fill(65, 7), clientNonce: fill(32, 8), deviceId: 'BBBBBBBBBBBBBBBBBBBBBB', devicePub: fill(65, 9),
  deviceName: "Bryan's iPad ✎", deviceKind: 'ipad', serverEph, serverNonce,
});
const shared = fill(32, 42);
const keys = await deriveSessionKeys(shared, transcript);
const client = new SecureChannel(keys, 'client');
const [frame] = await client.seal({ t: 'ping', n: 1 });
const vectors = {
  commit: b64u(commit), transcript: b64u(transcript), c2s: b64u(keys.c2s), s2c: b64u(keys.s2c), sas: keys.sas,
  sessionId: keys.sessionId, rekey: b64u(await nextEpochKey(keys.c2s)),
  pairMac: b64u(await pairProofMac('123456', transcript)), frame,
};
writeFileSync(new URL('../../../agent/crates/vp-agent/tests/vectors.json', import.meta.url), JSON.stringify(vectors, null, 2));
console.log(vectors);
