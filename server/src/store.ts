import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';

export interface UserRecord { id: string; email: string; name: string; passwordHash: string; createdAt: number; totpSecret?: string; passkeys: number; }
interface StoreFile { schema: 1; users: UserRecord[]; }

export class UserStore {
  private data: StoreFile = { schema: 1, users: [] };
  private readonly path: string;
  constructor(path: string) { this.path = path; }
  async load() { try { const parsed = JSON.parse(await readFile(this.path, 'utf8')) as StoreFile; if (parsed.schema === 1 && Array.isArray(parsed.users)) this.data = parsed; } catch { /* first start */ } }
  byEmail(email: string) { return this.data.users.find((user) => user.email === email); }
  byId(id: string) { return this.data.users.find((user) => user.id === id); }
  async insert(user: UserRecord) { this.data.users.push(user); await this.save(); }
  private async save() { await mkdir(dirname(this.path), { recursive: true }); const temp = `${this.path}.${process.pid}.tmp`; await writeFile(temp, JSON.stringify(this.data, null, 2), { encoding: 'utf8', mode: 0o600 }); await rename(temp, this.path); }
}
