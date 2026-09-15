import {
  Children,
  type HTMLAttributes,
  isValidElement,
  type ReactNode,
  useLayoutEffect,
  useRef,
} from 'react';

import {
  ChatContainerContent,
  ChatContainerRoot,
  syncChatContainerFooterHeight,
  type ChatContainerRootProps,
} from '@/components/prompt-kit/chat-container';
import { GOLD_CONVERSATION_SCROLLBAR_CLASS } from '@/lib/themed-scrollbar';
import { cn } from '@/lib/utils';

interface ConversationViewportProps {
  children: ReactNode;
  scrollClassName: string;
  contextRef?: ChatContainerRootProps['contextRef'];
  onAtBottomChange?: ChatContainerRootProps['onAtBottomChange'];
  canResumeFollowingAfterDisclosure?: ChatContainerRootProps['canResumeFollowingAfterDisclosure'];
  onFollowIntentChange?: ChatContainerRootProps['onFollowIntentChange'];
  onViewportScroll?: ChatContainerRootProps['onViewportScroll'];
  onViewportUserScroll?: ChatContainerRootProps['onViewportUserScroll'];
  initialFollowing?: boolean;
  className?: string;
  contentClassName?: string;
}

export function ConversationViewportFooter({
  className,
  ...props
}: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn('relative shrink-0', className)} {...props} />;
}

export function ConversationViewport({
  children,
  scrollClassName,
  contextRef,
  onAtBottomChange,
  canResumeFollowingAfterDisclosure,
  onFollowIntentChange,
  onViewportScroll,
  onViewportUserScroll,
  initialFollowing = true,
  className,
  contentClassName,
}: ConversationViewportProps) {
  const frameRef = useRef<HTMLDivElement>(null);
  const footerRef = useRef<HTMLDivElement>(null);
  const childItems = Children.toArray(children);
  const footer = childItems.find(
    (child) => isValidElement(child) && child.type === ConversationViewportFooter,
  );
  const content = footer
    ? childItems.filter((child) => child !== footer)
    : childItems;
  const hasFooter = footer != null;

  useLayoutEffect(() => {
    const frame = frameRef.current;
    const footerElement = footerRef.current;
    if (!frame || !footerElement) {
      frame?.style.removeProperty('--conversation-viewport-footer-height');
      return;
    }

    // Stable overhang wrappers remain mounted even when their content is empty.
    const overhangs = Array.from(footerElement.querySelectorAll<HTMLElement>(
      '[data-conversation-viewport-overhang]',
    ));
    let measureFrame: number | null = null;
    const measure = () => {
      syncChatContainerFooterHeight(footerElement, overhangs);
    };
    measure();

    const observer = new ResizeObserver(() => {
      if (measureFrame !== null) return;
      measureFrame = requestAnimationFrame(() => {
        measureFrame = null;
        measure();
      });
    });
    observer.observe(footerElement);
    overhangs.forEach((overhang) => observer.observe(overhang));
    return () => {
      observer.disconnect();
      if (measureFrame !== null) cancelAnimationFrame(measureFrame);
      frame.style.removeProperty('--conversation-viewport-footer-height');
    };
  }, [hasFooter]);

  return (
    <div
      ref={frameRef}
      className="relative h-full min-h-0 min-w-0 overflow-hidden"
      data-conversation-viewport-frame="true"
    >
      <ChatContainerRoot
        data-conversation-viewport="true"
        className={cn('h-full', className)}
        resize="instant"
        initial={initialFollowing ? 'instant' : false}
        contextRef={contextRef}
        onAtBottomChange={onAtBottomChange}
        canResumeFollowingAfterDisclosure={canResumeFollowingAfterDisclosure}
        onFollowIntentChange={onFollowIntentChange}
        onViewportScroll={onViewportScroll}
        onViewportUserScroll={onViewportUserScroll}
      >
        <ChatContainerContent
          className={cn('min-h-full', contentClassName)}
          scrollClassName={cn(GOLD_CONVERSATION_SCROLLBAR_CLASS, scrollClassName)}
          style={{
            paddingBottom: hasFooter
              ? 'var(--conversation-viewport-footer-height, 0px)'
              : undefined,
          }}
        >
          {content}
        </ChatContainerContent>
        {hasFooter ? (
          <div
            ref={footerRef}
            className="absolute inset-x-0 bottom-0 z-20"
            data-conversation-viewport-footer="true"
          >
            {footer}
          </div>
        ) : null}
      </ChatContainerRoot>
    </div>
  );
}
