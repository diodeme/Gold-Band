import { useState } from 'react';
import { ArrowLeft, Check, Copy } from 'lucide-react';
import { CodeBlock } from '@/components/PageScaffold';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { cn } from '@/lib/utils';

const requirementPreviewLimit = 100;

export function fullRequirementText(requirement?: string | null, fallback?: string | null, emptyLabel = '') {
  return requirement?.trim() || fallback?.trim() || emptyLabel;
}

export function isRequirementClipped(text: string) {
  return text.replace(/\s+/g, ' ').trim().length > requirementPreviewLimit;
}

export function clippedRequirementText(text: string) {
  const compact = text.replace(/\s+/g, ' ').trim();
  if (!isRequirementClipped(text)) return compact;
  return `${compact.slice(0, requirementPreviewLimit)}…`;
}

export function RequirementTeaser({ text, detailLabel, onOpenDetail, className, quote = false, compact = false }: { text: string; detailLabel: string; onOpenDetail: () => void; className?: string; quote?: boolean; compact?: boolean }) {
  const clipped = isRequirementClipped(text);
  return (
    <div className={cn('min-w-0', compact ? 'flex items-center gap-3 overflow-hidden' : 'space-y-1', className)}>
      <p className={cn('line-clamp-1 min-w-0 break-words text-sm text-muted-foreground', compact ? 'leading-5 text-muted-foreground/85' : 'leading-6')}>
        {quote ? '“' : null}{clippedRequirementText(text)}{quote ? '”' : null}
      </p>
      {clipped ? (
        <button
          type="button"
          className={cn('shrink-0 font-medium text-primary underline-offset-4 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/55', compact ? 'text-xs' : 'text-sm')}
          onClick={onOpenDetail}
        >
          {detailLabel}
        </button>
      ) : null}
    </div>
  );
}

export function RequirementDetailSheet({ open, title, description, requirement, closeLabel, backLabel, onBack, onOpenChange }: {
  open: boolean;
  title: string;
  description: string;
  requirement: string;
  closeLabel: string;
  backLabel?: string;
  onBack?: () => void;
  onOpenChange: (open: boolean) => void;
}) {
  const [copied, setCopied] = useState(false);

  const copyRequirement = async () => {
    await navigator.clipboard.writeText(requirement);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1200);
  };

  return (
    <Sheet modal={false} open={open} onOpenChange={onOpenChange}>
      <SheetContent className="gap-0 overflow-hidden p-0" resizeStorageKey={onBack ? 'requirement/detail/nested' : 'requirement/detail'} defaultSize={560} minSize={420} maxSize={920} closeLabel={closeLabel} showOverlay={false}>
        <SheetHeader className="shrink-0 gap-3 border-b px-5 py-4 text-left">
          {onBack ? (
            <Button variant="ghost" size="sm" className="h-8 w-fit px-2 text-muted-foreground" onClick={onBack}>
              <ArrowLeft className="h-4 w-4" />
              {backLabel}
            </Button>
          ) : null}
          <SheetDescription className="sr-only">{description}</SheetDescription>
          <div className="flex items-center justify-between gap-3 pr-10">
            <SheetTitle className="break-words text-xl">{title}</SheetTitle>
            <Button variant="ghost" size="icon" className="h-8 w-8 shrink-0 text-muted-foreground hover:text-foreground" aria-label="复制" onClick={copyRequirement}>
              {copied ? <Check className="h-4 w-4 text-primary" /> : <Copy className="h-4 w-4" />}
            </Button>
          </div>
        </SheetHeader>
        <div className="flex min-h-0 flex-1 flex-col p-5">
          <ScrollArea className="min-h-0 flex-1">
            <CodeBlock className="whitespace-pre-wrap font-sans text-sm leading-7">{requirement}</CodeBlock>
          </ScrollArea>
        </div>
      </SheetContent>
    </Sheet>
  );
}
