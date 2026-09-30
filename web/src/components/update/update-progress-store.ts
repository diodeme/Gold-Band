import { useSyncExternalStore } from 'react';
import type { UpdateDownloadProgressVm } from '@/types';

// Download progress ticks are high frequency; keep them out of the app bootstrap state so only
// progress consumers re-render.
let progress: UpdateDownloadProgressVm | null = null;
const listeners = new Set<() => void>();

export function setUpdateDownloadProgress(next: UpdateDownloadProgressVm | null) {
  if (progress === next) return;
  progress = next;
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function snapshot() {
  return progress;
}

export function useUpdateDownloadProgress() {
  return useSyncExternalStore(subscribe, snapshot, snapshot);
}

export function downloadPercent(value: UpdateDownloadProgressVm | null) {
  if (!value?.total) return null;
  return Math.min(100, Math.round((value.downloaded / value.total) * 100));
}
