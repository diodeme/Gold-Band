import { useCallback, useRef, useState, type ReactNode } from 'react';
import { Code2, Maximize2, Workflow } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { ImagePreviewTarget } from '@/components/shared/ImagePreviewDialog';
import {
  mermaidDiagramWorkspaceResourceKey,
  useOptionalRightWorkspaceCommands,
} from '@/components/workspace/right-workspace-context';
import { useMermaidDiagram } from '@/hooks/use-mermaid-diagram';
import { useNearViewport } from '@/hooks/use-near-viewport';
import { MERMAID_ERROR_CODES, mermaidInlineImageStyle, type MermaidDiagramError } from '@/lib/mermaid-diagram';
import { cn } from '@/lib/utils';

// Diagrams render shortly before they scroll into view, so reading history rarely sees a swap.
const MERMAID_RENDER_MARGIN = '800px 0px';

const MERMAID_ERROR_MESSAGE_KEYS: Record<MermaidDiagramError['code'], string> = {
  [MERMAID_ERROR_CODES.renderFailed]: 'common.mermaid.errors.renderFailed',
  [MERMAID_ERROR_CODES.cacheFull]: 'common.mermaid.errors.cacheFull',
  [MERMAID_ERROR_CODES.cancelled]: 'common.mermaid.errors.renderFailed',
  [MERMAID_ERROR_CODES.rasterFailed]: 'common.mermaid.errors.renderFailed',
};

const DIAGRAM_ACTION_CLASSES = {
  // Floats over the diagram, like the Markdown image download control.
  overlay: 'flex size-7 cursor-pointer items-center justify-center rounded-md border border-border bg-background/90 text-muted-foreground shadow-sm backdrop-blur-sm transition-colors hover:bg-background hover:text-foreground',
  // Sits beside the code block copy control and matches it.
  inline: 'inline-flex cursor-pointer p-1 text-muted-foreground transition-colors hover:text-foreground',
};

function DiagramAction({ label, onClick, variant, children }: {
  label: string;
  onClick: () => void;
  variant: keyof typeof DIAGRAM_ACTION_CLASSES;
  children: ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button type="button" className={DIAGRAM_ACTION_CLASSES[variant]} aria-label={label} onClick={onClick}>
          {children}
        </button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

export interface MarkdownMermaidBlockProps {
  source: string;
  /** Renders the fenced source as a code block; `actions` sit next to its copy button. */
  renderSource: (actions: ReactNode) => ReactNode;
  /** Modal Markdown zooms diagrams in place instead of opening a workspace tab. */
  onOpenPreview?: ((image: ImagePreviewTarget) => void) | null;
}

/**
 * A closed ```mermaid fence. The source stays visible until the diagram is ready,
 * so the block changes height at most once; the diagram then fits the message width
 * without cropping its height and never adds a vertical scroll region.
 */
export function MarkdownMermaidBlock({ source, renderSource, onOpenPreview }: MarkdownMermaidBlockProps) {
  const { t } = useTranslation();
  const hostRef = useRef<HTMLDivElement>(null);
  const near = useNearViewport(hostRef, MERMAID_RENDER_MARGIN);
  const view = useMermaidDiagram(source, near);
  const [showSource, setShowSource] = useState(false);
  const workspace = useOptionalRightWorkspaceCommands();
  const alt = t('common.mermaid.title');
  const asset = view.status === 'ready' ? view.asset : null;

  const open = useDiagramOpenAction({ source, alt, assetUrl: asset?.url ?? null, onOpenPreview, workspace });

  if (!asset || showSource) {
    return (
      <div ref={hostRef} className="min-w-0" data-gb-mermaid-block={showSource ? 'source' : view.status}>
        {renderSource(asset ? (
          <DiagramAction variant="inline" label={t('common.mermaid.showDiagram')} onClick={() => setShowSource(false)}>
            <Workflow className="size-3.5" aria-hidden="true" />
          </DiagramAction>
        ) : null)}
        {view.status === 'error' ? (
          <p role="alert" className="mt-1 text-xs text-destructive" data-gb-mermaid-error={view.error.code}>
            {t(MERMAID_ERROR_MESSAGE_KEYS[view.error.code])}
            {view.error.params.detail ? (
              <span className="mt-0.5 line-clamp-3 whitespace-pre-wrap font-mono text-muted-foreground">{view.error.params.detail}</span>
            ) : null}
          </p>
        ) : null}
      </div>
    );
  }

  return (
    <div ref={hostRef} className="group/mermaid relative my-2 min-w-0 max-w-full" data-gb-mermaid-block="ready">
      {/* Only wide diagrams scroll, and only horizontally; vertical wheel input stays with the conversation. */}
      <div className="max-w-full overflow-x-auto overflow-y-hidden" data-gb-mermaid-diagram="true">
        <img
          src={asset.url}
          alt={alt}
          width={asset.meta.width}
          height={asset.meta.height}
          draggable={false}
          className={cn('block select-none', open && 'cursor-zoom-in')}
          style={mermaidInlineImageStyle(asset.meta)}
          onClick={open ?? undefined}
        />
      </div>
      <div className="absolute right-1.5 top-1.5 flex gap-1 opacity-0 transition-opacity group-hover/mermaid:opacity-100 focus-within:opacity-100">
        {open ? (
          <DiagramAction variant="overlay" label={t('common.mermaid.open')} onClick={open}>
            <Maximize2 className="size-3.5" aria-hidden="true" />
          </DiagramAction>
        ) : null}
        <DiagramAction variant="overlay" label={t('common.mermaid.showSource')} onClick={() => setShowSource(true)}>
          <Code2 className="size-3.5" aria-hidden="true" />
        </DiagramAction>
      </div>
    </div>
  );
}

function useDiagramOpenAction({
  source,
  alt,
  assetUrl,
  onOpenPreview,
  workspace,
}: {
  source: string;
  alt: string;
  assetUrl: string | null;
  onOpenPreview: MarkdownMermaidBlockProps['onOpenPreview'];
  workspace: ReturnType<typeof useOptionalRightWorkspaceCommands>;
}) {
  const scopeKey = workspace?.scopeKey ?? null;
  const openWorkspace = useCallback(() => {
    if (!workspace?.scopeKey) return;
    void workspace.openResource({
      kind: 'mermaid-diagram',
      key: mermaidDiagramWorkspaceResourceKey(workspace.scopeKey, source),
      scopeKey: workspace.scopeKey,
      title: alt,
      attention: false,
      source,
    });
  }, [alt, source, workspace]);
  const openPreview = useCallback(() => {
    if (assetUrl) onOpenPreview?.({ src: assetUrl, alt });
  }, [alt, assetUrl, onOpenPreview]);
  if (onOpenPreview && assetUrl) return openPreview;
  return scopeKey ? openWorkspace : null;
}
