import type React from 'react';
import { createContext, isValidElement, memo, useContext, useLayoutEffect, useRef } from 'react';
import { FileCode2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { code } from '@streamdown/code';
import {
  Block,
  defaultUrlTransform,
  Streamdown,
  type BlockProps,
  type StreamdownProps,
} from 'streamdown';
import { openExternalUrl } from '@/api';
import { cn } from '@/lib/utils';
import { isExternalUrlHref, isLocalFileHref, parseLocalFileLinkTarget } from '@/lib/file-link';
import { createIncrementalMarkdownBlockParser } from '@/lib/incremental-markdown-blocks';
import {
  createStreamingMarkdownPlayback,
  type StreamingMarkdownPlayback,
} from '@/lib/streaming-markdown-playback';

export type MarkdownProps = {
  children: string;
  className?: string;
  streaming?: boolean;
};

export interface MarkdownResourceLinkHandler {
  openLocalFile: (rawHref: string, baseCanonicalPath?: string | null) => void | Promise<void>;
}

const MarkdownResourceLinkContext = createContext<MarkdownResourceLinkHandler | null>(null);
const LOCAL_FILE_PROXY_PREFIX = 'https://gold-band.local-file.invalid/?href=';

export function MarkdownResourceLinkProvider({ handler, children }: { handler: MarkdownResourceLinkHandler | null; children: React.ReactNode }) {
  return <MarkdownResourceLinkContext.Provider value={handler}>{children}</MarkdownResourceLinkContext.Provider>;
}

export function useMarkdownResourceLinkHandler() {
  return useContext(MarkdownResourceLinkContext);
}

export { isLocalFileHref };

export function proxyLocalFileLinks(markdown: string) {
  return markdown.replace(/(?<!!)\[([^\]\n]+)\]\(([^)\n]+)\)/gu, (match, label: string, destination: string) => {
    const trimmed = destination.trim();
    const href = trimmed.startsWith('<') && trimmed.endsWith('>') ? trimmed.slice(1, -1) : trimmed;
    return isLocalFileHref(href)
      ? `[${label}](${LOCAL_FILE_PROXY_PREFIX}${encodeURIComponent(href)})`
      : match;
  });
}

function localHrefFromRenderedHref(href: string | undefined) {
  if (!href) return null;
  if (href.startsWith(LOCAL_FILE_PROXY_PREFIX)) {
    try {
      return decodeURIComponent(href.slice(LOCAL_FILE_PROXY_PREFIX.length));
    } catch {
      return null;
    }
  }
  return isLocalFileHref(href) ? href : null;
}

function renderedLinkText(node: React.ReactNode): string {
  if (typeof node === 'string' || typeof node === 'number') return String(node);
  if (Array.isArray(node)) return node.map(renderedLinkText).join('');
  if (isValidElement<{ children?: React.ReactNode }>(node)) {
    return renderedLinkText(node.props.children);
  }
  return '';
}

const markdownUrlTransform: NonNullable<StreamdownProps['urlTransform']> = (url, key, node) => (
  isLocalFileHref(url) ? url : defaultUrlTransform(url, key, node)
);
const markdownLinkSafety: NonNullable<StreamdownProps['linkSafety']> = { enabled: false };
const markdownPlugins: NonNullable<StreamdownProps['plugins']> = { code };
const markdownControls: NonNullable<StreamdownProps['controls']> = {
  code: { copy: true, download: false },
  table: false,
  mermaid: false,
};

