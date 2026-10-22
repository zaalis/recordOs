import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';

export interface UserRecord { id: string; email: string; name: string; passwordHash: string; createdAt: number; totpSecret?: string; passkeys: number; }
interface StoreFile { schema: 1; users: UserRecord[]; }
export class DuplicateUserError extends Error { constructor() { super('email_taken'); } }

export class UserStore {
  private data: StoreFile = { schema: 1, users: [] };
  private readonly path: string;
  private writes: Promise<void> = Promise.resolve();
  constructor(path: string) { this.path = path; }
  async load() {
    let contents: string;
    try { contents = await readFile(this.path, 'utf8'); }
    catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return; throw error; }
    const parsed = JSON.parse(contents) as StoreFile;
    if (parsed?.schema !== 1 || !Array.isArray(parsed.users)) throw new Error('invalid_account_store');
    const emails = new Set<string>(); const ids = new Set<string>();
    for (const user of parsed.users) {
      if (!user || typeof user.id !== 'string' || !user.id || typeof user.email !== 'string' || !user.email ||
          typeof user.name !== 'string' || typeof user.passwordHash !== 'string' || !user.passwordHash ||
          !Number.isFinite(user.createdAt) || !Number.isInteger(user.passkeys) || user.passkeys < 0 ||
          (user.totpSecret !== undefined && typeof user.totpSecret !== 'string')) throw new Error('invalid_account_store');
      user.email = user.email.trim().toLowerCase();
      if (!user.email || emails.has(user.email) || ids.has(user.id)) throw new Error('duplicate_account_store');
      emails.add(user.email); ids.add(user.id);
    }
    this.data = parsed;
  }
  byEmail(email: string) { return this.data.users.find((user) => user.email === email); }
  byId(id: string) { return this.data.users.find((user) => user.id === id); }
  async insert(user: UserRecord) {
    const candidate = { ...user, email: user.email.trim().toLowerCase() };
    const write = this.writes.then(async () => {
      if (this.byEmail(candidate.email) || this.byId(candidate.id)) throw new DuplicateUserError();
      const next: StoreFile = { schema: 1, users: [...this.data.users, candidate] };
      await this.save(next);
      this.data = next;
    });
    this.writes = write.catch(() => {});
    return write;
  }
  private async save(data: StoreFile) { await mkdir(dirname(this.path), { recursive: true }); const temp = `${this.path}.${process.pid}.tmp`; await writeFile(temp, JSON.stringify(data, null, 2), { encoding: 'utf8', mode: 0o600 }); await rename(temp, this.path); }
}
