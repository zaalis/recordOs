/**
 * Session manager: authenticated signaling, WebRTC peer connection, data channels, statistics,
 * latency measurement and automatic reconnection.
 */
import {
  type AgentStats,
  type AppliedVideo,
  CHANNEL,
  type ClientCodecSupport,
  type ControlToAgent,
  type ControlToClient,
  type CursorUpdate,
  type DeviceIdentity,
  type DisplayInfo,
  type SessionReady,
  SignalingClient,
  type VideoSettings,
  b64u,
  decodeCursor,
  fingerprintOf,
  signRotation,
  generateDeviceIdentity,
} from '@vision-pro/protocol';
import { deviceKind, detectCodecs, measureRefreshRate, physicalViewport, signalingUrl } from './platform.ts';

export type ConnectionPhase =
  | 'idle'
  | 'connecting'
  | 'pairing'
  | 'negotiating'
  | 'streaming'
  | 'reconnecting'
  | 'ended'
  | 'failed';

export interface LiveStats {
  pingMs: number | null;
  inputLatencyMs: number | null;
  videoLatencyMs: number | null;
  fps: number | null;
  decodedFps: number | null;
  bitrateKbps: number | null;
  width: number | null;
  height: number | null;
  codec: string | null;
  packetLossPct: number | null;
  jitterMs: number | null;
  decodeMs: number | null;
  jitterBufferMs: number | null;
  iceState: string;
  connectionState: string;
  networkKbps: number | null;
  agent: AgentStats | null;
  candidateType: string | null;
}

export interface CursorShapeInfo {
  id: number;
  url: string;
  width: number;
  height: number;
  hotX: number;
  hotY: number;
}

export interface ConnectionSnapshot {
  phase: ConnectionPhase;
  error: string | null;
  sas: string | null;
  pairingExpiresAt: number | null;
  session: SessionReady | null;
  video: AppliedVideo | null;
  displays: DisplayInfo[];
  activeDisplayId: string | null;
  controlAllowed: boolean;
  activeApp: { exe: string; profile: string | null } | null;
  stats: LiveStats;
  stream: MediaStream | null;
  connectedAt: number | null;
  reconnectAttempt: number;
}

export interface ConnectTarget {
  agentId?: string;
  name: string;
  fingerprint?: string;
  url?: string;
  mode: 'pair' | 'auth';
  proof?: { kind: 'qr' | 'code'; secret: string };
}

const EMPTY_STATS: LiveStats = {
  pingMs: null,
  inputLatencyMs: null,
  videoLatencyMs: null,
  fps: null,
  decodedFps: null,
  bitrateKbps: null,
  width: null,
  height: null,
  codec: null,
  packetLossPct: null,
  jitterMs: null,
  decodeMs: null,
  jitterBufferMs: null,
  iceState: 'new',
  connectionState: 'new',
  networkKbps: null,
  agent: null,
  candidateType: null,
};

const ERROR_TEXT: Record<string, string> = {
  denied: 'La connexion a été refusée sur le Mac.',
  expired: 'La demande a expiré. Réessayez.',
  revoked: 'Cet iPad a été supprimé du Mac. Scannez un nouveau code QR pour l’associer à nouveau.',
  unknown_device: 'Cet iPad n’est plus enregistré sur ce Mac. Scannez un nouveau code QR pour l’associer à nouveau.',
  locked_out: 'Trop de tentatives. Patientez avant de réessayer.',
  rate_limited: 'Trop de tentatives. Patientez un instant.',
  pairing_disabled: 'L’association depuis les appareils proches est désactivée sur le Mac. Utilisez le code affiché sur le Mac.',
  pairing_busy: 'Un autre appareil attend déjà une autorisation sur le Mac.',
  agent_mismatch: 'L’identité du Mac ne correspond pas. La connexion a été arrêtée pour votre sécurité.',
  bad_signature: 'Le contrôle de sécurité a échoué. La connexion a été arrêtée.',
  commit_mismatch: 'Le contrôle de sécurité a échoué. La connexion a été arrêtée.',
  version: 'Mettez à jour Vision-Pro sur le Mac ou l’iPad.',
  not_local: 'Ce Mac accepte uniquement les appareils du même réseau.',
  protocol: 'La connexion a été interrompue.',
  unreachable: 'Le Mac ne répond pas. Vérifiez que Vision-Pro est ouvert sur le Mac, puis réessayez.',
};

