/**
 * Session manager: authenticated, end-to-end encrypted WebSocket to the PC, input delivery,
 * latency measurement, moves onto a USB cable and automatic reconnection.
 *
 * Nothing is streamed from the PC: this iPad is a drawing tablet / trackpad. Its input travels
 * inside the same encrypted channel as the handshake, so a session needs only one connection that
 * works on the local network, over Tailscale, through a USB cable or through the Internet relay.
 */
import {
  type ControlToAgent,
  type ControlToClient,
  type DeviceIdentity,
  type DisplayInfo,
  type SessionLink,
  type SessionReady,
  SignalingClient,
  b64u,
  cableSignalingUrls,
  fingerprintOf,
  firstReachable,
  signRotation,
  generateDeviceIdentity,
} from '@vision-pro/protocol';
import { deviceKind, signalingUrl } from './platform.ts';

export type ConnectionPhase = 'idle' | 'connecting' | 'pairing' | 'connected' | 'reconnecting' | 'ended' | 'failed';

export interface LiveStats {
  pingMs: number | null;
  /** Half the round trip: how long input takes to reach the PC. */
  inputLatencyMs: number | null;
}

export interface ConnectionSnapshot {
  phase: ConnectionPhase;
  error: string | null;
  sas: string | null;
  pairingExpiresAt: number | null;
  session: SessionReady | null;
  displays: DisplayInfo[];
  activeDisplayId: string | null;
  controlAllowed: boolean;
  activeApp: { exe: string; profile: string | null } | null;
  stats: LiveStats;
  connectedAt: number | null;
  reconnectAttempt: number;
  /** Physical path of the current session (Wi-Fi, USB cable, Internet relay). */
  link: SessionLink | null;
  /** The session is moving to another path (Wi-Fi → cable); the surface stays usable. */
  switching: boolean;
}

export interface ConnectTarget {
  agentId?: string;
  name: string;
  fingerprint?: string;
  /** Explicit endpoint (the Internet relay). */
  url?: string;
  /** USB cable endpoints of the PC, tried first when reachable. */
  usbUrls?: string[];
  mode: 'pair' | 'auth';
  /** The 6-digit code shown on the PC. */
  proof?: { kind: 'code'; secret: string };
}

/** Time allowed for a cable endpoint to answer before the local network is used. */
const CABLE_PROBE_MS = 1500;
/** A cable endpoint that did not answer is not retried for this long (unless the PC announces it). */
const CABLE_RETRY_MS = 15_000;
/** Above this many unsent bytes, hover moves wait (the newest one wins) instead of piling up. */
const REALTIME_BACKLOG_BYTES = 16_000;

const EMPTY_STATS: LiveStats = { pingMs: null, inputLatencyMs: null };

const ERROR_TEXT: Record<string, string> = {
  denied: 'La connexion a été refusée sur le PC.',
  expired: 'La demande a expiré. Réessayez.',
  revoked: 'Cet iPad a été supprimé du PC. Sur le PC, choisissez « Connecter un appareil » et entrez le nouveau code.',
  unknown_device: 'Cet iPad n’est plus enregistré sur ce PC. Sur le PC, choisissez « Connecter un appareil » et entrez le nouveau code.',
  bad_code: 'Ce code ne correspond pas à celui affiché sur le PC. Vérifiez-le et réessayez.',
  code_expired: 'Trop d’essais : le PC affiche maintenant un nouveau code. Entrez celui-ci.',
  pairing_disabled: 'Sur le PC, choisissez « Connecter un appareil » pour afficher un code, puis entrez-le ici.',
  locked_out: 'Trop de tentatives. Patientez avant de réessayer.',
  rate_limited: 'Trop de tentatives. Patientez un instant.',
  pairing_busy: 'Un autre appareil attend déjà une autorisation sur le PC.',
  agent_mismatch: 'L’identité du PC ne correspond pas. La connexion a été arrêtée pour votre sécurité.',
  bad_signature: 'Le contrôle de sécurité a échoué. La connexion a été arrêtée.',
  commit_mismatch: 'Le contrôle de sécurité a échoué. La connexion a été arrêtée.',
  version: 'Mettez à jour Record OS sur le PC ou l’iPad.',
  not_local: 'Ce PC accepte uniquement les appareils de son réseau (ou de son réseau Tailscale).',
  protocol: 'La connexion a été interrompue.',
  unreachable: 'Le PC ne répond pas. Vérifiez que Record OS est ouvert sur le PC (ou que le câble USB et le Partage de connexion sont actifs), puis réessayez.',
  offline: 'Ce PC n’est pas joignable par Internet. Vérifiez que Record OS est ouvert et que l’accès à distance est activé sur le PC.',
};

