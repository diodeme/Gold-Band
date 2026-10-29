import { useEffect, useSyncExternalStore } from 'react';
import * as api from '@/api';
import type { PromptQueueSettingsVm } from '@/types';

// Projection of the backend's prompt-queue preferences. The backend is authoritative: the first
// subscriber loads it once, and every write converges to the VM the command returns. Until the
// load settles the default (ask before interrupting) applies, so a slow load never skips the ask.
const DEFAULT_SETTINGS: PromptQueueSettingsVm = { confirmInterrupt: true };

let settings: PromptQueueSettingsVm = DEFAULT_SETTINGS;
let loadRequested = false;
let latestWrite = 0;
const listeners = new Set<() => void>();

function publish(next: PromptQueueSettingsVm) {
  settings = next;
  listeners.forEach((listener) => listener());
}

function ensureLoaded() {
  if (loadRequested) return;
  loadRequested = true;
  const writeAtLoad = latestWrite;
  void api.getPromptQueueSettings().then((next) => {
    // A write issued while loading already returned a newer value.
    if (writeAtLoad === latestWrite) publish(next);
  }).catch(() => {
    loadRequested = false;
  });
}

export async function savePromptQueueSettings(next: PromptQueueSettingsVm) {
  const id = ++latestWrite;
  const saved = await api.savePromptQueueSettings(next);
  // Never let an older response roll back a later write.
  if (id === latestWrite) publish(saved);
  return saved;
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function snapshot() {
  return settings;
}

export function usePromptQueueSettings() {
  useEffect(ensureLoaded, []);
  return useSyncExternalStore(subscribe, snapshot, snapshot);
}

export function resetPromptQueueSettingsForTests() {
  loadRequested = false;
  latestWrite = 0;
  publish(DEFAULT_SETTINGS);
}
