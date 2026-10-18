/** @vitest-environment jsdom */

import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const mermaid = vi.hoisted(() => ({
  initialize: vi.fn(),
  render: vi.fn(),
}));
vi.mock('mermaid', () => ({ default: mermaid }));

import {
  MERMAID_ERROR_CODES,
  MERMAID_INLINE_MIN_SCALE,
  acquireMermaidDiagram,
  mermaidInlineImageStyle,
  mermaidSourceDigest,
  peekMermaidDiagram,
  standaloneMermaidSvg,
} from '@/lib/mermaid-diagram';
import { deriveMermaidTheme, type MermaidThemeTokens } from '@/lib/mermaid-theme';

const lightTokens: MermaidThemeTokens = {
  background: { r: 255, g: 255, b: 255 },
  foreground: { r: 13, g: 13, b: 13 },
  accent: { r: 232, g: 245, b: 240 },
  accentForeground: { r: 10, g: 122, b: 94 },
  warning: { r: 164, g: 101, b: 8 },
  darkMode: false,
};
const light = deriveMermaidTheme(lightTokens);
const dark = deriveMermaidTheme({
  background: { r: 17, g: 17, b: 17 },
  foreground: { r: 240, g: 240, b: 240 },
  accent: { r: 24, g: 60, b: 51 },
  accentForeground: { r: 125, g: 226, b: 193 },
  warning: { r: 217, g: 133, b: 71 },
  darkMode: true,
});

let objectUrl = 0;
beforeEach(() => {
  mermaid.initialize.mockReset();
  mermaid.render.mockReset();
  URL.createObjectURL = vi.fn(() => `blob:mermaid-${(objectUrl += 1)}`);
  URL.revokeObjectURL = vi.fn();
});
afterEach(() => vi.restoreAllMocks());

const responsiveSvg = (width: number, height: number) =>
  `<svg id="gb" width="100%" viewBox="0 0 ${width} ${height}" style="max-width: ${width}px;"><g><text>A&nbsp;B</text></g></svg>`;

async function blobText(blob: Blob) {
  return new Promise<string>((resolve) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.readAsText(blob);
  });
}

it('turns the responsive Mermaid SVG into a standalone image with its intrinsic size', async () => {
  const { blob, meta } = standaloneMermaidSvg(responsiveSvg(640.4, 1800.2), '#ffffff');
  expect(meta).toEqual({ width: 641, height: 1801, background: '#ffffff' });
  expect(blob.type).toBe('image/svg+xml');
  const markup = await blobText(blob);
  expect(markup.match(/xmlns=/gu)).toHaveLength(1);
  expect(markup).toContain('xmlns="http://www.w3.org/2000/svg"');
  expect(markup).toContain('width="641"');
  expect(markup).toContain('height="1801"');
  expect(markup).not.toContain('max-width');
  // Mermaid serializes through HTML, so entities such as &nbsp; must become well-formed XML for <img>.
  expect(markup).not.toContain('&nbsp;');
  const parsed = new DOMParser().parseFromString(markup, 'image/svg+xml');
  expect(parsed.querySelector('parsererror')).toBeNull();
  expect(parsed.documentElement.textContent).toBe('A B');
});

it('rejects SVG output without a usable size as a structured render error', () => {
  expect(() => standaloneMermaidSvg('<svg viewBox="0 0 0 0"></svg>', '#ffffff')).toThrow();
  try {
    standaloneMermaidSvg('<div>no svg</div>', '#ffffff');
  } catch (error) {
    expect(error).toEqual({ code: MERMAID_ERROR_CODES.renderFailed, params: {} });
  }
});

it('fits the message width without upscaling, cropping height, or shrinking below the readable scale', () => {
  const style = mermaidInlineImageStyle({ width: 1200, height: 3000, background: '#ffffff' });
  expect(style).toEqual({
    width: '1200px',
    maxWidth: '100%',
    minWidth: `${Math.round(1200 * MERMAID_INLINE_MIN_SCALE)}px`,
    height: 'auto',
    aspectRatio: '1200 / 3000',
  });
  expect(style).not.toHaveProperty('maxHeight');
});

it('derives a stable tab identity from the diagram source', () => {
  expect(mermaidSourceDigest('flowchart TD\nA-->B')).toBe(mermaidSourceDigest('flowchart TD\nA-->B'));
  expect(mermaidSourceDigest('flowchart TD\nA-->B')).not.toBe(mermaidSourceDigest('flowchart TD\nA-->C'));
});

it('renders each source and theme once, serially, with the derived base palette', async () => {
  let active = 0;
  let maxActive = 0;
  mermaid.render.mockImplementation(async () => {
    active += 1;
    maxActive = Math.max(maxActive, active);
    await new Promise((resolve) => setTimeout(resolve, 5));
    active -= 1;
    return { svg: responsiveSvg(100, 50), diagramType: 'flowchart' };
  });
  const first = acquireMermaidDiagram('graph TD\nserial-a', light);
  const shared = acquireMermaidDiagram('graph TD\nserial-a', light);
  const second = acquireMermaidDiagram('graph TD\nserial-b', dark);
  const [a, b, c] = await Promise.all([first.promise, shared.promise, second.promise]);
  expect(a).toBe(b);
  expect(c.meta).toEqual({ width: 100, height: 50, background: dark.background });
  expect(mermaid.render).toHaveBeenCalledTimes(2);
  expect(maxActive).toBe(1);
  expect(mermaid.initialize).toHaveBeenCalledWith(expect.objectContaining({
    securityLevel: 'strict',
    htmlLabels: false,
    suppressErrorRendering: true,
    theme: 'base',
    look: 'classic',
    themeCSS: dark.css,
    themeVariables: expect.objectContaining({ ...dark.variables, darkMode: true }),
  }));
  expect(peekMermaidDiagram('graph TD\nserial-a', light)).toBe(a);
  expect(peekMermaidDiagram('graph TD\nserial-a', dark)).toBeNull();
  [first, shared, second].forEach((lease) => lease.release());
});

it('maps Mermaid syntax errors to a structured code and lets the source be retried', async () => {
  mermaid.render.mockRejectedValueOnce(new Error('Parse error on line 2'));
  const failed = acquireMermaidDiagram('graph TD\nbroken', light);
  await expect(failed.promise).rejects.toEqual({
    code: MERMAID_ERROR_CODES.renderFailed,
    params: { detail: 'Parse error on line 2' },
  });
  failed.release();
  expect(peekMermaidDiagram('graph TD\nbroken', light)).toBeNull();
  mermaid.render.mockResolvedValueOnce({ svg: responsiveSvg(10, 10), diagramType: 'flowchart' });
  const retried = acquireMermaidDiagram('graph TD\nbroken', light);
  await expect(retried.promise).resolves.toMatchObject({ meta: { width: 10, height: 10 } });
  retried.release();
});