export function describeError(code: string): string {
  return ERROR_TEXT[code] ?? 'Le PC est inaccessible.';
}

type Listener = () => void;

export class Connection {
  private snapshot: ConnectionSnapshot = this.initial();
  private listeners = new Set<Listener>();
  private signaling?: SignalingClient;
  private target?: ConnectTarget;
  private identity!: DeviceIdentity;
  private deviceName = 'iPad';
  private timers: number[] = [];
  private pingId = 0;
  private pings = new Map<number, number>();
  private closedByUser = false;
  private reconnectTimer?: number;
  private suspendedForPagehide = false;
  // A connect request can outlive the screen which started it (cable probes are asynchronous).
  // Keep late async work from resurrecting a cancelled or superseded session.
  private connectEpoch = 0;
  private signalUrl: string | null = null;
  private cableFailedAt = new Map<string, number>();
  private movingToCable = false;
  private cableRetryTimer?: number;
  /** Latest hover move not sent yet, and whether one is being encrypted. */
  private realtimePending: ArrayBuffer | null = null;
  private realtimeBusy = false;
  private realtimeTimer?: number;
  private lastRealtimeAt = -Infinity;
  private realtimeQueuedAt = 0;
  private lastMeasuredStats: LiveStats = { ...EMPTY_STATS };
  private lastMeasuredAt: number | null = null;
  private inputSequence = 0;
  private diagnosticEvents: { at: number; event: string; detail: string }[] = [];
  readonly counters = { received: 0, sent: 0, coalesced: 0, sendFailures: 0, reconnects: 0 };
  diagnostics() { return { lastStats: { ...this.lastMeasuredStats }, lastMeasuredAt: this.lastMeasuredAt, counters: { ...this.counters }, bufferedBytes: this.signaling?.bufferedAmount ?? 0, encryptionQueue: this.signaling?.pendingSends ?? 0, pendingMoves: this.realtimePending ? 1 : 0, pendingAgeMs: this.realtimePending ? Math.round(performance.now() - this.realtimeQueuedAt) : 0, events: [...this.diagnosticEvents] }; }
  private recordDiagnostic(event: string, detail = '') {
    this.diagnosticEvents.push({ at: Date.now(), event, detail: detail.slice(0, 120) });
    if (this.diagnosticEvents.length > 100) this.diagnosticEvents.shift();
  }
  private inputData(buffer: ArrayBuffer): string {
    const copy = buffer.slice(0);
    if (copy.byteLength >= 4) new DataView(copy).setUint16(2, this.inputSequence++ & 0xffff, true);
    return b64u(new Uint8Array(copy));
  }
  onPaired?: (info: { agentId: string; name: string; fingerprint: string; relayUrl?: string; usbUrls: string[] }) => void;
  /**
   * Current cable endpoints of a PC. Given the endpoints already known, returns the ones to try
   * (the app asks the PC when it can reach it, and otherwise falls back to what it learned).
   */
  cablePaths?: (agentId: string, known: string[]) => Promise<string[]>;
  onAccessRemoved?: (agentId: string) => void;
  onIdentityRotated?: (identity: DeviceIdentity) => void;

  private initial(): ConnectionSnapshot {
    return {
      phase: 'idle',
      error: null,
      sas: null,
      pairingExpiresAt: null,
      session: null,
      displays: [],
      activeDisplayId: null,
      controlAllowed: true,
      activeApp: null,
      stats: EMPTY_STATS,
      connectedAt: null,
      reconnectAttempt: 0,
      link: null,
      switching: false,
    };
  }

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  getSnapshot = (): ConnectionSnapshot => this.snapshot;

  private update(patch: Partial<ConnectionSnapshot>): void {
    this.snapshot = { ...this.snapshot, ...patch };
    for (const listener of this.listeners) listener();
  }

