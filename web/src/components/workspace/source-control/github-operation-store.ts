import { subscribeGitHubOperationUpdates } from '@/api';
import type { GitHubOperationVm } from '@/types';

type GitHubOperationListener = (operation: GitHubOperationVm) => void;
type GitHubOperationSubscriber = typeof subscribeGitHubOperationUpdates;

export class GitHubOperationEventStore {
  private static readonly MAX_RECENT_OPERATIONS = 64;

  private readonly listeners = new Set<GitHubOperationListener>();
  private readonly latest = new Map<string, GitHubOperationVm>();
  private subscriptionPromise: Promise<void> | null = null;

  constructor(private readonly subscribeUpdates: GitHubOperationSubscriber = subscribeGitHubOperationUpdates) {}

  subscribe(listener: GitHubOperationListener) {
    this.listeners.add(listener);
    void this.ensureSubscribed();
    return () => { this.listeners.delete(listener); };
  }

  reconcile(operation: GitHubOperationVm) {
    return this.latest.get(operation.operationId) ?? operation;
  }

  private ensureSubscribed() {
    if (this.subscriptionPromise) return this.subscriptionPromise;
    this.subscriptionPromise = this.subscribeUpdates((operation) => {
      this.latest.delete(operation.operationId);
      this.latest.set(operation.operationId, operation);
      while (this.latest.size > GitHubOperationEventStore.MAX_RECENT_OPERATIONS) {
        const oldest = this.latest.keys().next().value;
        if (oldest == null) break;
        this.latest.delete(oldest);
      }
      for (const listener of this.listeners) listener(operation);
    }).then(() => undefined).catch(() => undefined);
    return this.subscriptionPromise;
  }
}

export const githubOperationEventStore = new GitHubOperationEventStore();
