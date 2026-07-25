import type { ConversationPage } from '@/types';

interface ConversationWorkspaceRemovalTransitionInput {
  removedProjectId: string;
  lastActiveWorkspaceId?: string | null;
  activeWorkspaceId?: string | null;
  draftWorkspaceId?: string | null;
  page: ConversationPage;
}

export interface ConversationWorkspaceRemovalTransition {
  activeWorkspaceId: string | null;
  draftWorkspaceId: string | null;
  navigateHome: boolean;
}

export function resolveConversationWorkspaceRemovalTransition({
  removedProjectId,
  lastActiveWorkspaceId,
  activeWorkspaceId,
  draftWorkspaceId,
  page,
}: ConversationWorkspaceRemovalTransitionInput): ConversationWorkspaceRemovalTransition {
  const fallbackWorkspaceId = lastActiveWorkspaceId ?? null;
  const navigateHome = page.kind === 'conversation-run' && page.projectId === removedProjectId;

  return {
    activeWorkspaceId: activeWorkspaceId === removedProjectId || navigateHome
      ? fallbackWorkspaceId
      : (activeWorkspaceId ?? null),
    draftWorkspaceId: draftWorkspaceId === removedProjectId || navigateHome
      ? fallbackWorkspaceId
      : (draftWorkspaceId ?? null),
    navigateHome,
  };
}
