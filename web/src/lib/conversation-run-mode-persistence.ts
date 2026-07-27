import type { ConversationRunModeVm } from '@/types';

export type SaveConversationRunMode = (
  projectId: string,
  mode: ConversationRunModeVm,
) => Promise<void>;

export class ConversationRunModePersistence {
  private readonly queues = new Map<string, Promise<void>>();

  constructor(private readonly save: SaveConversationRunMode) {}

  enqueue(projectId: string, mode: ConversationRunModeVm): Promise<void> {
    const previous = this.queues.get(projectId) ?? Promise.resolve();
    const queued = previous
      .catch(() => undefined)
      .then(() => this.save(projectId, mode));
    this.queues.set(projectId, queued);
    void queued.then(
      () => this.clearCompleted(projectId, queued),
      () => this.clearCompleted(projectId, queued),
    );
    return queued;
  }

  waitFor(projectId: string): Promise<void> {
    return (this.queues.get(projectId) ?? Promise.resolve()).catch(() => undefined);
  }

  private clearCompleted(projectId: string, completed: Promise<void>) {
    if (this.queues.get(projectId) === completed) {
      this.queues.delete(projectId);
    }
  }
}