export function describeError(code: string): string {
  return ERROR_TEXT[code] ?? 'Le Mac est inaccessible.';
}

type Listener = () => void;

export class Connection {
  private snapshot: ConnectionSnapshot = this.initial();
  private listeners = new Set<Listener>();
  private signaling?: SignalingClient;
  private pc?: RTCPeerConnection;
  private control?: RTCDataChannel;
  private input?: RTCDataChannel;
  private realtime?: RTCDataChannel;
  private target?: ConnectTarget;
  private identity!: DeviceIdentity;
  private deviceName = 'iPad';
  private video!: VideoSettings;
  private codecs: ClientCodecSupport[] = [];
  private refreshHz = 60;
  private timers: number[] = [];
  private pingId = 0;
  private pings = new Map<number, number>();
  private lastBytes: { bytes: number; at: number } | null = null;
  private lastFrames: { decoded: number; decodeTime: number; jbDelay: number; jbCount: number; at: number } | null = null;
  private lowDecodeSamples = 0;
  private performanceRecoveryApplied = false;
  private closedByUser = false;
  private reconnectTimer?: number;
  private peerTimer?: number;
  private disconnectedTimer?: number;
  private lastFrameProgressAt = 0;
  private keyframeRecoveryRequested = false;
  private suspendedForPagehide = false;
  // A connect request can outlive the screen which started it while Safari is
  // detecting codecs. Keep late async work from resurrecting a cancelled or
  // superseded session.
  private connectEpoch = 0;
  // The same protection is needed for offer generation: a renegotiation or a
  // disconnect may close a peer while its createOffer promise is still pending.
  private peerEpoch = 0;
  private cursorShapes = new Map<number, CursorShapeInfo>();
  onCursor?: (cursor: CursorUpdate) => void;
  onCursorShape?: (shape: CursorShapeInfo) => void;
  onPaired?: (info: { agentId: string; name: string; fingerprint: string }) => void;
  onAccessRemoved?: (agentId: string) => void;
  onIdentityRotated?: (identity: DeviceIdentity) => void;

