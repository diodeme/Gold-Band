import { useSyncExternalStore } from 'react';
import * as api from '@/api';
import type { RemoteImageTrustVm } from '@/types';

// Projection of the backend's trusted image hosts. The backend state is authoritative: the
// bootstrap seeds it and every write converges to the VM the command returns. Each image only
// subscribes to its own host's boolean, so trusting a host re-renders only those images.
let trust: RemoteImageTrustVm = { schemaVersion: 1, trustedHosts: [] };
let trustedHosts = new Set<string>();
let latestWrite = 0;
let settledWrite = 0;
const listeners = new Set<() => void>();

function publish(next: RemoteImageTrustVm) {
  trust = next;
  trustedHosts = new Set(next.trustedHosts);
  listeners.forEach((listener) => listener());
}

/** Seeds from a freshly loaded bootstrap; ignored while a write is in flight so it cannot roll it back. */
export function seedRemoteImageTrust(next: RemoteImageTrustVm) {
  if (settledWrite !== latestWrite) return;
  publish(next);
}

async function write(run: () => Promise<RemoteImageTrustVm>) {
  const id = ++latestWrite;
  try {
    const next = await run();
    // A later write's response already reflects this one; never let an older response win.
    if (id === latestWrite) publish(next);
    return next;
  } finally {
    if (id === latestWrite) settledWrite = id;
  }
}

export function trustRemoteImageHosts(hosts: readonly string[]) {
  const missing = [...new Set(hosts)].filter((host) => !trustedHosts.has(host));
  if (missing.length === 0) return Promise.resolve(trust);
  return write(() => api.trustRemoteImageHosts(missing));
}

export function revokeRemoteImageHost(host: string) {
  return write(() => api.revokeRemoteImageHost(host));
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function snapshot() {
  return trust;
}

export function useRemoteImageTrust() {
  return useSyncExternalStore(subscribe, snapshot, snapshot);
}

export function useRemoteImageHostTrusted(host: string | null) {
  const read = () => (host ? trustedHosts.has(host) : false);
  return useSyncExternalStore(subscribe, read, read);
}

/** Non-reactive read for code outside React (e.g. CodeMirror decorations). */
export function isRemoteImageHostTrusted(host: string) {
  return trustedHosts.has(host);
}

export function resetRemoteImageTrustForTests() {
  latestWrite = 0;
  settledWrite = 0;
  publish({ schemaVersion: 1, trustedHosts: [] });
}
