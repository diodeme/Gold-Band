import { ACPChatDialog } from '@/components/acp/ACPChatDialog';
import type { AgentTranscriptResource } from './right-workspace-context';

export function AgentConversationPanel({
  resource,
  eventPageSize,
  eventWindowPageCount,
}: {
  resource: AgentTranscriptResource;
  eventPageSize?: number;
  eventWindowPageCount?: number;
}) {
  const locator = resource.locator;
  return (
    <div className="flex min-h-0 flex-1 flex-col bg-background" data-agent-conversation-panel={locator.branchId} data-read-only="true">
      <ACPChatDialog
        session={null}
        projectId={locator.projectId}
        taskId={locator.taskId}
        taskUuid={locator.taskUuid}
        runId={locator.runId}
        roundId={locator.roundId}
        nodeId={locator.nodeId}
        attemptId={locator.attemptId}
        outerNodeId={locator.outerNodeId}
        outerAttemptId={locator.outerAttemptId}
        branchId={locator.branchId}
        eventPageSize={eventPageSize}
        eventWindowPageCount={eventWindowPageCount}
        readOnly
        showSystemPromptAction
        showRawFramesAction
        allowEventOnlySessionShell={false}
        usageCompact
        cacheNamespace={`right-workspace-agent:${locator.taskUuid ?? 'missing-task-uuid'}`}
      />
    </div>
  );
}
