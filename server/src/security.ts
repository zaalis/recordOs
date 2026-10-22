import { createHash, randomBytes, timingSafeEqual } from 'node:crypto';

export const b64u = (bytes: Uint8Array) => Buffer.from(bytes).toString('base64url');
export const randomToken = (bytes = 32) => b64u(randomBytes(bytes));
export const tokenHash = (token: string) => createHash('sha256').update(token).digest('base64url');

export function constantEqual(a: string, b: string): boolean {
  const aa = Buffer.from(a); const bb = Buffer.from(b);
  return aa.length === bb.length && timingSafeEqual(aa, bb);
}

type Bucket = { attempts: number[]; lockedUntil: number };
export class RateGuard {
  private buckets = new Map<string, Bucket>();
  private readonly max: number;
  private readonly windowMs: number;
  private readonly lockMs: number;
  constructor(max: number, windowMs: number, lockMs: number) { this.max = max; this.windowMs = windowMs; this.lockMs = lockMs; }
  check(key: string, now = Date.now()): number {
    // Bound attacker-controlled keys. Reclaim inactive buckets before refusing new ones.
    if (this.buckets.size >= 10_000) {
      for (const [id, entry] of this.buckets) {
        if (entry.lockedUntil <= now && (entry.attempts.at(-1) ?? 0) <= now - this.windowMs) this.buckets.delete(id);
      }
      if (!this.buckets.has(key) && this.buckets.size >= 10_000) return this.lockMs;
    }
    const bucket = this.buckets.get(key);
    if (!bucket) return 0;
    if (bucket.lockedUntil > now) return bucket.lockedUntil - now;
    bucket.attempts = bucket.attempts.filter((at) => at > now - this.windowMs);
    return bucket.attempts.length >= this.max ? this.lock(key, now) : 0;
  }
  fail(key: string, now = Date.now()): number {
    const bucket = this.buckets.get(key) ?? { attempts: [], lockedUntil: 0 };
    bucket.attempts = bucket.attempts.filter((at) => at > now - this.windowMs);
    bucket.attempts.push(now); this.buckets.set(key, bucket);
    return bucket.attempts.length >= this.max ? this.lock(key, now) : 0;
  }
  success(key: string): void { this.buckets.delete(key); }
  private lock(key: string, now: number) { const bucket = this.buckets.get(key)!; bucket.lockedUntil = now + this.lockMs; return this.lockMs; }
}

export function securityHeaders(nonce: string): Record<string, string> {
  return {
    'content-security-policy': `default-src 'self'; script-src 'self' 'nonce-${nonce}'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; font-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'; upgrade-insecure-requests`,
    'cross-origin-opener-policy': 'same-origin', 'cross-origin-resource-policy': 'same-origin',
    'permissions-policy': 'camera=(self), microphone=(), geolocation=(), payment=(), usb=()',
    'referrer-policy': 'no-referrer', 'x-content-type-options': 'nosniff', 'x-frame-options': 'DENY',
    'strict-transport-security': 'max-age=31536000; includeSubDomains',
  };
}