  async connect(target: ConnectTarget, identity: DeviceIdentity, deviceName: string): Promise<void> {
    ++this.connectEpoch;
    Object.assign(this.counters, { received: 0, sent: 0, coalesced: 0, sendFailures: 0, reconnects: 0 });
    this.diagnosticEvents = [];
    this.teardown();
    this.closedByUser = false;
    this.target = target;
    this.identity = identity;
    this.deviceName = deviceName;
    this.suspendedForPagehide = false;
    this.update({ ...this.initial(), phase: 'connecting' });
    await this.openSignaling();
  }

  private cableRecentlyFailed(url: string): boolean {
    const at = this.cableFailedAt.get(url);
    return at !== undefined && Date.now() - at < CABLE_RETRY_MS;
  }

  /**
   * Endpoint for the next (re)connection: a USB cable endpoint when this iPad is at the other end
   * of the cable, otherwise the app's own origin (Wi-Fi/Ethernet or Tailscale), or the relay.
   */
  private async chooseSignalingUrl(): Promise<string> {
    const target = this.target!;
    if (target.url) return target.url;
    const agentId = target.agentId;
    if (!agentId) return signalingUrl();
    let cable = cableSignalingUrls(target.usbUrls ?? []);
    if (this.cablePaths) {
      try {
        cable = cableSignalingUrls(await this.cablePaths(agentId, cable));
      } catch {
        /* keep the known endpoints */
      }
    }
    cable = cable.filter((url) => !this.cableRecentlyFailed(url));
    if (!cable.length) return signalingUrl();
    const reachable = await firstReachable(cable, agentId, CABLE_PROBE_MS);
    if (reachable) return reachable;
    for (const url of cable) this.cableFailedAt.set(url, Date.now());
    return signalingUrl();
  }

  private async openSignaling(forcedUrl?: string): Promise<void> {
    const connectEpoch = this.connectEpoch;
    const url = forcedUrl ?? (await this.chooseSignalingUrl());
    if (this.closedByUser || connectEpoch !== this.connectEpoch || !this.target) return;
    const target = this.target;
    this.signalUrl = url;
    const signaling = new SignalingClient({
      url,
      identity: this.identity,
      deviceName: this.deviceName,
      deviceKind: deviceKind(),
      mode: target.mode,
      ...(target.fingerprint ? { expectedFingerprint: target.fingerprint } : {}),
      ...(target.proof ? { proof: target.proof } : {}),
    });
    this.signaling = signaling;
    signaling.on((event) => {
      if (this.signaling !== signaling) return;
      switch (event.type) {
        case 'hello':
          break;
        case 'pairing':
          this.update({ phase: 'pairing', sas: event.sas, pairingExpiresAt: Date.now() + event.expiresInMs });
          break;
        case 'paired':
          if (!event.approved) this.fail('denied');
          break;
        case 'ready':
          this.onReady(event.session);
          break;
        case 'message':
          this.onSignalMessage(event.message);
          break;
        case 'error':
          // Only a path problem falls back; an authorization answer (revoked, locked out…) is shown.
          if (this.snapshot.switching && (event.code === 'unreachable' || event.code === 'protocol')) this.abortCableMove(url);
          // A known PC that is briefly unreachable or rate limiting is retried, not abandoned.
          else if (target.mode === 'auth' && (event.code === 'unreachable' || event.code === 'rate_limited')) this.scheduleReconnect(event.retryAfterMs);
          else this.fail(event.code);
          break;
        case 'closed':
          if (event.code === 1008 && event.reason === 'rate_limited') { this.recordDiagnostic('rate_limited'); this.fail('rate_limited'); break; }
          this.recordDiagnostic('socket.closed', `${event.code ?? ''}:${event.reason ?? ''}`);
          if (this.snapshot.switching) this.abortCableMove(url);
          else this.onSignalClosed();
          break;
      }
    });
    signaling.connect();
  }

  /** The cable answered the probe but the session could not start there: go back to Wi-Fi. */
  private abortCableMove(url: string): void {
    this.cableFailedAt.set(url, Date.now());
    this.update({ switching: false });
    this.scheduleReconnect(0);
  }

