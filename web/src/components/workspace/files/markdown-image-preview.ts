import { ensureSyntaxTree, syntaxTree } from '@codemirror/language';
import { Prec, StateEffect, StateField, type EditorState, type Extension, type Range } from '@codemirror/state';
import { Decoration, EditorView, WidgetType, type DecorationSet } from '@codemirror/view';
import { createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { workspaceFilePreviewUrl } from '@/api';
import { isLocalFileHref } from '@/lib/file-link';
import { markdownAlertStyles, markdownAlertTypeFromMarker, type MarkdownAlertType } from '@/lib/markdown-alerts';
import { isRemoteImageSource, remoteImageHost } from '@/lib/remote-image-trust';
import type { MarkdownImageState } from './file-content-store';

const IMAGE_SOURCE_PATTERN = /!\[[^\]]*\]\((?:<([^>]+)>|([^\s)"']+))(?:\s+["'][^)]*["'])?\)/gu;
const HTML_IMAGE_SOURCE_PATTERN = /<img\b[^>]*\bsrc\s*=\s*(["'])(.*?)\1[^>]*>/giu;
const HTML_IMAGE_LINE_PATTERN = /^\s*<img\b([^>]*)\/?\s*>\s*$/iu;
const HTML_LINKED_IMAGE_LINE_PATTERN = /^\s*<a\b([^>]*)>\s*<img\b([^>]*)\/?\s*>\s*<\/a>\s*$/iu;
const HTML_ALIGN_OPEN_PATTERN = /^\s*<(div|p)\b[^>]*\balign\s*=\s*(["'])(center|left|right)\2[^>]*>\s*$/iu;
const HTML_ALIGN_CLOSE_PATTERN = /^\s*<\/(div|p)>\s*$/iu;
const HTML_BREAK_PATTERN = /^\s*<br\s*\/?>\s*$/iu;
const STANDALONE_MARKDOWN_IMAGE_PATTERN = /^\s*(?:\[)?!\[[^\]]*\]\((?:<[^>]+>|[^\s)"']+)(?:\s+["'][^)]*["'])?\)(?:\]\([^)]+\))?\s*$/iu;
const EMPTY_MARKDOWN_IMAGES = new Map<string, MarkdownImageState>();
const EMPTY_TRUSTED_HOSTS: ReadonlySet<string> = new Set();
const MARKDOWN_IMAGE_PREDECODE_OVERSCAN_LINES = 12;
const MARKDOWN_IMAGE_DECODE_CACHE_LIMIT = 128;
const decodedMarkdownImageUrls = new Set<string>();
const pendingMarkdownImageDecodes = new Map<string, Promise<void>>();

function allImageSources(markdown: string): string[] {
  const sources: string[] = [];
  for (const match of markdown.matchAll(IMAGE_SOURCE_PATTERN)) {
    const source = match[1] ?? match[2];
    if (source) sources.push(source);
  }
  for (const match of markdown.matchAll(HTML_IMAGE_SOURCE_PATTERN)) {
    if (match[2]) sources.push(match[2]);
  }
  return sources;
}

/** Local image sources, which load through preview grants. */
export function markdownImageSources(markdown: string): string[] {
  return [...new Set(allImageSources(markdown).filter((source) => !isRemoteImageSource(source)))];
}

/** Host of every remote image occurrence, in document order. */
export function markdownRemoteImageHosts(markdown: string): string[] {
  return allImageSources(markdown).flatMap((source) => remoteImageHost(source) ?? []);
}

function rememberDecodedMarkdownImage(url: string) {
  decodedMarkdownImageUrls.delete(url);
  decodedMarkdownImageUrls.add(url);
  while (decodedMarkdownImageUrls.size > MARKDOWN_IMAGE_DECODE_CACHE_LIMIT) {
    const oldest = decodedMarkdownImageUrls.values().next().value;
    if (!oldest) break;
    decodedMarkdownImageUrls.delete(oldest);
  }
}

async function predecodeMarkdownImageUrl(url: string) {
  if (decodedMarkdownImageUrls.has(url)) return;
  const pending = pendingMarkdownImageDecodes.get(url);
  if (pending) return pending;
  if (typeof Image === 'undefined') return;
  const decode = (async () => {
    const image = new Image();
    image.decoding = 'async';
    image.loading = 'eager';
    image.src = url;
    if (typeof image.decode !== 'function') return;
    try {
      await image.decode();
      rememberDecodedMarkdownImage(url);
    } catch {
      // The mounted widget owns the visible error state and token refresh path.
    }
  })().finally(() => pendingMarkdownImageDecodes.delete(url));
  pendingMarkdownImageDecodes.set(url, decode);
  return decode;
}

export async function predecodeMarkdownImagesNearViewport(
  view: EditorView,
  images: ReadonlyMap<string, MarkdownImageState>,
) {
  const firstVisibleLine = view.state.doc.lineAt(view.viewport.from).number;
  const lastVisibleLine = view.state.doc.lineAt(view.viewport.to).number;
  const firstLine = view.state.doc.line(Math.max(1, firstVisibleLine - MARKDOWN_IMAGE_PREDECODE_OVERSCAN_LINES));
  const lastLine = view.state.doc.line(Math.min(view.state.doc.lines, lastVisibleLine + MARKDOWN_IMAGE_PREDECODE_OVERSCAN_LINES));
  const sources = markdownImageSources(view.state.doc.sliceString(firstLine.from, lastLine.to));
  await Promise.all(sources.flatMap((source) => {
    const image = images.get(source);
    return image?.kind === 'ready'
      ? [predecodeMarkdownImageUrl(workspaceFilePreviewUrl(image.previewGrant.token))]
      : [];
  }));
}

function htmlAttribute(attributes: string, name: string) {
  const match = attributes.match(new RegExp(`\\b${name}\\s*=\\s*(["'])(.*?)\\1`, 'iu'));
  return match?.[2] ?? '';
}

class RemoteMarkdownImageLinkWidget extends WidgetType {
  constructor(
    private readonly href: string,
    private readonly label: string,
    private readonly onLinkClick?: (href: string) => void,
  ) {
    super();
  }

  eq(other: RemoteMarkdownImageLinkWidget) {
    return this.href === other.href
      && this.label === other.label
      && this.onLinkClick === other.onLinkClick;
  }

  toDOM() {
    const link = document.createElement('a');
    link.className = 'cm-gold-band-markdown-remote-image-link';
    const routedLocalLink = Boolean(this.onLinkClick && isLocalFileHref(this.href));
    if (routedLocalLink) {
      link.tabIndex = 0;
      link.role = 'link';
      link.dataset.href = this.href;
    } else {
      link.href = this.href;
      link.target = '_blank';
      link.rel = 'noreferrer';
    }
    link.textContent = this.label || this.href;
    if (this.onLinkClick) {
      const open = (event: Event) => {
        event.preventDefault();
        event.stopPropagation();
        this.onLinkClick?.(this.href);
      };
      link.addEventListener('click', open);
      if (routedLocalLink) {
        link.addEventListener('keydown', (event) => {
          if (event.key === 'Enter' || event.key === ' ') open(event);
        });
      }
    }
    return link;
  }

  ignoreEvent() {
    return true;
  }
}

/** Remote image from a trusted host. Clicking opens the outer link, or the image itself. */
class TrustedRemoteImageWidget extends WidgetType {
  constructor(
    private readonly src: string,
    private readonly href: string,
    private readonly alt: string,
    private readonly onLinkClick?: (href: string) => void,
  ) {
    super();
  }

  eq(other: TrustedRemoteImageWidget) {
    return this.src === other.src
      && this.href === other.href
      && this.alt === other.alt
      && this.onLinkClick === other.onLinkClick;
  }

  toDOM() {
    const image = document.createElement('img');
    image.className = 'cm-gold-band-markdown-remote-image';
    image.src = this.src;
    image.alt = this.alt;
    image.decoding = 'async';
    image.loading = 'lazy';
    image.referrerPolicy = 'no-referrer';
    if (this.onLinkClick) {
      image.addEventListener('click', (event) => {
        event.preventDefault();
        event.stopPropagation();
        this.onLinkClick?.(this.href);
      });
    }
    return image;
  }

  ignoreEvent() {
    return true;
  }
}

class MarkdownAlertTitleWidget extends WidgetType {
  private root: Root | null = null;

  constructor(
    private readonly type: MarkdownAlertType,
    private readonly title: string,
  ) {
    super();
  }

  eq(other: MarkdownAlertTitleWidget) {
    return this.type === other.type && this.title === other.title;
  }

  toDOM() {
    const wrap = document.createElement('span');
    wrap.className = 'cm-gold-band-markdown-alert-title';
    const icon = document.createElement('span');
    icon.className = 'cm-gold-band-markdown-alert-icon';
    icon.setAttribute('aria-hidden', 'true');
    this.root = createRoot(icon);
    this.root.render(createElement(markdownAlertStyles[this.type].icon, { className: 'size-[1em]' }));
    const label = document.createElement('span');
    label.textContent = this.title;
    wrap.append(icon, label);
    return wrap;
  }

  destroy() {
    const root = this.root;
    this.root = null;
    // CodeMirror may destroy widgets during a React commit; unmount outside it.
    if (root) queueMicrotask(() => root.unmount());
  }

  ignoreEvent() {
    return false;
  }
}

/** Shows syntax characters as plain text where the preview would otherwise hide them. */
class LiteralTextWidget extends WidgetType {
  constructor(private readonly text: string) {
    super();
  }

  eq(other: LiteralTextWidget) {
    return this.text === other.text;
  }

  toDOM() {
    const span = document.createElement('span');
    span.textContent = this.text;
    return span;
  }

  ignoreEvent() {
    return false;
  }
}

class SafeMarkdownImageWidget extends WidgetType {
  constructor(
    private readonly state: MarkdownImageState,
    private readonly alt: string,
    private readonly onPreviewError?: (rawSrc: string, failedToken: string) => void,
  ) {
    super();
  }

  eq(other: SafeMarkdownImageWidget) {
    if (other.state.kind !== this.state.kind || other.state.rawSrc !== this.state.rawSrc) return false;
    if (this.state.kind === 'ready' && other.state.kind === 'ready') {
      return this.state.previewGrant.token === other.state.previewGrant.token && this.alt === other.alt;
    }
    return this.alt === other.alt;
  }

  toDOM(view: EditorView) {
    const wrap = document.createElement('div');
    wrap.className = 'cm-atomic-image cm-gold-band-markdown-image';
    if (this.state.kind === 'ready') {
      const image = document.createElement('img');
      const previewUrl = workspaceFilePreviewUrl(this.state.previewGrant.token);
      image.src = previewUrl;
      image.alt = this.alt;
      image.decoding = 'async';
      image.loading = decodedMarkdownImageUrls.has(previewUrl) ? 'eager' : 'lazy';
      image.width = this.state.width;
      image.height = this.state.height;
      image.addEventListener('error', () => {
        if (this.state.kind === 'ready') {
          this.onPreviewError?.(this.state.rawSrc, this.state.previewGrant.token);
        }
      }, { once: true });
      wrap.appendChild(image);
    } else {
      const placeholder = document.createElement('span');
      placeholder.className = 'cm-gold-band-markdown-image-placeholder';
      placeholder.textContent = this.state.kind === 'loading' ? '…' : this.alt || this.state.rawSrc;
      wrap.appendChild(placeholder);
    }
    wrap.addEventListener('mousedown', (event) => {
      event.preventDefault();
      event.stopPropagation();
      const position = view.posAtDOM(wrap);
      if (position < 0) return;
      view.focus();
      view.dispatch({ selection: { anchor: Math.max(0, position - 1) } });
    });
    return wrap;
  }

  ignoreEvent(event: Event) {
    return event.type === 'mousedown' || event.type === 'click';
  }
}

export interface MarkdownImagePreviewConfig {
  images: ReadonlyMap<string, MarkdownImageState>;
  /** Hosts whose remote images may load; other remote images stay links. */
  trustedImageHosts?: ReadonlySet<string>;
  /** Localized GitHub alert titles; alerts render only when provided. */
  alertTitles?: Readonly<Record<MarkdownAlertType, string>>;
  onPreviewError?: (rawSrc: string, failedToken: string) => void;
  onLinkClick?: (href: string) => void;
}

interface MarkdownImagePreviewFieldValue extends MarkdownImagePreviewConfig {
  decorations: DecorationSet;
}

const setMarkdownImagePreviewConfig = StateEffect.define<MarkdownImagePreviewConfig>();
const EMPTY_CONFIG: MarkdownImagePreviewConfig = { images: EMPTY_MARKDOWN_IMAGES };

function markdownLinkTarget(state: EditorState, imageNode: { from: number; to: number; node: { parent: { name: string; from: number; to: number } | null } }, fallback: string) {
  const parent = imageNode.node.parent;
  if (parent?.name !== 'Link') return { from: imageNode.from, to: imageNode.to, href: fallback };
  const raw = state.doc.sliceString(parent.from, parent.to);
  const match = raw.match(/\]\((?:<([^>]+)>|([^\s)"']+))(?:\s+["'][^)]*["'])?\)$/u);
  return {
    from: parent.from,
    to: parent.to,
    href: match?.[1] ?? match?.[2] ?? fallback,
  };
}

