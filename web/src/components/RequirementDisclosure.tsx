import { ArrowLeft } from 'lucide-react';
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

export function RequirementTeaser({ text, detailLabel, onOpenDetail, className, quote = false }: { text: string; detailLabel: string; onOpenDetail: () => void; className?: string; quote?: boolean }) {
  const clipped = isRequirementClipped(text);
  return (
    <div className={cn('min-w-0 space-y-1', className)}>
      <p className="line-clamp-1 break-words text-sm leading-6 text-muted-foreground">
        {quote ? '“' : null}{clippedRequirementText(text)}{quote ? '”' : null}
      </p>
      {clipped ? (
        <button
          type="button"
          className="text-sm font-medium text-primary underline-offset-4 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/55"
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
  return (
    <Sheet modal={false} open={open} onOpenChange={onOpenChange}>
      <SheetContent className="w-[560px] max-w-[calc(100vw-2rem)] gap-0 overflow-hidden p-0 sm:max-w-[560px]" closeLabel={closeLabel} showOverlay={false}>
        <SheetHeader className="shrink-0 gap-3 border-b px-5 py-4 text-left">
          {onBack ? (
            <Button variant="ghost" size="sm" className="h-8 w-fit px-2 text-muted-foreground" onClick={onBack}>
              <ArrowLeft className="h-4 w-4" />
              {backLabel}
            </Button>
          ) : null}
          <SheetDescription className="sr-only">{description}</SheetDescription>
          <SheetTitle className="break-words text-xl">{title}</SheetTitle>
        </SheetHeader>
        <ScrollArea className="min-h-0 flex-1">
          <div className="p-5">
            <CodeBlock className="whitespace-pre-wrap text-sm leading-7">{requirement}</CodeBlock>
          </div>
        </ScrollArea>
      </SheetContent>
    </Sheet>
  );
}
