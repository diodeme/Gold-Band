import type { TFunction } from 'i18next';
import { displayAppError } from '@/i18n';
import type { AppErrorVm, UpdateStatusVm } from '@/types';

export type UpdateActionState = 'available' | 'downloading' | 'ready' | 'failed';

/** The update action is shown whenever the remote release is newer than the running client. */
export function updateActionState(status: UpdateStatusVm): UpdateActionState | null {
  if (!status.update) return null;
  if (status.status === 'downloading') return 'downloading';
  if (status.status === 'ready') return 'ready';
  if (status.error) return 'failed';
  return 'available';
}

const UPDATE_PHASES = ['check', 'download', 'install'] as const;

export function describeUpdateError(t: TFunction, error: AppErrorVm) {
  const reason = displayAppError(t, error);
  const phase = UPDATE_PHASES.find((candidate) => candidate === error.params.phase);
  return phase ? t(`settings.updater.failure.${phase}`, { reason }) : reason;
}

export function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
