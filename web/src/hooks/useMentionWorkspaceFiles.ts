import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { listWorkspaceDirectory, searchWorkspaceFiles } from '@/api';
import type { MentionFilesRequest, MentionFilesState, MentionMenuLabels } from '@/lib/slash-command';
import type { WorkspaceRootRef } from '@/types';

export const MENTION_FILE_SEARCH_LIMIT = 20;
export const MENTION_FILE_SEARCH_DEBOUNCE_MS = 150;

const IDLE: MentionFilesState = { status: 'ready', entries: [] };

/**
 * Workspace entries for the `@` menu: one directory level while browsing, or a
 * bounded search. Only the latest request may publish; a refining search keeps
 * the previous results until its own arrive. `root` is the file root prompt
 * references resolve against; `null` (e.g. an unavailable worktree) disables it.
 */
export function useMentionWorkspaceFiles(
  root: WorkspaceRootRef | null | undefined,
  request: MentionFilesRequest | null,
): MentionFilesState {
  const projectId = root?.projectId ?? null;
  const workspacePath = root?.workspacePath ?? null;
  const [state, setState] = useState<MentionFilesState>(IDLE);
  const generationRef = useRef(0);
  const requestKey = request
    ? `${request.kind}\0${request.kind === 'search' ? request.query : request.path}`
    : null;
  const previousKindRef = useRef<MentionFilesRequest['kind'] | null>(null);

  useEffect(() => {
    const generation = ++generationRef.current;
    const previousKind = previousKindRef.current;
    previousKindRef.current = request?.kind ?? null;
    if (!projectId || !request) {
      setState(IDLE);
      return;
    }
    setState((current) => ({
      status: 'loading',
      entries: request.kind === 'search' && previousKind === 'search' ? current.entries : [],
    }));
    const publish = (next: MentionFilesState) => {
      if (generationRef.current === generation) setState(next);
    };
    const load = () => {
      const pending = request.kind === 'search'
        ? searchWorkspaceFiles({ projectId, workspacePath }, request.query, `mention-${generation}`, MENTION_FILE_SEARCH_LIMIT)
          .then((result) => result.entries)
        : listWorkspaceDirectory({ projectId, workspacePath }, request.path);
      pending
        .then((entries) => publish({ status: 'ready', entries }))
        .catch(() => publish({ status: 'error', entries: [] }));
    };
    if (request.kind === 'directory') {
      load();
      return;
    }
    const timer = setTimeout(load, MENTION_FILE_SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
    // `requestKey` is the request's identity; the object itself changes every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId, workspacePath, requestKey]);

  return state;
}

export function useMentionMenuLabels(): MentionMenuLabels {
  const { t } = useTranslation();
  return useMemo(() => ({
    files: t('acp.mentionFiles'),
    roles: t('acp.mentionRoles'),
    workspaceRoot: t('acp.mentionWorkspaceRoot'),
  }), [t]);
}