function remoteImageWidget(source: string, href: string, alt: string, config: MarkdownImagePreviewConfig) {
  const host = remoteImageHost(source);
  const trusted = host !== null && (config.trustedImageHosts ?? EMPTY_TRUSTED_HOSTS).has(host);
  return trusted
    ? new TrustedRemoteImageWidget(source, href, alt, config.onLinkClick)
    : new RemoteMarkdownImageLinkWidget(href, alt, config.onLinkClick);
}

/** CommonMark link labels match case-insensitively with collapsed whitespace. */
function normalizeLinkLabel(label: string) {
  return label.trim().replace(/\s+/gu, ' ').toLowerCase();
}

function linkReferenceLabels(state: EditorState, tree: ReturnType<typeof syntaxTree>) {
  const labels = new Set<string>();
  tree.iterate({
    enter(node) {
      if (node.name !== 'LinkReference') return;
      const label = node.node.getChild('LinkLabel');
      if (label) labels.add(normalizeLinkLabel(state.doc.sliceString(label.from + 1, label.to - 1)));
      return false;
    },
  });
  return labels;
}

type SyntaxNodeRef = Parameters<NonNullable<Parameters<ReturnType<typeof syntaxTree>['iterate']>[0]['enter']>>[0];

/**
 * `[text]` and `[text][label]` are links only when the label is defined; Lezer parses every
 * bracket pair as Link, so undefined references are restored to plain bracketed text.
 */
