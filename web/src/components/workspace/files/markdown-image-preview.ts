import { ensureSyntaxTree, syntaxTree } from '@codemirror/language';
import { StateEffect, StateField, type EditorState, type Extension, type Range } from '@codemirror/state';
import { Decoration, EditorView, WidgetType, type DecorationSet } from '@codemirror/view';
import { workspaceFilePreviewUrl } from '@/api';
import { isLocalFileHref } from '@/lib/file-link';
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

export function isRemoteMarkdownImageSource(source: string) {
  return /^(?:https?:)?\/\//iu.test(source.trim());
}

export function markdownImageSources(markdown: string): string[] {
  const sources = new Set<string>();
  for (const match of markdown.matchAll(IMAGE_SOURCE_PATTERN)) {
    const source = match[1] ?? match[2];
    if (source && !isRemoteMarkdownImageSource(source)) sources.add(source);
  }
  for (const match of markdown.matchAll(HTML_IMAGE_SOURCE_PATTERN)) {
    if (match[2] && !isRemoteMarkdownImageSource(match[2])) sources.add(match[2]);
  }
  return [...sources];
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
      image.src = workspaceFilePreviewUrl(this.state.previewGrant.token);
      image.alt = this.alt;
      image.loading = 'lazy';
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

interface MarkdownImagePreviewConfig {
  images: ReadonlyMap<string, MarkdownImageState>;
  onPreviewError?: (rawSrc: string, failedToken: string) => void;
  onLinkClick?: (href: string) => void;
}

interface MarkdownImagePreviewFieldValue extends MarkdownImagePreviewConfig {
  decorations: DecorationSet;
}

const setMarkdownImagePreviewConfig = StateEffect.define<MarkdownImagePreviewConfig>();

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

function buildDecorations(state: EditorState, config: MarkdownImagePreviewConfig) {
  const ranges: Range<Decoration>[] = [];
  const tree = ensureSyntaxTree(state, state.doc.length, 100) ?? syntaxTree(state);
  tree.iterate({
    enter(node) {
      if (node.name === 'CommentBlock') {
        const startLine = state.doc.lineAt(node.from);
        const endLine = state.doc.lineAt(node.to);
        ranges.push(Decoration.replace({ block: true }).range(startLine.from, endLine.to));
        return false;
      }
      if (node.name !== 'Image') return;
      const raw = state.doc.sliceString(node.from, node.to);
      const match = raw.match(/^!\[([^\]]*)\]\((?:<([^>]+)>|([^\s)"']+))(?:\s+["'][^)]*["'])?\)$/u);
      const source = match?.[2] ?? match?.[3];
      if (!match || !source) return;
      if (isRemoteMarkdownImageSource(source)) {
        const link = markdownLinkTarget(state, node, source);
        ranges.push(Decoration.replace({
          widget: new RemoteMarkdownImageLinkWidget(link.href, match[1] ?? '', config.onLinkClick),
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
    if (source && isRemoteMarkdownImageSource(source)) {
      const href = linkedImageMatch ? htmlAttribute(linkedImageMatch[1] ?? '', 'href') || source : source;
      ranges.push(Decoration.replace({
          widget: new RemoteMarkdownImageLinkWidget(href, htmlAttribute(imageAttributes, 'alt'), config.onLinkClick),
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
    images: EMPTY_MARKDOWN_IMAGES,
    decorations: buildDecorations(state, { images: EMPTY_MARKDOWN_IMAGES }),
  }),
  update: (value, transaction) => {
    let config: MarkdownImagePreviewConfig = value;
    for (const effect of transaction.effects) {
      if (effect.is(setMarkdownImagePreviewConfig)) config = effect.value;
    }
    if (!transaction.docChanged && config === value) return value;
    return { ...config, decorations: buildDecorations(transaction.state, config) };
  },
  provide: (field) => EditorView.decorations.from(field, (value) => value.decorations),
});

export function markdownImagePreview(
  images: ReadonlyMap<string, MarkdownImageState> = EMPTY_MARKDOWN_IMAGES,
  onPreviewError?: (rawSrc: string, failedToken: string) => void,
  onLinkClick?: (href: string) => void,
): Extension {
  return [
    markdownImagePreviewField,
    markdownImagePreviewField.init((state) => ({
      images,
      onPreviewError,
      onLinkClick,
      decorations: buildDecorations(state, { images, onPreviewError, onLinkClick }),
    })),
  ];
}

export function updateMarkdownImagePreview(
  view: EditorView,
  images: ReadonlyMap<string, MarkdownImageState>,
  onPreviewError?: (rawSrc: string, failedToken: string) => void,
  onLinkClick?: (href: string) => void,
) {
  view.dispatch({ effects: setMarkdownImagePreviewConfig.of({ images, onPreviewError, onLinkClick }) });
}
