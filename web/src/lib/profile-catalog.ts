import { useEffect, useMemo, useRef, useSyncExternalStore } from 'react';
import { useTranslation } from 'react-i18next';
import { getProfiles } from '@/api';
import { displayAppError } from '@/i18n';
import type { AppErrorVm, ProfileVm } from '@/types';

export interface ProfileCatalogError {
  code: string;
  message: string;
}

export type ProfileCatalogState =
  | { status: 'loading'; profiles: ProfileVm[] }
  | { status: 'ready'; profiles: ProfileVm[] }
  | { status: 'error'; profiles: ProfileVm[]; error: ProfileCatalogError; retry: () => void };

type StoreSnapshot =
  | { status: 'loading' | 'ready'; profiles: ProfileVm[] }
  | { status: 'error'; profiles: ProfileVm[]; cause: unknown };

/**
 * The app-wide profile (role) list. Every consumer reads this one snapshot, so a
 * profile created, edited or deleted anywhere reaches every picker and menu.
 */
let snapshot: StoreSnapshot = { status: 'loading', profiles: [] };
const listeners = new Set<() => void>();
let inFlight: Promise<ProfileVm[]> | null = null;
let trailing: Promise<ProfileVm[]> | null = null;

function publish(next: StoreSnapshot) {
  snapshot = next;
  for (const listener of listeners) listener();
}

async function read(): Promise<ProfileVm[]> {
  try {
    const { profiles } = await getProfiles();
    // A revalidation queued behind this read publishes the newer list instead.
    if (!trailing) {
      const unchanged = snapshot.status === 'ready' && sameProfiles(snapshot.profiles, profiles);
      if (!unchanged) publish({ status: 'ready', profiles });
    }
    return profiles;
  } catch (cause) {
    // A failed background revalidation keeps the last loaded list.
    if (snapshot.status !== 'ready') publish({ status: 'error', profiles: snapshot.profiles, cause });
    throw cause;
  } finally {
    inFlight = null;
  }
}

/** Joins the read in flight, or starts one. Used when a consumer mounts. */
function ensureProfileCatalog(): Promise<ProfileVm[]> {
  return inFlight ?? (inFlight = read());
}

/**
 * Re-reads the list after its contents may have changed. A read already in
 * flight may predate the change, so callers during it share one trailing read.
 */
export function revalidateProfileCatalog(): Promise<ProfileVm[]> {
  if (!inFlight) return ensureProfileCatalog();
  trailing ??= inFlight.catch(() => undefined).then(() => {
    trailing = null;
    return ensureProfileCatalog();
  });
  return trailing;
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

function getSnapshot() {
  return snapshot;
}

export function readyProfileCatalog(profiles: ProfileVm[]): ProfileCatalogState {
  return { status: 'ready', profiles };
}

/** Reads the shared catalog; an enabled consumer refreshes it when it mounts. */
export function useProfileCatalog(enabled = true): ProfileCatalogState {
  const { t } = useTranslation();
  const translateRef = useRef(t);
  translateRef.current = t;
  const current = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);

  useEffect(() => {
    if (enabled) void ensureProfileCatalog().catch(() => undefined);
  }, [enabled]);

  return useMemo(() => {
    if (current.status !== 'error') return current;
    return {
      status: 'error',
      profiles: current.profiles,
      error: profileCatalogError(current.cause, displayAppError(translateRef.current, current.cause)),
      retry: () => { void revalidateProfileCatalog().catch(() => undefined); },
    };
  }, [current]);
}

function sameProfiles(left: readonly ProfileVm[], right: readonly ProfileVm[]) {
  if (left.length !== right.length) return false;
  return left.every((profile, index) => {
    const other = right[index];
    const keys = Object.keys(profile) as (keyof ProfileVm)[];
    return keys.length === Object.keys(other).length && keys.every((key) => profile[key] === other[key]);
  });
}

function profileCatalogError(cause: unknown, message: string): ProfileCatalogError {
  return {
    code: isAppError(cause) ? cause.code : 'app.unexpected',
    message,
  };
}

function isAppError(value: unknown): value is AppErrorVm {
  return Boolean(value)
    && typeof value === 'object'
    && typeof (value as Partial<AppErrorVm>).code === 'string'
    && Boolean((value as Partial<AppErrorVm>).params)
    && typeof (value as Partial<AppErrorVm>).params === 'object';
}

/** Restores the never-loaded catalog between tests. */
export function resetProfileCatalogForTests() {
  snapshot = { status: 'loading', profiles: [] };
  inFlight = null;
  trailing = null;
}
