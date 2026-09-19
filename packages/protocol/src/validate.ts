/**
 * Tiny strict schema validator. Unknown keys are rejected, every string is length-bounded,
 * every number is range-checked. Used on every message crossing a trust boundary.
 */

export class ValidationError extends Error {
  readonly path: string;
  constructor(path: string, reason: string) {
    super(`${path || '<root>'}: ${reason}`);
    this.path = path;
  }
}

export type Schema<T> = (value: unknown, path: string) => T;
export type Infer<S> = S extends Schema<infer T> ? T : never;

export const v = {
  string(opts: { min?: number; max: number; pattern?: RegExp }): Schema<string> {
    return (value, path) => {
      if (typeof value !== 'string') throw new ValidationError(path, 'expected string');
      if (value.length < (opts.min ?? 0) || value.length > opts.max) throw new ValidationError(path, 'length out of range');
      if (opts.pattern && !opts.pattern.test(value)) throw new ValidationError(path, 'invalid format');
      return value;
    };
  },
  b64u(minLen: number, maxLen = minLen): Schema<string> {
    const minChars = Math.ceil((minLen * 4) / 3);
    const maxChars = Math.ceil((maxLen * 4) / 3);
    return (value, path) => {
      if (typeof value !== 'string' || !/^[A-Za-z0-9_-]*$/.test(value)) throw new ValidationError(path, 'expected base64url');
      if (value.length < minChars || value.length > maxChars) throw new ValidationError(path, 'length out of range');
      return value;
    };
  },
  number(opts: { min: number; max: number; integer?: boolean }): Schema<number> {
    return (value, path) => {
      if (typeof value !== 'number' || !Number.isFinite(value)) throw new ValidationError(path, 'expected number');
      if (opts.integer && !Number.isInteger(value)) throw new ValidationError(path, 'expected integer');
      if (value < opts.min || value > opts.max) throw new ValidationError(path, 'out of range');
      return value;
    };
  },
  boolean(): Schema<boolean> {
    return (value, path) => {
      if (typeof value !== 'boolean') throw new ValidationError(path, 'expected boolean');
      return value;
    };
  },
  literal<const T extends string | number | boolean>(expected: T): Schema<T> {
    return (value, path) => {
      if (value !== expected) throw new ValidationError(path, `expected ${String(expected)}`);
      return expected;
    };
  },
  oneOf<const T extends readonly (string | number)[]>(options: T): Schema<T[number]> {
    return (value, path) => {
      if (!options.includes(value as string | number)) throw new ValidationError(path, 'unexpected value');
      return value as T[number];
    };
  },
  optional<T>(schema: Schema<T>): Schema<T | undefined> {
    return (value, path) => (value === undefined ? undefined : schema(value, path));
  },
  array<T>(schema: Schema<T>, maxItems: number): Schema<T[]> {
    return (value, path) => {
      if (!Array.isArray(value)) throw new ValidationError(path, 'expected array');
      if (value.length > maxItems) throw new ValidationError(path, 'too many items');
      return value.map((item, i) => schema(item, `${path}[${i}]`));
    };
  },
  object<const S extends Record<string, Schema<unknown>>>(shape: S): Schema<{ [K in keyof S]: Infer<S[K]> }> {
    return (value, path) => {
      if (typeof value !== 'object' || value === null || Array.isArray(value)) throw new ValidationError(path, 'expected object');
      const record = value as Record<string, unknown>;
      for (const key of Object.keys(record)) {
        if (!(key in shape)) throw new ValidationError(`${path}.${key}`, 'unexpected key');
      }
      const out: Record<string, unknown> = {};
      for (const [key, schema] of Object.entries(shape)) {
        const parsed = schema(record[key], path ? `${path}.${key}` : key);
        if (parsed !== undefined) out[key] = parsed;
      }
      return out as { [K in keyof S]: Infer<S[K]> };
    };
  },
  union<T>(...schemas: Schema<T>[]): Schema<T> {
    return (value, path) => {
      let last: unknown;
      for (const schema of schemas) {
        try {
          return schema(value, path);
        } catch (error) {
          last = error;
        }
      }
      throw last instanceof ValidationError ? last : new ValidationError(path, 'no matching variant');
    };
  },
};

export function parse<T>(schema: Schema<T>, value: unknown): T {
  return schema(value, '');
}
