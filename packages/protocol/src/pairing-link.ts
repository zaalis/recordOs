/**
 * Pairing links carried by QR codes. All secrets live in the URL fragment, which browsers never
 * send to a server, so they cannot leak into access logs or proxies.
 *
 *   https://<pc>.local:47800/#/pair?a=<agentId>&fp=<agentFingerprint>&t=<oneTimeToken>&n=<pcName>
 */

export interface PairingLink {
  agentId: string;
  fingerprint: string;
  token: string;
  name: string;
}

const B64U = /^[A-Za-z0-9_-]+$/;

export function buildPairingFragment(link: PairingLink): string {
  const params = new URLSearchParams({ a: link.agentId, fp: link.fingerprint, t: link.token, n: link.name });
  return `#/pair?${params.toString()}`;
}

export function parsePairingFragment(hash: string): PairingLink | null {
  const match = /^#\/pair\?(.*)$/.exec(hash);
  if (!match) return null;
  const params = new URLSearchParams(match[1]);
  const agentId = params.get('a') ?? '';
  const fingerprint = params.get('fp') ?? '';
  const token = params.get('t') ?? '';
  const name = (params.get('n') ?? '').slice(0, 64);
  if (agentId.length !== 22 || !B64U.test(agentId)) return null;
  if (fingerprint.length !== 43 || !B64U.test(fingerprint)) return null;
  if (token.length < 22 || token.length > 64 || !B64U.test(token)) return null;
  return { agentId, fingerprint, token, name };
}
