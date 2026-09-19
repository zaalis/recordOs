/**
 * Vision-Pro handshake & secure channel (client side, WebCrypto).
 *
 * Primitives: ECDSA P-256 / SHA-256 (identity), ECDH P-256 (ephemeral), HKDF-SHA256, AES-256-GCM.
 * The Rust agent implements the exact mirror of this file (agent/crates/vp-core/src/crypto).
 * See docs/PROTOCOL.md for the normative description.
 */
import { DIRECTION, LABEL, LIMITS } from './constants.ts';
import {
  type Bytes,
  b64u,
  concat,
  constantTimeEqual,
  fromB64u,
  fromUtf8,
  lp,
  randomBytes,
  u32be,
  u64be,
  utf8,
} from './encoding.ts';

const subtle = () => {
  const s = globalThis.crypto?.subtle;
  if (!s) throw new Error('Vision-Pro requires a secure context (HTTPS) for WebCrypto.');
  return s;
};

export async function sha256(...parts: Uint8Array[]): Promise<Bytes> {
  return new Uint8Array(await subtle().digest('SHA-256', concat(...parts)));
}

/** Stable identifier derived from a raw P-256 public key: base64url(SHA-256(pub)[0..16]). */
export async function idFromPublicKey(publicRaw: Uint8Array): Promise<string> {
  return b64u((await sha256(publicRaw)).slice(0, 16));
}

/** Full SHA-256 fingerprint, base64url. Used in QR codes and for key pinning. */
export async function fingerprintOf(publicRaw: Uint8Array): Promise<string> {
  return b64u(await sha256(publicRaw));
}

/** Human-readable fingerprint: 8 groups of 4 uppercase hex digits (first 16 bytes). */
export function formatFingerprint(fingerprintB64u: string): string {
  const bytes = fromB64u(fingerprintB64u).slice(0, 16);
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('').toUpperCase();
  return hex.match(/.{4}/g)!.join(' ');
}

// ---------------------------------------------------------------------------
// Identity keys
// ---------------------------------------------------------------------------

export interface DeviceIdentity {
  deviceId: string;
  publicRaw: Bytes;
  /** Non-extractable private key. Stored as a CryptoKey object in IndexedDB. */
  privateKey: CryptoKey;
  createdAt: number;
}

export async function generateDeviceIdentity(): Promise<DeviceIdentity> {
  const pair = await subtle().generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']);
  const publicRaw = new Uint8Array(await subtle().exportKey('raw', pair.publicKey));
  return {
    deviceId: await idFromPublicKey(publicRaw),
    publicRaw,
    privateKey: pair.privateKey,
    createdAt: Date.now(),
  };
}

async function importVerifyKey(publicRaw: Uint8Array): Promise<CryptoKey> {
  if (publicRaw.length !== 65 || publicRaw[0] !== 0x04) throw new Error('invalid P-256 public key');
  return subtle().importKey('raw', new Uint8Array(publicRaw), { name: 'ECDSA', namedCurve: 'P-256' }, false, ['verify']);
}

export async function signEcdsa(privateKey: CryptoKey, message: Uint8Array): Promise<Bytes> {
  return new Uint8Array(await subtle().sign({ name: 'ECDSA', hash: 'SHA-256' }, privateKey, new Uint8Array(message)));
}

export async function verifyEcdsa(publicRaw: Uint8Array, signature: Uint8Array, message: Uint8Array): Promise<boolean> {
  if (signature.length !== 64) return false;
  const key = await importVerifyKey(publicRaw);
  return subtle().verify({ name: 'ECDSA', hash: 'SHA-256' }, key, new Uint8Array(signature), new Uint8Array(message));
}

// ---------------------------------------------------------------------------
// Key schedule
// ---------------------------------------------------------------------------

async function hkdf(ikm: Uint8Array, salt: Uint8Array, info: string, bytes: number): Promise<Bytes> {
  const key = await subtle().importKey('raw', new Uint8Array(ikm), 'HKDF', false, ['deriveBits']);
  const bits = await subtle().deriveBits(
    { name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(salt), info: utf8(info) },
    key,
    bytes * 8,
  );
  return new Uint8Array(bits);
}

