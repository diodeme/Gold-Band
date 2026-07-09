import { forwardRef, useImperativeHandle, type ReactNode } from 'react';
import {
  ConversationComposerDraftProvider,
  createConversationComposerDraftBoundaryHandle,
  useConversationComposerDraftOwner,
  type ConversationComposerDraftBoundaryHandle,
} from '@/lib/conversation-composer-draft';

interface ConversationComposerDraftBoundaryProps {
  children: ReactNode;
}

export const ConversationComposerDraftBoundary = forwardRef<
  ConversationComposerDraftBoundaryHandle,
  ConversationComposerDraftBoundaryProps
>(function ConversationComposerDraftBoundary({ children }, ref) {
  const owner = useConversationComposerDraftOwner();

  useImperativeHandle(
    ref,
    () => createConversationComposerDraftBoundaryHandle(owner),
    [owner.reset],
  );

  return (
    <ConversationComposerDraftProvider value={owner}>
      {children}
    </ConversationComposerDraftProvider>
  );
});

export type { ConversationComposerDraftBoundaryHandle };