  /**
   * The PC reports cable endpoints while this session runs over Wi-Fi. If this iPad can reach one,
   * it is the tablet on the cable: move the session onto it.
   */
  private async moveToCable(urls: string[], announced: boolean): Promise<void> {
    const target = this.target;
    const agentId = target?.agentId;
    if (!agentId || !target || target.url || target.mode !== 'auth' || this.movingToCable) return;
    if (this.snapshot.link !== 'lan' || this.snapshot.switching) return;
    const all = cableSignalingUrls(urls);
    const candidates = all.filter((url) => announced || !this.cableRecentlyFailed(url));
    if (!candidates.length) {
      // The cable failed moments ago (unplugged, then plugged back?): look once more later.
      if (all.length) {
        window.clearTimeout(this.cableRetryTimer);
        this.cableRetryTimer = window.setTimeout(() => void this.moveToCable(all, true), CABLE_RETRY_MS);
      }
      return;
    }
    this.movingToCable = true;
    const connectEpoch = this.connectEpoch;
    try {
      const url = await firstReachable(candidates, agentId, CABLE_PROBE_MS);
      if (!url) {
        for (const candidate of candidates) this.cableFailedAt.set(candidate, Date.now());
        return;
      }
      const live = this.snapshot.phase === 'connected' || this.snapshot.phase === 'reconnecting';
      if (this.closedByUser || connectEpoch !== this.connectEpoch || this.snapshot.link !== 'lan' || !live) return;
      this.target = { ...target, usbUrls: [url, ...(target.usbUrls ?? []).filter((u) => u !== url)] };
      this.update({ switching: true });
      this.teardown();
      await this.openSignaling(url);
    } finally {
      this.movingToCable = false;
    }
  }

  private onReady(session: SessionReady): void {
    const hello = this.signaling?.hello;
    const agentPub = this.signaling?.agentPublicKey;
    if (hello && agentPub) {
      // Pin the PC key on every successful session (first pairing is proven by the code).
      const relayUrl = typeof session.relay?.url === 'string' ? session.relay.url : undefined;
      const usbUrls = cableSignalingUrls(session.usb?.urls);
      void fingerprintOf(agentPub).then((fingerprint) => this.onPaired?.({ agentId: hello.agentId, name: hello.agentName, fingerprint, usbUrls, ...(relayUrl ? { relayUrl } : {}) }));
    }
    if (hello) {
      this.target = { ...this.target!, mode: 'auth', agentId: hello.agentId, name: hello.agentName };
      delete this.target.proof;
    }
    const link: SessionLink = session.link ?? (this.target?.url ? 'relay' : 'lan');
    this.update({
      link,
      session,
      phase: 'connected',
      switching: false,
      sas: null,
      displays: session.caps.displays,
      activeDisplayId: session.caps.activeDisplayId,
      controlAllowed: session.device.permission === 'control',
      reconnectAttempt: 0,
      error: null,
      connectedAt: this.snapshot.connectedAt ?? Date.now(),
    });
    this.sendControl({ t: 'app.watch', enabled: true });
    this.startTimers();
    // Connected over Wi-Fi while a cable is plugged in (e.g. its probe was too slow): move now.
    if (link === 'lan' && session.usb?.urls?.length) void this.moveToCable(session.usb.urls, false);
  }

