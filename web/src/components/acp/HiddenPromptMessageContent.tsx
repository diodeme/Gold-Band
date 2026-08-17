import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { FileText } from "lucide-react";
import { Button } from "@/components/ui/button";
import { resolvePromptBubbleInlineSize } from "@/lib/prompt-bubble-width";
import { parseGoldBandHiddenSections } from "@/components/acp/hiddenPromptSections";

export interface HiddenPromptSectionOpenRequest {
  sourceIndex: number;
  title?: string;
  label: string;
}

export function HiddenPromptMessageContent({
  content,
  onOpenSection,
}: {
  content: string;
  onOpenSection?: (request: HiddenPromptSectionOpenRequest) => void;
}) {
  const { t } = useTranslation();
  const parts = useMemo(() => parseGoldBandHiddenSections(content), [content]);
  const displayParts = useMemo(() => projectHiddenPromptDisplayParts(parts), [parts]);
  const rootRef = useRef<HTMLDivElement>(null);
  const labelMeasureRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const visibleMeasureRefs = useRef<Array<HTMLDivElement | null>>([]);
  const [measurementRevision, setMeasurementRevision] = useState(0);
  const [measuredInlineSize, setMeasuredInlineSize] = useState<number | null>(null);

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
      const nextInlineSize = resolvePromptBubbleInlineSize({
        labelInlineSizes,
        visibleLineInlineSizes,
        expandedHiddenLineInlineSizes: [],
      });
      setMeasuredInlineSize((current) => current === nextInlineSize ? current : nextInlineSize);
    });

    return () => window.cancelAnimationFrame(frame);
  }, [displayParts, measurementRevision]);

  if (parts.length === 0) return null;

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
              onOpen={() => onOpenSection?.({
                sourceIndex,
                title: part.title,
                label: hiddenPromptTitle(part.title, t),
              })}
              disabled={!onOpenSection}
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
                  className="inline-flex h-auto w-max items-center justify-start gap-1.5 p-0 text-xs font-normal"
                  tabIndex={-1}
                >
                  <FileText className="size-3.5" />
                  <span className="font-medium">{label}</span>
                  <span className="text-ui-caption">{t("acp.hiddenPromptCharacters", { count: part.text.length })}</span>
                </button>
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
    .filter(({ part }) => part.type === "hidden" && part.show);
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
  onOpen,
  disabled,
}: {
  title?: string;
  text: string;
  onOpen: () => void;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const label = hiddenPromptTitle(title, t);

  return (
    <Button
      type="button"
      variant="link"
      data-hidden-prompt-link="true"
      disabled={disabled}
      className="h-auto min-w-0 max-w-full justify-start gap-1.5 p-0 text-left text-xs font-normal text-foreground/80 decoration-foreground/45 underline-offset-4 has-[>svg]:px-0 hover:text-foreground disabled:opacity-60"
      onClick={onOpen}
    >
      <FileText className="size-3.5 shrink-0 text-foreground/80" aria-hidden="true" />
      <span className="min-w-0 truncate font-medium">{label}</span>
      <span className="shrink-0 text-ui-caption text-muted-foreground">
        {t("acp.hiddenPromptCharacters", { count: text.length })}
      </span>
    </Button>
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

function measuredLineInlineSizes(element: HTMLElement | null) {
  if (!element) return [];
  const range = document.createRange();
  range.selectNodeContents(element);
  const lineInlineSizes = Array.from(range.getClientRects())
    .map((rect) => rect.width)
    .filter((width) => width > 0);
  range.detach();
  return lineInlineSizes;
}
