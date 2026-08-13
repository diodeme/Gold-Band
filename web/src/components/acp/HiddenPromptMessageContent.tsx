import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { ChevronDown } from "lucide-react";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { cn } from "@/lib/utils";
import {
  type ChatContainerContentExpansionToken,
  useOptionalChatContainerContentExpansion,
} from "@/components/prompt-kit/chat-container";
import { resolvePromptBubbleInlineSize } from "@/lib/prompt-bubble-width";
import { parseGoldBandHiddenSections } from "@/components/acp/hiddenPromptSections";

export function HiddenPromptMessageContent({ content }: { content: string }) {
  const { t } = useTranslation();
  const contentExpansion = useOptionalChatContainerContentExpansion();
  const parts = useMemo(() => parseGoldBandHiddenSections(content), [content]);
  const displayParts = useMemo(() => projectHiddenPromptDisplayParts(parts), [parts]);
  const rootRef = useRef<HTMLDivElement>(null);
  const labelMeasureRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const visibleMeasureRefs = useRef<Array<HTMLDivElement | null>>([]);
  const hiddenMeasureRefs = useRef<Array<HTMLPreElement | null>>([]);
  const [openSections, setOpenSections] = useState<Record<number, boolean>>({});
  const [measurementRevision, setMeasurementRevision] = useState(0);
  const [measuredInlineSize, setMeasuredInlineSize] = useState<number | null>(null);
  const expansionTokensRef = useRef(
    new Map<number, ChatContainerContentExpansionToken>(),
  );
  const contentExpansionRef = useRef(contentExpansion);
  contentExpansionRef.current = contentExpansion;

  useEffect(() => () => {
    const tokens = Array.from(expansionTokensRef.current.values());
    expansionTokensRef.current.clear();
    for (const token of tokens) {
      contentExpansionRef.current?.endContentExpansion(token);
    }
  }, []);

  useLayoutEffect(() => {
    const messageRow = rootRef.current?.closest<HTMLElement>("[data-acp-message-row]");
    if (!messageRow) return undefined;

    let disposed = false;
    const refresh = () => setMeasurementRevision((revision) => revision + 1);
    const observer = new ResizeObserver(refresh);
    observer.observe(messageRow);
    void document.fonts?.ready.then(() => {
      if (!disposed) refresh();
    });
    return () => {
      disposed = true;
      observer.disconnect();
    };
  }, []);

  useLayoutEffect(() => {
    const frame = window.requestAnimationFrame(() => {
      const labelInlineSizes = labelMeasureRefs.current
        .map((element) => element?.getBoundingClientRect().width ?? 0);
      const visibleLineInlineSizes = visibleMeasureRefs.current
        .flatMap((element) => measuredLineInlineSizes(element));
      const expandedHiddenLineInlineSizes = hiddenMeasureRefs.current
        .flatMap((element, index) => openSections[index]
          ? measuredLineInlineSizes(element, true)
          : []);
      const nextInlineSize = resolvePromptBubbleInlineSize({
        labelInlineSizes,
        visibleLineInlineSizes,
        expandedHiddenLineInlineSizes,
      });
      setMeasuredInlineSize((current) => current === nextInlineSize ? current : nextInlineSize);
    });

    return () => window.cancelAnimationFrame(frame);
  }, [displayParts, measurementRevision, openSections]);

  if (parts.length === 0) return null;

  const handleSectionOpenChange = (index: number, open: boolean) => {
    if (open && !expansionTokensRef.current.has(index)) {
      const token = contentExpansion?.beginContentExpansion() ?? null;
      if (token !== null) expansionTokensRef.current.set(index, token);
    } else if (!open) {
      const token = expansionTokensRef.current.get(index) ?? null;
      expansionTokensRef.current.delete(index);
      contentExpansion?.endContentExpansion(token);
    }
    setOpenSections((current) => ({
      ...current,
      [index]: open,
    }));
  };

  return (
    <div
      ref={rootRef}
      className="inline-grid min-w-0 max-w-full gap-2"
      style={measuredInlineSize ? { width: `${measuredInlineSize}px` } : undefined}
    >
      {displayParts.map(({ part, sourceIndex }, index) => {
        if (part.type === "hidden") {
          return (
            <HiddenPromptSection
              key={`${sourceIndex}:hidden`}
              title={part.title}
              text={part.text}
              open={Boolean(openSections[sourceIndex])}
              onOpenChange={(open) => handleSectionOpenChange(sourceIndex, open)}
            />
          );
        }

        const displayText = visiblePromptText(
          part.text,
          displayParts[index - 1]?.part.type === "hidden",
        );

        return (
          <div
            key={`${sourceIndex}:visible`}
            className="min-w-0 whitespace-pre-wrap break-words [overflow-wrap:anywhere]"
          >
            {displayText}
          </div>
        );
      })}
      <div
        aria-hidden="true"
        className="pointer-events-none fixed left-0 top-0 -z-50 invisible grid h-0 overflow-visible"
      >
        {displayParts.map(({ part, sourceIndex }, index) => {
          if (part.type === "hidden") {
            const label = hiddenPromptTitle(part.title, t);
            return (
              <div key={`${sourceIndex}:hidden-measure`}>
                <button
                  ref={(element) => { labelMeasureRefs.current[sourceIndex] = element; }}
                  className="grid w-max grid-cols-[max-content_auto] items-center gap-3 rounded-lg border px-3 py-2 text-xs"
                  tabIndex={-1}
                >
                  <span className="font-medium">{label}</span>
                  <span className="inline-flex items-center gap-1.5 text-[11px]">
                    {t("acp.hiddenPromptCharacters", { count: part.text.length })}
                    <ChevronDown className="size-3.5" />
                  </span>
                </button>
                <pre
                  ref={(element) => { hiddenMeasureRefs.current[sourceIndex] = element; }}
                  className="w-[calc(var(--conversation-message-max-inline-size)-2rem)] whitespace-pre-wrap break-words px-3 py-2 font-sans text-xs leading-5 [overflow-wrap:anywhere]"
                >
                  {part.text.trim()}
                </pre>
              </div>
            );
          }

          const displayText = visiblePromptText(
            part.text,
            displayParts[index - 1]?.part.type === "hidden",
          );
          return (
            <div
              key={`${sourceIndex}:visible-measure`}
              ref={(element) => { visibleMeasureRefs.current[sourceIndex] = element; }}
              className="w-[calc(var(--conversation-message-max-inline-size)-2rem)] whitespace-pre-wrap break-words [overflow-wrap:anywhere]"
            >
              {displayText}
            </div>
          );
        })}
      </div>
    </div>
  );
}

