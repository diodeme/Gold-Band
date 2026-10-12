import { useMemo } from 'react';
import { Workflow } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useMermaidDiagram } from '@/hooks/use-mermaid-diagram';
import type { ImageActionAsset } from '@/lib/image-actions';
import { rasterizeMermaidDiagram } from '@/lib/mermaid-diagram';
import type { MermaidDiagramWorkspaceResource } from '../right-workspace-context';
import { WorkspaceImageCanvas } from './WorkspaceImageCanvas';

const MERMAID_IMAGE_FILE_NAME = 'mermaid-diagram.png';

export function MermaidDiagramWorkspacePanel({ resource }: { resource: MermaidDiagramWorkspaceResource }) {
  const { t } = useTranslation();
  const view = useMermaidDiagram(resource.source, true);
  const asset = view.status === 'ready' ? view.asset : null;
  // Clipboard and save accept raster images only, so actions export a PNG of the diagram.
  const imageActionAsset = useMemo<ImageActionAsset | undefined>(() => asset ? {
    name: MERMAID_IMAGE_FILE_NAME,
    mime: 'image/png',
    loadOriginal: () => rasterizeMermaidDiagram(asset),
  } : undefined, [asset]);

  return (
    <section className="flex min-h-0 flex-1 flex-col" data-mermaid-diagram-workspace="true">
      <header className="flex h-9 shrink-0 items-center gap-2 border-b border-border/60 px-3 text-xs">
        <Workflow className="size-3.5 shrink-0" aria-hidden="true" />
        <span className="truncate">{resource.title}</span>
      </header>
      {asset ? (
        <WorkspaceImageCanvas src={asset.url} alt={resource.title} imageActionAsset={imageActionAsset} />
      ) : (
        <div className="flex min-h-0 flex-1 items-center justify-center px-6 text-center text-sm text-muted-foreground" role={view.status === 'error' ? 'alert' : undefined}>
          {view.status === 'error' ? t('common.mermaid.errors.renderFailed') : t('common.loading')}
        </div>
      )}
    </section>
  );
}
