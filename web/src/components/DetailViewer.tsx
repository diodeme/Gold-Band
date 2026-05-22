import type { ContentVm } from '../types';
import { AppCard } from '@/components/AppCard';
import { CodeBlock, EmptyState } from '@/components/PageScaffold';
import { CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { ScrollArea } from '@/components/ui/scroll-area';
import { cn } from '@/lib/utils';

interface DetailViewerProps {
  title: string;
  content?: ContentVm | null;
  emptyLabel: string;
}

export function DetailViewer({ title, content, emptyLabel }: DetailViewerProps) {
  return (
    <AppCard className="min-h-0 min-w-0 overflow-hidden py-0">
      <CardHeader className="border-b px-5 py-4">
        <div className="flex items-center justify-between gap-3">
          <CardTitle className="text-base">{title}</CardTitle>
          {content ? <span className="text-xs text-muted-foreground">{content.kind}</span> : null}
        </div>
      </CardHeader>
      <CardContent className="min-h-0 flex-1 px-0 py-0">
        <DetailViewerContent content={content} emptyLabel={emptyLabel} />
      </CardContent>
    </AppCard>
  );
}

export function DetailViewerContent({ content, emptyLabel }: Omit<DetailViewerProps, 'title'>) {
  const proseContent = content?.kind === 'requirement' || content?.kind === 'round';

  return (
    <ScrollArea className="h-full min-w-0 max-w-full overflow-hidden [&_[data-slot=scroll-area-viewport]]:min-w-0 [&_[data-slot=scroll-area-viewport]]:max-w-full [&_[data-slot=scroll-area-viewport]>div]:!block [&_[data-slot=scroll-area-viewport]>div]:min-w-0 [&_[data-slot=scroll-area-viewport]>div]:max-w-full">
      {content ? (
        <div className="w-full min-w-0 max-w-full space-y-4 overflow-hidden p-5">
          <h4 className="break-words text-lg font-semibold">{content.title}</h4>
          <CodeBlock className={cn('w-full min-w-0 max-w-full overflow-x-hidden whitespace-pre-wrap break-all [overflow-wrap:anywhere]', proseContent && 'font-sans text-sm leading-7')}>{content.content}</CodeBlock>
        </div>
      ) : (
        <div className="p-5"><EmptyState>{emptyLabel}</EmptyState></div>
      )}
    </ScrollArea>
  );
}
