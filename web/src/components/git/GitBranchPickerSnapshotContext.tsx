import { createContext, useContext, useRef, type ReactNode } from 'react';
import type { GitBranchPickerSnapshotVm } from '@/types';

const MAX_BRANCH_PICKER_SNAPSHOTS = 24;

function branchPickerSnapshotKey(projectId: string, workspacePath?: string | null) {
  return `${projectId}\u0000${workspacePath ?? ''}`;
}

export class GitBranchPickerSnapshotStore {
  private readonly snapshots = new Map<string, GitBranchPickerSnapshotVm>();

  peek(projectId: string, workspacePath?: string | null) {
    return this.snapshots.get(branchPickerSnapshotKey(projectId, workspacePath)) ?? null;
  }

  get(projectId: string, workspacePath?: string | null) {
    const key = branchPickerSnapshotKey(projectId, workspacePath);
    const snapshot = this.snapshots.get(key) ?? null;
    if (!snapshot) return null;
    this.snapshots.delete(key);
    this.snapshots.set(key, snapshot);
    return snapshot;
  }

  set(projectId: string, workspacePath: string | null | undefined, snapshot: GitBranchPickerSnapshotVm) {
    const key = branchPickerSnapshotKey(projectId, workspacePath);
    this.snapshots.delete(key);
    this.snapshots.set(key, snapshot);
    while (this.snapshots.size > MAX_BRANCH_PICKER_SNAPSHOTS) {
      const oldestKey = this.snapshots.keys().next().value;
      if (oldestKey === undefined) break;
      this.snapshots.delete(oldestKey);
    }
  }

  delete(projectId: string, workspacePath?: string | null) {
    this.snapshots.delete(branchPickerSnapshotKey(projectId, workspacePath));
  }
}

const GitBranchPickerSnapshotContext = createContext<GitBranchPickerSnapshotStore | null>(null);

export function GitBranchPickerSnapshotProvider({
  children,
  store,
}: {
  children: ReactNode;
  store?: GitBranchPickerSnapshotStore;
}) {
  const internalStoreRef = useRef<GitBranchPickerSnapshotStore | null>(null);
  if (!internalStoreRef.current) internalStoreRef.current = store ?? new GitBranchPickerSnapshotStore();
  return (
    <GitBranchPickerSnapshotContext.Provider value={internalStoreRef.current}>
      {children}
    </GitBranchPickerSnapshotContext.Provider>
  );
}

export function useGitBranchPickerSnapshotStore() {
  const store = useContext(GitBranchPickerSnapshotContext);
  if (!store) {
    throw new Error('useGitBranchPickerSnapshotStore must be used within GitBranchPickerSnapshotProvider');
  }
  return store;
}
