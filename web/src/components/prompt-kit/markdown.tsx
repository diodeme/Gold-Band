import type React from 'react';
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import {
  Streamdown,
  type StreamdownProps,
} from 'streamdown';
import { cn } from '@/lib/utils';
import {
  advanceStreamingMarkdownPresentation,
  createStreamingMarkdownPresentation,
  isStreamingMarkdownPresentationPending,
  STREAMING_MARKDOWN_FRAME_MS,
  streamingMarkdownPresentationText,
  syncStreamingMarkdownPresentation,
} from '@/lib/streaming-markdown';

export type MarkdownProps = {
  children: string;
  className?: string;
  streaming?: boolean;
};

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
  a: ({ href, children }: React.AnchorHTMLAttributes<HTMLAnchorElement>) => (
    <a className="font-medium text-primary underline underline-offset-2 [overflow-wrap:anywhere] hover:text-primary/80" href={href} target="_blank" rel="noreferrer">
      {children}
    </a>
  ),
  ul: ({ children }: { children?: React.ReactNode }) => <ul className="my-1.5 list-disc space-y-1 pl-5 marker:text-muted-foreground">{children}</ul>,
  ol: ({ children }: { children?: React.ReactNode }) => <ol className="my-1.5 list-decimal space-y-1 pl-5 marker:text-muted-foreground">{children}</ol>,
  li: ({ children }: { children?: React.ReactNode }) => <li className="pl-1 leading-6">{children}</li>,
  blockquote: ({ children }: { children?: React.ReactNode }) => <blockquote className="my-2 border-l-2 border-primary/40 pl-3 text-muted-foreground">{children}</blockquote>,
  code: ({ className, children, node: _node, ...props }: React.HTMLAttributes<HTMLElement> & { node?: unknown }) => (
    <code className={cn('rounded-md bg-muted/50 px-1.5 py-0.5 font-mono text-[0.86em] text-foreground', className)} {...props}>
      {children}
    </code>
  ),
  pre: ({ children }: { children?: React.ReactNode }) => (
    <pre className="my-2 max-w-full overflow-x-auto rounded-xl border border-border/60 bg-muted/35 p-3 font-mono text-xs leading-5 text-foreground shadow-sm shadow-background/20 [&_code]:bg-transparent [&_code]:p-0 [&_code]:text-[inherit]">
      {children}
    </pre>
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

export function Markdown({ children, className, streaming = false }: MarkdownProps) {
  const [presentation, setPresentation] = useState(() =>
    createStreamingMarkdownPresentation(children, streaming),
  );
  const lastFrameAtRef = useRef(0);
  const hasStreamedRef = useRef(streaming);
  if (streaming) hasStreamedRef.current = true;

  useLayoutEffect(() => {
    setPresentation((current) =>
      syncStreamingMarkdownPresentation(current, children, streaming),
    );
  }, [children, streaming]);

  const pending = isStreamingMarkdownPresentationPending(presentation);
  useEffect(() => {
    if (!pending || typeof window === 'undefined') return;
    if (window.matchMedia?.('(prefers-reduced-motion: reduce)').matches) {
      setPresentation((current) => ({
        ...current,
        offset: current.canonical.length,
        carry: 0,
      }));
      return;
    }

    let frameId = 0;
    const tick = (now: number) => {
      const elapsed = lastFrameAtRef.current === 0
        ? STREAMING_MARKDOWN_FRAME_MS
        : now - lastFrameAtRef.current;
      if (elapsed < STREAMING_MARKDOWN_FRAME_MS) {
        frameId = window.requestAnimationFrame(tick);
        return;
      }
      lastFrameAtRef.current = now;
      setPresentation((current) =>
        advanceStreamingMarkdownPresentation(current, elapsed),
      );
    };
    frameId = window.requestAnimationFrame(tick);
    return () => window.cancelAnimationFrame(frameId);
  }, [pending, presentation.canonical.length, presentation.offset]);

  const presentationStreaming = streaming || pending;
  const streamdownMode = hasStreamedRef.current ? 'streaming' : 'static';
  const visibleChildren = streamingMarkdownPresentationText(
    presentation,
    streaming,
  );

  return (
    <div className={cn('min-w-0 max-w-full space-y-2 break-words text-sm leading-6 [overflow-wrap:anywhere]', className)}>
      <Streamdown
        className="space-y-2"
        components={markdownComponents}
        controls={false}
        isAnimating={presentationStreaming}
        mode={streamdownMode}
        parseIncompleteMarkdown={presentationStreaming}
      >
        {visibleChildren}
      </Streamdown>
    </div>
  );
}