function MarkdownLink({ href, children, ...props }: React.AnchorHTMLAttributes<HTMLAnchorElement>) {
  const handler = useContext(MarkdownResourceLinkContext);
  const localHref = localHrefFromRenderedHref(href);
  const local = Boolean(localHref);
  const enabledLocal = Boolean(handler && localHref);
  const external = Boolean(href && isExternalUrlHref(href));
  const target = localHref ? parseLocalFileLinkTarget(localHref) : null;
  const visibleLabel = target ? renderedLinkText(children).trim() : '';
  const showTarget = Boolean(
    target
    && !visibleLabel.endsWith(target.displayText)
    && !visibleLabel.endsWith(target.sourceSuffix),
  );
  return (
    <a
      {...props}
      className={cn(
        'font-medium [overflow-wrap:anywhere] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-gold-running/45',
        enabledLocal
          ? 'mx-0.5 inline-flex items-center gap-1 rounded-sm bg-muted/45 px-1 py-px align-baseline text-foreground/90 no-underline transition-colors hover:bg-accent hover:text-accent-foreground'
          : 'text-gold-running underline decoration-gold-running/45 underline-offset-2 hover:decoration-gold-running',
        local && !handler && 'cursor-not-allowed opacity-60',
      )}
      href={enabledLocal ? localHref ?? undefined : local ? undefined : href}
      target={local || external ? undefined : props.target}
      rel={local || external ? undefined : props.rel}
      aria-disabled={local && !handler ? true : undefined}
      onClick={local
        ? (event) => {
          event.preventDefault();
          if (localHref && handler) void handler.openLocalFile(localHref);
        }
        : external
          ? (event) => {
            event.preventDefault();
            if (href) void openExternalUrl(href);
          }
          : props.onClick}
    >
      {enabledLocal ? <FileCode2 className="size-[1em] shrink-0 self-center stroke-[2.35] text-gold-running" aria-hidden="true" /> : null}
      <span className="min-w-0 [overflow-wrap:anywhere]">
        {children}
        {showTarget ? (
          <span
            className="whitespace-nowrap"
            data-gb-file-link-target="true"
          >
            {target?.displayText}
          </span>
        ) : null}
      </span>
    </a>
  );
}

function CompactHeading({ level, children }: { level: 1 | 2 | 3; children: React.ReactNode }) {
  if (level === 1) {
    return (
      <h1 className="mt-3 mb-1.5 flex min-w-0 items-center gap-2 text-sm font-semibold leading-6 text-foreground first:mt-0">
        <span className="h-3.5 w-1 shrink-0 rounded-full bg-primary/70" aria-hidden="true" />
        <span className="min-w-0 break-words [overflow-wrap:anywhere]">{children}</span>
      </h1>
    );
  }

  if (level === 2) {
    return <h2 className="mt-3 mb-1 text-sm font-semibold leading-6 text-foreground first:mt-0">{children}</h2>;
  }

  return <h3 className="mt-2.5 mb-1 text-sm font-medium leading-6 text-foreground first:mt-0">{children}</h3>;
}