  private onSignalMessage(message: { t: string } & Record<string, unknown>): void {
    const control = message as unknown as ControlToClient | { t: string; urls?: unknown; reason?: unknown };
    switch (control.t) {
      case 'pong': {
        const pong = control as Extract<ControlToClient, { t: 'pong' }>;
        const sent = this.pings.get(pong.id);
        if (sent !== undefined) {
          this.pings.delete(pong.id);
          const rtt = performance.now() - sent;
          this.lastMeasuredStats = { pingMs: rtt, inputLatencyMs: rtt / 2 };
          this.lastMeasuredAt = Date.now();
          this.update({ stats: this.lastMeasuredStats });
        }
        return;
      }
      case 'app.active': {
        const app = control as Extract<ControlToClient, { t: 'app.active' }>;
        this.update({ activeApp: { exe: app.exe, profile: app.profile } });
        return;
      }
      case 'displays': {
        const displays = control as Extract<ControlToClient, { t: 'displays' }>;
        this.update({ displays: displays.displays, activeDisplayId: displays.activeId });
        return;
      }
      case 'session.permission':
        this.update({ controlAllowed: (control as { control?: unknown }).control === true });
        return;
      case 'device.rotated':
        return;
      case 'link.usb': {
        const urls = (control as { urls?: unknown }).urls;
        void this.moveToCable(Array.isArray(urls) ? (urls as string[]) : [], true);
        return;
      }
      case 'session.end': {
        const reason = String((control as { reason?: unknown }).reason);
        // 'link': the cable carrying this session was unplugged; reconnect over Wi-Fi at once.
        if (reason === 'expired' || reason === 'replaced' || reason === 'link') {
          this.scheduleReconnect(0, reason === 'link', true);
        } else {
          if (reason === 'revoked' && this.target?.mode === 'auth' && this.target.agentId) {
            this.onAccessRemoved?.(this.target.agentId);
          }
          this.finishSession(
            reason === 'user' ? 'ended' : 'failed',
            reason === 'revoked' ? describeError('revoked') : reason === 'user' ? null : 'La session s’est arrêtée sur le PC.',
          );
        }
        return;
      }
    }
  }

  private startTimers(): void {
    this.clearTimers();
    this.timers.push(
      window.setInterval(() => {
        const id = ++this.pingId;
        this.pings.set(id, performance.now());
        if (this.pings.size > 10) this.pings.delete(this.pings.keys().next().value!);
        const rttMs = this.snapshot.stats.pingMs;
        this.sendControl({ t: 'ping', id, t0: Date.now(), ...(rttMs !== null ? { rttMs: Math.round(rttMs * 10) / 10 } : {}) });
      }, 1000),
    );
  }

  private clearTimers(): void {
    for (const timer of this.timers) window.clearInterval(timer);
    this.timers = [];
    this.pings.clear();
  }

  sendControl(message: ControlToAgent): void {
    const signaling = this.signaling;
    if (!signaling?.open || this.snapshot.phase !== 'connected') return;
    // A socket closing during a tab switch rejects; the session's own close handling follows.
    void signaling.send(message as unknown as { t: string } & Record<string, unknown>).catch(() => { this.counters.sendFailures++; this.recordDiagnostic('send.failed'); });
  }

  /** Reliable, ordered input (clicks, keys, pen strokes). */
  sendInput(buffer: ArrayBuffer): void {
    if (!this.snapshot.controlAllowed || !this.signaling?.open || this.snapshot.phase !== 'connected') return;
    // A hover move still waiting must reach the PC before the press that follows it. Messages
    // are encrypted and sent in call order, so queue it first.
    const hover = this.realtimePending;
    if (hover) {
      this.realtimePending = null;
      this.sendPendingBeforeBarrier(hover);
    }
    this.sendControl({ t: 'in', d: this.inputData(buffer) });
    this.counters.sent++;
  }

  /** Latest-wins input (hover moves). Coalesced under congestion rather than queued. */
  sendRealtime(buffer: ArrayBuffer): void {
    if (!this.snapshot.controlAllowed || !this.signaling?.open || this.snapshot.phase !== 'connected') return;
    this.counters.received++;
    const previous = this.realtimePending;
    if (!previous) this.realtimeQueuedAt = performance.now();
    if (previous) this.counters.coalesced++;
    if (previous?.byteLength === 12 && buffer.byteLength === 12 && new Uint8Array(previous)[0] === 2 && new Uint8Array(buffer)[0] === 2) {
      buffer = buffer.slice(0);
      const a = new DataView(previous), b = new DataView(buffer);
      b.setFloat32(4, a.getFloat32(4, true) + b.getFloat32(4, true), true);
      b.setFloat32(8, a.getFloat32(8, true) + b.getFloat32(8, true), true);
    } else if (previous && previous.byteLength >= 4 && buffer.byteLength >= 4 && new Uint8Array(previous)[0] !== new Uint8Array(buffer)[0]) {
      this.sendPendingBeforeBarrier(previous);
    }
    this.realtimePending = buffer;
    if (!this.realtimeBusy) this.flushRealtime();
  }

