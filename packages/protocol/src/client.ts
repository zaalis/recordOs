/**
 * Signaling client: runs the Vision-Pro handshake over a WebSocket and exposes an encrypted
 * message channel. Used by the iPad app and by end-to-end tests (Node's global WebSocket).
 */
import { PROTOCOL_VERSION } from './constants.ts';
import {
  ClientHandshake,
  type DeviceIdentity,
  type EncryptedFrame,
  HandshakeError,
  SecureChannel,
  type SessionKeys,
  pairProofMac,
} from './crypto.ts';
import { b64u } from './encoding.ts';
import type { DeviceKind, HandshakeErrorCode, ServerHello, SessionReady } from './signaling.ts';

export type ClientEvent =
  | { type: 'hello'; hello: ServerHello }
  | { type: 'pairing'; sas: string; expiresInMs: number }
  | { type: 'paired'; approved: boolean }
  | { type: 'ready'; session: SessionReady; sas: string }
  | { type: 'message'; message: { t: string } & Record<string, unknown> }
  | { type: 'error'; code: HandshakeErrorCode | string; retryAfterMs?: number }
  | { type: 'closed'; code?: number };

export interface SignalingOptions {
  url: string;
  identity: DeviceIdentity;
  deviceName: string;
  deviceKind: DeviceKind;
  mode: 'pair' | 'auth';
  /** Pinned agent fingerprint (known PC or QR code). Strongly recommended. */
  expectedFingerprint?: string;
  /** One-time secret from a QR code or the 6-digit code shown on the PC. */
  proof?: { kind: 'qr' | 'code'; secret: string };
  WebSocketImpl?: typeof WebSocket;
}

type Listener = (event: ClientEvent) => void;

export class SignalingClient {
  private socket?: WebSocket;
  private handshakeTimer?: ReturnType<typeof setTimeout>;
  private handshake: ClientHandshake;
  private channel?: SecureChannel;
  private keys?: SessionKeys;
  private queue: Promise<void> = Promise.resolve();
  private listeners = new Set<Listener>();
  private stage: 'hello' | 'reveal' | 'secure' | 'closed' = 'hello';
  private readonly options: SignalingOptions;
  hello?: ServerHello;

  constructor(options: SignalingOptions) {
    this.options = options;
    this.handshake = new ClientHandshake(
      options.identity,
      options.deviceName,
      options.deviceKind,
      options.mode,
      options.expectedFingerprint,
    );
  }

