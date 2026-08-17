import { MessageSquareQuote } from 'lucide-react';
import { useEffect, useRef, useState, type RefObject } from 'react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/components/ui/button';
import { readAgentMessageSelection, type AgentMessageSelection } from '@/lib/agent-message-selection';

type SelectionPosition = AgentMessageSelection & { top: number; left: number };

export function AgentSelectionQuoteButton({
  rootRef,
  onQuote,
}: {
  rootRef: RefObject<HTMLElement | null>;
  onQuote: (selection: AgentMessageSelection) => void;
}) {
  const { t } = useTranslation();
  const [position, setPosition] = useState<SelectionPosition | null>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const root = rootRef.current;
    if (!root || window.matchMedia?.('(pointer: coarse)').matches) return;
    let mounted = true;
    let scrollTimer: number | null = null;
    const handleMouseUp = (event: MouseEvent) => {
      if (buttonRef.current?.contains(event.target as Node)) return;
      window.setTimeout(() => {
        if (!mounted) return;
        const selected = readAgentMessageSelection(window.getSelection(), root);
        if (!selected) return setPosition(null);
        const above = selected.rect.top - 42;
        setPosition({
          ...selected,
          top: above >= 8 ? above : selected.rect.bottom + 8,
          left: Math.max(56, Math.min(selected.rect.left + selected.rect.width / 2, window.innerWidth - 56)),
        });
      }, 0);
    };
    const handleMouseDown = (event: MouseEvent) => {
      if (!buttonRef.current?.contains(event.target as Node)) setPosition(null);
    };
    const handleScroll = () => {
      if (scrollTimer !== null) window.clearTimeout(scrollTimer);
      scrollTimer = window.setTimeout(() => setPosition(null), 80);
    };
    root.addEventListener('mouseup', handleMouseUp);
    document.addEventListener('mousedown', handleMouseDown);
    root.addEventListener('scroll', handleScroll, true);
    return () => {
      mounted = false;
      if (scrollTimer !== null) window.clearTimeout(scrollTimer);
      root.removeEventListener('mouseup', handleMouseUp);
      document.removeEventListener('mousedown', handleMouseDown);
      root.removeEventListener('scroll', handleScroll, true);
    };
  }, [rootRef]);

  if (!position) return null;
  return (
    <Button
      ref={buttonRef}
      type="button"
      size="sm"
      variant="secondary"
      className="fixed z-50 h-8 gap-1.5 rounded-full border border-border/60 bg-popover px-3 text-xs text-popover-foreground shadow-md"
      style={{ top: position.top, left: position.left, transform: 'translateX(-50%)' }}
      onMouseDown={(event) => {
        event.preventDefault();
        onQuote(position);
        setPosition(null);
        window.getSelection()?.removeAllRanges();
      }}
      data-agent-selection-quote="true"
    >
      <MessageSquareQuote className="size-3.5" />
      {t('acp.quoteAction')}
    </Button>
  );
}
