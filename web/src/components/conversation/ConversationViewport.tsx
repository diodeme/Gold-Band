import type { ReactNode } from 'react';

import {
  ChatContainerContent,
  ChatContainerRoot,
  type ChatContainerRootProps,
} from '@/components/prompt-kit/chat-container';
import { cn } from '@/lib/utils';

interface ConversationViewportProps {
  children: ReactNode;
  scrollClassName: string;
  contextRef?: ChatContainerRootProps['contextRef'];
  onAtBottomChange?: ChatContainerRootProps['onAtBottomChange'];
  onViewportScroll?: ChatContainerRootProps['onViewportScroll'];
  onViewportWheel?: ChatContainerRootProps['onViewportWheel'];
  className?: string;
  contentClassName?: string;
}

export function ConversationViewport({
  children,
  scrollClassName,
  contextRef,
  onAtBottomChange,
  onViewportScroll,
  onViewportWheel,
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
      onViewportWheel={onViewportWheel}
    >
      <ChatContainerContent
        className={cn('min-h-full', contentClassName)}
        scrollClassName={scrollClassName}
      >
        {children}
      </ChatContainerContent>
    </ChatContainerRoot>
  );
}
