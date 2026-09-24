import type { ExecutionPlanSaveTarget } from '@/types';

export const EXECUTION_PLAN_SAVE_TARGET_STORAGE_KEY = 'gold-band.execution-plan.save-target';
const SCHEMA_VERSION = 1;

interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

interface SaveTargetPreference {
  schemaVersion: typeof SCHEMA_VERSION;
  target: ExecutionPlanSaveTarget;
}

const DEFAULT_TARGET: ExecutionPlanSaveTarget = 'next';

function browserStorage(): StorageLike | null {
  if (typeof window === 'undefined') return null;
  return window.localStorage;
}

export function readExecutionPlanSaveTarget(storage: StorageLike | null = browserStorage()): ExecutionPlanSaveTarget {
  if (!storage) return DEFAULT_TARGET;
  try {
    const parsed = JSON.parse(storage.getItem(EXECUTION_PLAN_SAVE_TARGET_STORAGE_KEY) ?? 'null') as SaveTargetPreference | null;
    if (!parsed || parsed.schemaVersion !== SCHEMA_VERSION) return DEFAULT_TARGET;
    if (parsed.target === 'current' || parsed.target === 'next' || parsed.target === 'current-and-next') return parsed.target;
    return DEFAULT_TARGET;
  } catch {
    return DEFAULT_TARGET;
  }
}

export function writeExecutionPlanSaveTarget(
  target: ExecutionPlanSaveTarget,
  storage: StorageLike | null = browserStorage(),
) {
  if (!storage) return;
  const preference: SaveTargetPreference = { schemaVersion: SCHEMA_VERSION, target };
  storage.setItem(EXECUTION_PLAN_SAVE_TARGET_STORAGE_KEY, JSON.stringify(preference));
}