  private splitRelative(buffer: ArrayBuffer): [ArrayBuffer, ArrayBuffer | null] {
    if (buffer.byteLength !== 12 || new Uint8Array(buffer)[0] !== 2) return [buffer, null];
    const view = new DataView(buffer), dx = view.getFloat32(4, true), dy = view.getFloat32(8, true);
    if (!Number.isFinite(dx) || !Number.isFinite(dy) || Math.abs(dx) > 1024 || Math.abs(dy) > 1024) {
      this.recordDiagnostic('relative.invalid'); this.fail('protocol'); return [buffer, null];
    }
    const x = Math.max(-1, Math.min(1, dx)), y = Math.max(-1, Math.min(1, dy));
    const part = buffer.slice(0), out = new DataView(part);
    out.setFloat32(4, x, true); out.setFloat32(8, y, true);
    if (Math.abs(dx - x) < 0.000001 && Math.abs(dy - y) < 0.000001) return [part, null];
    const rest = buffer.slice(0), r = new DataView(rest);
    r.setFloat32(4, dx - x, true); r.setFloat32(8, dy - y, true);
    return [part, rest];
  }
  private sendPendingBeforeBarrier(buffer: ArrayBuffer): void {
    let next: ArrayBuffer | null = buffer;
    while (next) {
      const [part, rest] = this.splitRelative(next);
      this.sendControl({ t: 'rt', d: this.inputData(part) }); this.counters.sent++;
      next = rest;
    }
  }

  private flushRealtime(): void {
    const signaling = this.signaling;
    const next = this.realtimePending;
    if (!next || !signaling?.open || this.snapshot.phase !== 'connected') {
      this.realtimePending = null;
      return;
    }
    const remaining = 1000 / 120 - (performance.now() - this.lastRealtimeAt);
    if (signaling.bufferedAmount > REALTIME_BACKLOG_BYTES || remaining > 0) {
      // The network is behind: keep only the newest move and look again shortly.
      window.clearTimeout(this.realtimeTimer);
      this.realtimeTimer = window.setTimeout(() => this.flushRealtime(), Math.max(1, Math.ceil(remaining), 8));
      return;
    }
    const [part, rest] = this.splitRelative(next);
    this.realtimePending = rest;
    this.realtimeBusy = true;
    this.lastRealtimeAt = performance.now();
    this.counters.sent++;
    void signaling
      .send({ t: 'rt', d: this.inputData(part) })
      .catch(() => { this.counters.sendFailures++; this.recordDiagnostic('move.send_failed'); })
      .finally(() => {
        if (this.signaling !== signaling) return;
        this.realtimeBusy = false;
        if (this.realtimePending && this.signaling === signaling) this.flushRealtime();
      });
  }

  selectDisplay(displayId: string): void {
    this.sendControl({ t: 'display.select', displayId });
  }

  async rotateDeviceKey(): Promise<DeviceIdentity | null> {
    const sessionId = this.snapshot.session?.sessionId;
    if (!sessionId || !this.signaling) return null;
    const next = await generateDeviceIdentity();
    await this.signaling.send({ t: 'device.rotate', newPub: b64u(next.publicRaw), sig: await signRotation(next, sessionId) });
    this.identity = next;
    this.onIdentityRotated?.(next);
    return next;
  }

  private onSignalClosed(): void {
    if (this.closedByUser) return;
    if (this.snapshot.phase === 'failed' || this.snapshot.phase === 'ended') return;
    if (this.target?.mode === 'auth' || this.snapshot.phase === 'connected' || this.snapshot.phase === 'reconnecting') {
      this.scheduleReconnect();
    } else {
      this.fail('protocol');
    }
  }

