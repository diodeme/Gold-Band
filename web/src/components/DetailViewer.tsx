import type { ContentVm } from '../types';
import { AppCard } from '@/components/AppCard';
import { CodeBlock, EmptyState } from '@/components/PageScaffold';
import { CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { ScrollArea } from '@/components/ui/scroll-area';

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
          {content ? <span className="font-mono text-xs text-muted-foreground">{content.kind}</span> : null}
        </div>
      </CardHeader>
      <CardContent className="min-h-0 flex-1 px-0 py-0">
        <DetailViewerContent content={content} emptyLabel={emptyLabel} />
      </CardContent>
    </AppCard>
  );
}

export function DetailViewerContent({ content, emptyLabel }: Omit<DetailViewerProps, 'title'>) {
  return (
    <ScrollArea className="h-full">
      {content ? (
        <div className="space-y-4 p-5">
          <h4 className="text-lg font-semibold">{content.title}</h4>
          <CodeBlock>{content.content}</CodeBlock>
        </div>
      ) : (
        <div className="p-5"><EmptyState>{emptyLabel}</EmptyState></div>
      )}
    </ScrollArea>
  );
}