function addUnresolvedReferenceDecorations(
  state: EditorState,
  node: SyntaxNodeRef,
  definedLabels: ReadonlySet<string>,
  ranges: Range<Decoration>[],
) {
  const link = node.node;
  if (link.getChild('URL')) return;
  const marks = link.getChildren('LinkMark');
  const labelNode = link.getChild('LinkLabel');
  const textClose = marks.find((mark) => state.doc.sliceString(mark.from, mark.to) === ']');
  if (!textClose || marks[0]?.from !== link.from) return;
  const label = labelNode && labelNode.to - labelNode.from > 2
    ? state.doc.sliceString(labelNode.from + 1, labelNode.to - 1)
    : state.doc.sliceString(link.from + 1, textClose.from);
  if (definedLabels.has(normalizeLinkLabel(label))) return;
  ranges.push(Decoration.mark({ class: 'cm-gold-band-markdown-plain-reference' }).range(link.from, link.to));
  for (const mark of marks) {
    ranges.push(Decoration.replace({
      widget: new LiteralTextWidget(state.doc.sliceString(mark.from, mark.to)),
    }).range(mark.from, mark.to));
  }
}

/** GitHub alert: a blockquote whose first line is exactly `> [!TYPE]`. */
function addAlertDecorations(
  state: EditorState,
  node: SyntaxNodeRef,
  alertTitles: Readonly<Record<MarkdownAlertType, string>>,
  ranges: Range<Decoration>[],
) {
  const firstLine = state.doc.lineAt(node.from);
  const marker = firstLine.text.match(/^\s*>\s?(\[![^\]]+\])\s*$/u)?.[1];
  const type = marker ? markdownAlertTypeFromMarker(marker) : null;
  if (!marker || !type) return false;
  const lastLine = state.doc.lineAt(node.to);
  for (let lineNumber = firstLine.number; lineNumber <= lastLine.number; lineNumber += 1) {
    ranges.push(Decoration.line({
      class: `cm-gold-band-markdown-alert cm-gold-band-markdown-alert-${type}`,
      attributes: { style: `--gold-band-markdown-alert-color: ${markdownAlertStyles[type].colorVar}` },
    }).range(state.doc.line(lineNumber).from));
  }
  const markerFrom = firstLine.from + firstLine.text.indexOf(marker);
  ranges.push(Decoration.replace({
    widget: new MarkdownAlertTitleWidget(type, alertTitles[type]),
  }).range(markerFrom, markerFrom + marker.length));
  return true;
}

