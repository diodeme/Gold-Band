import { ensureSyntaxTree, syntaxTree } from '@codemirror/language';
import { StateField, type EditorState, type Extension, type Range } from '@codemirror/state';
import { Decoration, EditorView, WidgetType, type DecorationSet } from '@codemirror/view';
import { workspaceFilePreviewUrl } from '@/api';
import type { MarkdownImageState } from './file-content-store';

const IMAGE_SOURCE_PATTERN = /!\[[^\]]*\]\((?:<([^>]+)>|([^\s)"']+))(?:\s+["'][^)]*["'])?\)/gu;
const HTML_IMAGE_SOURCE_PATTERN = /<img\b[^>]*\bsrc\s*=\s*(["'])(.*?)\1[^>]*>/giu;
const HTML_IMAGE_LINE_PATTERN = /^\s*<img\b([^>]*)\/?\s*>\s*$/iu;
const HTML_ALIGN_OPEN_PATTERN = /^\s*<(div|p)\b[^>]*\balign\s*=\s*(["'])(center|left|right)\2[^>]*>\s*$/iu;
const HTML_ALIGN_CLOSE_PATTERN = /^\s*<\/(div|p)>\s*$/iu;
const HTML_BREAK_PATTERN = /^\s*<br\s*\/?>\s*$/iu;

export function markdownImageSources(markdown: string): string[] {
  const sources = new Set<string>();
  for (const match of markdown.matchAll(IMAGE_SOURCE_PATTERN)) {
    const source = match[1] ?? match[2];
    if (source) sources.add(source);
  }
  for (const match of markdown.matchAll(HTML_IMAGE_SOURCE_PATTERN)) {
    if (match[2]) sources.add(match[2]);
  }
  return [...sources];
}

function htmlAttribute(attributes: string, name: string) {
  const match = attributes.match(new RegExp(`\\b${name}\\s*=\\s*(["'])(.*?)\\1`, 'iu'));
  return match?.[2] ?? '';
}

function isRemoteSource(source: string) {
  return /^https?:\/\//iu.test(source);
}

class SafeMarkdownImageWidget extends WidgetType {
  constructor(
    private readonly state: MarkdownImageState,
    private readonly alt: string,
    private readonly inline = false,
  ) {
    super();
  }

  eq(other: SafeMarkdownImageWidget) {
    if (other.inline !== this.inline || other.state.kind !== this.state.kind || other.state.rawSrc !== this.state.rawSrc) return false;
    if (this.state.kind === 'ready' && other.state.kind === 'ready') {
      return this.state.previewToken === other.state.previewToken && this.alt === other.alt;
    }
    return this.alt === other.alt;
  }

  toDOM(view: EditorView) {
    const wrap = document.createElement(this.inline ? 'span' : 'div');
    wrap.className = this.inline
      ? 'cm-gold-band-markdown-image-inline'
      : 'cm-atomic-image cm-gold-band-markdown-image';
    if (this.state.kind === 'ready') {
      const image = document.createElement('img');
      image.src = workspaceFilePreviewUrl(this.state.previewToken);
      image.alt = this.alt;
      image.loading = 'lazy';
      image.width = this.state.width;
      image.height = this.state.height;
      wrap.appendChild(image);
    } else {
      const placeholder = document.createElement('span');
      placeholder.className = 'cm-gold-band-markdown-image-placeholder';
      placeholder.textContent = this.state.kind === 'loading'
        ? '…'
        : this.alt || this.state.rawSrc;
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

function buildDecorations(state: EditorState, images: ReadonlyMap<string, MarkdownImageState>) {
  const ranges: Range<Decoration>[] = [];
  const tree = ensureSyntaxTree(state, state.doc.length, 100) ?? syntaxTree(state);
  tree.iterate({
    enter(node) {
      if (node.name !== 'Image') return;
      const raw = state.doc.sliceString(node.from, node.to);
      const match = raw.match(/^!\[([^\]]*)\]\((?:<([^>]+)>|([^\s)"']+))(?:\s+["'][^)]*["'])?\)$/u);
      const source = match?.[2] ?? match?.[3];
      if (!match || !source) return;
      const image = images.get(source);
      if (!image) return;
      if (image.kind !== 'ready' && isRemoteSource(source)) {
        ranges.push(Decoration.replace({
          widget: new SafeMarkdownImageWidget(image, match[1] ?? '', true),
        }).range(node.from, node.to));
        return;
      }
      const line = state.doc.lineAt(node.from);
      ranges.push(Decoration.widget({
        widget: new SafeMarkdownImageWidget(image, match[1] ?? ''),
        block: true,
        side: 1,
      }).range(line.to));
    },
  });
  addSafeHtmlDecorations(state, images, ranges);
  return Decoration.set(ranges, true);
}

function addSafeHtmlDecorations(
  state: EditorState,
  images: ReadonlyMap<string, MarkdownImageState>,
  ranges: Range<Decoration>[],
) {
  const alignStack: Array<{ tag: string; align: string }> = [];
  for (let lineNumber = 1; lineNumber <= state.doc.lines; lineNumber += 1) {
    const line = state.doc.line(lineNumber);
    const text = line.text;
    if (isCodeLine(state, line.from)) {
      const currentAlign = alignStack.at(-1)?.align;
      if (currentAlign) {
        ranges.push(Decoration.line({
          attributes: { class: `cm-gold-band-markdown-align-${currentAlign}` },
        }).range(line.from));
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
    const imageMatch = text.match(HTML_IMAGE_LINE_PATTERN);
    if (imageMatch) {
      const source = htmlAttribute(imageMatch[1] ?? '', 'src');
      const image = images.get(source);
      if (source && image) {
        const inline = image.kind !== 'ready' && isRemoteSource(source);
        ranges.push(Decoration.replace({
          widget: new SafeMarkdownImageWidget(image, htmlAttribute(imageMatch[1] ?? '', 'alt'), inline),
          block: !inline,
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

export function markdownImagePreview(images: ReadonlyMap<string, MarkdownImageState>): Extension {
  const field = StateField.define<DecorationSet>({
    create: (state) => buildDecorations(state, images),
    update: (decorations, transaction) => transaction.docChanged
      ? buildDecorations(transaction.state, images)
      : decorations,
    provide: (value) => EditorView.decorations.from(value),
  });
  return field;
}
