import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getProfiles } from '@/api';
import { displayAppError } from '@/i18n';
import type { AppErrorVm, ProfileVm } from '@/types';

export interface WorkflowProfileCatalogError {
  code: string;
  message: string;
}

export type WorkflowProfileCatalogState =
  | { status: 'loading'; profiles: ProfileVm[] }
  | { status: 'ready'; profiles: ProfileVm[] }
  | { status: 'error'; profiles: ProfileVm[]; error: WorkflowProfileCatalogError; retry: () => void };

export function readyWorkflowProfileCatalog(profiles: ProfileVm[]): WorkflowProfileCatalogState {
  return { status: 'ready', profiles };
}

export function useWorkflowProfileCatalog(enabled = true): WorkflowProfileCatalogState {
  const { t } = useTranslation();
  const translateRef = useRef(t);
  const requestGenerationRef = useRef(0);
  const [requestRevision, setRequestRevision] = useState(0);
  const retry = useCallback(() => setRequestRevision((revision) => revision + 1), []);
  const [state, setState] = useState<WorkflowProfileCatalogState>({ status: 'loading', profiles: [] });
  translateRef.current = t;

  useEffect(() => {
    if (!enabled) return undefined;
    const requestGeneration = ++requestGenerationRef.current;
    setState((current) => ({ status: 'loading', profiles: current.profiles }));
    void getProfiles()
      .then((result) => {
        if (requestGeneration !== requestGenerationRef.current) return;
        setState({ status: 'ready', profiles: result.profiles });
      })
      .catch((cause: unknown) => {
        if (requestGeneration !== requestGenerationRef.current) return;
        setState((current) => ({
          status: 'error',
          profiles: current.profiles,
          error: profileCatalogError(cause, displayAppError(translateRef.current, cause)),
          retry,
        }));
      });
    return () => {
      if (requestGeneration === requestGenerationRef.current) requestGenerationRef.current += 1;
    };
  }, [enabled, requestRevision, retry]);

  return state;
}

function profileCatalogError(cause: unknown, message: string): WorkflowProfileCatalogError {
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
