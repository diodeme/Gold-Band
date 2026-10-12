import type { CSSProperties } from 'react';
import { LeasedBlobCache, type LeasedBlob, type LeasedBlobAsset } from '@/lib/leased-blob-cache';
import type { MermaidTheme } from '@/lib/mermaid-theme';

export const MERMAID_LANGUAGE = 'mermaid';
/** Below this scale inline labels stop being readable, so the diagram keeps it and scrolls horizontally. */
export const MERMAID_INLINE_MIN_SCALE = 0.6;
const MERMAID_CACHE_ENTRIES = 96;
const MERMAID_CACHE_BYTES = 32 * 1024 * 1024;
// A diagram is shown through <img>, which cannot reach the document's web fonts. Mermaid measures
// labels with the same system stack the image renders with, so boxes fit their text.
const MERMAID_FONT_FAMILY = '"Segoe UI", "Microsoft YaHei UI", "PingFang SC", "Noto Sans CJK SC", sans-serif';
const MERMAID_RASTER_SCALE = 2;
const MERMAID_RASTER_MAX_EDGE = 8192;
const SVG_NAMESPACE = 'http://www.w3.org/2000/svg';

export const MERMAID_ERROR_CODES = {
  renderFailed: 'mermaid.render-failed',
  cacheFull: 'mermaid.cache-full',
  cancelled: 'mermaid.cancelled',
  rasterFailed: 'mermaid.raster-failed',
} as const;

export interface MermaidDiagramError {
  code: (typeof MERMAID_ERROR_CODES)[keyof typeof MERMAID_ERROR_CODES];
  params: { detail?: string };
}

export interface MermaidDiagramMeta {
  width: number;
  height: number;
  /** Surface the diagram colors were derived for; the SVG itself is transparent. */
  background: string;
}

export type MermaidDiagramAsset = LeasedBlobAsset<MermaidDiagramMeta>;

function mermaidError(code: MermaidDiagramError['code'], detail?: string): MermaidDiagramError {
  return { code, params: detail ? { detail } : {} };
}

export function isMermaidDiagramError(error: unknown): error is MermaidDiagramError {
  return typeof error === 'object' && error !== null
    && Object.values(MERMAID_ERROR_CODES).includes((error as { code?: unknown }).code as MermaidDiagramError['code']);
}

type MermaidApi = typeof import('mermaid')['default'];
let mermaidModule: Promise<MermaidApi> | null = null;
let initializedThemeKey: string | null = null;
let renderSequence = 0;

function loadMermaid() {
  mermaidModule ??= import('mermaid').then((module) => module.default);
  return mermaidModule;
}

/** Turns Mermaid's responsive SVG into a standalone image with an intrinsic size. */
export function standaloneMermaidSvg(svg: string, background: string): LeasedBlob<MermaidDiagramMeta> {
  // HTML parsing tolerates the non-XML markup some diagram types emit; serialization makes it well-formed.
  const root = new DOMParser().parseFromString(svg, 'text/html').querySelector('svg');
  if (!root) throw mermaidError(MERMAID_ERROR_CODES.renderFailed);
  const viewBox = root.getAttribute('viewBox')?.trim().split(/[\s,]+/u).map(Number) ?? [];
  const width = Math.ceil(viewBox[2] ?? Number.parseFloat(root.getAttribute('width') ?? ''));
  const height = Math.ceil(viewBox[3] ?? Number.parseFloat(root.getAttribute('height') ?? ''));
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
    throw mermaidError(MERMAID_ERROR_CODES.renderFailed);
  }
  root.setAttribute('width', String(width));
  root.setAttribute('height', String(height));
  root.style.removeProperty('max-width');
  if (!root.getAttribute('style')) root.removeAttribute('style');
  // The serializer declares the SVG namespace itself; a second literal xmlns would make the XML invalid.
  if (root.namespaceURI !== SVG_NAMESPACE) throw mermaidError(MERMAID_ERROR_CODES.renderFailed);
  const markup = new XMLSerializer().serializeToString(root);
  return { blob: new Blob([markup], { type: 'image/svg+xml' }), meta: { width, height, background } };
}

