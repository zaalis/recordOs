/**
 * Optional Record OS account (served by the account server). Sessions use Secure, HttpOnly,
 * SameSite=Strict cookies; state-changing requests carry a double-submit CSRF token.
 * Passkeys are preferred; passwords are hashed with Argon2id on the server.
 */

export interface AccountConfig {
  enabled: boolean;
  registration: boolean;
  passkeys: boolean;
}

export interface AccountUser {
  id: string;
  email: string;
  name: string;
  totpEnabled: boolean;
  passkeys: number;
}

function csrfToken(): string {
  const match = document.cookie.match(/(?:^|;\s*)vp_csrf=([A-Za-z0-9_-]+)/);
  return match?.[1] ?? '';
}

async function call<T>(path: string, body?: unknown): Promise<T> {
  const response = await fetch(`/api/account/${path}`, {
    method: body === undefined ? 'GET' : 'POST',
    credentials: 'same-origin',
    headers: body === undefined ? {} : { 'content-type': 'application/json', 'x-csrf-token': csrfToken() },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data = (await response.json().catch(() => ({}))) as T & { error?: string };
  if (!response.ok) throw new AccountError(data.error ?? 'request_failed', response.status);
  return data;
}

export class AccountError extends Error {
  readonly code: string;
  readonly status: number;
  constructor(code: string, status: number) {
    super(code);
    this.code = code;
    this.status = status;
  }
}

export async function accountConfig(): Promise<AccountConfig | null> {
  try {
    return await call<AccountConfig>('config');
  } catch {
    return null;
  }
}

export const account = {
  me: () => call<{ user: AccountUser | null }>('me'),
  register: (email: string, password: string, name: string) => call<{ user: AccountUser }>('register', { email, password, name }),
  login: (email: string, password: string, totp?: string) => call<{ user?: AccountUser; totpRequired?: boolean }>('login', { email, password, ...(totp ? { totp } : {}) }),
  logout: () => call<{ ok: true }>('logout', {}),
};

const toB64u = (buffer: ArrayBuffer) => btoa(String.fromCharCode(...new Uint8Array(buffer))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
const fromB64u = (text: string) => Uint8Array.from(atob(text.replace(/-/g, '+').replace(/_/g, '/') + '==='.slice((text.length + 3) % 4)), (c) => c.charCodeAt(0));

interface JsonCreationOptions {
  challenge: string;
  rp: { name: string; id?: string };
  user: { id: string; name: string; displayName: string };
  pubKeyCredParams: { type: 'public-key'; alg: number }[];
  timeout?: number;
  excludeCredentials?: { id: string; type: 'public-key' }[];
  authenticatorSelection?: AuthenticatorSelectionCriteria;
  attestation?: AttestationConveyancePreference;
}

interface JsonRequestOptions {
  challenge: string;
  timeout?: number;
  rpId?: string;
  allowCredentials?: { id: string; type: 'public-key' }[];
  userVerification?: UserVerificationRequirement;
}

export async function registerPasskey(): Promise<void> {
  const options = await call<JsonCreationOptions>('passkey/register/options', {});
  const credential = (await navigator.credentials.create({
    publicKey: {
      ...options,
      challenge: fromB64u(options.challenge),
      user: { ...options.user, id: fromB64u(options.user.id) },
      excludeCredentials: options.excludeCredentials?.map((c) => ({ ...c, id: fromB64u(c.id) })),
    },
  })) as PublicKeyCredential | null;
  if (!credential) throw new AccountError('cancelled', 0);
  const response = credential.response as AuthenticatorAttestationResponse;
  await call('passkey/register/verify', {
    id: credential.id,
    rawId: toB64u(credential.rawId),
    type: credential.type,
    response: {
      clientDataJSON: toB64u(response.clientDataJSON),
      attestationObject: toB64u(response.attestationObject),
      transports: response.getTransports?.() ?? [],
    },
    clientExtensionResults: credential.getClientExtensionResults(),
  });
}

export async function signInWithPasskey(): Promise<AccountUser> {
  const options = await call<JsonRequestOptions>('passkey/login/options', {});
  const credential = (await navigator.credentials.get({
    publicKey: {
      ...options,
      challenge: fromB64u(options.challenge),
      allowCredentials: options.allowCredentials?.map((c) => ({ ...c, id: fromB64u(c.id) })),
    },
  })) as PublicKeyCredential | null;
  if (!credential) throw new AccountError('cancelled', 0);
  const response = credential.response as AuthenticatorAssertionResponse;
  const result = await call<{ user: AccountUser }>('passkey/login/verify', {
    id: credential.id,
    rawId: toB64u(credential.rawId),
    type: credential.type,
    response: {
      clientDataJSON: toB64u(response.clientDataJSON),
      authenticatorData: toB64u(response.authenticatorData),
      signature: toB64u(response.signature),
      userHandle: response.userHandle ? toB64u(response.userHandle) : null,
    },
    clientExtensionResults: credential.getClientExtensionResults(),
  });
  return result.user;
}

export function accountErrorText(code: string): string {
  switch (code) {
    case 'invalid_credentials':
      return 'Email or password is incorrect.';
    case 'email_taken':
      return 'An account already exists with this email.';
    case 'weak_password':
      return 'Use at least 12 characters for your password.';
    case 'rate_limited':
    case 'locked':
      return 'Too many attempts. Please wait a few minutes.';
    case 'invalid_totp':
      return 'The verification code is not valid.';
    case 'cancelled':
      return 'Passkey request was cancelled.';
    default:
      return 'Something went wrong. Please try again.';
  }
}
