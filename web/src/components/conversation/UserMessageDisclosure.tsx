import {
  type ReactNode,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown } from "lucide-react";

import {
  type ChatContainerContentExpansionToken,
  useOptionalChatContainerContentExpansion,
} from "@/components/prompt-kit/chat-container";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export const USER_MESSAGE_COLLAPSED_MAX_HEIGHT_PX = 240;

export function UserMessageDisclosure({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const contentExpansion = useOptionalChatContainerContentExpansion();
  const contentId = useId();
  const contentRef = useRef<HTMLDivElement>(null);
  const expansionTokenRef = useRef<ChatContainerContentExpansionToken | null>(null);
  const contentExpansionRef = useRef(contentExpansion);
  const [expanded, setExpanded] = useState(false);
  const [overflowing, setOverflowing] = useState(false);
  contentExpansionRef.current = contentExpansion;

  const finishExpansion = useCallback(() => {
    const token = expansionTokenRef.current;
    expansionTokenRef.current = null;
    if (token !== null) {
      contentExpansionRef.current?.endContentExpansion(token);
    }
  }, []);

  useLayoutEffect(() => {
    const content = contentRef.current;
    if (!content) return;

    const measure = () => {
      const next =
        content.scrollHeight > USER_MESSAGE_COLLAPSED_MAX_HEIGHT_PX + 1;
      setOverflowing((current) => (current === next ? current : next));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(content);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (overflowing || !expanded) return;
    finishExpansion();
    setExpanded(false);
  }, [expanded, finishExpansion, overflowing]);

  useEffect(() => finishExpansion, [finishExpansion]);

  const handleToggle = useCallback(() => {
    if (expanded) {
      finishExpansion();
      setExpanded(false);
      return;
    }
    expansionTokenRef.current =
      contentExpansion?.beginContentExpansion() ?? null;
    setExpanded(true);
  }, [contentExpansion, expanded, finishExpansion]);

  return (
    <div className="min-w-0 max-w-full" data-user-message-disclosure="true">
      <div
        id={contentId}
        className={cn("min-w-0", !expanded && "overflow-hidden")}
        style={
          expanded
            ? undefined
            : { maxHeight: `${USER_MESSAGE_COLLAPSED_MAX_HEIGHT_PX}px` }
        }
        data-user-message-disclosure-content="true"
        data-state={expanded ? "expanded" : "collapsed"}
      >
        <div ref={contentRef} className="min-w-0">
          {children}
        </div>
      </div>
      {overflowing ? (
        <Button
          type="button"
          variant="link"
          size="xs"
          className="mt-1 h-auto gap-1 p-0 text-message-user-foreground/75 underline-offset-4 has-[>svg]:px-0 hover:text-message-user-foreground"
          aria-controls={contentId}
          aria-expanded={expanded}
          data-user-message-disclosure-trigger="true"
          onClick={handleToggle}
        >
          {t(expanded ? "acp.userMessageCollapse" : "acp.userMessageShowMore")}
          <ChevronDown
            aria-hidden="true"
            className={cn(
              "size-3.5 transition-transform",
              expanded && "rotate-180",
            )}
          />
        </Button>
      ) : null}
    </div>
  );
}