async function renderMermaidDiagram(source: string, theme: MermaidTheme) {
  const mermaid = await loadMermaid();
  if (initializedThemeKey !== theme.key) {
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: 'strict',
      suppressErrorRendering: true,
      // Only `base` accepts a full palette; every color is derived from the Gold Band theme.
      theme: 'base',
      // The flat look; `neo` adds drop shadows that read as heavy borders on dark surfaces.
      look: 'classic',
      themeVariables: { ...theme.variables, darkMode: theme.darkMode, fontFamily: MERMAID_FONT_FAMILY },
      themeCSS: theme.css,
      // HTML labels live in foreignObject, which renders unreliably inside <img> and taints rasterization.
      htmlLabels: false,
      fontFamily: MERMAID_FONT_FAMILY,
    });
    initializedThemeKey = theme.key;
  }
  renderSequence += 1;
  let svg: string;
  try {
    ({ svg } = await mermaid.render(`gb-mermaid-${renderSequence}`, source));
  } catch (error) {
    throw mermaidError(MERMAID_ERROR_CODES.renderFailed, error instanceof Error ? error.message : undefined);
  }
  return standaloneMermaidSvg(svg, theme.background);
}

// Mermaid keeps global render state, so diagrams render one at a time.
const cache = new LeasedBlobCache<MermaidDiagramMeta>({
  maxEntries: MERMAID_CACHE_ENTRIES,
  maxBytes: MERMAID_CACHE_BYTES,
  concurrency: 1,
  errors: {
    full: mermaidError(MERMAID_ERROR_CODES.cacheFull),
    cancelled: mermaidError(MERMAID_ERROR_CODES.cancelled),
  },
});

export function mermaidDiagramKey(source: string, theme: MermaidTheme) {
  return `${theme.key}\n${source}`;
}

export function peekMermaidDiagram(source: string, theme: MermaidTheme) {
  return cache.peek(mermaidDiagramKey(source, theme));
}

export function acquireMermaidDiagram(source: string, theme: MermaidTheme) {
  return cache.acquire(mermaidDiagramKey(source, theme), () => renderMermaidDiagram(source, theme));
}

/** Fits the message width without upscaling or cropping; the aspect ratio fixes the height before the image decodes. */
export function mermaidInlineImageStyle({ width, height }: MermaidDiagramMeta): CSSProperties {
  return {
    width: `${width}px`,
    maxWidth: '100%',
    minWidth: `${Math.round(width * MERMAID_INLINE_MIN_SCALE)}px`,
    height: 'auto',
    aspectRatio: `${width} / ${height}`,
  };
}

/** Stable digest of a diagram source for workspace tab identity (cyrb53). */
export function mermaidSourceDigest(source: string) {
  let h1 = 0xdeadbeef;
  let h2 = 0x41c6ce57;
  for (let index = 0; index < source.length; index += 1) {
    const code = source.charCodeAt(index);
    h1 = Math.imul(h1 ^ code, 2654435761);
    h2 = Math.imul(h2 ^ code, 1597334677);
  }
  h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507) ^ Math.imul(h2 ^ (h2 >>> 13), 3266489909);
  h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507) ^ Math.imul(h1 ^ (h1 >>> 13), 3266489909);
  return `${(4294967296 * (2097151 & h2) + (h1 >>> 0)).toString(36)}-${source.length.toString(36)}`;
}

/** PNG for clipboard and save actions, which only accept raster images. */
export async function rasterizeMermaidDiagram(asset: MermaidDiagramAsset): Promise<Blob> {
  const { width, height, background } = asset.meta;
  const scale = Math.min(MERMAID_RASTER_SCALE, MERMAID_RASTER_MAX_EDGE / Math.max(width, height));
  const image = new Image();
  image.src = asset.url;
  await image.decode();
  const canvas = document.createElement('canvas');
  canvas.width = Math.max(1, Math.round(width * scale));
  canvas.height = Math.max(1, Math.round(height * scale));
  const context = canvas.getContext('2d');
  if (!context) throw mermaidError(MERMAID_ERROR_CODES.rasterFailed);
  // The SVG is transparent and its colors assume the surface they were derived for.
  context.fillStyle = background;
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.drawImage(image, 0, 0, canvas.width, canvas.height);
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'));
  if (!blob) throw mermaidError(MERMAID_ERROR_CODES.rasterFailed);
  return blob;
}
