import { ensureSyntaxTree, syntaxTree } from '@codemirror/language';
import { StateField, type EditorState, type Extension, type Range } from '@codemirror/state';
import { Decoration, EditorView, WidgetType, type DecorationSet } from '@codemirror/view';
import { workspaceFilePreviewUrl } from '@/api';
import type { MarkdownImageState } from './file-content-store';

const IMAGE_SOURCE_PATTERN = /!\[[^\]]*\]\((?:<([^>]+)>|([^\s)"']+))(?:\s+["'][^)]*["'])?\)/gu;

export function markdownImageSources(markdown: string): string[] {
  const sources = new Set<string>();
  for (const match of markdown.matchAll(IMAGE_SOURCE_PATTERN)) {
    const source = match[1] ?? match[2];
    if (source) sources.add(source);
  }
  return [...sources];
}

class SafeMarkdownImageWidget extends WidgetType {
  constructor(
    private readonly state: MarkdownImageState,
    private readonly alt: string,
  ) {
    super();
  }

  eq(other: SafeMarkdownImageWidget) {
    if (other.state.kind !== this.state.kind || other.state.rawSrc !== this.state.rawSrc) return false;
    if (this.state.kind === 'ready' && other.state.kind === 'ready') {
      return this.state.previewToken === other.state.previewToken && this.alt === other.alt;
    }
    return this.alt === other.alt;
  }

  toDOM(view: EditorView) {
    const wrap = document.createElement('div');
    wrap.className = 'cm-atomic-image cm-gold-band-markdown-image';
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
      const line = state.doc.lineAt(node.from);
      ranges.push(Decoration.widget({
        widget: new SafeMarkdownImageWidget(image, match[1] ?? ''),
        block: true,
        side: 1,
      }).range(line.to));
    },
  });
  return Decoration.set(ranges, true);
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