export interface TranscriptInput {
  agentId: string;
  agentPub: Uint8Array;
  commit: Uint8Array;
  mode: 'pair' | 'auth';
  clientEph: Uint8Array;
  clientNonce: Uint8Array;
  deviceId: string;
  devicePub: Uint8Array;
  deviceName: string;
  deviceKind: string;
  serverEph: Uint8Array;
  serverNonce: Uint8Array;
}

export async function computeCommit(serverEph: Uint8Array, serverNonce: Uint8Array): Promise<Bytes> {
  return sha256(utf8(LABEL.commit), lp(serverEph), lp(serverNonce));
}

export async function computeTranscript(t: TranscriptInput): Promise<Bytes> {
  return sha256(
    utf8(LABEL.transcript),
    lp(t.agentId),
    lp(t.agentPub),
    lp(t.commit),
    lp(t.mode),
    lp(t.clientEph),
    lp(t.clientNonce),
    lp(t.deviceId),
    lp(t.devicePub),
    lp(t.deviceName),
    lp(t.deviceKind),
    lp(t.serverEph),
    lp(t.serverNonce),
  );
}

export interface SessionKeys {
  c2s: Bytes;
  s2c: Bytes;
  /** 6-digit short authentication string shown on both screens while pairing. */
  sas: string;
  sessionId: string;
  transcript: Bytes;
}

export async function deriveSessionKeys(sharedSecret: Uint8Array, transcript: Bytes): Promise<SessionKeys> {
  const [c2s, s2c, sasBytes, sid] = await Promise.all([
    hkdf(sharedSecret, transcript, LABEL.c2s, 32),
    hkdf(sharedSecret, transcript, LABEL.s2c, 32),
    hkdf(sharedSecret, transcript, LABEL.sas, 8),
    hkdf(sharedSecret, transcript, LABEL.sessionId, 16),
  ]);
  return { c2s, s2c, sas: sasFromBytes(sasBytes), sessionId: b64u(sid), transcript };
}

export function sasFromBytes(bytes: Uint8Array): string {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const value = (BigInt(view.getUint32(0, false)) << 32n) | BigInt(view.getUint32(4, false));
  return (value % 1_000_000n).toString().padStart(6, '0');
}

export async function nextEpochKey(key: Uint8Array): Promise<Bytes> {
  return hkdf(key, new Uint8Array(0), LABEL.rekey, 32);
}