function buildDecorations(state: EditorState, config: MarkdownImagePreviewConfig) {
  const ranges: Range<Decoration>[] = [];
  const tree = ensureSyntaxTree(state, state.doc.length, 100) ?? syntaxTree(state);
  const definedLabels = linkReferenceLabels(state, tree);
  const alertMarkerLinks = new Set<number>();
  tree.iterate({
    enter(node) {
      if (node.name === 'CommentBlock') {
        const startLine = state.doc.lineAt(node.from);
        const endLine = state.doc.lineAt(node.to);
        ranges.push(Decoration.replace({ block: true }).range(startLine.from, endLine.to));
        return false;
      }
      if (node.name === 'Blockquote' && config.alertTitles && addAlertDecorations(state, node, config.alertTitles, ranges)) {
        const firstLine = state.doc.lineAt(node.from);
        alertMarkerLinks.add(firstLine.from + firstLine.text.indexOf('['));
        return;
      }
      if (node.name === 'Link') {
        if (!alertMarkerLinks.has(node.from)) addUnresolvedReferenceDecorations(state, node, definedLabels, ranges);
        return;
      }
      if (node.name !== 'Image') return;
      const raw = state.doc.sliceString(node.from, node.to);
      const match = raw.match(/^!\[([^\]]*)\]\((?:<([^>]+)>|([^\s)"']+))(?:\s+["'][^)]*["'])?\)$/u);
      const source = match?.[2] ?? match?.[3];
      if (!match || !source) return;
      if (isRemoteImageSource(source)) {
        const link = markdownLinkTarget(state, node, source);
        ranges.push(Decoration.replace({
          widget: remoteImageWidget(source, link.href, match[1] ?? '', config),
        }).range(link.from, link.to));
        return;
      }
      const image = config.images.get(source);
      if (!image) return;
      const line = state.doc.lineAt(node.from);
      if (STANDALONE_MARKDOWN_IMAGE_PATTERN.test(line.text)) {
        ranges.push(Decoration.replace({
          widget: new SafeMarkdownImageWidget(image, match[1] ?? '', config.onPreviewError),
          block: true,
        }).range(line.from, line.to));
        return;
      }
      ranges.push(Decoration.widget({
        widget: new SafeMarkdownImageWidget(image, match[1] ?? '', config.onPreviewError),
        block: true,
        side: 1,
      }).range(line.to));
    },
  });
  addSafeHtmlDecorations(state, config, ranges);
  return Decoration.set(ranges, true);
}

