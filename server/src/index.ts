import argon2 from 'argon2';
import { createReadStream } from 'node:fs';
import { stat } from 'node:fs/promises';
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { extname, isAbsolute, join, normalize, relative, resolve, sep } from 'node:path';
import { randomUUID } from 'node:crypto';
import { RateGuard, constantEqual, randomToken, securityHeaders, tokenHash } from './security.ts';
import { UserStore } from './store.ts';

const port = boundedInt(process.env.VP_ACCOUNT_PORT, 3000, 1024, 65535);
const registration = process.env.VP_ALLOW_REGISTRATION === '1';
const insecureDev = process.env.VP_INSECURE_DEV === '1';
const dataPath = resolve(process.env.VP_ACCOUNT_DATA ?? join(process.cwd(), 'data', 'users.json'));
const webRoot = resolve(process.env.VP_WEB_ROOT ?? join(process.cwd(), '..', 'apps', 'ipad', 'dist'));
const store = new UserStore(dataPath);
const sessions = new Map<string, { userId: string; expiresAt: number; csrf: string }>();
const guard = new RateGuard(5, 10 * 60_000, 15 * 60_000);
await store.load();

const server = createServer(async (req, res) => {
  const nonce = randomToken(18);
  for (const [name, value] of Object.entries(securityHeaders(nonce))) res.setHeader(name, value);
  res.setHeader('cache-control', 'no-store');
  const url = new URL(req.url ?? '/', `http://${req.headers.host ?? 'localhost'}`);
  try {
    if (url.pathname.startsWith('/api/account/')) return await accountRoute(url.pathname.slice(13), req, res);
    if (req.method !== 'GET' && req.method !== 'HEAD') return json(res, 405, { error: 'method_not_allowed' });
    return await staticFile(url.pathname, req, res);
  } catch (error) {
    console.error('request failed', error instanceof Error ? error.message : error);
    return json(res, 500, { error: 'request_failed' });
  }
});
server.headersTimeout = 10_000; server.requestTimeout = 15_000; server.keepAliveTimeout = 5_000;
server.listen(port, '127.0.0.1', () => console.log(`Vision-Pro account server listening on http://127.0.0.1:${port}`));

async function accountRoute(route: string, req: IncomingMessage, res: ServerResponse) {
  if (route === 'config' && req.method === 'GET') return json(res, 200, { enabled: true, registration, passkeys: false });
  if (route === 'me' && req.method === 'GET') { const session = sessionOf(req); return json(res, 200, { user: session ? publicUser(store.byId(session.userId)) : null }); }
  if (req.method !== 'POST') return json(res, 405, { error: 'method_not_allowed' });
  if (!sameOrigin(req)) return json(res, 403, { error: 'origin' });
  if (route !== 'login' && route !== 'register' && !validCsrf(req)) return json(res, 403, { error: 'csrf' });
  const body = await readJson(req);
  const ip = clientIp(req); const email = normalizeEmail(body.email);

  if (route === 'register') {
    if (!registration) return json(res, 403, { error: 'registration_disabled' });
    if (guard.check(`ip:${ip}`)) return json(res, 429, { error: 'rate_limited' });
    const password = typeof body.password === 'string' ? body.password : '';
    const name = cleanName(body.name);
    if (!validEmail(email) || !name) return json(res, 400, { error: 'invalid_input' });
    if (password.length < 12 || password.length > 256) return json(res, 400, { error: 'weak_password' });
    if (store.byEmail(email)) return json(res, 409, { error: 'email_taken' });
    const user = { id: randomUUID(), email, name, passwordHash: await argon2.hash(password, { type: argon2.argon2id, memoryCost: 65536, timeCost: 3, parallelism: 1 }), createdAt: Date.now(), passkeys: 0 };
    await store.insert(user); return establish(req, res, user.id);
  }
  if (route === 'login') {
    const key = `login:${ip}:${email}`; const wait = guard.check(key);
    if (wait) { res.setHeader('retry-after', Math.ceil(wait / 1000)); return json(res, 429, { error: 'locked' }); }
    const user = store.byEmail(email); const password = typeof body.password === 'string' ? body.password : '';
    const ok = !!user && password.length <= 256 && await argon2.verify(user.passwordHash, password).catch(() => false);
    if (!ok) { guard.fail(key); return json(res, 401, { error: 'invalid_credentials' }); }
    guard.success(key); return establish(req, res, user.id);
  }
  if (route === 'logout') { const token = cookie(req, sessionCookieName()); if (token) sessions.delete(tokenHash(token)); clearSession(res); return json(res, 200, { ok: true }); }
  return json(res, 404, { error: 'not_found' });
}

