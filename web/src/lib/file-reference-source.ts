import type { PromptWorkspaceFileRef } from '@/types';
import type { RightWorkspaceResource } from '@/components/workspace/right-workspace-context';

export function fileReferenceProjectId(source: PromptWorkspaceFileRef) {
  return 'locator' in source ? source.locator.projectId : source.projectId;
}

export function fileReferenceLabel(source: PromptWorkspaceFileRef) {
  switch (source.kind) {
    case 'workspace': case 'git-file': return source.relativePath;
    case 'artifact': return source.name;
    case 'turn-file': case 'turn-attachment': return '';
  }
}

export function fileReferenceIdentity(source: PromptWorkspaceFileRef) {
  const path = (value: string) => /win/i.test(globalThis.navigator?.platform ?? '')
    ? value.replaceAll('\\', '/').toLowerCase() : value.replaceAll('\\', '/');
  switch (source.kind) {
    case 'workspace': return JSON.stringify([source.kind, source.projectId, path(source.relativePath)]);
    case 'git-file': return JSON.stringify([source.kind, source.projectId, path(source.workspacePath ?? ''), path(source.relativePath)]);
    case 'artifact': case 'turn-file': case 'turn-attachment': {
      const l = source.locator;
      const identity = source.kind === 'artifact' ? [source.name]
        : source.kind === 'turn-file' ? [source.changeSetId, source.changeId]
          : [source.changeSetId, source.attachmentId];
      return JSON.stringify([source.kind, l.projectId, l.taskId, l.taskUuid ?? null, l.runId, l.roundId, l.nodeId,
        l.attemptId, l.outerNodeId ?? null, l.outerAttemptId ?? null, l.branchId, ...identity]);
    }
  }
}

export function parseFileReferenceSource(value: unknown): PromptWorkspaceFileRef | null {
  if (!value || typeof value !== 'object') return null;
  const v = value as Record<string, unknown>;
  const text = (s: unknown): s is string => typeof s === 'string' && s.length > 0;
  if (v.kind === 'workspace' || v.kind === 'git-file') {
    if (!text(v.projectId) || !text(v.relativePath)) return null;
    if (v.kind === 'git-file') return v.workspacePath === null || text(v.workspacePath)
      ? { kind: v.kind, projectId: v.projectId, relativePath: v.relativePath, workspacePath: v.workspacePath } : null;
    return { kind: v.kind, projectId: v.projectId, relativePath: v.relativePath };
  }
  if (v.kind !== 'artifact' && v.kind !== 'turn-file' && v.kind !== 'turn-attachment') return null;
  if (!v.locator || typeof v.locator !== 'object') return null;
  const l = v.locator as Record<string, unknown>;
  if (!['projectId', 'taskId', 'runId', 'roundId', 'nodeId', 'attemptId', 'branchId'].every(k => text(l[k]))) return null;
  for (const k of ['taskUuid', 'outerNodeId', 'outerAttemptId']) if (l[k] != null && !text(l[k])) return null;
  const locator = v.locator as Extract<PromptWorkspaceFileRef, { kind: 'artifact' }>['locator'];
  if (v.kind === 'artifact') return text(v.name) ? { kind: v.kind, locator, name: v.name } : null;
  if (v.kind === 'turn-file') return text(v.changeSetId) && text(v.changeId)
    ? { kind: v.kind, locator, changeSetId: v.changeSetId, changeId: v.changeId } : null;
  return text(v.changeSetId) && text(v.attachmentId)
    ? { kind: v.kind, locator, changeSetId: v.changeSetId, attachmentId: v.attachmentId } : null;
}

export function rightWorkspaceFileSource(resource: RightWorkspaceResource): PromptWorkspaceFileRef | null {
  if (resource.kind === 'conversation-asset' && resource.assetKind === 'artifact') {
    return { kind: 'artifact', locator: resource.locator, name: resource.name };
  }
  if (resource.kind === 'turn-attachment') {
    return { kind: 'turn-attachment', locator: resource.locator, changeSetId: resource.changeSetId, attachmentId: resource.attachmentId };
  }
  if (resource.kind !== 'file-diff' && resource.kind !== 'file-version') return null;
  if ('gitSource' in resource) return { kind: 'git-file', projectId: resource.projectId,
    workspacePath: resource.gitSource.workspacePath ?? null, relativePath: resource.gitSource.path };
  return { kind: 'turn-file', locator: resource.locator, changeSetId: resource.changeSetId, changeId: resource.changeId };
}