function addSafeHtmlDecorations(
  state: EditorState,
  config: MarkdownImagePreviewConfig,
  ranges: Range<Decoration>[],
) {
  const alignStack: Array<{ tag: string; align: string }> = [];
  for (let lineNumber = 1; lineNumber <= state.doc.lines; lineNumber += 1) {
    const line = state.doc.line(lineNumber);
    const text = line.text;
    if (isCodeLine(state, line.from)) {
      const currentAlign = alignStack.at(-1)?.align;
      if (currentAlign) {
        ranges.push(Decoration.line({ attributes: { class: `cm-gold-band-markdown-align-${currentAlign}` } }).range(line.from));
      }
      continue;
    }
    const close = text.match(HTML_ALIGN_CLOSE_PATTERN);
    const open = text.match(HTML_ALIGN_OPEN_PATTERN);
    const currentAlign = alignStack.at(-1)?.align;
    if (currentAlign || open?.[3]) {
      ranges.push(Decoration.line({
        attributes: { class: `cm-gold-band-markdown-align-${open?.[3]?.toLowerCase() ?? currentAlign}` },
      }).range(line.from));
    }

    const linkedImageMatch = text.match(HTML_LINKED_IMAGE_LINE_PATTERN);
    const imageMatch = text.match(HTML_IMAGE_LINE_PATTERN);
    const imageAttributes = linkedImageMatch?.[2] ?? imageMatch?.[1] ?? '';
    const source = htmlAttribute(imageAttributes, 'src');
    if (source && isRemoteImageSource(source)) {
      const href = linkedImageMatch ? htmlAttribute(linkedImageMatch[1] ?? '', 'href') || source : source;
      ranges.push(Decoration.replace({
        widget: remoteImageWidget(source, href, htmlAttribute(imageAttributes, 'alt'), config),
      }).range(line.from, line.to));
    } else if (source) {
      const image = config.images.get(source);
      if (image) {
        ranges.push(Decoration.replace({
          widget: new SafeMarkdownImageWidget(image, htmlAttribute(imageAttributes, 'alt'), config.onPreviewError),
          block: true,
        }).range(line.from, line.to));
      }
    } else if ((open || close || HTML_BREAK_PATTERN.test(text)) && line.from < line.to) {
      ranges.push(Decoration.replace({}).range(line.from, line.to));
    }
    if (open) alignStack.push({ tag: open[1]!.toLowerCase(), align: open[3]!.toLowerCase() });
    if (close) {
      const tag = close[1]!.toLowerCase();
      const matchIndex = alignStack.map((entry) => entry.tag).lastIndexOf(tag);
      if (matchIndex >= 0) alignStack.splice(matchIndex, 1);
    }
  }
}

