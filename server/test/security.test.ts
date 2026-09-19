import assert from 'node:assert/strict';
import test from 'node:test';
import { RateGuard, constantEqual, randomToken, tokenHash } from '../src/security.ts';

test('tokens are random, URL-safe and stored as hashes', () => { const token = randomToken(); assert.match(token, /^[A-Za-z0-9_-]+$/); assert.notEqual(tokenHash(token), token); assert.equal(constantEqual(token, token), true); assert.equal(constantEqual(token, `${token}x`), false); });
test('rate guard locks and recovers', () => { const guard = new RateGuard(3, 1000, 5000); assert.equal(guard.fail('a', 0), 0); assert.equal(guard.fail('a', 1), 0); assert.equal(guard.fail('a', 2), 5000); assert.equal(guard.check('a', 3), 4999); guard.success('a'); assert.equal(guard.check('a', 4), 0); });
