import type React from 'react';
import { createContext, isValidElement, memo, useCallback, useContext, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { Download, FileCode2, ImageIcon, Loader2 } from 'lucide-react';
import remarkBreaks from 'remark-breaks';
import { remarkAlert } from 'remark-github-blockquote-alert';
import { useTranslation } from 'react-i18next';
import {
  Block,
  CodeBlock,
  CodeBlockCopyButton,
  defaultRehypePlugins,
  defaultRemarkPlugins,
  defaultUrlTransform,
  Streamdown,
  type BlockProps,
  type StreamdownProps,
  useIsCodeFenceIncomplete,
} from 'streamdown';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { ImagePreviewDialog, type ImagePreviewTarget } from '@/components/shared/ImagePreviewDialog';
import { openExternalUrl } from '@/api';
import { cn } from '@/lib/utils';
import { isExternalUrlHref, isHttpUrlHref, isLocalFileHref, isSystemHandlerHref, parseLocalFileLinkTarget } from '@/lib/file-link';
import { createIncrementalMarkdownBlockParser } from '@/lib/incremental-markdown-blocks';
import { MARKDOWN_ALERT_TYPES, markdownAlertStyles, markdownAlertTitleKey } from '@/lib/markdown-alerts';
import { remoteImageHost, type RemoteImagePolicy } from '@/lib/remote-image-trust';
import { trustRemoteImageHosts, useRemoteImageHostTrusted } from '@/lib/remote-image-trust-store';
import {
  createStreamingMarkdownPlayback,
  type StreamingMarkdownPlayback,
} from '@/lib/streaming-markdown-playback';
import { wasmCode } from '@/lib/streamdown-wasm-code';

/**
 * Every flavor renders CommonMark + GFM, including GitHub alerts (`> [!NOTE]` …).
 * `chat` is for replies inside a message flow and keeps headings close to body size.
 * `document` is for standalone documents (prompts, context bodies) and uses a document heading hierarchy.
 * `github-release` is a document flavor for release notes Gold Band publishes: single newlines are
 * line breaks and remote images load without host trust.
 */
export type MarkdownFlavor = 'chat' | 'document' | 'github-release';

export type MarkdownProps = {
  children: string;
  className?: string;
  streaming?: boolean;
  flavor?: MarkdownFlavor;
};

export interface MarkdownResourceLinkError {
  code: string;
  params: Record<string, unknown>;
}

export type MarkdownResourceLinkOpenResult =
  | { status: 'opened' }
  | { status: 'error'; error: MarkdownResourceLinkError };

export interface MarkdownResourceLinkHandler {
  openLocalFile: (
    rawHref: string,
    baseCanonicalPath?: string | null,
  ) => void | MarkdownResourceLinkOpenResult | Promise<void | MarkdownResourceLinkOpenResult>;
  openWebUrl?: (href: string) => void | Promise<void>;
}

const MarkdownResourceLinkContext = createContext<MarkdownResourceLinkHandler | null>(null);
const LOCAL_FILE_PROXY_PREFIX = 'https://gold-band.local-file.invalid/?href=';

export function MarkdownResourceLinkProvider({ handler, children }: { handler: MarkdownResourceLinkHandler | null; children: React.ReactNode }) {
  return <MarkdownResourceLinkContext.Provider value={handler}>{children}</MarkdownResourceLinkContext.Provider>;
}

export function useMarkdownResourceLinkHandler() {
  return useContext(MarkdownResourceLinkContext);
}

const MarkdownImagePreviewContext = createContext<((image: ImagePreviewTarget) => void) | null>(null);

/** Lets images inside modal Markdown (e.g. release notes) open a zoomed preview. */
export function MarkdownImagePreviewProvider({ children }: { children: React.ReactNode }) {
  const [image, setImage] = useState<ImagePreviewTarget | null>(null);
  return (
    <MarkdownImagePreviewContext.Provider value={setImage}>
      {children}
      <ImagePreviewDialog image={image} onClose={() => setImage(null)} />
    </MarkdownImagePreviewContext.Provider>
  );
}

export { isLocalFileHref };

export function proxyLocalFileLinks(markdown: string) {
  return markdown.replace(/\[([^\]\n]+)\]\(([^)\n]+)\)/gu, (match, label: string, destination: string, offset: number) => {
    if (markdown[offset - 1] === '!') return match;
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
const markdownPlugins: NonNullable<StreamdownProps['plugins']> = { code: wasmCode };
const markdownControls: NonNullable<StreamdownProps['controls']> = {
  code: { copy: false, download: false },
  table: false,
  mermaid: false,
};

function MarkdownLink({ href, children, ...props }: React.AnchorHTMLAttributes<HTMLAnchorElement>) {
  const { t } = useTranslation();
  const handler = useContext(MarkdownResourceLinkContext);
  const localHref = localHrefFromRenderedHref(href);
  const local = Boolean(localHref);
  const enabledLocal = Boolean(handler && localHref);
  const requestRevisionRef = useRef(0);
  const openingRef = useRef<{
    handler: MarkdownResourceLinkHandler;
    href: string;
    revision: number;
  } | null>(null);
  const [openFailure, setOpenFailure] = useState<{
    handler: MarkdownResourceLinkHandler;
    href: string;
    error: MarkdownResourceLinkError;
  } | null>(null);
  const openError = openFailure?.handler === handler && openFailure.href === localHref
    ? openFailure.error
    : null;
  const external = Boolean(href && isExternalUrlHref(href));
  const target = localHref ? parseLocalFileLinkTarget(localHref) : null;
  const visibleLabel = target ? renderedLinkText(children).trim() : '';
  const showTarget = Boolean(
    target
    && !visibleLabel.endsWith(target.displayText)
    && !visibleLabel.endsWith(target.sourceSuffix),
  );
  useLayoutEffect(() => {
    requestRevisionRef.current += 1;
    openingRef.current = null;
    return () => {
      requestRevisionRef.current += 1;
      openingRef.current = null;
    };
  }, [handler, localHref]);
  const openResolvedLocalFile = useCallback(async () => {
    if (!localHref || !handler) return;
    if (openingRef.current?.handler === handler && openingRef.current.href === localHref) return;
    const revision = requestRevisionRef.current + 1;
    requestRevisionRef.current = revision;
    openingRef.current = { handler, href: localHref, revision };
    setOpenFailure(null);
    try {
      const result = await handler.openLocalFile(localHref);
      if (requestRevisionRef.current !== revision) return;
      if (result?.status === 'error') {
        setOpenFailure({ handler, href: localHref, error: result.error });
      }
    } catch {
      if (requestRevisionRef.current !== revision) return;
      setOpenFailure({
        handler,
        href: localHref,
        error: { code: 'workspace-file.path-invalid', params: {} },
      });
    } finally {
      if (openingRef.current?.revision === revision) openingRef.current = null;
    }
  }, [handler, localHref]);
  return (
    <>
      <a
        {...props}
        className={cn(
          'font-medium [overflow-wrap:anywhere] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-link/45',
          enabledLocal
            ? 'mx-0.5 inline-flex items-center gap-1 rounded-sm align-baseline text-link no-underline decoration-link/45 underline-offset-2 transition-colors hover:underline hover:decoration-link'
            : local
              ? 'mx-0.5 inline-flex cursor-not-allowed items-center gap-1 rounded-sm align-baseline text-muted-foreground no-underline opacity-60'
              : 'text-link underline decoration-link/45 underline-offset-2 hover:decoration-link',
          openError && 'text-destructive decoration-destructive/45 hover:decoration-destructive',
        )}
        href={enabledLocal ? localHref ?? undefined : local ? undefined : href}
        target={local || external ? undefined : props.target}
        rel={local || external ? undefined : props.rel}
        aria-disabled={local && !handler ? true : undefined}
        onClick={local
          ? (event) => {
            event.preventDefault();
            void openResolvedLocalFile();
          }
          : external
            ? (event) => {
              event.preventDefault();
              if (!href) return;
              if (isSystemHandlerHref(href) || !handler?.openWebUrl || !isHttpUrlHref(href)) {
                void openExternalUrl(href);
                return;
              }
              void handler.openWebUrl(href);
            }
            : props.onClick}
      >
        {local ? <FileCode2 className="size-[0.9em] shrink-0 self-center stroke-[1.85]" aria-hidden="true" /> : null}
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
      {openError ? (
        <span
          role="alert"
          className="ml-1 text-xs font-normal text-destructive"
          data-workspace-file-link-error={openError.code}
        >
          {t(`workspace.filesPanel.errors.${openError.code}`, openError.code)}
        </span>
      ) : null}
    </>
  );
}

// Static Markdown drops the first block's top margin through Streamdown's root; streamed blocks are each
// wrapped in a `contents` div, so only the heading in the first wrapper may drop it. `first:` would match every block.
function CompactHeading({ level, children }: { level: 1 | 2 | 3; children: React.ReactNode }) {
  if (level === 1) {
    return (
      <h1 className="mt-3.5 mb-1.5 flex min-w-0 items-center gap-2 text-[15px] font-bold leading-6 text-foreground [[data-gb-stream-block]:first-child>&]:mt-0">
        <span className="h-3.5 w-1 shrink-0 rounded-full bg-primary/70" aria-hidden="true" />
        <span className="min-w-0 break-words [overflow-wrap:anywhere]">{children}</span>
      </h1>
    );
  }

  if (level === 2) {
    return <h2 className="mt-3.5 mb-1 text-[15px] font-bold leading-6 text-foreground [[data-gb-stream-block]:first-child>&]:mt-0">{children}</h2>;
  }

  return <h3 className="mt-3 mb-1 text-sm font-semibold leading-6 text-foreground [[data-gb-stream-block]:first-child>&]:mt-0">{children}</h3>;
}

const CODE_LANGUAGE_PATTERN = /language-([^\s]+)/;
const CODE_START_LINE_PATTERN = /startLine=(\d+)/;
const CODE_WITHOUT_LINE_NUMBERS_PATTERN = /\bnoLineNumbers\b/;
const IMAGE_EXTENSION_PATTERN = /\.[^/.]+$/;

type MarkdownCodeBlockProps = React.HTMLAttributes<HTMLElement> & {
  node?: {
    properties?: {
      metastring?: string;
    };
  };
};

function MarkdownCodeBlock({ className, children, node, ...props }: MarkdownCodeBlockProps) {
  const { t } = useTranslation();
  const isIncomplete = useIsCodeFenceIncomplete();
  const language = className?.match(CODE_LANGUAGE_PATTERN)?.[1] ?? '';
  const meta = node?.properties?.metastring;
  const parsedStartLine = meta?.match(CODE_START_LINE_PATTERN)?.[1];
  const startLine = parsedStartLine ? Number.parseInt(parsedStartLine, 10) : undefined;
  const lineNumbers = !CODE_WITHOUT_LINE_NUMBERS_PATTERN.test(meta ?? '');
  let source = '';

  if (isValidElement<{ children?: React.ReactNode }>(children) && typeof children.props.children === 'string') {
    source = children.props.children;
  } else if (typeof children === 'string') {
    source = children;
  }

  return (
    <CodeBlock
      {...props}
      className={className}
      code={source}
      isIncomplete={isIncomplete}
      language={language}
      lineNumbers={lineNumbers}
      startLine={startLine && startLine >= 1 ? startLine : undefined}
    >
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="inline-flex">
            <CodeBlockCopyButton
              aria-label={t('common.copyCode')}
              code={source}
              title={undefined}
            />
          </span>
        </TooltipTrigger>
        <TooltipContent>{t('common.copyCode')}</TooltipContent>
      </Tooltip>
    </CodeBlock>
  );
}

type MarkdownImageProps = React.ImgHTMLAttributes<HTMLImageElement> & { node?: unknown };

function imageDownloadName(src: string, alt: string) {
  const sourceName = new URL(src, window.location.origin).pathname.split('/').at(-1) ?? '';
  const sourceExtension = sourceName.split('.').at(-1);
  if (sourceName.includes('.') && sourceExtension && sourceExtension.length <= 4) return sourceName;
  return alt.replace(IMAGE_EXTENSION_PATTERN, '') || sourceName || 'image';
}

function downloadImageBlob(blob: Blob, fileName: string) {
  const extension = blob.type.includes('jpeg') || blob.type.includes('jpg')
    ? 'jpg'
    : blob.type.includes('svg')
      ? 'svg'
      : blob.type.includes('gif')
        ? 'gif'
        : blob.type.includes('webp')
          ? 'webp'
          : 'png';
  const downloadName = fileName.includes('.') ? fileName : `${fileName}.${extension}`;
  const objectUrl = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = objectUrl;
  anchor.download = downloadName;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(objectUrl);
}

const MarkdownRemoteImagePolicyContext = createContext<RemoteImagePolicy>('user-trust');

function MarkdownImage(props: MarkdownImageProps) {
  const policy = useContext(MarkdownRemoteImagePolicyContext);
  const host = props.src ? remoteImageHost(props.src) : null;
  const trusted = useRemoteImageHostTrusted(host);
  if (host && policy === 'user-trust' && !trusted) {
    return <BlockedRemoteImage host={host} src={props.src ?? ''} alt={props.alt ?? ''} />;
  }
  return <LoadedMarkdownImage {...props} />;
}

function imageLabel(src: string, alt: string) {
  if (alt.trim()) return alt.trim();
  try {
    return new URL(src).pathname.split('/').at(-1) || src;
  } catch {
    return src;
  }
}

/** Remote image from an untrusted host: no request is made until the user trusts the host. */
function BlockedRemoteImage({ host, src, alt }: { host: string; src: string; alt: string }) {
  const { t } = useTranslation();
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState(false);
  const load = useCallback(async (event: React.MouseEvent) => {
    // The image may sit inside a link; loading must not also follow it.
    event.preventDefault();
    event.stopPropagation();
    setPending(true);
    setFailed(false);
    try {
      await trustRemoteImageHosts([host]);
    } catch {
      setFailed(true);
    } finally {
      setPending(false);
    }
  }, [host]);
  return (
    <span
      className="my-2 inline-flex max-w-full items-center gap-2 rounded-md border border-dashed border-border/70 px-2.5 py-1.5 align-middle text-xs text-muted-foreground"
      data-gb-markdown-image-blocked={host}
    >
      <ImageIcon className="size-3.5 shrink-0" aria-hidden="true" />
      <span className="min-w-0 truncate text-foreground">{imageLabel(src, alt)}</span>
      <span className="min-w-0 truncate">{host}</span>
      <Button
        type="button"
        size="sm"
        variant="ghost"
        className="h-6 shrink-0 px-2 text-xs"
        disabled={pending}
        onClick={(event) => void load(event)}
      >
        {pending ? <Loader2 className="size-3 animate-spin" aria-hidden="true" /> : null}
        {t('common.remoteImage.load')}
      </Button>
      {failed ? <span className="shrink-0 text-destructive">{t('common.remoteImage.trustFailed')}</span> : null}
    </span>
  );
}

function LoadedMarkdownImage({ node: _node, className, src, alt = '', onLoad, onError, ...props }: MarkdownImageProps) {
  const { t } = useTranslation();
  const openPreview = useContext(MarkdownImagePreviewContext);
  const linkHandler = useContext(MarkdownResourceLinkContext);
  const imageRef = useRef<HTMLImageElement>(null);
  const [loaded, setLoaded] = useState(false);
  const [failed, setFailed] = useState(false);
  const hasDeclaredSize = props.width !== undefined || props.height !== undefined;
  const showImage = (loaded || hasDeclaredSize) && !failed;
  const showFallback = failed && !hasDeclaredSize;

  useEffect(() => {
    const image = imageRef.current;
    if (!image?.complete) return;
    const succeeded = image.naturalWidth > 0;
    setLoaded(succeeded);
    setFailed(!succeeded);
  }, []);

  const handleLoad = useCallback<React.ReactEventHandler<HTMLImageElement>>((event) => {
    setLoaded(true);
    setFailed(false);
    onLoad?.(event);
  }, [onLoad]);

  const handleError = useCallback<React.ReactEventHandler<HTMLImageElement>>((event) => {
    setLoaded(false);
    setFailed(true);
    onError?.(event);
  }, [onError]);

  const handleDownload = useCallback(async () => {
    if (!src) return;
    try {
      const response = await fetch(src);
      const blob = await response.blob();
      downloadImageBlob(blob, imageDownloadName(src, alt));
    } catch {
      await openExternalUrl(src);
    }
  }, [alt, src]);

  // Clicking an image opens its own source like a link to it: a modal that provides a preview
  // zooms in place; otherwise web images open like web links and local images like file links.
  const openMode = !src || !showImage
    ? null
    : openPreview
      ? 'preview'
      : isExternalUrlHref(src) || (linkHandler && isLocalFileHref(src))
        ? 'open'
        : null;
  const handleClick = useCallback((event: React.MouseEvent<HTMLImageElement>) => {
    // A linked image follows its outer link instead.
    if (!src || !openMode || event.currentTarget.closest('a')) return;
    if (openMode === 'preview') {
      openPreview?.({ src, alt });
      return;
    }
    if (isLocalFileHref(src)) {
      void linkHandler?.openLocalFile(src);
      return;
    }
    if (isSystemHandlerHref(src) || !linkHandler?.openWebUrl || !isHttpUrlHref(src)) {
      void openExternalUrl(src);
      return;
    }
    void linkHandler.openWebUrl(src);
  }, [alt, linkHandler, openMode, openPreview, src]);

  if (!src) return null;
  return (
    <span className="group relative my-4 inline-block" data-gb-markdown-image="true">
      <img
        {...props}
        ref={imageRef}
        alt={alt}
        className={cn('max-w-full rounded-lg', showFallback && 'hidden', openMode === 'preview' && 'cursor-zoom-in', openMode === 'open' && 'cursor-pointer', className)}
        src={src}
        onClick={handleClick}
        onLoad={handleLoad}
        onError={handleError}
      />
      {showFallback ? <span className="text-xs italic text-muted-foreground">{t('common.imageNotAvailable')}</span> : null}
      <span className="pointer-events-none absolute inset-0 hidden rounded-lg bg-black/10 group-hover:block" aria-hidden="true" />
      {showImage ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              className="absolute bottom-2 right-2 flex size-8 cursor-pointer items-center justify-center rounded-md border border-border bg-background/90 opacity-0 shadow-sm backdrop-blur-sm transition-all duration-200 hover:bg-background group-hover:opacity-100"
              aria-label={t('common.downloadImage')}
              onClick={() => void handleDownload()}
            >
              <Download className="size-3.5" aria-hidden="true" />
            </button>
          </TooltipTrigger>
          <TooltipContent>{t('common.downloadImage')}</TooltipContent>
        </Tooltip>
      ) : null}
    </span>
  );
}

function MarkdownParagraph({ children }: { children?: React.ReactNode }) {
  return <p className="my-0 min-w-0 break-words [overflow-wrap:anywhere]">{children}</p>;
}

function MarkdownBlockquote({ children }: { children?: React.ReactNode }) {
  return <blockquote className="my-2 border-l-2 border-primary/40 pl-3 text-muted-foreground">{children}</blockquote>;
}

const markdownComponents = {
  h1: ({ children }: { children?: React.ReactNode }) => <CompactHeading level={1}>{children}</CompactHeading>,
  h2: ({ children }: { children?: React.ReactNode }) => <CompactHeading level={2}>{children}</CompactHeading>,
  h3: ({ children }: { children?: React.ReactNode }) => <CompactHeading level={3}>{children}</CompactHeading>,
  h4: ({ children }: { children?: React.ReactNode }) => <h4 className="mt-2 mb-1 text-sm font-medium leading-6 text-foreground [[data-gb-stream-block]:first-child>&]:mt-0">{children}</h4>,
  h5: ({ children }: { children?: React.ReactNode }) => <h5 className="mt-2 mb-1 text-sm font-medium leading-6 text-foreground [[data-gb-stream-block]:first-child>&]:mt-0">{children}</h5>,
  h6: ({ children }: { children?: React.ReactNode }) => <h6 className="mt-2 mb-1 text-sm font-medium leading-6 text-muted-foreground [[data-gb-stream-block]:first-child>&]:mt-0">{children}</h6>,
  p: MarkdownParagraph,
  strong: ({ children }: { children?: React.ReactNode }) => <strong className="font-semibold text-foreground">{children}</strong>,
  em: ({ children }: { children?: React.ReactNode }) => <em className="text-foreground/90">{children}</em>,
  a: MarkdownLink,
  code: MarkdownCodeBlock,
  img: MarkdownImage,
  ul: ({ children }: { children?: React.ReactNode }) => <ul className="my-1.5 list-disc space-y-1 pl-5 marker:text-muted-foreground">{children}</ul>,
  ol: ({ children }: { children?: React.ReactNode }) => <ol className="my-1.5 list-decimal space-y-1 pl-5 marker:text-muted-foreground">{children}</ol>,
  li: ({ children }: { children?: React.ReactNode }) => <li className="pl-1 leading-6">{children}</li>,
  blockquote: MarkdownBlockquote,
  inlineCode: ({ className, children, node: _node, ...props }: React.HTMLAttributes<HTMLElement> & { node?: unknown }) => (
    <code className={cn('rounded-md bg-gold-surface-high px-1.5 py-0.5 font-sans text-[1em] font-normal leading-[inherit] tracking-normal text-foreground', className)} {...props}>
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

const MARKDOWN_ALERT_TITLE_CLASS = 'markdown-alert-title';

function classTokens(className: unknown) {
  return typeof className === 'string' ? className.split(/\s+/u) : [];
}

function MarkdownAlertBlockquote({ className, children }: { className?: string; children?: React.ReactNode }) {
  const { t } = useTranslation();
  const tokens = classTokens(className);
  const type = MARKDOWN_ALERT_TYPES.find((candidate) => tokens.includes(`markdown-alert-${candidate}`));
  if (!type) return <MarkdownBlockquote>{children}</MarkdownBlockquote>;
  const { icon: Icon, className: typeClassName } = markdownAlertStyles[type];
  return (
    <Alert
      role="note"
      className={cn('my-2 rounded-none border-0 border-l-2 bg-transparent px-3 py-1', typeClassName)}
      data-gb-markdown-alert={type}
    >
      <Icon aria-hidden="true" />
      <AlertTitle className="font-semibold">{t(markdownAlertTitleKey(type))}</AlertTitle>
      <AlertDescription className="block space-y-2 text-foreground">{children}</AlertDescription>
    </Alert>
  );
}

// The alert title is rendered by MarkdownAlertBlockquote in the UI language.
function MarkdownAlertAwareParagraph({ className, children }: { className?: string; children?: React.ReactNode }) {
  if (classTokens(className).includes(MARKDOWN_ALERT_TITLE_CLASS)) return null;
  return <MarkdownParagraph>{children}</MarkdownParagraph>;
}

// Single newlines are line breaks in GitHub Release bodies, so paragraphs need visible spacing.
function GithubReleaseParagraph({ className, children }: { className?: string; children?: React.ReactNode }) {
  if (classTokens(className).includes(MARKDOWN_ALERT_TITLE_CLASS)) return null;
  return <p className="my-0 min-w-0 break-words [overflow-wrap:anywhere] [&+p]:mt-3">{children}</p>;
}

// Standalone documents are read top to bottom, so their sections keep a GitHub-like heading hierarchy.
const documentHeadingComponents = {
  h1: ({ children }: { children?: React.ReactNode }) => <h1 className="mt-8 mb-3 border-b border-border/60 pb-2 text-lg font-bold leading-7 text-foreground">{children}</h1>,
  h2: ({ children }: { children?: React.ReactNode }) => <h2 className="mt-8 mb-3 border-b border-border/60 pb-2 text-lg font-bold leading-7 text-foreground">{children}</h2>,
  h3: ({ children }: { children?: React.ReactNode }) => <h3 className="mt-6 mb-2 text-base font-bold leading-6 text-foreground">{children}</h3>,
};

type Pluggable = NonNullable<StreamdownProps['remarkPlugins']>[number];
type SanitizeSchema = { attributes?: Record<string, unknown[]> };
const [sanitizePlugin, sanitizeSchema] = defaultRehypePlugins.sanitize as [Extract<Pluggable, (...parameters: never[]) => unknown>, SanitizeSchema];
// GitHub alerts are GFM syntax, so every Markdown surface renders them.
const alertRemarkPlugins: Pluggable[] = [
  ...Object.values(defaultRemarkPlugins),
  [remarkAlert, { tagName: 'blockquote' }],
];
// Streamdown's sanitizer strips classes; allow only the alert markers so callouts survive.
const alertRehypePlugins: Pluggable[] = [
  defaultRehypePlugins.raw,
  [sanitizePlugin, {
    ...sanitizeSchema,
    attributes: {
      ...sanitizeSchema.attributes,
      blockquote: [
        ...(sanitizeSchema.attributes?.blockquote ?? []),
        ['className', 'markdown-alert', ...MARKDOWN_ALERT_TYPES.map((type) => `markdown-alert-${type}`)],
      ],
      p: [...(sanitizeSchema.attributes?.p ?? []), ['className', MARKDOWN_ALERT_TITLE_CLASS]],
    },
  }],
  defaultRehypePlugins.harden,
];

type MarkdownFlavorProfile = Pick<StreamdownProps, 'components' | 'remarkPlugins' | 'rehypePlugins'> & {
  /** `user-trust` loads remote images only from hosts the user trusted; `allow` is for content we publish. */
  remoteImages: RemoteImagePolicy;
};

const markdownFlavors: Record<MarkdownFlavor, MarkdownFlavorProfile> = {
  chat: {
    components: { ...markdownComponents, blockquote: MarkdownAlertBlockquote, p: MarkdownAlertAwareParagraph } as NonNullable<StreamdownProps['components']>,
    remarkPlugins: alertRemarkPlugins,
    rehypePlugins: alertRehypePlugins,
    remoteImages: 'user-trust',
  },
  document: {
    components: { ...markdownComponents, ...documentHeadingComponents, blockquote: MarkdownAlertBlockquote, p: MarkdownAlertAwareParagraph } as NonNullable<StreamdownProps['components']>,
    remarkPlugins: alertRemarkPlugins,
    rehypePlugins: alertRehypePlugins,
    remoteImages: 'user-trust',
  },
  'github-release': {
    components: { ...markdownComponents, ...documentHeadingComponents, blockquote: MarkdownAlertBlockquote, p: GithubReleaseParagraph } as NonNullable<StreamdownProps['components']>,
    remarkPlugins: [...alertRemarkPlugins, remarkBreaks],
    rehypePlugins: alertRehypePlugins,
    remoteImages: 'allow',
  },
};

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

export const Markdown = memo(function Markdown({ children, className, streaming = false, flavor = 'chat' }: MarkdownProps) {
  const { t } = useTranslation();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const playbackRef = useRef<StreamingMarkdownPlayback | null>(null);
  const previousStreamingRef = useRef(streaming);
  const blockParserRef = useRef<ReturnType<typeof createIncrementalMarkdownBlockParser> | null>(null);
  if (!blockParserRef.current) {
    blockParserRef.current = createIncrementalMarkdownBlockParser();
  }

  useLayoutEffect(() => {
    const wasStreaming = previousStreamingRef.current;
    previousStreamingRef.current = streaming;
    const currentPlayback = playbackRef.current;

    if (!streaming) {
      if (!currentPlayback) return;
      currentPlayback.setCanonical(children);
      currentPlayback.setStreaming(false);
      currentPlayback.dispose();
      if (playbackRef.current === currentPlayback) playbackRef.current = null;
      return;
    }

    if (currentPlayback) {
      currentPlayback.setCanonical(children);
      currentPlayback.setStreaming(true);
      return;
    }

    const root = rootRef.current;
    if (!root) return;
    const playback = createStreamingMarkdownPlayback(root, {
      canonical: children,
      // History that was already rendered statically is the settled baseline.
      // A newly mounted streaming message still plays from its first token.
      streaming: wasStreaming,
    });
    playbackRef.current = playback;
    if (!wasStreaming) playback.setStreaming(true);
  }, [children, streaming]);

  useLayoutEffect(() => () => {
    const playback = playbackRef.current;
    if (!playback) return;
    playback.dispose();
    if (playbackRef.current === playback) playbackRef.current = null;
  }, []);

  return (
    <div
      className={cn('min-w-0 max-w-full space-y-2 break-words text-sm leading-6 [overflow-wrap:anywhere]', className)}
      data-gb-streaming-markdown={streaming ? 'true' : undefined}
      ref={rootRef}
    >
      <MarkdownRemoteImagePolicyContext.Provider value={markdownFlavors[flavor].remoteImages}>
      <Streamdown
        animated={streaming ? streamdownPlaybackTokens : false}
        BlockComponent={StreamingMarkdownBlock}
        className="space-y-2"
        components={markdownFlavors[flavor].components}
        remarkPlugins={markdownFlavors[flavor].remarkPlugins}
        rehypePlugins={markdownFlavors[flavor].rehypePlugins}
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
      </MarkdownRemoteImagePolicyContext.Provider>
    </div>
  );
});