function establish(req: IncomingMessage, res: ServerResponse, userId: string) {
  const token = randomToken(); const csrf = randomToken(24); const expiresAt = Date.now() + 7 * 24 * 60 * 60_000;
  sessions.set(tokenHash(token), { userId, expiresAt, csrf });
  const secure = !insecureDev; const base = `Path=/; SameSite=Strict; Max-Age=604800${secure ? '; Secure' : ''}`;
  res.setHeader('set-cookie', [`${sessionCookieName()}=${token}; ${base}; HttpOnly`, `vp_csrf=${csrf}; ${base}`]);
  return json(res, 200, { user: publicUser(store.byId(userId)) });
}
function sessionOf(req: IncomingMessage) { const token = cookie(req, sessionCookieName()); if (!token) return null; const key = tokenHash(token); const session = sessions.get(key); if (!session || session.expiresAt < Date.now()) { sessions.delete(key); return null; } return session; }
function validCsrf(req: IncomingMessage) { const session = sessionOf(req); const header = req.headers['x-csrf-token']; return !!session && typeof header === 'string' && constantEqual(header, session.csrf) && constantEqual(cookie(req, 'vp_csrf') ?? '', session.csrf); }
function sessionCookieName() { return insecureDev ? 'vp_session' : '__Host-vp_session'; }
function clearSession(res: ServerResponse) { res.setHeader('set-cookie', [`${sessionCookieName()}=; Path=/; SameSite=Strict; Max-Age=0; HttpOnly${insecureDev ? '' : '; Secure'}`, `vp_csrf=; Path=/; SameSite=Strict; Max-Age=0${insecureDev ? '' : '; Secure'}`]); }
function publicUser(user: ReturnType<UserStore['byId']>) { return user ? { id: user.id, email: user.email, name: user.name, totpEnabled: !!user.totpSecret, passkeys: user.passkeys } : null; }

async function readJson(req: IncomingMessage): Promise<Record<string, unknown>> { const chunks: Buffer[] = []; let size = 0; for await (const chunk of req) { size += chunk.length; if (size > 32_768) throw new Error('body_too_large'); chunks.push(chunk); } try { return JSON.parse(Buffer.concat(chunks).toString('utf8')) as Record<string, unknown>; } catch { return {}; } }
function cookie(req: IncomingMessage, name: string) { for (const pair of (req.headers.cookie ?? '').split(';')) { const [key, ...value] = pair.trim().split('='); if (key === name) return value.join('='); } return undefined; }
function sameOrigin(req: IncomingMessage) { const origin = req.headers.origin; if (!origin) return false; try { return new URL(origin).host === req.headers.host; } catch { return false; } }
function clientIp(req: IncomingMessage) { return req.socket.remoteAddress ?? 'unknown'; }
function normalizeEmail(value: unknown) { return typeof value === 'string' ? value.trim().toLowerCase() : ''; }
function validEmail(value: string) { return value.length <= 254 && /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value); }
function cleanName(value: unknown) { return typeof value === 'string' ? value.replace(/[\u0000-\u001f\u007f]/g, '').trim().slice(0, 64) : ''; }
function boundedInt(value: string | undefined, fallback: number, min: number, max: number) { const n = Number(value); return Number.isInteger(n) && n >= min && n <= max ? n : fallback; }
function json(res: ServerResponse, status: number, value: unknown) { res.writeHead(status, { 'content-type': 'application/json; charset=utf-8' }); res.end(JSON.stringify(value)); }

async function staticFile(pathname: string, req: IncomingMessage, res: ServerResponse) {
  const requestPath = pathname === '/' ? 'index.html' : pathname.slice(1); const clean = normalize(requestPath).replace(/^(\.\.(\/|\\|$))+/, '');
  let file = resolve(webRoot, clean); const inside = relative(webRoot, file); if (inside.startsWith('..') || isAbsolute(inside)) return json(res, 400, { error: 'path' });
  try { const info = await stat(file); if (!info.isFile()) throw new Error('not file'); } catch { if (!extname(clean)) file = join(webRoot, 'index.html'); else return json(res, 404, { error: 'not_found' }); }
  const types: Record<string,string> = { '.html':'text/html; charset=utf-8','.js':'text/javascript; charset=utf-8','.css':'text/css; charset=utf-8','.svg':'image/svg+xml','.webmanifest':'application/manifest+json' };
  const assetPrefix = `assets${sep}`;
  res.statusCode = 200; res.setHeader('content-type', types[extname(file)] ?? 'application/octet-stream'); res.setHeader('cache-control', relative(webRoot, file).startsWith(assetPrefix) ? 'public, max-age=31536000, immutable' : 'no-cache'); if (req.method === 'HEAD') return res.end(); createReadStream(file).pipe(res);
}