  private initial(): ConnectionSnapshot {
    return {
      phase: 'idle',
      error: null,
      sas: null,
      pairingExpiresAt: null,
      session: null,
      video: null,
      displays: [],
      activeDisplayId: null,
      controlAllowed: true,
      activeApp: null,
      stats: EMPTY_STATS,
      stream: null,
      connectedAt: null,
      reconnectAttempt: 0,
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

  getCursorShape(id: number): CursorShapeInfo | undefined {
    return this.cursorShapes.get(id);
  }

  async connect(target: ConnectTarget, identity: DeviceIdentity, deviceName: string, video: VideoSettings): Promise<void> {
    const connectEpoch = ++this.connectEpoch;
    this.teardown();
    this.closedByUser = false;
    this.suspendedForPagehide = false;
    this.target = target;
    this.identity = identity;
    this.deviceName = deviceName;
    this.video = video;
    this.lowDecodeSamples = 0;
    this.performanceRecoveryApplied = false;
    this.update({ ...this.initial(), phase: 'connecting' });
    [this.codecs, this.refreshHz] = await Promise.all([detectCodecs(), measureRefreshRate()]);
    if (this.closedByUser || connectEpoch !== this.connectEpoch) return;
    this.openSignaling();
  }

  private openSignaling(): void {
    const target = this.target!;
    const signaling = new SignalingClient({
      url: target.url ?? signalingUrl(),
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
          void this.onSignalMessage(event.message);
          break;
        case 'error':
          if (target.mode === 'auth' && (event.code === 'unreachable' || event.code === 'rate_limited')) {
            this.scheduleReconnect(event.retryAfterMs);
          } else this.fail(event.code);
          break;
        case 'closed':
          this.onSignalClosed();
          break;
      }
    });
    signaling.connect();
  }

  private onReady(session: SessionReady): void {
    const hello = this.signaling?.hello;
    const agentPub = this.signaling?.agentPublicKey;
    if (hello && agentPub) {
      // Pin the Mac key on every successful session (first pairing is trust-on-approval + SAS).
      void fingerprintOf(agentPub).then((fingerprint) => this.onPaired?.({ agentId: hello.agentId, name: hello.agentName, fingerprint }));
    }
    if (hello) {
      this.target = { ...this.target!, mode: 'auth', agentId: hello.agentId, name: hello.agentName };
      delete this.target.proof;
    }
    this.update({
      session,
      phase: 'negotiating',
      sas: null,
      displays: session.caps.displays,
      activeDisplayId: session.caps.activeDisplayId,
      controlAllowed: session.device.permission === 'control',
      error: null,
    });
    void this.startPeer();
  }

  private async startPeer(): Promise<void> {
    this.closePeer();
    this.update({ phase: 'negotiating', stream: null, connectedAt: null, stats: EMPTY_STATS });
    const peerEpoch = this.peerEpoch;
    const pc = new RTCPeerConnection({ iceServers: [], bundlePolicy: 'max-bundle', rtcpMuxPolicy: 'require' });
    let trackReceived = false;
    this.pc = pc;
    const transceiver = pc.addTransceiver('video', { direction: 'recvonly' });
    this.applyCodecPreferences(transceiver);

    this.control = pc.createDataChannel(CHANNEL.control, { ordered: true });
    this.input = pc.createDataChannel(CHANNEL.input, { ordered: true });
    this.realtime = pc.createDataChannel(CHANNEL.inputRealtime, { ordered: false, maxRetransmits: 0 });
    for (const channel of [this.control, this.input, this.realtime]) channel.binaryType = 'arraybuffer';
    this.control.onmessage = (e) => this.onControl(e.data);
    this.realtime.onmessage = (e) => {
      if (e.data instanceof ArrayBuffer) {
        const cursor = decodeCursor(e.data);
        if (cursor) this.onCursor?.(cursor);
      }
    };
    this.control.onopen = () => {
      this.sendControl({ t: 'app.watch', enabled: true });
      this.startTimers();
    };

    pc.ontrack = (event) => {
      if (!this.isCurrentPeer(pc, peerEpoch)) return;
      trackReceived = true;
      if (pc.connectionState === 'connected') {
        window.clearTimeout(this.peerTimer);
        this.peerTimer = undefined;
      }
      const receiver = event.receiver as RTCRtpReceiver & { jitterBufferTarget?: number; playoutDelayHint?: number };
      try {
        if ('jitterBufferTarget' in receiver) receiver.jitterBufferTarget = 0;
        if ('playoutDelayHint' in receiver) receiver.playoutDelayHint = 0;
      } catch {
        /* not supported */
      }
      const stream = event.streams[0] ?? new MediaStream([event.track]);
      event.track.onended = () => {
        if (this.isCurrentPeer(pc, peerEpoch)) this.scheduleReconnect(0);
      };
      this.update({ stream });
    };
    pc.onconnectionstatechange = () => {
      if (!this.isCurrentPeer(pc, peerEpoch)) return;
      const state = pc.connectionState;
      this.update({ stats: { ...this.snapshot.stats, connectionState: state } });
      if (state === 'connected') {
        if (trackReceived) {
          window.clearTimeout(this.peerTimer);
          this.peerTimer = undefined;
        }
        window.clearTimeout(this.disconnectedTimer);
        this.disconnectedTimer = undefined;
        if (this.snapshot.phase !== 'streaming') {
          this.update({ phase: 'streaming', connectedAt: Date.now(), reconnectAttempt: 0 });
        }
      }
      if (state === 'failed') this.scheduleReconnect(0);
      if (state === 'disconnected' && this.disconnectedTimer === undefined) {
        this.disconnectedTimer = window.setTimeout(() => {
          this.disconnectedTimer = undefined;
          if (this.isCurrentPeer(pc, peerEpoch) && pc.connectionState === 'disconnected') this.scheduleReconnect();
        }, 4_000);
      }
    };
    pc.oniceconnectionstatechange = () => {
      if (!this.isCurrentPeer(pc, peerEpoch)) return;
      this.update({ stats: { ...this.snapshot.stats, iceState: pc.iceConnectionState } });
      if (pc.iceConnectionState === 'failed') this.scheduleReconnect(0);
    };

    this.peerTimer = window.setTimeout(() => {
      if (!this.isCurrentPeer(pc, peerEpoch) || (pc.connectionState === 'connected' && trackReceived)) return;
      if (this.target?.mode === 'auth') this.scheduleReconnect();
      else this.failMessage('La diffusion sécurisée n’a pas démarré. Réessayez la connexion.');
    }, 22_000);

    try {
      const offer = await pc.createOffer();
      if (!this.isCurrentPeer(pc, peerEpoch)) return;
      await pc.setLocalDescription(offer);
      // There is intentionally no trickle-ICE message in the encrypted
      // signaling protocol. Sending the SDP before gathering is complete can
      // therefore omit Safari's LAN candidate and leave the UI stuck on
      // "Starting the display". Host-only LAN gathering is normally immediate;
      // the timeout keeps a broken network from blocking the UI indefinitely.
      await waitForIceGathering(pc);
      if (!this.isCurrentPeer(pc, peerEpoch)) return;
      const signaling = this.signaling;
      const sdp = pc.localDescription?.sdp;
      if (!signaling || !sdp) throw new Error('missing local WebRTC description');
      await signaling.send({
        t: 'rtc.offer',
        sdp,
        video: this.video as unknown as Record<string, unknown>,
        display: { ...physicalViewport(), refreshHz: this.refreshHz },
        codecs: this.codecs as unknown as Record<string, unknown>[],
      } as unknown as { t: string });
    } catch {
      if (this.isCurrentPeer(pc, peerEpoch) && !this.closedByUser) {
        if (this.target?.mode === 'auth') this.scheduleReconnect();
        else this.failMessage('La diffusion sécurisée n’a pas pu démarrer. Réessayez la connexion.');
      }
    }
  }

  private isCurrentPeer(pc: RTCPeerConnection, peerEpoch: number): boolean {
    return this.pc === pc && this.peerEpoch === peerEpoch;
  }

  private applyCodecPreferences(transceiver: RTCRtpTransceiver): void {
    const caps = RTCRtpReceiver.getCapabilities?.('video');
    if (!caps || typeof transceiver.setCodecPreferences !== 'function') return;
    const order = ['video/AV1', 'video/H265', 'video/H264'];
    const wanted = this.video.codec === 'auto' ? order : [`video/${{ h264: 'H264', hevc: 'H265', av1: 'AV1' }[this.video.codec]}`, ...order];
    const rank = (mime: string) => {
      const index = wanted.findIndex((w) => w.toLowerCase() === mime.toLowerCase());
      return index === -1 ? 99 : index;
    };
    const codecs = caps.codecs
      .filter((c) => /video\/(h264|h265|av1|rtx|red|ulpfec|flexfec-03)/i.test(c.mimeType))
      .sort((a, b) => rank(a.mimeType) - rank(b.mimeType));
    try {
      transceiver.setCodecPreferences(codecs);
    } catch {
      /* browser rejected ordering; defaults are fine */
    }
  }

  private async onSignalMessage(message: { t: string } & Record<string, unknown>): Promise<void> {
    switch (message.t) {
      case 'rtc.answer': {
        const pc = this.pc;
        if (!pc) return;
        try {
          await pc.setRemoteDescription({ type: 'answer', sdp: String(message.sdp) });
        } catch {
          if (pc === this.pc) this.failMessage('La réponse vidéo du Mac est invalide. Réessayez la connexion.');
          return;
        }
        if (pc === this.pc) this.update({ video: message.video as AppliedVideo });
        return;
      }
      case 'rtc.error':
        // Previously this only populated a hidden error field while the phase
        // remained "negotiating", leaving the iPad behind a permanent spinner.
        this.failMessage(String(message.message ?? 'La diffusion n’a pas pu démarrer.'));
        return;
      case 'session.permission':
        this.update({ controlAllowed: message.control === true });
        return;
      case 'device.rotated':
        return;
      case 'session.end': {
        const reason = String(message.reason);
        if (reason === 'expired' || reason === 'replaced') {
          this.scheduleReconnect(0);
        } else {
          this.closedByUser = true;
          if (reason === 'revoked' && this.target?.mode === 'auth' && this.target.agentId) {
            this.onAccessRemoved?.(this.target.agentId);
          }
          this.teardown();
          this.update({ phase: reason === 'user' ? 'ended' : 'failed', error: reason === 'revoked' ? describeError('revoked') : reason === 'user' ? null : 'The session ended on the Mac.' });
        }
        return;
      }
    }
  }

  private onControl(data: unknown): void {
    if (typeof data !== 'string' || data.length > 256_000) return;
    let message: ControlToClient;
    try {
      message = JSON.parse(data) as ControlToClient;
    } catch {
      return;
    }
    switch (message.t) {
      case 'pong': {
        const sent = this.pings.get(message.id);
        if (sent !== undefined) {
          this.pings.delete(message.id);
          const rtt = performance.now() - sent;
          this.update({ stats: { ...this.snapshot.stats, pingMs: rtt, inputLatencyMs: rtt / 2 + 0.5 } });
        }
        return;
      }
      case 'stats':
        this.update({ stats: { ...this.snapshot.stats, agent: message.stats } });
        return;
      case 'cursor.shape': {
        const bytes = Uint8Array.from(atob(message.png.replace(/-/g, '+').replace(/_/g, '/') + '==='.slice((message.png.length + 3) % 4)), (c) => c.charCodeAt(0));
        const url = URL.createObjectURL(new Blob([bytes], { type: 'image/png' }));
        const previous = this.cursorShapes.get(message.id);
        if (previous) URL.revokeObjectURL(previous.url);
        const shape = { id: message.id, url, width: message.width, height: message.height, hotX: message.hotX, hotY: message.hotY };
        this.cursorShapes.set(message.id, shape);
        if (this.cursorShapes.size > 32) {
          const oldest = this.cursorShapes.keys().next().value!;
          URL.revokeObjectURL(this.cursorShapes.get(oldest)!.url);
          this.cursorShapes.delete(oldest);
        }
        this.onCursorShape?.(shape);
        return;
      }
      case 'app.active':
        this.update({ activeApp: { exe: message.exe, profile: message.profile } });
        return;
      case 'displays':
        this.update({ displays: message.displays, activeDisplayId: message.activeId });
        return;
      case 'video.applied':
        this.update({ video: message.video });
        return;
      case 'video.renegotiate':
        void this.startPeer();
        return;
      case 'permission':
        this.update({ controlAllowed: message.control });
        return;
    }
  }

  private startTimers(): void {
    this.clearTimers();
    this.timers.push(
      window.setInterval(() => {
        const id = ++this.pingId;
        this.pings.set(id, performance.now());
        if (this.pings.size > 10) this.pings.delete(this.pings.keys().next().value!);
        this.sendControl({ t: 'ping', id, t0: Date.now() });
      }, 1000),
      window.setInterval(() => void this.collectStats(), 1000),
    );
  }

  private clearTimers(): void {
    for (const timer of this.timers) window.clearInterval(timer);
    this.timers = [];
    this.pings.clear();
  }

  private async collectStats(): Promise<void> {
    const pc = this.pc;
    if (!pc || document.hidden) return;
    let report: RTCStatsReport;
    try {
      report = await pc.getStats();
    } catch {
      // Closing a peer while an interval tick is pending is expected.
      return;
    }
    if (pc !== this.pc) return;
    const stats: LiveStats = { ...this.snapshot.stats };
    let codecId: string | undefined;
    let decodedProgress = false;
    let decodedCounterAvailable = false;
    const now = performance.now();
    report.forEach((entry: Record<string, unknown> & { type: string }) => {
      if (entry.type === 'inbound-rtp' && entry.kind === 'video') {
        decodedCounterAvailable = typeof entry.framesDecoded === 'number';
        stats.fps = numberOr(entry.framesPerSecond, stats.fps);
        stats.width = numberOr(entry.frameWidth, stats.width);
        stats.height = numberOr(entry.frameHeight, stats.height);
        const lost = Number(entry.packetsLost ?? 0);
        const received = Number(entry.packetsReceived ?? 0);
        stats.packetLossPct = received + lost > 0 ? (lost / (received + lost)) * 100 : 0;
        stats.jitterMs = entry.jitter !== undefined ? Number(entry.jitter) * 1000 : stats.jitterMs;
        const bytes = Number(entry.bytesReceived ?? 0);
        if (this.lastBytes) {
          const seconds = (now - this.lastBytes.at) / 1000;
          if (seconds > 0) stats.bitrateKbps = ((bytes - this.lastBytes.bytes) * 8) / 1000 / seconds;
        }
        this.lastBytes = { bytes, at: now };
        const decoded = Number(entry.framesDecoded ?? 0);
        const decodeTime = Number(entry.totalDecodeTime ?? 0);
        const jbDelay = Number(entry.jitterBufferDelay ?? 0);
        const jbCount = Number(entry.jitterBufferEmittedCount ?? 0);
        if (this.lastFrames) {
          const frames = decoded - this.lastFrames.decoded;
          decodedProgress = frames > 0;
          const seconds = (now - this.lastFrames.at) / 1000;
          if (seconds > 0) stats.decodedFps = Math.max(0, frames) / seconds;
          if (frames > 0) {
            stats.decodeMs = ((decodeTime - this.lastFrames.decodeTime) / frames) * 1000;
          }
          const emitted = jbCount - this.lastFrames.jbCount;
          if (emitted > 0) stats.jitterBufferMs = ((jbDelay - this.lastFrames.jbDelay) / emitted) * 1000;
        }
        this.lastFrames = { decoded, decodeTime, jbDelay, jbCount, at: now };
        codecId = entry.codecId as string | undefined;
      }
      if (entry.type === 'candidate-pair' && (entry.nominated || entry.selected) && entry.state === 'succeeded') {
        if (entry.availableIncomingBitrate) stats.networkKbps = Number(entry.availableIncomingBitrate) / 1000;
        const local = report.get(String(entry.localCandidateId)) as { candidateType?: string } | undefined;
        stats.candidateType = local?.candidateType ?? stats.candidateType;
      }
    });
    if (codecId) {
      const codec = report.get(codecId) as { mimeType?: string } | undefined;
      stats.codec = codec?.mimeType?.split('/')[1] ?? stats.codec;
    }
    const agent = stats.agent;
    const halfRtt = (stats.pingMs ?? 0) / 2;
    if (agent) {
      const frame = 1000 / Math.max(30, stats.fps ?? 60);
      stats.videoLatencyMs = agent.captureMs + agent.encodeMs + halfRtt + (stats.jitterBufferMs ?? 0) + (stats.decodeMs ?? 0) + frame / 2;
    }
    this.maybeRecoverPerformance(stats);
    if (decodedCounterAvailable) this.maybeRecoverFrozenVideo(stats, decodedProgress);
    this.update({ stats });
  }

  private maybeRecoverFrozenVideo(stats: LiveStats, decodedProgress: boolean): void {
    if (this.snapshot.phase !== 'streaming') return;
    if (decodedProgress) {
      this.lastFrameProgressAt = Date.now();
      this.keyframeRecoveryRequested = false;
      return;
    }
    // Idle desktops still send refresh frames. Require evidence that the Mac
    // is sending before treating a static picture as a stalled receiver.
    if ((stats.agent?.sentFps ?? 0) < 1 || !this.lastFrames) return;
    this.lastFrameProgressAt ||= Date.now();
    const stalledMs = Date.now() - this.lastFrameProgressAt;
    if (stalledMs > 6_000 && !this.keyframeRecoveryRequested) {
      this.keyframeRecoveryRequested = true;
      this.requestKeyframe();
    }
    if (stalledMs > 14_000) this.scheduleReconnect(0);
  }

  /**
   * A legacy 4K/200 Mb/s profile can overload a browser decoder even on a
   * strong LAN. Only intervene when the Mac is demonstrably capturing and
   * encoding at speed while the browser decodes fewer than 15 frames/s for
   * three consecutive samples. Static desktops therefore never trigger a
   * quality downgrade merely because they are idle.
   */
  private maybeRecoverPerformance(stats: LiveStats): void {
    if (this.performanceRecoveryApplied || this.video.profile !== 'max-quality') return;
    if (!this.snapshot.connectedAt || Date.now() - this.snapshot.connectedAt < 4_000) return;
    const sourceIsSmooth = (stats.agent?.captureFps ?? 0) >= 24 && (stats.agent?.encodeFps ?? 0) >= 24;
    if (!sourceIsSmooth || stats.decodedFps === null) return;
    if (stats.decodedFps >= 15) {
      this.lowDecodeSamples = 0;
      return;
    }
    this.lowDecodeSamples += 1;
    if (this.lowDecodeSamples < 3) return;

    this.performanceRecoveryApplied = true;
    const recovery: VideoSettings = {
      profile: 'balanced',
      resolution: '1080p',
      fps: 30,
      bitrate: 'auto',
      // H.264 is the common hardware-decoder baseline for iPad Safari and
      // Chrome, avoiding an overloaded HEVC/AV1 decoder on this recovery path.
      codec: 'h264',
    };
    this.video = recovery;
    this.updateVideo(recovery);
  }

  sendControl(message: ControlToAgent): void {
    try {
      if (this.control?.readyState === 'open') this.control.send(JSON.stringify(message));
    } catch {
      // The channel can close between readyState and send during a tab switch.
    }
  }

  /** Reliable, ordered input (clicks, keys, pen strokes). */
  sendInput(buffer: ArrayBuffer): void {
    try {
      if (this.input?.readyState === 'open' && this.snapshot.controlAllowed) this.input.send(buffer);
    } catch {
      // Input is discarded while a failed transport is being replaced.
    }
  }

  /** Latest-wins input (hover moves). Dropped under congestion rather than delayed. */
  sendRealtime(buffer: ArrayBuffer): void {
    const channel = this.realtime;
    try {
      if (channel?.readyState === 'open' && this.snapshot.controlAllowed && channel.bufferedAmount < 16_000) channel.send(buffer);
    } catch {
      // Movement is latest-wins; a closing channel must not queue stale input.
    }
  }

  updateVideo(video: VideoSettings): void {
    this.video = video;
    this.sendControl({ t: 'video.settings', video });
  }

  selectDisplay(displayId: string): void {
    this.sendControl({ t: 'display.select', displayId });
  }

  requestKeyframe(): void {
    this.sendControl({ t: 'keyframe' });
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
    if (this.target?.mode === 'auth') {
      this.scheduleReconnect();
    } else {
      this.fail('protocol');
    }
  }

  private scheduleReconnect(delay?: number): void {
    if (this.closedByUser || !this.target || this.target.mode !== 'auth') {
      this.fail('protocol');
      return;
    }
    const attempt = this.snapshot.reconnectAttempt + 1;
    if (attempt > 8) {
      this.teardown();
      this.update({ phase: 'failed', error: describeError('protocol') });
      return;
    }
    this.teardown();
    this.update({ phase: 'reconnecting', reconnectAttempt: attempt, session: null, video: null, connectedAt: null, stats: EMPTY_STATS });
    window.clearTimeout(this.reconnectTimer);
    const connectEpoch = this.connectEpoch;
    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = undefined;
      if (!this.closedByUser && connectEpoch === this.connectEpoch && this.snapshot.phase === 'reconnecting' && !this.signaling) this.openSignaling();
    }, delay ?? Math.min(8000, 400 * 2 ** (attempt - 1)));
  }

  private fail(code: string): void {
    if ((code === 'revoked' || code === 'unknown_device') && this.target?.mode === 'auth' && this.target.agentId) {
      this.onAccessRemoved?.(this.target.agentId);
    }
    this.teardown();
    this.update({ phase: 'failed', error: describeError(code), sas: null });
  }

  private failMessage(message: string): void {
    this.teardown();
    this.update({ phase: 'failed', error: message, sas: null });
  }

  private closePeer(): void {
    this.peerEpoch += 1;
    this.clearTimers();
    window.clearTimeout(this.peerTimer);
    window.clearTimeout(this.disconnectedTimer);
    this.peerTimer = this.disconnectedTimer = undefined;
    for (const channel of [this.control, this.input, this.realtime]) {
      if (channel) {
        channel.onmessage = null;
        channel.onopen = null;
        try {
          channel.close();
        } catch {
          /* ignore */
        }
      }
    }
    this.control = this.input = this.realtime = undefined;
    if (this.pc) {
      this.pc.ontrack = null;
      this.pc.onconnectionstatechange = null;
      this.pc.close();
      this.pc = undefined;
    }
    this.lastBytes = null;
    this.lastFrames = null;
    this.lastFrameProgressAt = 0;
    this.keyframeRecoveryRequested = false;
    for (const shape of this.cursorShapes.values()) URL.revokeObjectURL(shape.url);
    this.cursorShapes.clear();
  }

  private teardown(clearStream = true): void {
    window.clearTimeout(this.reconnectTimer);
    this.closePeer();
    const signaling = this.signaling;
    this.signaling = undefined;
    signaling?.close();
    if (clearStream) this.update({ stream: null });
  }

  /** Release live sockets when Safari caches or closes this page. */
  suspendForPagehide(): void {
    if (!this.target || this.closedByUser || this.snapshot.phase === 'idle' || this.snapshot.phase === 'failed') return;
    this.suspendedForPagehide = true;
    this.connectEpoch += 1;
    this.teardown();
    if (this.target.mode === 'auth') {
      this.update({ phase: 'reconnecting', session: null, video: null, connectedAt: null, stats: EMPTY_STATS });
    }
  }

  resumeVisible(): void {
    if (this.closedByUser || !this.target) return;
    // Re-entering a cached page resumes JS timers but not necessarily its
    // WebSocket or ICE transport. Start a fresh authenticated session.
    if (this.suspendedForPagehide) {
      this.suspendedForPagehide = false;
      if (this.target.mode === 'auth') this.scheduleReconnect(0);
      else this.failMessage('La demande d’association a été interrompue. Réessayez.');
      return;
    }
    this.lastFrames = null;
    this.lastBytes = null;
    this.lastFrameProgressAt = Date.now();
    if (this.snapshot.phase === 'reconnecting') {
      if (!this.signaling) {
        window.clearTimeout(this.reconnectTimer);
        this.reconnectTimer = undefined;
        this.openSignaling();
      }
    } else if (this.snapshot.phase === 'streaming') {
      if (!this.pc || this.pc.connectionState === 'failed' || this.pc.connectionState === 'closed' || this.pc.connectionState === 'disconnected') {
        this.scheduleReconnect(0);
      } else this.requestKeyframe();
    }
  }

  disconnect(): void {
    this.connectEpoch += 1;
    this.closedByUser = true;
    this.suspendedForPagehide = false;
    this.teardown();
    this.update({ phase: 'ended', session: null, stats: EMPTY_STATS, connectedAt: null });
  }

  reset(): void {
    this.connectEpoch += 1;
    this.closedByUser = true;
    this.suspendedForPagehide = false;
    this.teardown();
    this.update(this.initial());
  }
}

function numberOr(value: unknown, fallback: number | null): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

function waitForIceGathering(pc: RTCPeerConnection, timeoutMs = 4_000): Promise<void> {
  if (pc.iceGatheringState === 'complete') return Promise.resolve();
  return new Promise((resolve) => {
    const finish = () => {
      window.clearTimeout(timeout);
      pc.removeEventListener('icegatheringstatechange', onChange);
      resolve();
    };
    const onChange = () => {
      if (pc.iceGatheringState === 'complete') finish();
    };
    const timeout = window.setTimeout(finish, timeoutMs);
    pc.addEventListener('icegatheringstatechange', onChange);
  });
}

export const connection = new Connection();