export async function pairProofMac(secret: string, transcript: Uint8Array): Promise<Bytes> {
  const keyBytes = await sha256(utf8(LABEL.pairSecret), utf8(secret));
  const key = await subtle().importKey('raw', keyBytes, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  return new Uint8Array(await subtle().sign('HMAC', key, new Uint8Array(transcript)));
}

// ---------------------------------------------------------------------------
// Client handshake state machine
// ---------------------------------------------------------------------------

export interface ServerHelloFields {
  agentId: string;
  agentPub: string;
  commit: string;
}

export interface ClientInitResult {
  clientEph: string;
  nC: string;
}

export class ClientHandshake {
  private ephemeral?: CryptoKeyPair;
  private ephRaw?: Bytes;
  private nonce?: Bytes;
  private hello?: { agentId: string; agentPub: Bytes; commit: Bytes };
  keys?: SessionKeys;
  private readonly identity: DeviceIdentity;
  private readonly deviceName: string;
  private readonly deviceKind: string;
  private readonly mode: 'pair' | 'auth';
  /** Expected agent key fingerprint (pinned from a previous pairing or from a QR code). */
  private readonly expectedAgentFingerprint: string | undefined;

  constructor(
    identity: DeviceIdentity,
    deviceName: string,
    deviceKind: string,
    mode: 'pair' | 'auth',
    expectedAgentFingerprint?: string,
  ) {
    this.identity = identity;
    this.deviceName = deviceName;
    this.deviceKind = deviceKind;
    this.mode = mode;
    this.expectedAgentFingerprint = expectedAgentFingerprint;
  }

  async onHello(hello: ServerHelloFields): Promise<ClientInitResult> {
    const agentPub = fromB64u(hello.agentPub, 65);
    const agentId = await idFromPublicKey(agentPub);
    if (agentId !== hello.agentId) throw new HandshakeError('agent_mismatch');
    if (this.expectedAgentFingerprint) {
      const fp = await fingerprintOf(agentPub);
      if (!constantTimeEqual(utf8(fp), utf8(this.expectedAgentFingerprint))) throw new HandshakeError('agent_mismatch');
    }
    this.hello = { agentId, agentPub, commit: fromB64u(hello.commit, 32) };
    this.ephemeral = await subtle().generateKey({ name: 'ECDH', namedCurve: 'P-256' }, false, ['deriveBits']);
    this.ephRaw = new Uint8Array(await subtle().exportKey('raw', this.ephemeral.publicKey));
    this.nonce = randomBytes(32);
    return { clientEph: b64u(this.ephRaw), nC: b64u(this.nonce) };
  }

  /** Verifies the server's reveal and returns the device signature for the `prove` message. */
  async onReveal(reveal: { serverEph: string; nS: string; sig: string }): Promise<{ sig: string; keys: SessionKeys }> {
    if (!this.hello || !this.ephemeral || !this.ephRaw || !this.nonce) throw new HandshakeError('protocol');
    const serverEph = fromB64u(reveal.serverEph, 65);
    const serverNonce = fromB64u(reveal.nS, 32);
    const expectedCommit = await computeCommit(serverEph, serverNonce);
    if (!constantTimeEqual(expectedCommit, this.hello.commit)) throw new HandshakeError('commit_mismatch');

    const transcript = await computeTranscript({
      agentId: this.hello.agentId,
      agentPub: this.hello.agentPub,
      commit: this.hello.commit,
      mode: this.mode,
      clientEph: this.ephRaw,
      clientNonce: this.nonce,
      deviceId: this.identity.deviceId,
      devicePub: this.identity.publicRaw,
      deviceName: this.deviceName,
      deviceKind: this.deviceKind,
      serverEph,
      serverNonce,
    });

    const agentSigOk = await verifyEcdsa(
      this.hello.agentPub,
      fromB64u(reveal.sig, 64),
      concat(utf8(LABEL.agentSig), transcript),
    );
    if (!agentSigOk) throw new HandshakeError('bad_signature');

    const serverEphKey = await subtle().importKey('raw', serverEph, { name: 'ECDH', namedCurve: 'P-256' }, false, []);
    const shared = new Uint8Array(
      await subtle().deriveBits({ name: 'ECDH', public: serverEphKey }, this.ephemeral.privateKey, 256),
    );
    const keys = await deriveSessionKeys(shared, transcript);
    shared.fill(0);
    this.keys = keys;

    const sig = await signEcdsa(this.identity.privateKey, concat(utf8(LABEL.deviceSig), transcript));
    return { sig: b64u(sig), keys };
  }

  get agentPub(): Bytes | undefined {
    return this.hello?.agentPub;
  }
}

export class HandshakeError extends Error {
  readonly code: string;
  constructor(code: string) {
    super(`handshake failed: ${code}`);
    this.code = code;
  }
}

// ---------------------------------------------------------------------------
// Secure channel (AES-256-GCM frames with strict sequencing and ratcheting rekey)
// ---------------------------------------------------------------------------

export interface EncryptedFrame {
  t: 'enc';
  e: number;
  s: number;
  d: string;
}

class DirectionalCipher {
  private key?: CryptoKey;
  private raw: Bytes;
  epoch = 0;
  seq = 0;
  startedAt = Date.now();
  private readonly direction: number;

  constructor(initialKey: Bytes, direction: number) {
    this.raw = initialKey;
    this.direction = direction;
  }

  private async cryptoKey(): Promise<CryptoKey> {
    if (!this.key) {
      this.key = await subtle().importKey('raw', this.raw, { name: 'AES-GCM' }, false, ['encrypt', 'decrypt']);
    }
    return this.key;
  }

  nonceAndAad(epoch: number, seq: number): { iv: Bytes; aad: Bytes } {
    const iv = concat(u32be(this.direction), u64be(seq));
    const aad = concat(utf8(LABEL.frame), u32be(this.direction), u32be(epoch), u64be(seq));
    return { iv, aad };
  }

  async encrypt(plain: Bytes): Promise<EncryptedFrame> {
    const { iv, aad } = this.nonceAndAad(this.epoch, this.seq);
    const cipher = await subtle().encrypt({ name: 'AES-GCM', iv, additionalData: aad, tagLength: 128 }, await this.cryptoKey(), plain);
    const frame: EncryptedFrame = { t: 'enc', e: this.epoch, s: this.seq, d: b64u(new Uint8Array(cipher)) };
    this.seq++;
    return frame;
  }

  async decrypt(frame: EncryptedFrame): Promise<Bytes> {
    if (frame.e !== this.epoch || frame.s !== this.seq) throw new HandshakeError('replay_or_reorder');
    const { iv, aad } = this.nonceAndAad(frame.e, frame.s);
    const data = fromB64u(frame.d);
    const plain = await subtle().decrypt({ name: 'AES-GCM', iv, additionalData: aad, tagLength: 128 }, await this.cryptoKey(), data);
    this.seq++;
    return new Uint8Array(plain);
  }

  async advance(): Promise<void> {
    const next = await nextEpochKey(this.raw);
    this.raw.fill(0);
    this.raw = next;
    this.key = undefined;
    this.epoch++;
    this.seq = 0;
    this.startedAt = Date.now();
  }

  needsRekey(): boolean {
    return this.seq >= LIMITS.rekeyAfterMessages || Date.now() - this.startedAt >= LIMITS.rekeyAfterMs;
  }
}

/**
 * Serializes all encrypt/decrypt operations so sequence numbers stay strictly ordered
 * even though WebCrypto is asynchronous.
 */
export class SecureChannel {
  private readonly tx: DirectionalCipher;
  private readonly rx: DirectionalCipher;
  private txQueue: Promise<unknown> = Promise.resolve();
  private rxQueue: Promise<unknown> = Promise.resolve();

  constructor(keys: SessionKeys, role: 'client' | 'server') {
    const c2s = new Uint8Array(keys.c2s);
    const s2c = new Uint8Array(keys.s2c);
    this.tx = role === 'client' ? new DirectionalCipher(c2s, DIRECTION.c2s) : new DirectionalCipher(s2c, DIRECTION.s2c);
    this.rx = role === 'client' ? new DirectionalCipher(s2c, DIRECTION.s2c) : new DirectionalCipher(c2s, DIRECTION.c2s);
  }

  seal(message: { t: string } & Record<string, unknown>): Promise<EncryptedFrame[]> {
    const run = async (): Promise<EncryptedFrame[]> => {
      const frames: EncryptedFrame[] = [];
      frames.push(await this.tx.encrypt(utf8(JSON.stringify(message))));
      if (this.tx.needsRekey()) {
        frames.push(await this.tx.encrypt(utf8(JSON.stringify({ t: 'rekey', epoch: this.tx.epoch + 1 }))));
        await this.tx.advance();
      }
      return frames;
    };
    const result = this.txQueue.then(run, run);
    this.txQueue = result.catch(() => undefined);
    return result;
  }

  /** Returns the decrypted message, or `null` for internal messages (rekey) that were consumed. */
  open(frame: EncryptedFrame): Promise<({ t: string } & Record<string, unknown>) | null> {
    const run = async () => {
      if (typeof frame.d !== 'string' || frame.d.length > (LIMITS.maxSignalFrameBytes * 4) / 3 + 32) {
        throw new HandshakeError('frame_too_large');
      }
      const plain = await this.rx.decrypt(frame);
      const message = JSON.parse(fromUtf8(plain)) as { t: string } & Record<string, unknown>;
      if (typeof message !== 'object' || message === null || typeof message.t !== 'string') {
        throw new HandshakeError('protocol');
      }
      if (message.t === 'rekey') {
        if (message.epoch !== this.rx.epoch + 1) throw new HandshakeError('protocol');
        await this.rx.advance();
        return null;
      }
      return message;
    };
    const result = this.rxQueue.then(run, run);
    this.rxQueue = result.catch(() => undefined);
    return result;
  }
}

export async function signRotation(newIdentity: DeviceIdentity, sessionId: string): Promise<string> {
  const sig = await signEcdsa(newIdentity.privateKey, concat(utf8(LABEL.rotate), lp(sessionId), lp(newIdentity.publicRaw)));
  return b64u(sig);
}
