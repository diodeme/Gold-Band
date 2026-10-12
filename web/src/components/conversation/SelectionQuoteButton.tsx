import { MessageSquareQuote } from 'lucide-react';
import { useEffect, useRef, useState, type RefObject } from 'react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/components/ui/button';

/** A selection the user can quote, with the viewport rect the button anchors to. */
export interface QuotableSelection<T> {
  value: T;
  rect: DOMRect;
}

type SelectionPosition<T> = QuotableSelection<T> & { top: number; left: number };

const BUTTON_OFFSET_ABOVE = 42;
const BUTTON_OFFSET_BELOW = 8;
const BUTTON_EDGE_INSET = 56;
const SCROLL_HIDE_DELAY_MS = 80;

/**
 * Floating "quote" pill shown when the user finishes a selection inside `rootRef`.
 * Each quotable surface supplies its own reader, so DOM text and editor documents share one control.
 */
export function SelectionQuoteButton<T>({
  rootRef,
  readSelection,
  onQuote,
}: {
  rootRef: RefObject<HTMLElement | null>;
  readSelection: (root: HTMLElement) => QuotableSelection<T> | null;
  onQuote: (value: T) => void;
}) {
  const { t } = useTranslation();
  const [position, setPosition] = useState<SelectionPosition<T> | null>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const readSelectionRef = useRef(readSelection);
  readSelectionRef.current = readSelection;

  useEffect(() => {
    const root = rootRef.current;
    if (!root || window.matchMedia?.('(pointer: coarse)').matches) return;
    let mounted = true;
    let scrollTimer: number | null = null;
    const showForSelection = () => {
      window.setTimeout(() => {
        if (!mounted) return;
        const selected = readSelectionRef.current(root);
        if (!selected) return setPosition(null);
        const above = selected.rect.top - BUTTON_OFFSET_ABOVE;
        setPosition({
          ...selected,
          top: above >= BUTTON_OFFSET_BELOW ? above : selected.rect.bottom + BUTTON_OFFSET_BELOW,
          left: Math.max(
            BUTTON_EDGE_INSET,
            Math.min(selected.rect.left + selected.rect.width / 2, window.innerWidth - BUTTON_EDGE_INSET),
          ),
        });
      }, 0);
    };
    const handleMouseUp = (event: MouseEvent) => {
      if (!buttonRef.current?.contains(event.target as Node)) showForSelection();
    };
    const handleKeyUp = (event: KeyboardEvent) => {
      if (event.key === 'Shift' || event.shiftKey) showForSelection();
    };
    const handleMouseDown = (event: MouseEvent) => {
      if (!buttonRef.current?.contains(event.target as Node)) setPosition(null);
    };
    const handleScroll = () => {
      if (scrollTimer !== null) window.clearTimeout(scrollTimer);
      scrollTimer = window.setTimeout(() => setPosition(null), SCROLL_HIDE_DELAY_MS);
    };
    root.addEventListener('mouseup', handleMouseUp);
    root.addEventListener('keyup', handleKeyUp);
    document.addEventListener('mousedown', handleMouseDown);
    root.addEventListener('scroll', handleScroll, true);
    return () => {
      mounted = false;
      if (scrollTimer !== null) window.clearTimeout(scrollTimer);
      root.removeEventListener('mouseup', handleMouseUp);
      root.removeEventListener('keyup', handleKeyUp);
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
        onQuote(position.value);
        setPosition(null);
        window.getSelection()?.removeAllRanges();
      }}
      data-selection-quote="true"
    >
      <MessageSquareQuote className="size-3.5" />
      {t('acp.quoteAction')}
    </Button>
  );
}
