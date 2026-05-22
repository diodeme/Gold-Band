import type React from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { cn } from '@/lib/utils';

export type MarkdownProps = {
  children: string;
  className?: string;
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

export function Markdown({ children, className }: MarkdownProps) {
  return (
    <div className={cn('min-w-0 max-w-full space-y-2 break-words text-sm leading-6 [overflow-wrap:anywhere]', className)}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          h1: ({ children }) => <CompactHeading level={1}>{children}</CompactHeading>,
          h2: ({ children }) => <CompactHeading level={2}>{children}</CompactHeading>,
          h3: ({ children }) => <CompactHeading level={3}>{children}</CompactHeading>,
          h4: ({ children }) => <h4 className="mt-2 mb-1 text-sm font-medium leading-6 text-foreground first:mt-0">{children}</h4>,
          h5: ({ children }) => <h5 className="mt-2 mb-1 text-sm font-medium leading-6 text-foreground first:mt-0">{children}</h5>,
          h6: ({ children }) => <h6 className="mt-2 mb-1 text-sm font-medium leading-6 text-muted-foreground first:mt-0">{children}</h6>,
          p: ({ children }) => <p className="my-0 min-w-0 break-words [overflow-wrap:anywhere]">{children}</p>,
          strong: ({ children }) => <strong className="font-semibold text-foreground">{children}</strong>,
          em: ({ children }) => <em className="text-foreground/90">{children}</em>,
          a: ({ href, children }) => (
            <a className="font-medium text-primary underline underline-offset-2 [overflow-wrap:anywhere] hover:text-primary/80" href={href} target="_blank" rel="noreferrer">
              {children}
            </a>
          ),
          ul: ({ children }) => <ul className="my-1.5 list-disc space-y-1 pl-5 marker:text-muted-foreground">{children}</ul>,
          ol: ({ children }) => <ol className="my-1.5 list-decimal space-y-1 pl-5 marker:text-muted-foreground">{children}</ol>,
          li: ({ children }) => <li className="pl-1 leading-6">{children}</li>,
          blockquote: ({ children }) => <blockquote className="my-2 border-l-2 border-primary/40 pl-3 text-muted-foreground">{children}</blockquote>,
          code: ({ className, children, ...props }) => (
            <code className={cn('rounded-md bg-muted/50 px-1.5 py-0.5 font-mono text-[0.86em] text-foreground', className)} {...props}>
              {children}
            </code>
          ),
          pre: ({ children }) => (
            <pre className="my-2 max-w-full overflow-x-auto rounded-xl border border-border/60 bg-muted/35 p-3 font-mono text-xs leading-5 text-foreground shadow-sm shadow-background/20 [scrollbar-color:hsl(var(--muted-foreground)/0.35)_transparent] [scrollbar-width:thin] [&_code]:bg-transparent [&_code]:p-0 [&_code]:text-[inherit] [&::-webkit-scrollbar]:h-2 [&::-webkit-scrollbar-thumb]:rounded-full [&::-webkit-scrollbar-thumb]:bg-muted-foreground/30 [&::-webkit-scrollbar-track]:bg-transparent">
              {children}
            </pre>
          ),
          table: ({ children }) => (
            <div className="my-2 max-w-full overflow-x-auto rounded-xl border border-border/60">
              <table className="w-full min-w-max border-collapse text-left text-xs leading-5">{children}</table>
            </div>
          ),
          thead: ({ children }) => <thead className="bg-muted/50 text-foreground">{children}</thead>,
          th: ({ children }) => <th className="border-b border-border/60 px-3 py-2 font-semibold">{children}</th>,
          td: ({ children }) => <td className="border-t border-border/40 px-3 py-2 text-muted-foreground">{children}</td>,
          hr: () => <hr className="my-3 border-border/70" />,
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  );
}
