import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtemp, readFile, rm, writeFile, mkdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { DuplicateUserError, UserStore, type UserRecord } from '../src/store.ts';

const user = (id: string, email = `${id}@example.com`): UserRecord => ({ id, email, name: id, passwordHash: 'test-hash', createdAt: Date.now(), passkeys: 0 });

test('concurrent inserts enforce normalized email uniqueness and survive reload', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'vp-store-'));
  try {
    const path = join(dir, 'users.json'); const store = new UserStore(path); await store.load();
    const results = await Promise.allSettled([store.insert(user('a', 'Alice@Example.com')), store.insert(user('b', ' alice@example.com ')), store.insert(user('c'))]);
    assert.equal(results.filter(r => r.status === 'fulfilled').length, 2);
    const rejected = results.find(r => r.status === 'rejected');
    assert.ok(rejected?.status === 'rejected' && rejected.reason instanceof DuplicateUserError);
    const loaded = new UserStore(path); await loaded.load();
    assert.equal(loaded.byEmail('alice@example.com')?.id, 'a'); assert.ok(loaded.byId('c')); assert.equal(loaded.byId('b'), undefined);
    await assert.rejects(loaded.insert(user('a', 'other@example.com')), DuplicateUserError);
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('invalid or duplicated account files fail closed without changing disk contents', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'vp-store-'));
  try {
    const path = join(dir, 'users.json');
    for (const contents of ['{broken', JSON.stringify({ schema: 2, users: [] }), JSON.stringify({ schema: 1, users: [user('a'), user('b', 'A@example.com')] }), JSON.stringify({ schema: 1, users: [user('a'), user('a', 'other@example.com')] })]) {
      await writeFile(path, contents); await assert.rejects(new UserStore(path).load()); assert.equal(await readFile(path, 'utf8'), contents);
    }
    await assert.rejects(new UserStore(dir).load());
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('failed persistence does not authorize a phantom account and the queue recovers', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'vp-store-'));
  try {
    const path = join(dir, 'users.json'); const store = new UserStore(path); await store.load();
    await mkdir(path); await assert.rejects(store.insert(user('a'))); assert.equal(store.byId('a'), undefined);
    await rm(path, { recursive: true }); await store.insert(user('b')); assert.ok(store.byId('b'));
  } finally { await rm(dir, { recursive: true, force: true }); }
});
