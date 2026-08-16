import type { ReactNode } from 'react';

import {
  ChatContainerContent,
  ChatContainerRoot,
  type ChatContainerRootProps,
} from '@/components/prompt-kit/chat-container';
import { GOLD_CONVERSATION_SCROLLBAR_CLASS } from '@/lib/themed-scrollbar';
import { cn } from '@/lib/utils';

interface ConversationViewportProps {
  children: ReactNode;
  scrollClassName: string;
  contextRef?: ChatContainerRootProps['contextRef'];
  onAtBottomChange?: ChatContainerRootProps['onAtBottomChange'];
  onViewportScroll?: ChatContainerRootProps['onViewportScroll'];
  onViewportUserScroll?: ChatContainerRootProps['onViewportUserScroll'];
  className?: string;
  contentClassName?: string;
}

export function ConversationViewport({
  children,
  scrollClassName,
  contextRef,
  onAtBottomChange,
  onViewportScroll,
  onViewportUserScroll,
  className,
  contentClassName,
}: ConversationViewportProps) {
  return (
    <ChatContainerRoot
      data-conversation-viewport="true"
      className={cn('h-full', className)}
      resize="instant"
      initial="instant"
      contextRef={contextRef}
      onAtBottomChange={onAtBottomChange}
      onViewportScroll={onViewportScroll}
      onViewportUserScroll={onViewportUserScroll}
    >
      <ChatContainerContent
        className={cn('min-h-full', contentClassName)}
        scrollClassName={cn(GOLD_CONVERSATION_SCROLLBAR_CLASS, scrollClassName)}
      >
        {children}
      </ChatContainerContent>
    </ChatContainerRoot>
  );
}
