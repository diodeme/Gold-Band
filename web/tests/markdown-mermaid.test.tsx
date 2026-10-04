/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

vi.mock('react-i18next', () => ({
  initReactI18next: { type: '3rdParty', init: () => {} },
  useTranslation: () => ({ t: (key: string) => key }),
}));

const mermaid = vi.hoisted(() => ({ initialize: vi.fn(), render: vi.fn() }));
vi.mock('mermaid', () => ({ default: mermaid }));

const workspace = vi.hoisted(() => ({
  commands: null as null | { scopeKey: string; openResource: ReturnType<typeof vi.fn> },
}));
vi.mock('@/components/workspace/right-workspace-context', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/components/workspace/right-workspace-context')>()),
  useOptionalRightWorkspaceCommands: () => workspace.commands,
}));

import { Markdown } from '@/components/prompt-kit/markdown';
import { TooltipProvider } from '@/components/ui/tooltip';
import { mermaidDiagramWorkspaceResourceKey } from '@/components/workspace/right-workspace-context';
import { MERMAID_ERROR_CODES } from '@/lib/mermaid-diagram';

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let container: HTMLDivElement;
let objectUrl = 0;

beforeEach(() => {
  mermaid.initialize.mockReset();
  mermaid.render.mockReset();
  mermaid.render.mockResolvedValue({
    svg: '<svg width="100%" viewBox="0 0 900 2400" style="max-width: 900px;"><g></g></svg>',
    diagramType: 'flowchart',
  });
  URL.createObjectURL = vi.fn(() => `blob:diagram-${(objectUrl += 1)}`);
  URL.revokeObjectURL = vi.fn();
  workspace.commands = null;
  // Streaming playback scrolls its root; jsdom has no layout to scroll.
  Element.prototype.scrollTo ??= () => {};
  container = document.createElement('div');
  document.body.appendChild(container);
});

afterEach(() => {
  act(() => root?.unmount());
  root = null;
  container.remove();
});

const fence = (body: string, closed = true) => `Before\n\n\`\`\`mermaid\n${body}\n${closed ? '```\n' : ''}`;

async function render(markdown: string, streaming = false) {
  await act(async () => {
    root ??= createRoot(container);
    root.render(<TooltipProvider><Markdown streaming={streaming}>{markdown}</Markdown></TooltipProvider>);
  });
  // Let the dynamic mermaid import, the serial render, and the lease resolution settle.
  for (let index = 0; index < 5; index += 1) {
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
  }
}

it('keeps an unclosed streaming fence as a code block and renders nothing yet', async () => {
  await render(fence('flowchart TD\nA-->B', false), true);
  expect(container.querySelector('[data-gb-mermaid-block]')).toBeNull();
  expect(container.textContent).toContain('A-->B');
  expect(mermaid.render).not.toHaveBeenCalled();
});

it('shows a closed fence as an uncropped diagram whose height is fixed before the image decodes', async () => {
  await render(fence('flowchart TD\nfull-height-->B'));
  const block = container.querySelector('[data-gb-mermaid-block="ready"]');
  const image = block?.querySelector('img');
  expect(image).not.toBeNull();
  expect(image?.getAttribute('width')).toBe('900');
  expect(image?.getAttribute('height')).toBe('2400');
  expect(image?.style.width).toBe('900px');
  expect(image?.style.maxWidth).toBe('100%');
  expect(image?.style.aspectRatio).toBe('900 / 2400');
  expect(image?.style.maxHeight).toBe('');
  // Only horizontal overflow may scroll; the conversation keeps every vertical scroll gesture.
  const scroller = block?.querySelector<HTMLElement>('[data-gb-mermaid-diagram]');
  expect(scroller?.className).toContain('overflow-x-auto');
  expect(scroller?.className).toContain('overflow-y-hidden');
  expect(mermaid.render).toHaveBeenCalledTimes(1);
});

it('renders a cached diagram at its final size on the first commit after remount', async () => {
  const markdown = fence('flowchart TD\ncached-->B');
  await render(markdown);
  act(() => root?.unmount());
  root = createRoot(container);
  act(() => {
    root?.render(<TooltipProvider><Markdown>{markdown}</Markdown></TooltipProvider>);
  });
  // No awaited tick: the remounted block must already be the diagram, not the source.
  expect(container.querySelector('[data-gb-mermaid-block="ready"] img')?.getAttribute('height')).toBe('2400');
  expect(mermaid.render).toHaveBeenCalledTimes(1);
});

it('falls back to the source with a structured error when Mermaid rejects the diagram', async () => {
  mermaid.render.mockRejectedValueOnce(new Error('Parse error on line 2'));
  await render(fence('flowchart TD\nbroken -->'));
  const block = container.querySelector('[data-gb-mermaid-block="error"]');
  expect(block?.textContent).toContain('broken -->');
  const alert = block?.querySelector(`[data-gb-mermaid-error="${MERMAID_ERROR_CODES.renderFailed}"]`);
  expect(alert?.textContent).toContain('common.mermaid.errors.renderFailed');
  expect(alert?.textContent).toContain('Parse error on line 2');
});

it('switches between the diagram and its source without re-rendering it', async () => {
  await render(fence('flowchart TD\ntoggle-->B'));
  const showSource = container.querySelector<HTMLButtonElement>('button[aria-label="common.mermaid.showSource"]');
  await act(async () => showSource?.click());
  expect(container.querySelector('[data-gb-mermaid-block="source"]')?.textContent).toContain('toggle-->B');
  const showDiagram = container.querySelector<HTMLButtonElement>('button[aria-label="common.mermaid.showDiagram"]');
  await act(async () => showDiagram?.click());
  expect(container.querySelector('[data-gb-mermaid-block="ready"] img')).not.toBeNull();
  expect(mermaid.render).toHaveBeenCalledTimes(1);
});

it('opens the diagram as a content-addressed workspace tab', async () => {
  const openResource = vi.fn();
  workspace.commands = { scopeKey: 'conversation:p:t:r', openResource };
  const body = 'flowchart TD\nworkspace-->B';
  // The fenced code keeps its trailing newline, exactly as Mermaid receives it.
  const source = `${body}\n`;
  await render(fence(body));
  await act(async () => container.querySelector<HTMLImageElement>('[data-gb-mermaid-block="ready"] img')?.click());
  await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="common.mermaid.open"]')?.click());
  expect(openResource).toHaveBeenCalledTimes(2);
  expect(openResource).toHaveBeenLastCalledWith({
    kind: 'mermaid-diagram',
    key: mermaidDiagramWorkspaceResourceKey('conversation:p:t:r', source),
    scopeKey: 'conversation:p:t:r',
    title: 'common.mermaid.title',
    attention: false,
    source,
  });
});

it('offers no open action outside a workspace', async () => {
  await render(fence('flowchart TD\nno-workspace-->B'));
  expect(container.querySelector('button[aria-label="common.mermaid.open"]')).toBeNull();
  expect(container.querySelector('[data-gb-mermaid-block="ready"] img')?.className).not.toContain('cursor-zoom-in');
});
