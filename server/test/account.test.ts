import assert from 'node:assert/strict';
import test from 'node:test';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

test('HTTP accounts isolate sessions, reject concurrent duplicate emails, enforce CSRF and throttle registration', { timeout: 20_000 }, async () => {
  const dir = await mkdtemp(join(tmpdir(), 'vp-account-http-'));
  const reservation = createServer(); reservation.listen(0, '127.0.0.1'); await once(reservation, 'listening');
  const address = reservation.address(); assert.ok(address && typeof address !== 'string');
  const port = address.port; await new Promise<void>(r => reservation.close(() => r()));
  const origin = `http://127.0.0.1:${port}`;
  const path = join(dir, 'users.json');
  const child = spawn(process.execPath, ['--experimental-strip-types', resolve(import.meta.dirname, '../src/index.ts')], {
    env: { ...process.env, VP_ACCOUNT_PORT: String(port), VP_ACCOUNT_DATA: path, VP_ALLOW_REGISTRATION: '1', VP_INSECURE_DEV: '1' }, stdio: ['ignore', 'pipe', 'pipe'],
  });
  let output = ''; child.stdout.on('data', chunk => { output += chunk; }); child.stderr.on('data', chunk => { output += chunk; });
  const post = (route: string, body: object, headers: Record<string, string> = {}) => fetch(`${origin}/api/account/${route}`, { method: 'POST', headers: { origin, 'content-type': 'application/json', ...headers }, body: JSON.stringify(body) });
  try {
    for (let i = 0; i < 100 && !output.includes('listening on'); i++) { if (child.exitCode !== null) throw new Error(output); await new Promise(r => setTimeout(r, 25)); }
    assert.match(output, /listening on/);
    const credentials = { email: 'Alice@Example.com', name: 'Alice', password: 'Strong-test-password-123' };
    assert.equal((await post('register', credentials, { origin: 'http://foreign.example' })).status, 403);
    const attempts = await Promise.all([post('register', credentials), post('register', { ...credentials, email: ' alice@example.com ' })]);
    assert.deepEqual(attempts.map(r => r.status).sort(), [200, 409]);
    const registered = attempts.find(r => r.status === 200)!;
    const cookies = registered.headers.getSetCookie(); assert.ok(cookies.some(c => c.includes('HttpOnly')));
    const cookie = cookies.map(c => c.split(';')[0]).join('; ');
    const csrf = cookies.find(c => c.startsWith('vp_csrf='))!.split(';')[0]!.slice('vp_csrf='.length);
    assert.deepEqual(await (await fetch(`${origin}/api/account/me`)).json(), { user: null });
    const me = await (await fetch(`${origin}/api/account/me`, { headers: { cookie } })).json() as { user: { email: string } };
    assert.equal(me.user.email, 'alice@example.com');
    assert.equal((await post('logout', {}, { cookie })).status, 403);
    assert.equal((await post('logout', {}, { cookie, 'x-csrf-token': csrf })).status, 200);
    assert.deepEqual(await (await fetch(`${origin}/api/account/me`, { headers: { cookie } })).json(), { user: null });
    assert.equal((await post('login', { ...credentials, password: 'wrong' })).status, 401);
    assert.equal((await post('login', credentials)).status, 200);
    for (let i = 0; i < 3; i++) assert.equal((await post('register', credentials)).status, 409);
    assert.equal((await post('register', credentials)).status, 429);
    const saved = JSON.parse(await readFile(path, 'utf8')) as { users: Array<{ passwordHash: string }> };
    assert.equal(saved.users.length, 1); assert.match(saved.users[0]!.passwordHash, /^\$argon2id\$/);
  } finally {
    if (child.exitCode === null) { const closed = once(child, 'exit'); child.kill(); await closed; }
    await rm(dir, { recursive: true, force: true });
  }
});
