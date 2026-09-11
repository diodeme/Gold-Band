import { sha256 as createSha256 } from '@noble/hashes/sha2.js';

const toHex = (bytes: Uint8Array) => Array.from(bytes, (value) => value.toString(16).padStart(2, '0')).join('');

/**
 * Hashes bytes the same way everywhere.
 *
 * `crypto.subtle` only exists in a secure context, so a plain-HTTP deployment would otherwise
 * lose every asset integrity check. The fallback is the implementation the export scripts
 * already use, which keeps the recorded hashes verifiable on any origin.
 */
export async function sha256Hex(bytes: ArrayBuffer): Promise<string> {
  const subtle = globalThis.crypto?.subtle;
  if (subtle) return toHex(new Uint8Array(await subtle.digest('SHA-256', bytes)));
  return toHex(createSha256(new Uint8Array(bytes)));
}