  /**
   * `avoidCable`: the path failed, so a cable that carried the session is probably unplugged.
   * `quiet`: an expected, fast path change, without the reconnection overlay (a second attempt
   * shows it).
   */
  private scheduleReconnect(delay?: number, avoidCable = true, quiet = false): void {
    if (this.snapshot.phase === 'reconnecting' && this.reconnectTimer !== undefined && delay === 0) return;
    if (this.closedByUser || !this.target || this.target.mode !== 'auth') {
      this.fail('protocol');
      return;
    }
    // The cable that carried the session is probably gone: prefer another path this time.
    const fromCable = this.snapshot.link === 'usb';
    if (avoidCable && fromCable && this.signalUrl) this.cableFailedAt.set(this.signalUrl, Date.now());
    // Losing the cable falls back to Wi-Fi within a second or two: no overlay for that first try.
    const silent = (quiet || fromCable) && this.snapshot.reconnectAttempt === 0;
    const attempt = this.snapshot.reconnectAttempt + 1;
    this.counters.reconnects++; this.recordDiagnostic('reconnect', String(attempt));
    if (attempt > 8) {
      this.teardown();
      this.update({ phase: 'failed', error: describeError('protocol') });
      return;
    }
    this.teardown();
    this.update({ phase: 'reconnecting', reconnectAttempt: attempt, switching: silent });
    window.clearTimeout(this.reconnectTimer);
    const connectEpoch = this.connectEpoch;
    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = undefined;
      if (!this.closedByUser && connectEpoch === this.connectEpoch && this.snapshot.phase === 'reconnecting' && !this.signaling) void this.openSignaling();
    }, delay ?? Math.min(8000, 400 * 2 ** (attempt - 1)));
  }

  private fail(code: string): void {
    if ((code === 'revoked' || code === 'unknown_device') && this.target?.mode === 'auth' && this.target.agentId) {
      this.onAccessRemoved?.(this.target.agentId);
    }
    this.finishSession('failed', describeError(code));
  }

  private teardown(): void {
    window.clearTimeout(this.reconnectTimer);
    window.clearTimeout(this.cableRetryTimer);
    window.clearTimeout(this.realtimeTimer);
    this.reconnectTimer = undefined;
    this.cableRetryTimer = undefined;
    this.realtimeTimer = undefined;
    this.realtimePending = null;
    this.realtimeBusy = false;
    this.clearTimers();
    const signaling = this.signaling;
    this.signaling = undefined;
    signaling?.close();
  }

  /** End all transient session state while leaving persisted pairing and preferences untouched. */
  private finishSession(phase: 'idle' | 'ended' | 'failed', error: string | null = null): void {
    // Invalidate path probes and any pending connection work before releasing the socket.
    this.connectEpoch += 1;
    this.closedByUser = true;
    this.suspendedForPagehide = false;
    this.teardown();
    this.target = undefined;
    this.signalUrl = null;
    this.cableFailedAt.clear();
    this.movingToCable = false;
    this.update({ ...this.initial(), phase, error });
  }

  /** Release the live socket when Safari caches or closes this page. */
  suspendForPagehide(): void {
    if (!this.target || this.closedByUser || this.snapshot.phase === 'idle' || this.snapshot.phase === 'failed') return;
    this.suspendedForPagehide = true;
    this.connectEpoch += 1;
    this.teardown();
    if (this.target.mode === 'auth') {
      this.update({ phase: 'reconnecting', session: null, connectedAt: null, stats: EMPTY_STATS, switching: false });
    }
  }

  resumeVisible(): void {
    if (this.closedByUser || !this.target) return;
    // Re-entering a cached page resumes JS timers but not necessarily its WebSocket. Start a
    // fresh authenticated session.
    if (this.suspendedForPagehide) {
      this.suspendedForPagehide = false;
      if (this.target.mode === 'auth') this.scheduleReconnect(0);
      else this.finishSession('failed', 'La demande d’association a été interrompue. Réessayez.');
      return;
    }
    if (this.snapshot.phase === 'reconnecting' && !this.signaling) {
      window.clearTimeout(this.reconnectTimer);
      this.reconnectTimer = undefined;
      void this.openSignaling();
    } else if (this.snapshot.phase === 'connected' && !this.signaling?.open) {
      this.scheduleReconnect(0);
    }
  }

  disconnect(): void {
    this.finishSession('ended');
  }

  reset(): void {
    this.finishSession('idle');
  }
}

export const connection = new Connection();


export function exportConnectionDiagnostics(): void {
  const snapshot = connection.getSnapshot();
  const blob = new Blob([JSON.stringify({ capturedAt: new Date().toISOString(), stats: snapshot.stats, link: snapshot.link, ...connection.diagnostics() }, null, 2)], { type: 'application/json' });
  const url = URL.createObjectURL(blob), anchor = document.createElement('a');
  anchor.href = url; anchor.download = 'record-os-diagnostic.json';
  document.body.append(anchor); anchor.click(); anchor.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 10000);
}