const markdownComponents = {
  h1: ({ children }: { children?: React.ReactNode }) => <CompactHeading level={1}>{children}</CompactHeading>,
  h2: ({ children }: { children?: React.ReactNode }) => <CompactHeading level={2}>{children}</CompactHeading>,
  h3: ({ children }: { children?: React.ReactNode }) => <CompactHeading level={3}>{children}</CompactHeading>,
  h4: ({ children }: { children?: React.ReactNode }) => <h4 className="mt-2 mb-1 text-sm font-medium leading-6 text-foreground first:mt-0">{children}</h4>,
  h5: ({ children }: { children?: React.ReactNode }) => <h5 className="mt-2 mb-1 text-sm font-medium leading-6 text-foreground first:mt-0">{children}</h5>,
  h6: ({ children }: { children?: React.ReactNode }) => <h6 className="mt-2 mb-1 text-sm font-medium leading-6 text-muted-foreground first:mt-0">{children}</h6>,
  p: ({ children }: { children?: React.ReactNode }) => <p className="my-0 min-w-0 break-words [overflow-wrap:anywhere]">{children}</p>,
  strong: ({ children }: { children?: React.ReactNode }) => <strong className="font-semibold text-foreground">{children}</strong>,
  em: ({ children }: { children?: React.ReactNode }) => <em className="text-foreground/90">{children}</em>,
  a: MarkdownLink,
  ul: ({ children }: { children?: React.ReactNode }) => <ul className="my-1.5 list-disc space-y-1 pl-5 marker:text-muted-foreground">{children}</ul>,
  ol: ({ children }: { children?: React.ReactNode }) => <ol className="my-1.5 list-decimal space-y-1 pl-5 marker:text-muted-foreground">{children}</ol>,
  li: ({ children }: { children?: React.ReactNode }) => <li className="pl-1 leading-6">{children}</li>,
  blockquote: ({ children }: { children?: React.ReactNode }) => <blockquote className="my-2 border-l-2 border-primary/40 pl-3 text-muted-foreground">{children}</blockquote>,
  inlineCode: ({ className, children, node: _node, ...props }: React.HTMLAttributes<HTMLElement> & { node?: unknown }) => (
    <code className={cn('rounded-md bg-muted/50 px-1.5 py-0.5 font-sans text-[1em] font-normal leading-[inherit] tracking-normal text-foreground', className)} {...props}>
      {children}
    </code>
  ),
  table: ({ children }: { children?: React.ReactNode }) => (
    <div className="my-2 max-w-full overflow-x-auto rounded-xl border border-border/60">
      <table className="w-full min-w-max border-collapse text-left text-xs leading-5">{children}</table>
    </div>
  ),
  thead: ({ children }: { children?: React.ReactNode }) => <thead className="bg-muted/50 text-foreground">{children}</thead>,
  th: ({ children }: { children?: React.ReactNode }) => <th className="border-b border-border/60 px-3 py-2 font-semibold">{children}</th>,
  td: ({ children }: { children?: React.ReactNode }) => <td className="border-t border-border/40 px-3 py-2 text-muted-foreground">{children}</td>,
  hr: () => <hr className="my-3 border-border/70" />,
} as NonNullable<StreamdownProps['components']>;

const streamdownPlaybackTokens: NonNullable<StreamdownProps['animated']> = {
  animation: 'fadeIn',
  duration: 0,
  easing: 'linear',
  sep: 'char',
  stagger: 0,
};

function StreamingMarkdownBlock(props: BlockProps) {
  return (
    <div className="contents" data-gb-stream-block="true">
      <Block {...props} />
    </div>
  );
}

export const Markdown = memo(function Markdown({ children, className, streaming = false }: MarkdownProps) {
  const { t } = useTranslation();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const playbackRef = useRef<StreamingMarkdownPlayback | null>(null);
  const blockParserRef = useRef<ReturnType<typeof createIncrementalMarkdownBlockParser> | null>(null);
  if (!blockParserRef.current) {
    blockParserRef.current = createIncrementalMarkdownBlockParser();
  }

  useLayoutEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const playback = createStreamingMarkdownPlayback(root, {
      canonical: children,
      streaming,
    });
    playbackRef.current = playback;
    return () => {
      playback.dispose();
      if (playbackRef.current === playback) playbackRef.current = null;
    };
  }, []);

  useLayoutEffect(() => {
    playbackRef.current?.setCanonical(children);
  }, [children]);

  useLayoutEffect(() => {
    playbackRef.current?.setStreaming(streaming);
  }, [streaming]);

  return (
    <div
      className={cn('min-w-0 max-w-full space-y-2 break-words text-sm leading-6 [overflow-wrap:anywhere]', className)}
      data-gb-streaming-markdown={streaming ? 'true' : undefined}
      ref={rootRef}
    >
      <Streamdown
        animated={streaming ? streamdownPlaybackTokens : false}
        BlockComponent={StreamingMarkdownBlock}
        className="space-y-2"
        components={markdownComponents}
        controls={markdownControls}
        isAnimating={streaming}
        lineNumbers={false}
        mode={streaming ? 'streaming' : 'static'}
        parseIncompleteMarkdown={streaming}
        parseMarkdownIntoBlocksFn={blockParserRef.current}
        plugins={markdownPlugins}
        translations={{
          copied: t('common.copied'),
          copyCode: t('common.copyCode'),
        }}
        urlTransform={markdownUrlTransform}
        linkSafety={markdownLinkSafety}
      >
        {proxyLocalFileLinks(children)}
      </Streamdown>
    </div>
  );
});
