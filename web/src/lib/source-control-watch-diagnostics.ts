export type SourceControlWatchReport =
  | { event: 'subscriptions'; success: boolean }
  | { event: 'session-reuse'; projectId: string; monitorStarted: boolean }
  | {
    event: 'workspace-events';
    projectId: string;
    receivedEvents: number;
    projectSessions: number;
    routedSessions: number;
    metadataFilteredSessions: number;
    outOfScopeSessions: number;
    scopeMismatchSessions: number;
    nestedWorktreeFilteredSessions: number;
    pathOutsideWorkspaceSessions: number;
  }
  | { event: 'refresh-start'; projectId: string; pendingPaths: number; invalidateAll: boolean }
  | {
    event: 'refresh-end';
    projectId: string;
    outcome: 'ready' | 'error' | 'superseded';
    elapsedMs: number;
  };