export function visiblePromptText(text: string, followsHiddenSection: boolean) {
  return followsHiddenSection
    ? text.replace(/^(?:[\t ]*\r?\n)+/, "")
    : text;
}

export function projectHiddenPromptDisplayParts(
  parts: ReturnType<typeof parseGoldBandHiddenSections>,
) {
  const hiddenParts = parts
    .map((part, sourceIndex) => ({ part, sourceIndex }))
    .filter(({ part }) => part.type === "hidden");
  const visibleText = parts
    .filter((part) => part.type === "visible")
    .map((part) => part.text)
    .join("");
  const normalizedVisibleText = visiblePromptText(
    visibleText,
    hiddenParts.length > 0,
  );
  return normalizedVisibleText
    ? [
        ...hiddenParts,
        {
          part: { type: "visible" as const, text: normalizedVisibleText },
          sourceIndex: parts.length,
        },
      ]
    : hiddenParts;
}

function HiddenPromptSection({
  title,
  text,
  open,
  onOpenChange,
}: {
  title?: string;
  text: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = useTranslation();
  const label = hiddenPromptTitle(title, t);

  return (
    <Collapsible
      className="grid min-w-0 max-w-full"
      open={open}
      onOpenChange={onOpenChange}
    >
      <CollapsibleTrigger
        className={cn(
          "group grid min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-3 rounded-lg border border-foreground/10 bg-foreground/[0.025] px-3 py-2 text-left text-xs text-muted-foreground transition-colors hover:bg-foreground/[0.045]",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-0",
        )}
      >
        <span className="min-w-0 truncate font-medium text-foreground/80">
          {label}
        </span>
        <span className="inline-flex shrink-0 items-center gap-1.5 text-[11px] text-muted-foreground">
          {t("acp.hiddenPromptCharacters", { count: text.length })}
          <ChevronDown
            className={cn(
              "size-3.5 transition-transform duration-150",
              open && "rotate-180",
            )}
          />
        </span>
      </CollapsibleTrigger>
      <CollapsibleContent className="min-w-0 max-w-full">
        <pre className="mt-2 max-h-72 w-max min-w-0 max-w-full overflow-auto whitespace-pre-wrap break-words rounded-lg border border-foreground/10 bg-foreground/[0.025] px-3 py-2 font-sans text-xs leading-5 text-foreground/80 [overflow-wrap:anywhere]">
          {text.trim()}
        </pre>
      </CollapsibleContent>
    </Collapsible>
  );
}

function hiddenPromptTitle(title: string | undefined, t: TFunction) {
  if (title === "Gold Band stable system prompt") {
    return t("acp.hiddenStableSystemPrompt");
  }
  if (!title || title === "Gold Band runtime context") {
    return t("acp.hiddenRuntimeContext");
  }
  return title;
}

function measuredLineInlineSizes(
  element: HTMLElement | null,
  includeInlineChrome = false,
) {
  if (!element) return [];
  const range = document.createRange();
  range.selectNodeContents(element);
  const lineInlineSizes = Array.from(range.getClientRects())
    .map((rect) => rect.width)
    .filter((width) => width > 0);
  range.detach();
  if (!includeInlineChrome) return lineInlineSizes;

  const style = window.getComputedStyle(element);
  const inlineChrome = [
    style.paddingInlineStart,
    style.paddingInlineEnd,
    style.borderInlineStartWidth,
    style.borderInlineEndWidth,
  ].reduce((total, value) => total + (Number.parseFloat(value) || 0), 0);
  return lineInlineSizes.map((width) => width + inlineChrome);
}
