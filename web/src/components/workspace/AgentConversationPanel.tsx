import { ACPChatDialog } from '@/components/acp/ACPChatDialog';
import type { AgentTranscriptResource } from './right-workspace-context';

export function AgentConversationPanel({ resource }: { resource: AgentTranscriptResource }) {
  const locator = resource.locator;
  return (
    <div className="flex min-h-0 flex-1 flex-col bg-background">
      <ACPChatDialog
        session={null}
        projectId={locator.projectId}
        taskId={locator.taskId}
        runId={locator.runId}
        roundId={locator.roundId}
        nodeId={locator.nodeId}
        attemptId={locator.attemptId}
        outerNodeId={locator.outerNodeId}
        outerAttemptId={locator.outerAttemptId}
        branchId={locator.branchId}
        readOnly
        showSystemPromptAction={false}
        allowEventOnlySessionShell={false}
        cacheNamespace="right-workspace-agent"
      />
    </div>
  );
}