  on(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private emit(event: ClientEvent): void {
    for (const listener of this.listeners) listener(event);
  }

  get sessionKeys(): SessionKeys | undefined {
    return this.keys;
  }

  /** Raw public key of the agent, available after `hello` (used to pin it after pairing). */
  get agentPublicKey(): Uint8Array | undefined {
    return this.handshake.agentPub;
  }

  connect(): void {
    const Impl = this.options.WebSocketImpl ?? WebSocket;
    let socket: WebSocket;
    try {
      socket = new Impl(this.options.url);
    } catch {
      // A malformed or unavailable endpoint must be visible to the session UI;
      // otherwise a Safari WebSocket can remain on its initial screen forever.
      this.fail('unreachable');
      return;
    }
    this.socket = socket;
    this.handshakeTimer = setTimeout(() => this.fail('unreachable'), 15_000);
    socket.onmessage = (event: MessageEvent) => {
      if (typeof event.data !== 'string') {
        this.fail('protocol');
        return;
      }
      const text = event.data;
      this.queue = this.queue.then(() => this.handle(text)).catch((error: unknown) => {
        this.fail(error instanceof HandshakeError ? error.code : 'protocol');
      });
    };
    socket.onclose = (event: CloseEvent) => {
      this.clearHandshakeTimer();
      if (this.stage !== 'closed') {
        this.stage = 'closed';
        this.emit({ type: 'closed', code: event.code });
      }
    };
    socket.onerror = () => {
      /* onclose follows */
    };
  }

  private sendPlain(message: object): void {
    this.socket?.send(JSON.stringify(message));
  }

  async send(message: { t: string } & Record<string, unknown>): Promise<void> {
    if (!this.channel || this.stage !== 'secure') throw new Error('channel not ready');
    const frames = await this.channel.seal(message);
    for (const frame of frames) this.socket?.send(JSON.stringify(frame));
  }

  close(): void {
    this.clearHandshakeTimer();
    if (this.stage === 'secure') {
      void this.send({ t: 'session.close' }).catch(() => undefined);
    }
    this.stage = 'closed';
    setTimeout(() => this.socket?.close(), 50);
  }

  private fail(code: string): void {
    if (this.stage === 'closed') return;
    this.clearHandshakeTimer();
    this.emit({ type: 'error', code });
    this.stage = 'closed';
    this.socket?.close();
  }

  private async handle(text: string): Promise<void> {
    if (this.stage === 'closed') return;
    if (text.length > 96 * 1024) throw new HandshakeError('protocol');
    const message = JSON.parse(text) as Record<string, unknown>;

    if (message.t === 'error') {
      this.emit({
        type: 'error',
        code: String(message.code),
        ...(typeof message.retryAfterMs === 'number' ? { retryAfterMs: message.retryAfterMs } : {}),
      });
      this.stage = 'closed';
      this.socket?.close();
      return;
    }

    switch (this.stage) {
      case 'hello': {
        if (message.t !== 'hello' || message.v !== PROTOCOL_VERSION) throw new HandshakeError('version');
        const hello = message as unknown as ServerHello;
        this.hello = hello;
        this.emit({ type: 'hello', hello });
        const init = await this.handshake.onHello(hello);
        this.sendPlain({
          t: 'init',
          v: PROTOCOL_VERSION,
          mode: this.options.mode,
          clientEph: init.clientEph,
          nC: init.nC,
          deviceId: this.options.identity.deviceId,
          devicePub: b64u(this.options.identity.publicRaw),
          deviceName: this.options.deviceName,
          deviceKind: this.options.deviceKind,
        });
        this.stage = 'reveal';
        return;
      }
      case 'reveal': {
        if (message.t !== 'reveal') throw new HandshakeError('protocol');
        const { sig, keys } = await this.handshake.onReveal(message as { serverEph: string; nS: string; sig: string });
        this.keys = keys;
        this.channel = new SecureChannel(keys, 'client');
        this.sendPlain({ t: 'prove', sig });
        this.stage = 'secure';
        this.clearHandshakeTimer();
        if (this.options.mode === 'pair') {
          const proof = this.options.proof
            ? { kind: this.options.proof.kind, mac: b64u(await pairProofMac(this.options.proof.secret, keys.transcript)) }
            : undefined;
          await this.send(proof ? { t: 'pair.request', proof } : { t: 'pair.request' });
        }
        return;
      }
      case 'secure': {
        if (message.t !== 'enc') throw new HandshakeError('protocol');
        const opened = await this.channel!.open(message as unknown as EncryptedFrame);
        if (!opened) return;
        switch (opened.t) {
          case 'pair.pending':
            this.emit({ type: 'pairing', sas: this.keys!.sas, expiresInMs: Number(opened.expiresInMs) || 0 });
            return;
          case 'pair.result':
            this.emit({ type: 'paired', approved: opened.approved === true });
            return;
          case 'session.ready':
            this.emit({ type: 'ready', session: opened as unknown as SessionReady, sas: this.keys!.sas });
            return;
          default:
            this.emit({ type: 'message', message: opened });
        }
      }
    }
  }

  private clearHandshakeTimer(): void {
    if (this.handshakeTimer !== undefined) {
      clearTimeout(this.handshakeTimer);
      this.handshakeTimer = undefined;
    }
  }
}

/**
 * Checks that a signaling endpoint answers with the expected PC, without authenticating: the
 * handshake stops after the agent's plaintext `hello`. Used to pick a USB cable path before
 * connecting (only the tablet at the other end of the cable can reach a cable address).
 */
export function probeAgent(url: string, agentId: string, timeoutMs = 1500, WebSocketImpl: typeof WebSocket = WebSocket): Promise<boolean> {
  return new Promise((resolve) => {
    let socket: WebSocket | undefined;
    let settled = false;
    const finish = (ok: boolean) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (socket) {
        socket.onmessage = socket.onerror = socket.onclose = null;
        try {
          socket.close();
        } catch {
          /* already closed */
        }
      }
      resolve(ok);
    };
    const timer = setTimeout(() => finish(false), timeoutMs);
    try {
      socket = new WebSocketImpl(url);
    } catch {
      finish(false);
      return;
    }
    socket.onmessage = (event: MessageEvent) => {
      try {
        const hello = JSON.parse(String(event.data)) as { t?: unknown; agentId?: unknown };
        finish(hello.t === 'hello' && hello.agentId === agentId);
      } catch {
        finish(false);
      }
    };
    socket.onerror = () => finish(false);
    socket.onclose = () => finish(false);
  });
}

/** First endpoint (in list order of answers) that reaches the expected PC, or null. */
export function firstReachable(urls: string[], agentId: string, timeoutMs = 1500, WebSocketImpl: typeof WebSocket = WebSocket): Promise<string | null> {
  if (!urls.length) return Promise.resolve(null);
  return new Promise((resolve) => {
    let pending = urls.length;
    let settled = false;
    for (const url of urls) {
      void probeAgent(url, agentId, timeoutMs, WebSocketImpl).then((ok) => {
        pending -= 1;
        if (settled) return;
        if (ok) {
          settled = true;
          resolve(url);
        } else if (pending === 0) {
          settled = true;
          resolve(null);
        }
      });
    }
  });
}
