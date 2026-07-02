import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { ChevronDown } from "lucide-react";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { cn } from "@/lib/utils";
import { parseGoldBandHiddenSections } from "@/components/acp/hiddenPromptSections";

export function HiddenPromptMessageContent({ content }: { content: string }) {
  const parts = useMemo(() => parseGoldBandHiddenSections(content), [content]);

  if (parts.length === 0) return null;

  return (
    <div className="min-w-0 space-y-2">
      {parts.map((part, index) => {
        if (part.type === "hidden") {
          return (
            <HiddenPromptSection
              key={`${index}:hidden`}
              title={part.title}
              text={part.text}
            />
          );
        }

        const displayText = visiblePromptText(
          part.text,
          parts[index - 1]?.type === "hidden",
        );

        return (
          <div
            key={`${index}:visible`}
            className="min-w-0 whitespace-pre-wrap break-words [overflow-wrap:anywhere]"
          >
            {displayText}
          </div>
        );
      })}
    </div>
  );
}

export function visiblePromptText(text: string, followsHiddenSection: boolean) {
  return followsHiddenSection ? text.replace(/^[\r\n]+/, "") : text;
}

function HiddenPromptSection({ title, text }: { title?: string; text: string }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const label = hiddenPromptTitle(title, t);

  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <CollapsibleTrigger
        className={cn(
          "group flex w-full min-w-0 items-center justify-between gap-3 rounded-lg border border-border/50 bg-background/35 px-3 py-2 text-left text-xs text-muted-foreground transition hover:bg-background/50",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
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
      <CollapsibleContent>
        <pre className="mt-2 max-h-72 min-w-0 overflow-auto whitespace-pre-wrap break-words rounded-lg border border-border/40 bg-background/45 px-3 py-2 font-sans text-xs leading-5 text-foreground/80 [overflow-wrap:anywhere]">
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