function isCodeLine(state: EditorState, position: number) {
  let node = syntaxTree(state).resolveInner(position, 1);
  while (node) {
    if (node.name === 'FencedCode' || node.name === 'CodeBlock' || node.name === 'InlineCode') return true;
    node = node.parent!;
  }
  return false;
}

const markdownImagePreviewField = StateField.define<MarkdownImagePreviewFieldValue>({
  create: (state) => ({
    ...EMPTY_CONFIG,
    decorations: buildDecorations(state, EMPTY_CONFIG),
  }),
  update: (value, transaction) => {
    let config: MarkdownImagePreviewConfig = value;
    for (const effect of transaction.effects) {
      if (effect.is(setMarkdownImagePreviewConfig)) config = effect.value;
    }
    if (!transaction.docChanged && config === value) return value;
    return { ...config, decorations: buildDecorations(transaction.state, config) };
  },
  // Images, alerts and reference text are rendered here, not by Atomic's inline preview, which
  // only hides their source. Higher precedence makes these widgets win the shared ranges.
  provide: (field) => Prec.high(EditorView.decorations.from(field, (value) => value.decorations)),
});

export function markdownImagePreview(config: MarkdownImagePreviewConfig = EMPTY_CONFIG): Extension {
  return [
    markdownImagePreviewField,
    markdownImagePreviewField.init((state) => ({
      ...config,
      decorations: buildDecorations(state, config),
    })),
  ];
}

export function updateMarkdownImagePreview(view: EditorView, config: MarkdownImagePreviewConfig) {
  view.dispatch({ effects: setMarkdownImagePreviewConfig.of(config) });
}
